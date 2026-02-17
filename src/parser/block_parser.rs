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
mod marker_utils;
mod metadata;
mod paragraphs;
pub mod reference_definitions; // Public for use in inline_parser
mod tables;
mod utils;

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
use lists::{markers_match, try_parse_list_marker};
use marker_utils::{count_blockquote_markers, parse_blockquote_marker_info};
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

    /// Returns true if the line was consumed.
    fn parse_line(&mut self, line: &str) -> bool {
        // Count blockquote markers on this line
        let (bq_depth, inner_content) = count_blockquote_markers(line);
        let current_bq_depth = blockquotes::current_blockquote_depth(&self.containers);

        log::debug!(
            "parse_line [{}]: bq_depth={}, current_bq={}, depth={}, line={:?}",
            self.pos,
            bq_depth,
            current_bq_depth,
            self.containers.depth(),
            line.trim_end()
        );

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

            // Close Plain node in Definition if open
            // Blank lines should close Plain, allowing subsequent content to be siblings
            if let Some(Container::Definition {
                plain_open: true, ..
            }) = self.containers.last()
            {
                self.builder.finish_node(); // Close Plain node
                // Mark Plain as closed
                if let Some(Container::Definition { plain_open, .. }) =
                    self.containers.stack.last_mut()
                {
                    *plain_open = false;
                }
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
                blockquotes::close_blockquotes_to_depth(
                    &mut self.containers,
                    &mut self.builder,
                    bq_depth,
                );
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
                        log::debug!(
                            "Closing {:?} at blank line (depth {} > levels_to_keep {})",
                            self.containers.last(),
                            self.containers.depth(),
                            levels_to_keep
                        );

                        // If closing a Definition with open Plain, close Plain first
                        if let Some(Container::Definition {
                            plain_open: true, ..
                        }) = self.containers.last()
                        {
                            self.builder.finish_node(); // Close Plain node
                            // Update container to mark Plain as closed before removing it
                            if let Some(Container::Definition { plain_open, .. }) =
                                self.containers.stack.last_mut()
                            {
                                *plain_open = false;
                            }
                        }

                        self.containers
                            .close_to(self.containers.depth() - 1, &mut self.builder);
                    }
                    _ => break,
                }
            }

            // Emit blockquote markers for this blank line if inside blockquotes
            if bq_depth > 0 {
                let marker_info = parse_blockquote_marker_info(line);
                for i in 0..bq_depth {
                    if let Some(info) = marker_info.get(i) {
                        blockquotes::emit_one_blockquote_marker(
                            &mut self.builder,
                            info.leading_spaces,
                            info.has_trailing_space,
                        );
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
            if current_bq_depth == 0 && !blockquotes::can_start_blockquote(self.pos, &self.lines) {
                // Can't start blockquote without blank line - treat as paragraph
                paragraphs::start_paragraph_if_needed(&mut self.containers, &mut self.builder);
                paragraphs::append_paragraph_line(&mut self.builder, line);
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
                    blockquotes::strip_n_blockquote_markers(line, current_bq_depth);

                // Emit blockquote markers for current depth (for losslessness)
                let marker_info = parse_blockquote_marker_info(line);
                for i in 0..current_bq_depth {
                    if let Some(info) = marker_info.get(i) {
                        blockquotes::emit_one_blockquote_marker(
                            &mut self.builder,
                            info.leading_spaces,
                            info.has_trailing_space,
                        );
                    }
                }

                if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                    // Lazy continuation with the extra > as content
                    paragraphs::append_paragraph_line(&mut self.builder, content_at_current_depth);
                    self.pos += 1;
                    return true;
                } else {
                    // Start new paragraph with the extra > as content
                    paragraphs::start_paragraph_if_needed(&mut self.containers, &mut self.builder);
                    paragraphs::append_paragraph_line(&mut self.builder, content_at_current_depth);
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
            let marker_info = parse_blockquote_marker_info(line);

            // First, emit markers for existing blockquote levels (before opening new ones)
            for level in 0..current_bq_depth {
                if let Some(info) = marker_info.get(level) {
                    blockquotes::emit_one_blockquote_marker(
                        &mut self.builder,
                        info.leading_spaces,
                        info.has_trailing_space,
                    );
                }
            }

            // Then open new blockquotes and emit their markers
            for level in current_bq_depth..bq_depth {
                self.builder.start_node(SyntaxKind::BlockQuote.into());

                // Emit the marker for this new level
                if let Some(info) = marker_info.get(level) {
                    blockquotes::emit_one_blockquote_marker(
                        &mut self.builder,
                        info.leading_spaces,
                        info.has_trailing_space,
                    );
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
                    paragraphs::append_paragraph_line(&mut self.builder, line);
                    self.pos += 1;
                    return true;
                }

                // Check for lazy list continuation - if we're in a list item and
                // this line looks like a list item with matching marker
                if lists::in_blockquote_list(&self.containers)
                    && let Some((marker, marker_len, spaces_after)) =
                        try_parse_list_marker(line, self.config)
                {
                    let (indent_cols, indent_bytes) = leading_indent(line);
                    if let Some(level) =
                        lists::find_matching_list_level(&self.containers, &marker, indent_cols)
                    {
                        // Continue the list inside the blockquote
                        lists::continue_list_at_level(
                            &mut self.containers,
                            &mut self.builder,
                            level,
                        );
                        lists::add_list_item(
                            &mut self.containers,
                            &mut self.builder,
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
            blockquotes::close_blockquotes_to_depth(
                &mut self.containers,
                &mut self.builder,
                bq_depth,
            );

            // Parse the inner content at the new depth
            if bq_depth > 0 {
                // Emit markers at current depth before parsing content
                let marker_info = parse_blockquote_marker_info(line);
                for i in 0..bq_depth {
                    if let Some(info) = marker_info.get(i) {
                        blockquotes::emit_one_blockquote_marker(
                            &mut self.builder,
                            info.leading_spaces,
                            info.has_trailing_space,
                        );
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

            // Check if we should close the ListItem
            // ListItem should continue if the line is properly indented for continuation
            if matches!(
                self.containers.last(),
                Some(Container::ListItem { content_col: _ })
            ) {
                let (indent_cols, _) = leading_indent(inner_content);
                let content_indent = self.content_container_indent_to_strip();
                let effective_indent = indent_cols.saturating_sub(content_indent);
                let content_col = match self.containers.last() {
                    Some(Container::ListItem { content_col }) => *content_col,
                    _ => 0,
                };

                // Check if this line starts a new list item at outer level
                let is_new_item_at_outer_level = if let Some((_marker, _, _)) =
                    try_parse_list_marker(inner_content, self.config)
                {
                    effective_indent < content_col
                } else {
                    false
                };

                // Close ListItem if:
                // 1. It's a new list item at an outer (or same) level, OR
                // 2. The line is not indented enough to continue the current item
                if is_new_item_at_outer_level || effective_indent < content_col {
                    log::debug!(
                        "Closing ListItem: is_new_item={}, effective_indent={} < content_col={}",
                        is_new_item_at_outer_level,
                        effective_indent,
                        content_col
                    );
                    self.containers
                        .close_to(self.containers.depth() - 1, &mut self.builder);
                } else {
                    log::debug!(
                        "Keeping ListItem: effective_indent={} >= content_col={}",
                        effective_indent,
                        content_col
                    );
                }
            }

            let marker_info = parse_blockquote_marker_info(line);
            for i in 0..bq_depth {
                if let Some(info) = marker_info.get(i) {
                    blockquotes::emit_one_blockquote_marker(
                        &mut self.builder,
                        info.leading_spaces,
                        info.has_trailing_space,
                    );
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
                paragraphs::append_paragraph_line(&mut self.builder, line);
                self.pos += 1;
                return true;
            }

            // Check for lazy list continuation
            if lists::in_blockquote_list(&self.containers)
                && let Some((marker, marker_len, spaces_after)) =
                    try_parse_list_marker(line, self.config)
            {
                let (indent_cols, indent_bytes) = leading_indent(line);
                if let Some(level) =
                    lists::find_matching_list_level(&self.containers, &marker, indent_cols)
                {
                    lists::continue_list_at_level(&mut self.containers, &mut self.builder, level);
                    lists::add_list_item(
                        &mut self.containers,
                        &mut self.builder,
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

        // No blockquote markers - use original line
        self.parse_inner_content(line, None)
    }

    /// Compute how many container levels to keep open based on next line content.
    fn compute_levels_to_keep(&self, next_line: &str) -> usize {
        let (next_bq_depth, next_inner) = count_blockquote_markers(next_line);
        let (raw_indent_cols, _) = leading_indent(next_inner);
        let next_marker = try_parse_list_marker(next_inner, self.config);

        // Calculate current blockquote depth for proper indent calculation
        let current_bq_depth = blockquotes::current_blockquote_depth(&self.containers);

        log::debug!(
            "compute_levels_to_keep: next_line indent={}, has_marker={}, stack_depth={}, current_bq={}, next_bq={}",
            raw_indent_cols,
            next_marker.is_some(),
            self.containers.depth(),
            current_bq_depth,
            next_bq_depth
        );

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
                Container::Definition { content_col, .. } => {
                    // Definition continuation: line must be indented at least 4 spaces
                    // After a blank line, only keep if there's nested block content (lists, code, etc)
                    // Plain text after blank line should close the definition
                    let min_indent = (*content_col).max(4);
                    if raw_indent_cols >= min_indent {
                        // Check what kind of content this is
                        let after_content_indent = if raw_indent_cols >= content_indent_so_far {
                            let idx = byte_index_at_column(next_line, content_indent_so_far);
                            &next_line[idx..]
                        } else {
                            next_line
                        };

                        // Keep Definition if there's a definition marker or nested block structure
                        let has_definition_marker =
                            try_parse_definition_marker(after_content_indent).is_some();
                        let has_list_marker =
                            try_parse_list_marker(after_content_indent, self.config).is_some();
                        let has_block_structure = has_list_marker
                            || count_blockquote_markers(after_content_indent).0 > 0
                            || try_parse_fence_open(after_content_indent).is_some()
                            || try_parse_math_fence_open(
                                after_content_indent,
                                self.config.extensions.tex_math_single_backslash,
                            )
                            .is_some()
                            || try_parse_div_fence_open(after_content_indent).is_some()
                            || try_parse_horizontal_rule(after_content_indent).is_some();

                        if !has_definition_marker && has_block_structure {
                            // Keep Definition for nested block content
                            keep_level = i + 1;
                        }
                        // Otherwise let Definition close (either new definition or plain text)
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

                    // Special case: if next line has MORE blockquote markers than current depth,
                    // those extra markers count as "content" that should be indented for list continuation.
                    // Example: "> - item" followed by ">   > nested" - the 2 spaces between the markers
                    // indicate list continuation, and the second > is content.
                    let effective_indent = if next_bq_depth > current_bq_depth {
                        // The line has extra blockquote markers. After stripping current depth's markers,
                        // check the indent before any remaining markers.
                        let after_current_bq =
                            blockquotes::strip_n_blockquote_markers(next_line, current_bq_depth);
                        let (spaces_before_next_marker, _) = leading_indent(after_current_bq);
                        spaces_before_next_marker.saturating_sub(content_indent_so_far)
                    } else {
                        raw_indent_cols.saturating_sub(content_indent_so_far)
                    };

                    log::debug!(
                        "ListItem continuation check: content_col={}, effective_indent={}, next_bq_depth={}, current_bq_depth={}",
                        content_col,
                        effective_indent,
                        next_bq_depth,
                        current_bq_depth
                    );

                    let is_new_item_at_outer_level = if let Some((ref _nm, _, _)) = next_marker {
                        // Check if this marker would start a sibling item (at parent list level)
                        // by checking if it's at or before the current item's start
                        effective_indent < *content_col
                    } else {
                        false
                    };

                    if !is_new_item_at_outer_level && effective_indent >= *content_col {
                        keep_level = i + 1;
                        log::debug!(
                            "Keeping ListItem: keep_level now {} (i={}, effective_indent={} >= content_col={})",
                            keep_level,
                            i,
                            effective_indent,
                            content_col
                        );
                    } else {
                        log::debug!(
                            "NOT keeping ListItem: is_new_item={}, effective_indent={} < content_col={}",
                            is_new_item_at_outer_level,
                            effective_indent,
                            content_col
                        );
                    }
                }
                _ => {}
            }
        }

        log::debug!("compute_levels_to_keep returning: {}", keep_level);
        keep_level
    }

    /// Get the total indentation to strip from content containers (footnotes + definitions).
    fn content_container_indent_to_strip(&self) -> usize {
        self.containers
            .stack
            .iter()
            .filter_map(|c| match c {
                Container::FootnoteDefinition { content_col, .. } => Some(*content_col),
                Container::Definition { content_col, .. } => Some(*content_col),
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
        log::debug!(
            "parse_inner_content [{}]: depth={}, last={:?}, content={:?}",
            self.pos,
            self.containers.depth(),
            self.containers.last(),
            content.trim_end()
        );
        // Calculate how much indentation should be stripped for content containers
        // (definitions, footnotes) FIRST, so we can check for block markers correctly
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

        // Check if we have an open Plain node in a Definition container
        // If so, append this line to the Plain node instead of creating a new block
        // BUT: Don't treat lines with block element markers as continuations
        if let Some(Container::Definition {
            plain_open: true, ..
        }) = self.containers.last()
        {
            // Check if this line starts with any block element marker
            // Use stripped_content so we check AFTER removing footnote/definition indent
            let is_block_element = try_parse_definition_marker(stripped_content).is_some()
                || try_parse_list_marker(stripped_content, self.config).is_some()
                || count_blockquote_markers(stripped_content).0 > 0
                || try_parse_fence_open(stripped_content).is_some()
                || try_parse_math_fence_open(
                    stripped_content,
                    self.config.extensions.tex_math_single_backslash,
                )
                .is_some()
                || try_parse_div_fence_open(stripped_content).is_some()
                || try_parse_horizontal_rule(stripped_content).is_some()
                || try_parse_atx_heading(stripped_content).is_some()
                || (self.config.extensions.raw_html
                    && try_parse_html_block_start(stripped_content).is_some())
                || (self.config.extensions.raw_tex
                    && try_parse_latex_env_begin(stripped_content).is_some());

            if is_block_element {
                // Close the Plain node before processing the block element
                self.builder.finish_node(); // Close Plain
                // Update container to mark Plain as closed
                if let Some(Container::Definition { plain_open, .. }) =
                    self.containers.stack.last_mut()
                {
                    *plain_open = false;
                }
                // Fall through to parse the block element
            } else {
                // This is a continuation line for an open Plain block
                // For lossless parsing, we need to preserve the entire line including indent

                // Get the original line to preserve exact whitespace
                let full_line = self.lines[self.pos];

                // Split off trailing newline
                let (text_without_newline, newline_str) = utils::strip_newline(full_line);

                // Emit the entire line (including indent) as TEXT + NEWLINE to open Plain node
                if !text_without_newline.is_empty() {
                    self.builder
                        .token(SyntaxKind::TEXT.into(), text_without_newline);
                }

                if !newline_str.is_empty() {
                    self.builder.token(SyntaxKind::NEWLINE.into(), newline_str);
                }

                self.pos += 1;
                return true;
            }
        }

        // Store the stripped content for later use
        let content = stripped_content;

        // Check for heading (needs blank line before, or at start of container)
        let has_blank_before = self.pos == 0
            || self.lines[self.pos - 1].trim().is_empty()
            || matches!(self.containers.last(), Some(Container::BlockQuote { .. }))
            || matches!(self.containers.last(), Some(Container::List { .. }));

        // For indented code blocks, we need a stricter condition - only actual blank lines count
        // Being at document start (pos == 0) is OK only if we're not inside a blockquote
        let at_document_start =
            self.pos == 0 && blockquotes::current_blockquote_depth(&self.containers) == 0;
        let prev_line_blank = if self.pos > 0 {
            let prev_line = self.lines[self.pos - 1];
            let (prev_bq_depth, prev_inner) = count_blockquote_markers(prev_line);
            prev_line.trim().is_empty() || (prev_bq_depth > 0 && prev_inner.trim().is_empty())
        } else {
            false
        };
        let has_blank_before_strict = at_document_start || prev_line_blank;

        // At top level only (not inside blockquotes), check for YAML metadata
        if blockquotes::current_blockquote_depth(&self.containers) == 0 && content.trim() == "---" {
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

            let bq_depth = blockquotes::current_blockquote_depth(&self.containers);
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
        let list_indent_stripped = if lists::in_list(&self.containers) {
            let content_col = paragraphs::current_content_col(&self.containers);
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
            let bq_depth = blockquotes::current_blockquote_depth(&self.containers);
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
                paragraphs::start_paragraph_if_needed(&mut self.containers, &mut self.builder);
                paragraphs::append_paragraph_line(&mut self.builder, first_line_content);
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
            let bq_depth = blockquotes::current_blockquote_depth(&self.containers);
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

            let bq_depth = blockquotes::current_blockquote_depth(&self.containers);
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

            let bq_depth = blockquotes::current_blockquote_depth(&self.containers);
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
            if indent_cols >= 4 && !lists::in_list(&self.containers) {
                // Code block at top-level, treat as paragraph
                paragraphs::start_paragraph_if_needed(&mut self.containers, &mut self.builder);
                paragraphs::append_paragraph_line(&mut self.builder, content);
                self.pos += 1;
                return true;
            }

            // Close paragraph before list item
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
            }

            // Check if this continues an existing list level
            let matched_level =
                lists::find_matching_list_level(&self.containers, &marker, indent_cols);
            let current_content_col = paragraphs::current_content_col(&self.containers);

            // Decision tree:
            // 1. If indent < content_col: Must be continuing a parent list (close nested and continue)
            // 2. If indent >= content_col:
            //    a. If exactly matches a nested list's base_indent: Continue that nested list
            //    b. Otherwise: Start new nested list

            if current_content_col > 0 && indent_cols >= current_content_col {
                // Potentially nested - but check if it EXACTLY matches an existing nested list first
                if let Some(level) = matched_level
                    && let Some(Container::List {
                        base_indent_cols, ..
                    }) = self.containers.stack.get(level)
                    && indent_cols == *base_indent_cols
                {
                    // Exact match - this is a sibling item in the matched list
                    let num_parent_lists = self.containers.stack[..level]
                        .iter()
                        .filter(|c| matches!(c, Container::List { .. }))
                        .count();

                    if num_parent_lists > 0 {
                        // This matches a nested list - continue it
                        lists::continue_list_at_level(
                            &mut self.containers,
                            &mut self.builder,
                            level,
                        );
                        if let Some(indent_str) = indent_to_emit {
                            self.builder
                                .token(SyntaxKind::WHITESPACE.into(), indent_str);
                        }
                        lists::add_list_item(
                            &mut self.containers,
                            &mut self.builder,
                            content,
                            marker_len,
                            spaces_after,
                            indent_cols,
                            indent_bytes,
                        );
                        self.pos += 1;
                        return true;
                    }
                }

                // No exact match - start new nested list
                lists::start_nested_list(
                    &mut self.containers,
                    &mut self.builder,
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

            // indent < content_col: Continue parent list if matched
            if let Some(level) = matched_level {
                lists::continue_list_at_level(&mut self.containers, &mut self.builder, level);
                if let Some(indent_str) = indent_to_emit {
                    self.builder
                        .token(SyntaxKind::WHITESPACE.into(), indent_str);
                }
                lists::add_list_item(
                    &mut self.containers,
                    &mut self.builder,
                    content,
                    marker_len,
                    spaces_after,
                    indent_cols,
                    indent_bytes,
                );
                self.pos += 1;
                return true;
            }

            // No match and not nested - start new top-level list
            lists::start_new_list(
                &mut self.containers,
                &mut self.builder,
                &marker,
                indent_cols,
                indent_to_emit,
            );
            lists::add_list_item(
                &mut self.containers,
                &mut self.builder,
                content,
                marker_len,
                spaces_after,
                indent_cols,
                indent_bytes,
            );
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
            if !definition_lists::in_definition_list(&self.containers) {
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

            // Emit container indent (e.g., footnote indent) before the marker
            if let Some(indent_str) = indent_to_emit {
                self.builder
                    .token(SyntaxKind::WHITESPACE.into(), indent_str);
            }

            emit_definition_marker(&mut self.builder, marker_char, indent);
            if spaces_after > 0 {
                self.builder
                    .token(SyntaxKind::WHITESPACE.into(), &" ".repeat(spaces_after));
            }

            // Calculate content column (marker + spaces)
            let content_col = indent + 1 + spaces_after;

            // Emit remaining content on this line if any
            let after_marker_and_spaces = &content[indent + 1 + spaces_after..];
            let has_content = !after_marker_and_spaces.trim().is_empty();

            if has_content {
                // Wrap inline content in a Plain node (keep open for continuation lines)
                self.builder.start_node(SyntaxKind::Plain.into());
                self.builder
                    .token(SyntaxKind::TEXT.into(), after_marker_and_spaces.trim_end());

                // Extract and emit the actual newline from the original line
                let current_line = self.lines[self.pos];
                let (_, newline_str) = strip_newline(current_line);
                if !newline_str.is_empty() {
                    self.builder.token(SyntaxKind::NEWLINE.into(), newline_str);
                }

                // DON'T close Plain node yet - continuation lines will be added to it
                // self.builder.finish_node(); // Plain
            } else {
                // No content on this line, just emit newline directly
                let current_line = self.lines[self.pos];
                let (_, newline_str) = strip_newline(current_line);
                if !newline_str.is_empty() {
                    self.builder.token(SyntaxKind::NEWLINE.into(), newline_str);
                }
            }

            self.containers.push(Container::Definition {
                content_col,
                plain_open: has_content,
            });
            self.pos += 1;
            return true;
        }

        // Term line (if next line has definition marker)?
        if let Some(blank_count) =
            definition_lists::next_line_is_definition_marker(&self.lines, self.pos)
            && !content.trim().is_empty()
        {
            // Close any open structures
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.containers
                    .close_to(self.containers.depth() - 1, &mut self.builder);
            }

            // Start definition list if not in one
            if !definition_lists::in_definition_list(&self.containers) {
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

        // Paragraph or list item continuation
        // Check if we're inside a ListItem - if so, emit bare tokens instead of wrapping in PARAGRAPH
        if matches!(self.containers.last(), Some(Container::ListItem { .. })) {
            log::debug!(
                "Inside ListItem - emitting bare tokens for: {:?}",
                line_to_append.unwrap_or(self.lines[self.pos]).trim_end()
            );
            // Inside list item - emit as bare tokens for postprocessor to wrap later
            let line = line_to_append.unwrap_or(self.lines[self.pos]);
            utils::emit_line_tokens(&mut self.builder, line);
            self.pos += 1;
            return true;
        }

        log::debug!(
            "Not in ListItem - creating paragraph for: {:?}",
            line_to_append.unwrap_or(self.lines[self.pos]).trim_end()
        );
        // Not in list item - create paragraph as usual
        paragraphs::start_paragraph_if_needed(&mut self.containers, &mut self.builder);
        // For lossless parsing: use line_to_append if provided (e.g., for blockquotes
        // where markers have been stripped), otherwise use the original line
        let line = line_to_append.unwrap_or(self.lines[self.pos]);
        paragraphs::append_paragraph_line(&mut self.builder, line);
        self.pos += 1;
        true
    }

    fn in_fenced_div(&self) -> bool {
        self.containers
            .stack
            .iter()
            .any(|c| matches!(c, Container::FencedDiv { .. }))
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
