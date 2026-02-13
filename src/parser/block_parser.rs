use crate::config::Config;
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::GreenNodeBuilder;

pub mod attributes; // Public for use in inline_parser
mod blockquotes;
pub mod chunk_options; // Public for hashpipe formatter
pub mod code_blocks; // Public for formatter access to InfoString and CodeBlockType
mod container_stack;
mod definition_lists;
mod display_math;
mod fenced_divs;
mod headings;
mod horizontal_rules;
mod html_blocks;
mod indented_code;
mod latex_envs;
mod line_blocks;
mod lists;
mod metadata;
mod paragraphs;
pub mod reference_definitions; // Public for use in inline_parser
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
use line_blocks::{parse_line_block, try_parse_line_block_start};
use lists::{ListMarker, emit_list_item, markers_match, try_parse_list_marker};
use metadata::{try_parse_pandoc_title_block, try_parse_yaml_block};
pub use reference_definitions::{
    ReferenceRegistry, try_parse_footnote_definition, try_parse_footnote_marker,
    try_parse_reference_definition,
};
use tables::{
    is_caption_followed_by_table, try_parse_grid_table, try_parse_multiline_table,
    try_parse_pipe_table, try_parse_simple_table,
};
use utils::{split_lines_inclusive, strip_newline};

fn init_logger() {
    let _ = env_logger::builder().is_test(true).try_init();
}

pub struct BlockParser<'a> {
    lines: Vec<&'a str>,
    pos: usize,
    builder: GreenNodeBuilder<'static>,
    containers: ContainerStack,
    reference_registry: ReferenceRegistry,
    #[allow(dead_code)] // TODO: Will be used for extension configuration
    config: &'a Config,
}

impl<'a> BlockParser<'a> {
    pub fn new(input: &'a str, config: &'a Config) -> Self {
        // Use split_lines_inclusive to preserve line endings (both LF and CRLF)
        let lines = split_lines_inclusive(input);
        Self {
            lines,
            pos: 0,
            builder: GreenNodeBuilder::new(),
            containers: ContainerStack::new(),
            reference_registry: ReferenceRegistry::new(),
            config,
        }
    }

    pub fn parse(mut self) -> (SyntaxNode, ReferenceRegistry) {
        #[cfg(debug_assertions)]
        {
            init_logger();
        }

        self.builder.start_node(SyntaxKind::ROOT.into());
        self.parse_document_stack();
        self.builder.finish_node(); // ROOT

        let tree = SyntaxNode::new_root(self.builder.finish());
        (tree, self.reference_registry)
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

    /// Parse blockquote markers and return their positions.
    /// Returns Vec of (leading_spaces, has_trailing_space) for each marker found.
    fn parse_blockquote_marker_info(line: &str) -> Vec<(usize, bool)> {
        let mut markers = Vec::new();
        let mut remaining = line;

        loop {
            let bytes = remaining.as_bytes();
            let mut i = 0;

            // Count leading whitespace (up to 3 spaces before >)
            let mut spaces = 0;
            while i < bytes.len() && bytes[i] == b' ' && spaces < 3 {
                spaces += 1;
                i += 1;
            }

            // Check if there's a > marker
            if i >= bytes.len() || bytes[i] != b'>' {
                break;
            }
            i += 1; // skip '>'

            // Check for optional space after >
            let has_trailing_space = i < bytes.len() && bytes[i] == b' ';
            if has_trailing_space {
                i += 1;
            }

            markers.push((spaces, has_trailing_space));
            remaining = &remaining[i..];
        }

        markers
    }

    /// Emit one blockquote marker with its whitespace.
    fn emit_one_blockquote_marker(&mut self, leading_spaces: usize, has_trailing_space: bool) {
        if leading_spaces > 0 {
            self.builder
                .token(SyntaxKind::WHITESPACE.into(), &" ".repeat(leading_spaces));
        }
        self.builder.token(SyntaxKind::BlockQuoteMarker.into(), ">");
        if has_trailing_space {
            self.builder.token(SyntaxKind::WHITESPACE.into(), " ");
        }
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
        // Note: lines may end with \n from split_inclusive
        let is_blank = line.trim_end_matches('\n').trim().is_empty()
            || (bq_depth > 0 && inner_content.trim_end_matches('\n').trim().is_empty());

        if is_blank {
            // Close paragraph if open
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
            }

            // Note: Blank lines between terms and definitions are now preserved
            // and emitted as part of the term parsing logic

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
            log::trace!(
                "Blank line: depth={}, levels_to_keep={}, next='{}'",
                self.containers.depth(),
                levels_to_keep,
                if peek < self.lines.len() {
                    self.lines[peek]
                } else {
                    "<EOF>"
                }
            );

            // Close containers down to the level we want to keep
            while self.containers.depth() > levels_to_keep {
                match self.containers.last() {
                    Some(Container::ListItem { .. })
                    | Some(Container::List { .. })
                    | Some(Container::FootnoteDefinition { .. })
                    | Some(Container::Paragraph { .. })
                    | Some(Container::Definition { .. })
                    | Some(Container::DefinitionItem { .. })
                    | Some(Container::DefinitionList { .. }) => {
                        log::trace!(
                            "Closing container at blank line (depth {})",
                            self.containers.depth()
                        );
                        self.containers
                            .close_to(self.containers.depth() - 1, &mut self.builder);
                    }
                    _ => break,
                }
            }

            // Emit blockquote markers for this blank line if inside blockquotes
            if bq_depth > 0 {
                let marker_info = Self::parse_blockquote_marker_info(line);
                for i in 0..bq_depth {
                    if let Some(&(leading_spaces, has_trailing_space)) = marker_info.get(i) {
                        self.emit_one_blockquote_marker(leading_spaces, has_trailing_space);
                    }
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

                // Emit blockquote markers for current depth (for losslessness)
                let marker_info = Self::parse_blockquote_marker_info(line);
                for i in 0..current_bq_depth {
                    if let Some(&(leading_spaces, has_trailing_space)) = marker_info.get(i) {
                        self.emit_one_blockquote_marker(leading_spaces, has_trailing_space);
                    }
                }

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

            // Parse marker information for all levels
            let marker_info = Self::parse_blockquote_marker_info(line);

            // First, emit markers for existing blockquote levels (before opening new ones)
            for level in 0..current_bq_depth {
                if let Some(&(leading_spaces, has_trailing_space)) = marker_info.get(level) {
                    self.emit_one_blockquote_marker(leading_spaces, has_trailing_space);
                }
            }

            // Then open new blockquotes and emit their markers
            for level in current_bq_depth..bq_depth {
                self.builder.start_node(SyntaxKind::BlockQuote.into());

                // Emit the marker for this new level
                if let Some(&(leading_spaces, has_trailing_space)) = marker_info.get(level) {
                    self.emit_one_blockquote_marker(leading_spaces, has_trailing_space);
                }

                self.containers
                    .push(Container::BlockQuote { content_col: 0 });
            }

            // Now parse the inner content
            // Pass inner_content as line_to_append since markers are already stripped
            return self.parse_inner_content(inner_content, Some(inner_content));
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
                    && let Some((marker, marker_len, spaces_after)) =
                        try_parse_list_marker(line, self.config)
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
                // Emit markers at current depth before parsing content
                let marker_info = Self::parse_blockquote_marker_info(line);
                for i in 0..bq_depth {
                    if let Some(&(leading_spaces, has_trailing_space)) = marker_info.get(i) {
                        self.emit_one_blockquote_marker(leading_spaces, has_trailing_space);
                    }
                }
                // Content with markers stripped - use inner_content for paragraph appending
                return self.parse_inner_content(inner_content, Some(inner_content));
            } else {
                // Not inside blockquotes - use original line
                return self.parse_inner_content(line, None);
            }
        } else if bq_depth > 0 {
            // Same blockquote depth - emit markers and continue parsing inner content

            // Before emitting markers for a new line, close any line-level containers
            // (ListItems should be closed when we see the next line)
            if matches!(self.containers.last(), Some(Container::ListItem { .. })) {
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
            }

            let marker_info = Self::parse_blockquote_marker_info(line);
            for i in 0..bq_depth {
                if let Some(&(leading_spaces, has_trailing_space)) = marker_info.get(i) {
                    self.emit_one_blockquote_marker(leading_spaces, has_trailing_space);
                }
            }
            // Same blockquote depth - markers stripped, use inner_content for appending
            return self.parse_inner_content(inner_content, Some(inner_content));
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
                && let Some((marker, marker_len, spaces_after)) =
                    try_parse_list_marker(line, self.config)
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

        // No blockquote markers - use original line
        self.parse_inner_content(line, None)
    }

    /// Compute how many container levels to keep open based on next line content.
    fn compute_levels_to_keep(&self, next_line: &str) -> usize {
        let (next_bq_depth, next_inner) = count_blockquote_markers(next_line);
        let (raw_indent_cols, _) = leading_indent(next_inner);
        let next_marker = try_parse_list_marker(next_inner, self.config);

        let mut keep_level = 0;
        let mut content_indent_so_far = 0usize;

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
                Container::FootnoteDefinition { content_col, .. } => {
                    // Track footnote indent for nested containers
                    content_indent_so_far += *content_col;
                    // Footnote continuation: line must be indented at least 4 spaces
                    // (or at the content column if content started after marker)
                    let min_indent = (*content_col).max(4);
                    if raw_indent_cols >= min_indent {
                        keep_level = i + 1;
                    }
                }
                Container::Definition { content_col } => {
                    // Definition continuation: line must be indented at least 4 spaces
                    let min_indent = (*content_col).max(4);
                    if raw_indent_cols >= min_indent {
                        keep_level = i + 1;
                    }
                }
                Container::List {
                    marker,
                    base_indent_cols,
                } => {
                    // Adjust indent for footnote context
                    let effective_indent = raw_indent_cols.saturating_sub(content_indent_so_far);
                    let continues_list = if let Some((ref nm, _, _)) = next_marker {
                        markers_match(marker, nm) && effective_indent <= base_indent_cols + 3
                    } else {
                        // For non-list-marker lines, must be indented past list content
                        let item_content_col = self
                            .containers
                            .stack
                            .get(i + 1)
                            .and_then(|c| match c {
                                Container::ListItem { content_col } => Some(*content_col),
                                _ => None,
                            })
                            // If no list item, require at least 1 space indent to continue list
                            .unwrap_or(1);
                        effective_indent >= item_content_col
                    };
                    if continues_list {
                        keep_level = i + 1;
                    }
                }
                Container::ListItem { content_col } => {
                    // Keep list item if next line is indented to content column
                    // BUT NOT if it's a new list item marker at an outer level
                    let effective_indent = raw_indent_cols.saturating_sub(content_indent_so_far);
                    let is_new_item_at_outer_level = if let Some((ref _nm, _, _)) = next_marker {
                        // Check if this marker would start a sibling item (at parent list level)
                        // by checking if it's at or before the current item's start
                        effective_indent < *content_col
                    } else {
                        false
                    };

                    if !is_new_item_at_outer_level && effective_indent >= *content_col {
                        keep_level = i + 1;
                    }
                }
                _ => {}
            }
        }

        keep_level
    }

    /// Get the total indentation to strip from content containers (footnotes + definitions).
    fn content_container_indent_to_strip(&self) -> usize {
        self.containers
            .stack
            .iter()
            .filter_map(|c| match c {
                Container::FootnoteDefinition { content_col, .. } => Some(*content_col),
                Container::Definition { content_col } => Some(*content_col),
                _ => None,
            })
            .sum()
    }

    /// Parse content inside blockquotes (or at top level).
    ///
    /// `content` - The content to parse (may have indent/markers stripped)
    /// `line_to_append` - Optional line to use when appending to paragraphs.
    ///                    If None, uses self.lines[self.pos]
    fn parse_inner_content(&mut self, content: &str, line_to_append: Option<&str>) -> bool {
        // Calculate how much indentation should be stripped for content containers
        // (definitions, footnotes), but don't strip it yet - we need to handle it
        // carefully to preserve losslessness
        let content_indent = self.content_container_indent_to_strip();
        let (stripped_content, indent_to_emit) = if content_indent > 0 {
            let (indent_cols, _) = leading_indent(content);
            if indent_cols >= content_indent {
                let idx = byte_index_at_column(content, content_indent);
                (&content[idx..], Some(&content[..idx]))
            } else {
                // Line has less indent than required - preserve leading whitespace
                let trimmed_start = content.trim_start();
                let ws_len = content.len() - trimmed_start.len();
                if ws_len > 0 {
                    (trimmed_start, Some(&content[..ws_len]))
                } else {
                    (content, None)
                }
            }
        } else {
            (content, None)
        };

        // Store the indent for later emission when starting new blocks
        // (emitted for block elements like lists, not paragraph continuations)
        let content = stripped_content;

        // Check for heading (needs blank line before, or at start of container)
        let has_blank_before = self.pos == 0
            || self.lines[self.pos - 1].trim().is_empty()
            || matches!(self.containers.last(), Some(Container::BlockQuote { .. }))
            || matches!(self.containers.last(), Some(Container::List { .. }));

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
        // When inside a list, strip list indentation before checking
        let list_indent_stripped = if self.in_list() {
            let content_col = self.current_content_col();
            if content_col > 0 {
                // We're inside a list item - strip up to content column
                let (indent_cols, _) = leading_indent(content);
                indent_cols.min(content_col)
            } else {
                // Inside list but not in item (shouldn't happen normally)
                // Strip up to 4 spaces (typical list indentation)
                if content.starts_with("    ") {
                    4
                } else if content.starts_with("   ") {
                    3
                } else if content.starts_with("  ") {
                    2
                } else {
                    0
                }
            }
        } else {
            0
        };
        let content_for_fence_check = if list_indent_stripped > 0 {
            let idx = byte_index_at_column(content, list_indent_stripped);
            &content[idx..]
        } else {
            content
        };

        if has_blank_before && let Some(fence) = try_parse_fence_open(content_for_fence_check) {
            let bq_depth = self.current_blockquote_depth();
            log::debug!(
                "Parsed fenced code block at line {}: {} fence",
                self.pos,
                fence.fence_char
            );
            // Pass total indent (footnote + list) to the parser
            let total_indent = content_indent + list_indent_stripped;
            let new_pos = parse_fenced_code_block(
                &mut self.builder,
                &self.lines,
                self.pos,
                fence,
                bq_depth,
                total_indent,
            );
            self.pos = new_pos;
            return true;
        }

        // Check for footnote definition: [^id]: content
        // Similar to list items - marker followed by content that can span multiple lines
        // Must check BEFORE reference definitions since both start with [
        if let Some((id, content_start)) = try_parse_footnote_marker(content) {
            log::debug!("Parsed footnote definition at line {}: [^{}]", self.pos, id);

            // Close paragraph if one is open
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
            }

            // Close previous footnote if one is open
            while matches!(
                self.containers.last(),
                Some(Container::FootnoteDefinition { .. })
            ) {
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
            }

            // Start the footnote definition container
            self.builder
                .start_node(SyntaxKind::FootnoteDefinition.into());

            // Emit the marker
            let marker_text = &content[..content_start];
            self.builder
                .token(SyntaxKind::FootnoteReference.into(), marker_text);

            // Calculate content column (minimum 4 spaces for continuation)
            // The first line can start right after the marker, but subsequent lines
            // need at least 4 spaces of indentation
            let content_col = 4;
            self.containers.push(Container::FootnoteDefinition {
                id: id.clone(),
                content_col,
            });

            // Parse the first line content (if any)
            let first_line_content = &content[content_start..];
            if !first_line_content.trim().is_empty() {
                self.start_paragraph_if_needed();
                self.append_paragraph_line(first_line_content);
            }

            self.pos += 1;
            return true;
        }

        // Check for reference definition: [label]: url "title"
        // These can appear anywhere in the document
        if let Some((_len, label, url, title)) = try_parse_reference_definition(content) {
            log::debug!(
                "Parsed reference definition at line {}: [{}]",
                self.pos,
                label
            );
            // Store in registry
            self.reference_registry.add(label, url, title);

            // Emit as a node - preserve original text including newline
            self.builder
                .start_node(SyntaxKind::ReferenceDefinition.into());

            // Get the full original line to preserve losslessness
            let full_line = self.lines[self.pos];
            let content_without_newline = full_line.trim_end_matches('\n');

            // Emit the reference definition content
            self.builder
                .token(SyntaxKind::TEXT.into(), content_without_newline);

            // Emit newline separately if present
            if full_line.ends_with('\n') {
                self.builder.token(SyntaxKind::NEWLINE.into(), "\n");
            }

            self.builder.finish_node();
            self.pos += 1;
            return true;
        }

        // Check for indented code block (must have actual blank line before)
        // Inside a footnote, content needs 4 spaces for code (8 total in raw line)
        // BUT: Don't treat as code if it's a list marker (list takes precedence)
        if has_blank_before_strict
            && is_indented_code_line(content)
            && try_parse_list_marker(content, self.config).is_none()
        {
            let bq_depth = self.current_blockquote_depth();
            log::debug!("Parsed indented code block at line {}", self.pos);
            let new_pos = parse_indented_code_block(
                &mut self.builder,
                &self.lines,
                self.pos,
                bq_depth,
                content_indent,
            );
            self.pos = new_pos;
            return true;
        }

        // Check for display math block
        // Close paragraph first if one is open, then parse as MathBlock
        if let Some(math_fence) =
            try_parse_math_fence_open(content, self.config.extensions.tex_math_single_backslash)
        {
            log::debug!(
                "Parsed display math block at line {}: {:?}",
                self.pos,
                math_fence.fence_type
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

            // Emit opening fence with attributes as child node to avoid duplication
            self.builder.start_node(SyntaxKind::DivFenceOpen.into());

            // Get original full line
            let full_line = self.lines[self.pos];
            let trimmed = full_line.trim_start();

            // Emit leading whitespace if present
            let leading_ws_len = full_line.len() - trimmed.len();
            if leading_ws_len > 0 {
                self.builder
                    .token(SyntaxKind::WHITESPACE.into(), &full_line[..leading_ws_len]);
            }

            // Emit fence colons
            let fence_str: String = ":".repeat(div_fence.fence_count);
            self.builder.token(SyntaxKind::TEXT.into(), &fence_str);

            // Parse everything after colons
            let after_colons = &trimmed[div_fence.fence_count..];
            let (content_before_newline, newline_str) = strip_newline(after_colons);

            // Emit optional space before attributes
            let has_leading_space = content_before_newline.starts_with(' ');
            if has_leading_space {
                self.builder.token(SyntaxKind::WHITESPACE.into(), " ");
            }

            // Get content after the leading space (if any)
            let content_after_space = if has_leading_space {
                &content_before_newline[1..]
            } else {
                content_before_newline
            };

            // Emit attributes as DivInfo child node (avoids duplication)
            self.builder.start_node(SyntaxKind::DivInfo.into());
            self.builder
                .token(SyntaxKind::TEXT.into(), &div_fence.attributes);
            self.builder.finish_node(); // DivInfo

            // Check for trailing colons after attributes (symmetric fences)
            let (trailing_space, trailing_colons) = if div_fence.attributes.starts_with('{') {
                // For bracketed attributes like {.class}, find what's after the closing brace
                if let Some(close_idx) = content_after_space.find('}') {
                    let after_attrs = &content_after_space[close_idx + 1..];
                    let trailing = after_attrs.trim_start();
                    let space_count = after_attrs.len() - trailing.len();
                    if !trailing.is_empty() && trailing.chars().all(|c| c == ':') {
                        (space_count > 0, trailing)
                    } else {
                        (false, "")
                    }
                } else {
                    (false, "")
                }
            } else {
                // For simple class names like "Warning", check after first word
                // content_after_space starts with the attribute (e.g., "Warning ::::::")
                let after_attrs = &content_after_space[div_fence.attributes.len()..];
                if let Some(after_space) = after_attrs.strip_prefix(' ') {
                    if !after_space.is_empty() && after_space.chars().all(|c| c == ':') {
                        (true, after_space)
                    } else {
                        (false, "")
                    }
                } else {
                    (false, "")
                }
            };

            // Emit space before trailing colons if present
            if trailing_space {
                self.builder.token(SyntaxKind::WHITESPACE.into(), " ");
            }

            // Emit trailing colons if present
            if !trailing_colons.is_empty() {
                self.builder.token(SyntaxKind::TEXT.into(), trailing_colons);
            }

            // Emit newline
            if !newline_str.is_empty() {
                self.builder.token(SyntaxKind::NEWLINE.into(), newline_str);
            }
            self.builder.finish_node(); // DivFenceOpen

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

            // Emit closing fence - parse to avoid newline duplication
            self.builder.start_node(SyntaxKind::DivFenceClose.into());

            // Get original full line
            let full_line = self.lines[self.pos];
            let trimmed = full_line.trim_start();

            // Emit leading whitespace if present
            let leading_ws_len = full_line.len() - trimmed.len();
            if leading_ws_len > 0 {
                self.builder
                    .token(SyntaxKind::WHITESPACE.into(), &full_line[..leading_ws_len]);
            }

            // Emit fence content without newline
            let content_without_newline = trimmed.trim_end_matches('\n');
            self.builder
                .token(SyntaxKind::TEXT.into(), content_without_newline);

            // Emit newline separately
            if full_line.ends_with('\n') {
                self.builder.token(SyntaxKind::NEWLINE.into(), "\n");
            }
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
        if let Some((marker, marker_len, spaces_after)) =
            try_parse_list_marker(content, self.config)
        {
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
                    indent_to_emit,
                );
                self.pos += 1;
                return true;
            }

            // Find matching list level
            let matched_level = self.find_matching_list_level(&marker, indent_cols);

            if let Some(level) = matched_level {
                self.continue_list_at_level(level);
                // Emit footnote/definition indent before list item (for losslessness)
                if let Some(indent_str) = indent_to_emit {
                    self.builder
                        .token(SyntaxKind::WHITESPACE.into(), indent_str);
                }
            } else {
                self.start_new_list(&marker, indent_cols, indent_to_emit);
            }

            // Start list item
            self.add_list_item(content, marker_len, spaces_after, indent_cols, indent_bytes);
            self.pos += 1;
            return true;
        }

        // Definition list marker?
        if let Some((marker_char, indent, spaces_after)) = try_parse_definition_marker(content) {
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

            // Extract and emit the actual newline from the original line
            let current_line = self.lines[self.pos];
            let (_, newline_str) = strip_newline(current_line);
            if !newline_str.is_empty() {
                self.builder.token(SyntaxKind::NEWLINE.into(), newline_str);
            }

            self.containers.push(Container::Definition { content_col });
            self.pos += 1;
            return true;
        }

        // Term line (if next line has definition marker)?
        if let Some(blank_count) = self.next_line_is_definition_marker()
            && !content.trim().is_empty()
        {
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

            // Emit blank lines between term and definition marker
            for _ in 0..blank_count {
                if self.pos < self.lines.len() {
                    let blank_line = self.lines[self.pos];
                    self.builder.start_node(SyntaxKind::BlankLine.into());
                    self.builder.token(SyntaxKind::BlankLine.into(), blank_line);
                    self.builder.finish_node();
                    self.pos += 1;
                }
            }

            return true;
        }

        // Check if this is a table caption followed by a table
        // If so, don't parse as paragraph - let table parser handle it
        if is_caption_followed_by_table(&self.lines, self.pos) {
            // Don't parse as paragraph - this will be consumed by table parser
            return false;
        }

        // Check for line block (if line_blocks extension is enabled)
        if self.config.extensions.line_blocks && try_parse_line_block_start(content).is_some() {
            log::debug!("Parsed line block at line {}", self.pos);
            // Close paragraph before opening line block
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
            }

            let new_pos = parse_line_block(&self.lines, self.pos, &mut self.builder);
            self.pos = new_pos;
            return true;
        }

        // Paragraph
        self.start_paragraph_if_needed();
        // For lossless parsing: use line_to_append if provided (e.g., for blockquotes
        // where markers have been stripped), otherwise use the original line
        let line = line_to_append.unwrap_or(self.lines[self.pos]);
        self.append_paragraph_line(line);
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
        // Search from deepest (last) to shallowest (first)
        // But for shallow items (0-3 indent), prefer matching at the closest base indent
        let mut best_match: Option<(usize, usize)> = None; // (index, distance)

        for (i, c) in self.containers.stack.iter().enumerate().rev() {
            if let Container::List {
                marker: list_marker,
                base_indent_cols,
            } = c
                && markers_match(marker, list_marker)
            {
                let matches = if indent_cols >= 4 && *base_indent_cols >= 4 {
                    // Both deeply indented - require close match
                    indent_cols >= *base_indent_cols && indent_cols <= base_indent_cols + 3
                } else if indent_cols >= 4 || *base_indent_cols >= 4 {
                    // One shallow, one deep - no match
                    false
                } else {
                    // Both at shallow indentation (0-3)
                    // Allow items within 3 spaces
                    indent_cols.abs_diff(*base_indent_cols) <= 3
                };

                if matches {
                    let distance = indent_cols.abs_diff(*base_indent_cols);
                    if let Some((_, best_dist)) = best_match {
                        if distance < best_dist {
                            best_match = Some((i, distance));
                        }
                    } else {
                        best_match = Some((i, distance));
                    }

                    // If we found an exact match, return immediately
                    if distance == 0 {
                        return Some(i);
                    }
                }
            }
        }

        best_match.map(|(i, _)| i)
    }

    fn in_definition_list(&self) -> bool {
        self.containers
            .stack
            .iter()
            .any(|c| matches!(c, Container::DefinitionList { .. }))
    }

    fn next_line_is_definition_marker(&self) -> Option<usize> {
        // Look ahead past blank lines to find a definition marker
        // Returns Some(blank_line_count) if found, None otherwise
        let mut check_pos = self.pos + 1;
        let mut blank_count = 0;
        while check_pos < self.lines.len() {
            let line = self.lines[check_pos];
            if line.trim().is_empty() {
                blank_count += 1;
                check_pos += 1;
                continue;
            }
            if try_parse_definition_marker(line).is_some() {
                return Some(blank_count);
            } else {
                return None;
            }
        }
        None
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

    fn start_new_list(
        &mut self,
        marker: &ListMarker,
        indent_cols: usize,
        indent_to_emit: Option<&str>,
    ) {
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
        // Emit footnote/definition indent for losslessness
        if let Some(indent_str) = indent_to_emit {
            self.builder
                .token(SyntaxKind::WHITESPACE.into(), indent_str);
        }
        self.containers.push(Container::List {
            marker: marker.clone(),
            base_indent_cols: indent_cols,
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn start_nested_list(
        &mut self,
        content: &str,
        marker: &ListMarker,
        marker_len: usize,
        spaces_after: usize,
        indent_cols: usize,
        indent_bytes: usize,
        indent_to_emit: Option<&str>,
    ) {
        if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
            self.containers
                .close_to(self.containers.depth() - 1, &mut self.builder);
        }
        self.builder.start_node(SyntaxKind::List.into());
        // Emit footnote/definition indent for losslessness
        if let Some(indent_str) = indent_to_emit {
            self.builder
                .token(SyntaxKind::WHITESPACE.into(), indent_str);
        }
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
        // For lossless parsing, preserve the line exactly as-is
        // Don't strip to content column in the parser - that's the formatter's job

        // Split off trailing newline (LF or CRLF) if present
        let (text_without_newline, newline_str) = utils::strip_newline(line);

        if !text_without_newline.is_empty() {
            self.builder
                .token(SyntaxKind::TEXT.into(), text_without_newline);
        }

        if !newline_str.is_empty() {
            self.builder.token(SyntaxKind::NEWLINE.into(), newline_str);
        }
    }

    fn current_content_col(&self) -> usize {
        self.containers
            .stack
            .iter()
            .rev()
            .find_map(|c| match c {
                Container::ListItem { content_col } => Some(*content_col),
                Container::FootnoteDefinition { content_col, .. } => Some(*content_col),
                _ => None,
            })
            .unwrap_or(0)
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
    mod losslessness;
}
