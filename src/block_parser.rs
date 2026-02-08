use crate::config::Config;
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::GreenNodeBuilder;

mod attributes;
mod blockquotes;
mod code_blocks;
mod container_stack;
mod definition_lists;
mod display_math;
mod fenced_divs;
mod headings;
mod horizontal_rules;
mod html_blocks;
mod indented_code;
mod latex_envs;
mod lists;
mod metadata;
mod paragraphs;
mod tables;
mod utils;

use blockquotes::count_blockquote_markers;
use code_blocks::{parse_fenced_code_block, try_parse_fence_open};
use container_stack::{Container, ContainerStack, byte_index_at_column, leading_indent};
use definition_lists::{emit_definition_marker, emit_term, try_parse_definition_marker};
use display_math::{parse_display_math_block, try_parse_math_fence_open};
use fenced_divs::{is_div_closing_fence, try_parse_div_fence_open};
use headings::{emit_atx_heading, try_parse_atx_heading};
use horizontal_rules::{emit_horizontal_rule, try_parse_horizontal_rule};
use html_blocks::{parse_html_block, try_parse_html_block_start};
use indented_code::{is_indented_code_line, parse_indented_code_block};
use latex_envs::{parse_latex_environment, try_parse_latex_env_begin};
use lists::{ListMarker, emit_list_item, markers_match, try_parse_list_marker};
use metadata::{try_parse_pandoc_title_block, try_parse_yaml_block};
use tables::{
    is_caption_followed_by_table, try_parse_grid_table, try_parse_multiline_table,
    try_parse_pipe_table, try_parse_simple_table,
};

fn init_logger() {
    let _ = env_logger::builder().is_test(true).try_init();
}

pub struct BlockParser<'a> {
    lines: Vec<&'a str>,
    pos: usize,
    builder: GreenNodeBuilder<'static>,
    containers: ContainerStack,
    #[allow(dead_code)] // TODO: Will be used for extension configuration
    config: &'a Config,
}

impl<'a> BlockParser<'a> {
    pub fn new(input: &'a str, config: &'a Config) -> Self {
        let lines: Vec<&str> = input.lines().collect();
        Self {
            lines,
            pos: 0,
            builder: GreenNodeBuilder::new(),
            containers: ContainerStack::new(),
            config,
        }
    }

    pub fn parse(mut self) -> SyntaxNode {
        #[cfg(debug_assertions)]
        {
            init_logger();
        }

        self.builder.start_node(SyntaxKind::ROOT.into());
        self.parse_document_stack();
        self.builder.finish_node(); // ROOT

        SyntaxNode::new_root(self.builder.finish())
    }

    fn parse_document_stack(&mut self) {
        self.builder.start_node(SyntaxKind::DOCUMENT.into());

        log::debug!("Starting document parse");

        // Check for Pandoc title block at document start
        if self.pos == 0
            && !self.lines.is_empty()
            && let Some(new_pos) = try_parse_pandoc_title_block(&self.lines, 0, &mut self.builder)
        {
            self.pos = new_pos;
        }

        while self.pos < self.lines.len() {
            let line = self.lines[self.pos];

            log::debug!("Parsing line {}: {}", self.pos + 1, line);

            if self.parse_line(line) {
                continue;
            }
            self.pos += 1;
        }

        self.containers.close_to(0, &mut self.builder);
        self.builder.finish_node(); // DOCUMENT
    }

    /// Check if we need a blank line before starting a new blockquote.
    /// Returns true if a blockquote can start here.
    fn can_start_blockquote(&self) -> bool {
        // At start of document, no blank line needed
        if self.pos == 0 {
            return true;
        }
        // After a blank line, can start blockquote
        if self.pos > 0 && self.lines[self.pos - 1].trim().is_empty() {
            return true;
        }
        // If we're already in a blockquote, nested blockquotes need blank line too
        // (blank_before_blockquote extension)
        false
    }

    /// Get the current blockquote depth from the container stack.
    fn current_blockquote_depth(&self) -> usize {
        self.containers
            .stack
            .iter()
            .filter(|c| matches!(c, Container::BlockQuote { .. }))
            .count()
    }

    /// Strip exactly n blockquote markers from a line, returning the rest.
    fn strip_n_blockquote_markers<'b>(&self, line: &'b str, n: usize) -> &'b str {
        use blockquotes::try_parse_blockquote_marker;
        let mut remaining = line;
        for _ in 0..n {
            if let Some((_, content_start)) = try_parse_blockquote_marker(remaining) {
                remaining = &remaining[content_start..];
            } else {
                break;
            }
        }
        remaining
    }

    /// Close blockquotes down to a target depth.
    fn close_blockquotes_to_depth(&mut self, target_depth: usize) {
        let mut current = self.current_blockquote_depth();
        while current > target_depth {
            // Close everything until we hit a blockquote, then close it
            while !matches!(self.containers.last(), Some(Container::BlockQuote { .. })) {
                if self.containers.depth() == 0 {
                    break;
                }
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
            }
            if matches!(self.containers.last(), Some(Container::BlockQuote { .. })) {
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
                current -= 1;
            } else {
                break;
            }
        }
    }

    /// Returns true if the line was consumed.
    fn parse_line(&mut self, line: &str) -> bool {
        // Count blockquote markers on this line
        let (bq_depth, inner_content) = count_blockquote_markers(line);
        let current_bq_depth = self.current_blockquote_depth();

        // Handle blank lines specially (including blank lines inside blockquotes)
        // A line like ">" with nothing after is a blank line inside a blockquote
        let is_blank = line.trim().is_empty() || (bq_depth > 0 && inner_content.trim().is_empty());

        if is_blank {
            // Close paragraph if open
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
            }

            // Skip blank lines between terms and definitions in definition lists
            if self.in_definition_item() && self.next_line_is_definition_marker() {
                self.pos += 1;
                return true;
            }

            // For blank lines inside blockquotes, we need to handle them at the right depth
            // First, adjust blockquote depth if needed
            if bq_depth > current_bq_depth {
                // Open blockquotes
                for _ in current_bq_depth..bq_depth {
                    self.builder.start_node(SyntaxKind::BlockQuote.into());
                    self.containers
                        .push(Container::BlockQuote { content_col: 0 });
                }
            } else if bq_depth < current_bq_depth {
                // Close blockquotes down to bq_depth
                self.close_blockquotes_to_depth(bq_depth);
            }

            // Peek ahead to determine what containers to keep open
            let mut peek = self.pos + 1;
            while peek < self.lines.len() && self.lines[peek].trim().is_empty() {
                peek += 1;
            }

            // Determine what containers to keep open based on next line
            let levels_to_keep = if peek < self.lines.len() {
                self.compute_levels_to_keep(self.lines[peek])
            } else {
                0
            };

            // Close containers down to the level we want to keep
            while self.containers.depth() > levels_to_keep {
                match self.containers.last() {
                    Some(Container::ListItem { .. }) | Some(Container::List { .. }) => {
                        log::trace!(
                            "Closing list container at blank line (depth {})",
                            self.containers.depth()
                        );
                        self.containers
                            .close_to(self.containers.depth() - 1, &mut self.builder);
                    }
                    _ => break,
                }
            }

            self.builder.start_node(SyntaxKind::BlankLine.into());
            self.builder
                .token(SyntaxKind::BlankLine.into(), inner_content);
            self.builder.finish_node();
            self.pos += 1;
            return true;
        }

        // Handle blockquote depth changes
        if bq_depth > current_bq_depth {
            // Need to open new blockquote(s)
            // But first check blank_before_blockquote requirement
            if current_bq_depth == 0 && !self.can_start_blockquote() {
                // Can't start blockquote without blank line - treat as paragraph
                self.start_paragraph_if_needed();
                self.append_paragraph_line(line);
                self.pos += 1;
                return true;
            }

            // For nested blockquotes, also need blank line before (blank_before_blockquote)
            // Check if previous line inside the blockquote was blank
            let can_nest = if current_bq_depth > 0 {
                // Check if we're right after a blank line or at start of blockquote
                matches!(self.containers.last(), Some(Container::BlockQuote { .. }))
                    || (self.pos > 0 && {
                        let prev_line = self.lines[self.pos - 1];
                        let (prev_bq_depth, prev_inner) = count_blockquote_markers(prev_line);
                        prev_bq_depth >= current_bq_depth && prev_inner.trim().is_empty()
                    })
            } else {
                true
            };

            if !can_nest {
                // Can't nest deeper - treat extra > as content
                // Only strip markers up to current depth
                let content_at_current_depth =
                    self.strip_n_blockquote_markers(line, current_bq_depth);

                if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                    // Lazy continuation with the extra > as content
                    self.append_paragraph_line(content_at_current_depth);
                    self.pos += 1;
                    return true;
                } else {
                    // Start new paragraph with the extra > as content
                    self.start_paragraph_if_needed();
                    self.append_paragraph_line(content_at_current_depth);
                    self.pos += 1;
                    return true;
                }
            }

            // Close paragraph before opening blockquote
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
            }

            // Open blockquotes up to the required depth
            for _ in current_bq_depth..bq_depth {
                self.builder.start_node(SyntaxKind::BlockQuote.into());
                self.containers
                    .push(Container::BlockQuote { content_col: 0 });
            }

            // Now parse the inner content
            return self.parse_inner_content(inner_content);
        } else if bq_depth < current_bq_depth {
            // Need to close some blockquotes, but first check for lazy continuation
            // Lazy continuation: line without > continues content in a blockquote
            if bq_depth == 0 {
                // Check for lazy paragraph continuation
                if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                    self.append_paragraph_line(line);
                    self.pos += 1;
                    return true;
                }

                // Check for lazy list continuation - if we're in a list item and
                // this line looks like a list item with matching marker
                if self.in_blockquote_list()
                    && let Some((marker, marker_len, spaces_after)) = try_parse_list_marker(line)
                {
                    let (indent_cols, indent_bytes) = leading_indent(line);
                    if let Some(level) = self.find_matching_list_level(&marker, indent_cols) {
                        // Continue the list inside the blockquote
                        self.continue_list_at_level(level);
                        self.add_list_item(
                            line,
                            marker_len,
                            spaces_after,
                            indent_cols,
                            indent_bytes,
                        );
                        self.pos += 1;
                        return true;
                    }
                }
            }

            // Not lazy continuation - close paragraph if open
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
            }

            // Close blockquotes down to the new depth
            self.close_blockquotes_to_depth(bq_depth);

            // Parse the inner content at the new depth
            if bq_depth > 0 {
                return self.parse_inner_content(inner_content);
            } else {
                return self.parse_inner_content(line);
            }
        } else if bq_depth > 0 {
            // Same blockquote depth - continue parsing inner content
            return self.parse_inner_content(inner_content);
        }

        // No blockquote markers - parse as regular content
        // But check for lazy continuation first
        if current_bq_depth > 0 {
            // Check for lazy paragraph continuation
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.append_paragraph_line(line);
                self.pos += 1;
                return true;
            }

            // Check for lazy list continuation
            if self.in_blockquote_list()
                && let Some((marker, marker_len, spaces_after)) = try_parse_list_marker(line)
            {
                let (indent_cols, indent_bytes) = leading_indent(line);
                if let Some(level) = self.find_matching_list_level(&marker, indent_cols) {
                    self.continue_list_at_level(level);
                    self.add_list_item(line, marker_len, spaces_after, indent_cols, indent_bytes);
                    self.pos += 1;
                    return true;
                }
            }
        }

        self.parse_inner_content(line)
    }

    /// Compute how many container levels to keep open based on next line content.
    fn compute_levels_to_keep(&self, next_line: &str) -> usize {
        let (next_bq_depth, next_inner) = count_blockquote_markers(next_line);
        let (next_indent_cols, _) = leading_indent(next_inner);
        let next_marker = try_parse_list_marker(next_inner);

        let mut keep_level = 0;

        // First, account for blockquotes
        for (i, c) in self.containers.stack.iter().enumerate() {
            match c {
                Container::BlockQuote { .. } => {
                    // Count blockquotes up to this point
                    let bq_count = self.containers.stack[..=i]
                        .iter()
                        .filter(|x| matches!(x, Container::BlockQuote { .. }))
                        .count();
                    if bq_count <= next_bq_depth {
                        keep_level = i + 1;
                    }
                }
                Container::List {
                    marker,
                    base_indent_cols,
                } => {
                    let continues_list = if let Some((ref nm, _, _)) = next_marker {
                        markers_match(marker, nm) && next_indent_cols <= base_indent_cols + 3
                    } else {
                        let item_content_col = self
                            .containers
                            .stack
                            .get(i + 1)
                            .and_then(|c| match c {
                                Container::ListItem { content_col } => Some(*content_col),
                                _ => None,
                            })
                            .unwrap_or(0);
                        next_indent_cols >= item_content_col
                    };
                    if continues_list {
                        keep_level = i + 1;
                    }
                }
                _ => {}
            }
        }

        keep_level
    }

    /// Parse content inside blockquotes (or at top level).
    fn parse_inner_content(&mut self, content: &str) -> bool {
        // Check for heading (needs blank line before, or at start of container)
        let has_blank_before = self.pos == 0
            || self.lines[self.pos - 1].trim().is_empty()
            || matches!(self.containers.last(), Some(Container::BlockQuote { .. }));

        // For indented code blocks, we need a stricter condition - only actual blank lines count
        // Being at document start (pos == 0) is OK only if we're not inside a blockquote
        let at_document_start = self.pos == 0 && self.current_blockquote_depth() == 0;
        let prev_line_blank = if self.pos > 0 {
            let prev_line = self.lines[self.pos - 1];
            let (prev_bq_depth, prev_inner) = count_blockquote_markers(prev_line);
            prev_line.trim().is_empty() || (prev_bq_depth > 0 && prev_inner.trim().is_empty())
        } else {
            false
        };
        let has_blank_before_strict = at_document_start || prev_line_blank;

        // At top level only (not inside blockquotes), check for YAML metadata
        if self.current_blockquote_depth() == 0 && content.trim() == "---" {
            let at_document_start = self.pos == 0;
            if let Some(new_pos) =
                try_parse_yaml_block(&self.lines, self.pos, &mut self.builder, at_document_start)
            {
                self.pos = new_pos;
                return true;
            }
        }

        // Check for HTML block (if raw_html extension is enabled)
        if self.config.extensions.raw_html
            && let Some(block_type) = try_parse_html_block_start(content)
        {
            log::debug!("Parsed HTML block at line {}: {:?}", self.pos, block_type);
            // Close paragraph before opening HTML block
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
            }

            let bq_depth = self.current_blockquote_depth();
            let new_pos = parse_html_block(
                &mut self.builder,
                &self.lines,
                self.pos,
                block_type,
                bq_depth,
            );
            self.pos = new_pos;
            return true;
        }

        if has_blank_before {
            // Try to parse grid table (check before pipe/simple since + is most specific)
            if let Some(lines_consumed) =
                try_parse_grid_table(&self.lines, self.pos, &mut self.builder)
            {
                log::debug!(
                    "Parsed grid table at line {} ({} lines)",
                    self.pos,
                    lines_consumed
                );
                self.pos += lines_consumed;
                return true;
            }

            // Try to parse multiline table (check before pipe/simple since full-width dashes are specific)
            if let Some(lines_consumed) =
                try_parse_multiline_table(&self.lines, self.pos, &mut self.builder)
            {
                log::debug!(
                    "Parsed multiline table at line {} ({} lines)",
                    self.pos,
                    lines_consumed
                );
                self.pos += lines_consumed;
                return true;
            }

            // Try to parse pipe table (check before simple table since pipes are more specific)
            if let Some(lines_consumed) =
                try_parse_pipe_table(&self.lines, self.pos, &mut self.builder)
            {
                log::debug!(
                    "Parsed pipe table at line {} ({} lines)",
                    self.pos,
                    lines_consumed
                );
                self.pos += lines_consumed;
                return true;
            }

            // Try to parse simple table
            if let Some(lines_consumed) =
                try_parse_simple_table(&self.lines, self.pos, &mut self.builder)
            {
                log::debug!(
                    "Parsed simple table at line {} ({} lines)",
                    self.pos,
                    lines_consumed
                );
                self.pos += lines_consumed;
                return true;
            }

            // Try to parse horizontal rule (but only if not YAML)
            if try_parse_horizontal_rule(content).is_some() {
                log::debug!("Parsed horizontal rule at line {}", self.pos);
                emit_horizontal_rule(&mut self.builder, content);
                self.pos += 1;
                return true;
            }

            // Try to parse ATX heading from stripped content
            if let Some(heading_level) = try_parse_atx_heading(content) {
                log::debug!(
                    "Parsed ATX heading at line {}: level {}",
                    self.pos,
                    heading_level
                );
                emit_atx_heading(&mut self.builder, content, heading_level);
                self.pos += 1;
                return true;
            }
        }

        // Check for fenced code block
        if has_blank_before && let Some(fence) = try_parse_fence_open(content) {
            let bq_depth = self.current_blockquote_depth();
            log::debug!(
                "Parsed fenced code block at line {}: {} fence",
                self.pos,
                fence.fence_char
            );
            let new_pos =
                parse_fenced_code_block(&mut self.builder, &self.lines, self.pos, fence, bq_depth);
            self.pos = new_pos;
            return true;
        }

        // Check for indented code block (must have actual blank line before)
        if has_blank_before_strict && is_indented_code_line(content) {
            let bq_depth = self.current_blockquote_depth();
            log::debug!("Parsed indented code block at line {}", self.pos);
            let new_pos =
                parse_indented_code_block(&mut self.builder, &self.lines, self.pos, bq_depth);
            self.pos = new_pos;
            return true;
        }

        // Check for display math block
        // Close paragraph first if one is open, then parse as MathBlock
        if let Some(math_fence) = try_parse_math_fence_open(content) {
            log::debug!(
                "Parsed display math block at line {}: {} dollars",
                self.pos,
                math_fence.fence_count
            );
            // Close paragraph before opening display math block
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
            }

            let bq_depth = self.current_blockquote_depth();
            let new_pos = parse_display_math_block(
                &mut self.builder,
                &self.lines,
                self.pos,
                math_fence,
                bq_depth,
            );
            self.pos = new_pos;
            return true;
        }

        // Check for fenced div opening
        if has_blank_before && let Some(div_fence) = try_parse_div_fence_open(content) {
            log::debug!(
                "Parsed fenced div at line {}: {} colons",
                self.pos,
                div_fence.fence_count
            );
            // Close paragraph before opening fenced div
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
            }

            // Start FencedDiv node
            self.builder.start_node(SyntaxKind::FencedDiv.into());

            // Emit opening fence
            self.builder.start_node(SyntaxKind::DivFenceOpen.into());
            self.builder.token(SyntaxKind::TEXT.into(), content);
            self.builder.token(SyntaxKind::NEWLINE.into(), "\n");
            self.builder.finish_node(); // DivFenceOpen

            // Store attributes as DivInfo
            self.builder.start_node(SyntaxKind::DivInfo.into());
            self.builder
                .token(SyntaxKind::TEXT.into(), &div_fence.attributes);
            self.builder.finish_node(); // DivInfo

            // Push FencedDiv container
            self.containers.push(Container::FencedDiv {});

            self.pos += 1;
            return true;
        }

        // Check for fenced div closing
        if self.in_fenced_div() && is_div_closing_fence(content) {
            // Close paragraph before closing fenced div
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
            }

            // Emit closing fence
            self.builder.start_node(SyntaxKind::DivFenceClose.into());
            self.builder.token(SyntaxKind::TEXT.into(), content);
            self.builder.token(SyntaxKind::NEWLINE.into(), "\n");
            self.builder.finish_node(); // DivFenceClose

            // Pop the FencedDiv container (this will finish the FencedDiv node)
            self.containers
                .close_to(self.containers.depth() - 1, &mut self.builder);

            self.pos += 1;
            return true;
        }

        // Check for LaTeX environment (if raw_tex extension is enabled)
        if self.config.extensions.raw_tex
            && let Some(env_info) = try_parse_latex_env_begin(content)
        {
            log::debug!(
                "Parsed LaTeX environment at line {}: \\begin{{{}}}",
                self.pos,
                env_info.env_name
            );
            // Close paragraph before opening LaTeX environment
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
            }

            let bq_depth = self.current_blockquote_depth();
            let new_pos = parse_latex_environment(
                &mut self.builder,
                &self.lines,
                self.pos,
                env_info,
                bq_depth,
            );
            self.pos = new_pos;
            return true;
        }

        // List marker?
        if let Some((marker, marker_len, spaces_after)) = try_parse_list_marker(content) {
            let (indent_cols, indent_bytes) = leading_indent(content);
            if indent_cols >= 4 && !self.in_list() {
                // Code block at top-level, treat as paragraph
                self.start_paragraph_if_needed();
                self.append_paragraph_line(content);
                self.pos += 1;
                return true;
            }

            // Close paragraph before list item
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
            }

            // Nested list inside current item if indented to content column or beyond
            let current_content_col = self.current_content_col();
            if current_content_col > 0 && indent_cols >= current_content_col {
                self.start_nested_list(
                    content,
                    &marker,
                    marker_len,
                    spaces_after,
                    indent_cols,
                    indent_bytes,
                );
                self.pos += 1;
                return true;
            }

            // Find matching list level
            let matched_level = self.find_matching_list_level(&marker, indent_cols);

            if let Some(level) = matched_level {
                self.continue_list_at_level(level);
            } else {
                self.start_new_list(&marker, indent_cols);
            }

            // Start list item
            self.add_list_item(content, marker_len, spaces_after, indent_cols, indent_bytes);
            self.pos += 1;
            return true;
        }

        // Definition list marker?
        if let Some((marker_char, indent, spaces_after)) = try_parse_definition_marker(content) {
            // Check if this is actually a table caption, not a definition marker
            if is_caption_followed_by_table(&self.lines, self.pos) {
                // Don't parse as definition - let table parser handle it
                return false;
            }

            // Close paragraph before starting definition
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
            }

            // Start definition list if not in one
            if !self.in_definition_list() {
                self.builder.start_node(SyntaxKind::DefinitionList.into());
                self.containers.push(Container::DefinitionList {});
            }

            // Close previous definition if one is open (but keep DefinitionItem open)
            if matches!(self.containers.last(), Some(Container::Definition { .. })) {
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
            }

            // Start new definition item if not in one
            if !matches!(
                self.containers.last(),
                Some(Container::DefinitionItem { .. })
            ) {
                self.builder.start_node(SyntaxKind::DefinitionItem.into());
                self.containers.push(Container::DefinitionItem {
                    in_definition: true,
                });
            }

            // Start Definition node
            self.builder.start_node(SyntaxKind::Definition.into());
            emit_definition_marker(&mut self.builder, marker_char, indent);
            if spaces_after > 0 {
                self.builder
                    .token(SyntaxKind::WHITESPACE.into(), &" ".repeat(spaces_after));
            }

            // Calculate content column (marker + spaces)
            let content_col = indent + 1 + spaces_after;

            // Emit remaining content on this line if any
            let after_marker_and_spaces = &content[indent + 1 + spaces_after..];
            if !after_marker_and_spaces.trim().is_empty() {
                self.builder
                    .token(SyntaxKind::TEXT.into(), after_marker_and_spaces.trim_end());
            }
            self.builder.token(SyntaxKind::NEWLINE.into(), "\n");

            self.containers.push(Container::Definition { content_col });
            self.pos += 1;
            return true;
        }

        // Term line (if next line has definition marker)?
        if !content.trim().is_empty() && self.next_line_is_definition_marker() {
            // Close any open structures
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
            }

            // Start definition list if not in one
            if !self.in_definition_list() {
                self.builder.start_node(SyntaxKind::DefinitionList.into());
                self.containers.push(Container::DefinitionList {});
            }

            // Close previous definition item if exists
            while matches!(
                self.containers.last(),
                Some(Container::Definition { .. }) | Some(Container::DefinitionItem { .. })
            ) {
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
            }

            // Start new definition item
            self.builder.start_node(SyntaxKind::DefinitionItem.into());
            self.containers.push(Container::DefinitionItem {
                in_definition: false,
            });

            // Emit term
            emit_term(&mut self.builder, content);
            self.pos += 1;
            return true;
        }

        // Check if this is a table caption followed by a table
        // If so, don't parse as paragraph - let table parser handle it
        if is_caption_followed_by_table(&self.lines, self.pos) {
            // Don't parse as paragraph - this will be consumed by table parser
            return false;
        }

        // Paragraph
        self.start_paragraph_if_needed();
        self.append_paragraph_line(content);
        self.pos += 1;
        true
    }

    fn in_list(&self) -> bool {
        self.containers
            .stack
            .iter()
            .any(|c| matches!(c, Container::List { .. }))
    }

    fn in_fenced_div(&self) -> bool {
        self.containers
            .stack
            .iter()
            .any(|c| matches!(c, Container::FencedDiv { .. }))
    }

    /// Check if we're in a list inside a blockquote.
    fn in_blockquote_list(&self) -> bool {
        let mut seen_blockquote = false;
        for c in &self.containers.stack {
            if matches!(c, Container::BlockQuote { .. }) {
                seen_blockquote = true;
            }
            if seen_blockquote && matches!(c, Container::List { .. }) {
                return true;
            }
        }
        false
    }

    fn find_matching_list_level(&self, marker: &ListMarker, indent_cols: usize) -> Option<usize> {
        for (i, c) in self.containers.stack.iter().enumerate() {
            if let Container::List {
                marker: list_marker,
                base_indent_cols,
            } = c
                && markers_match(marker, list_marker)
                && indent_cols <= base_indent_cols + 3
            {
                return Some(i);
            }
        }
        None
    }

    fn in_definition_list(&self) -> bool {
        self.containers
            .stack
            .iter()
            .any(|c| matches!(c, Container::DefinitionList { .. }))
    }

    fn in_definition_item(&self) -> bool {
        self.containers
            .stack
            .iter()
            .any(|c| matches!(c, Container::DefinitionItem { .. }))
    }

    fn next_line_is_definition_marker(&self) -> bool {
        // Look ahead past blank lines to find a definition marker
        let mut check_pos = self.pos + 1;
        while check_pos < self.lines.len() {
            let line = self.lines[check_pos];
            if line.trim().is_empty() {
                check_pos += 1;
                continue;
            }
            return try_parse_definition_marker(line).is_some();
        }
        false
    }

    fn continue_list_at_level(&mut self, level: usize) {
        self.containers.close_to(level + 1, &mut self.builder);
        if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
            self.containers
                .close_to(self.containers.depth() - 1, &mut self.builder);
        }
        if matches!(self.containers.last(), Some(Container::ListItem { .. })) {
            self.containers
                .close_to(self.containers.depth() - 1, &mut self.builder);
        }
    }

    fn start_new_list(&mut self, marker: &ListMarker, indent_cols: usize) {
        if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
            self.containers
                .close_to(self.containers.depth() - 1, &mut self.builder);
        }
        while matches!(self.containers.last(), Some(Container::ListItem { .. })) {
            self.containers
                .close_to(self.containers.depth() - 1, &mut self.builder);
        }
        while matches!(self.containers.last(), Some(Container::List { .. })) {
            self.containers
                .close_to(self.containers.depth() - 1, &mut self.builder);
        }
        self.builder.start_node(SyntaxKind::List.into());
        self.containers.push(Container::List {
            marker: marker.clone(),
            base_indent_cols: indent_cols,
        });
    }

    fn start_nested_list(
        &mut self,
        content: &str,
        marker: &ListMarker,
        marker_len: usize,
        spaces_after: usize,
        indent_cols: usize,
        indent_bytes: usize,
    ) {
        if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
            self.containers
                .close_to(self.containers.depth() - 1, &mut self.builder);
        }
        self.builder.start_node(SyntaxKind::List.into());
        self.containers.push(Container::List {
            marker: marker.clone(),
            base_indent_cols: indent_cols,
        });
        let content_col = emit_list_item(
            &mut self.builder,
            content,
            marker_len,
            spaces_after,
            indent_cols,
            indent_bytes,
        );
        self.containers.push(Container::ListItem { content_col });
    }

    fn add_list_item(
        &mut self,
        content: &str,
        marker_len: usize,
        spaces_after: usize,
        indent_cols: usize,
        indent_bytes: usize,
    ) {
        let content_col = emit_list_item(
            &mut self.builder,
            content,
            marker_len,
            spaces_after,
            indent_cols,
            indent_bytes,
        );
        self.containers.push(Container::ListItem { content_col });
    }

    fn start_paragraph_if_needed(&mut self) {
        if !matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
            let content_col = self.current_content_col();
            self.builder.start_node(SyntaxKind::PARAGRAPH.into());
            self.containers.push(Container::Paragraph { content_col });
        }
    }

    fn append_paragraph_line(&mut self, line: &str) {
        let text = self.strip_to_content_col(line);
        self.builder.token(SyntaxKind::TEXT.into(), text);
        self.builder.token(SyntaxKind::NEWLINE.into(), "\n");
    }

    fn current_content_col(&self) -> usize {
        self.containers
            .stack
            .iter()
            .rev()
            .find_map(|c| match c {
                Container::ListItem { content_col } => Some(*content_col),
                _ => None,
            })
            .unwrap_or(0)
    }

    fn strip_to_content_col<'b>(&self, line: &'b str) -> &'b str {
        let target = self.current_content_col();
        if target == 0 {
            return line;
        }
        let (indent_cols, _) = leading_indent(line);
        if indent_cols >= target {
            let idx = byte_index_at_column(line, target);
            &line[idx..]
        } else {
            line.trim_start()
        }
    }
}

#[cfg(test)]
mod tests {
    mod blanklines;
    mod blockquotes;
    mod code_blocks;
    mod display_math;
    mod headings;
    mod helpers;
    mod lists;
}
