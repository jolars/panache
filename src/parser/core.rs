use crate::config::Config;
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::GreenNodeBuilder;

use super::block_dispatcher::{BlockContext, BlockDetectionResult, BlockParserRegistry};
use super::blocks::blockquotes;
use super::blocks::code_blocks;
use super::blocks::definition_lists;
use super::blocks::fenced_divs;
use super::blocks::headings;
use super::blocks::horizontal_rules;
use super::blocks::html_blocks;
use super::blocks::indented_code;
use super::blocks::latex_envs;
use super::blocks::line_blocks;
use super::blocks::lists;
use super::blocks::metadata;
use super::blocks::paragraphs;
use super::blocks::reference_links;
use super::blocks::tables;
use super::utils::container_stack;
use super::utils::helpers::{split_lines_inclusive, strip_newline};
use super::utils::inline_emission;
use super::utils::marker_utils;
use super::utils::text_buffer;

use code_blocks::try_parse_fence_open;
use container_stack::{Container, ContainerStack, byte_index_at_column, leading_indent};
use definition_lists::{emit_definition_marker, emit_term, try_parse_definition_marker};
use fenced_divs::{is_div_closing_fence, try_parse_div_fence_open};
use headings::try_parse_atx_heading;
use horizontal_rules::try_parse_horizontal_rule;
use html_blocks::{parse_html_block, try_parse_html_block_start};
use indented_code::{is_indented_code_line, parse_indented_code_block};
use latex_envs::{parse_latex_environment, try_parse_latex_env_begin};
use line_blocks::{parse_line_block, try_parse_line_block_start};
use lists::{is_content_nested_bullet_marker, markers_match, try_parse_list_marker};
use marker_utils::{count_blockquote_markers, parse_blockquote_marker_info};
use metadata::try_parse_pandoc_title_block;
use reference_links::try_parse_footnote_marker;
use tables::{
    is_caption_followed_by_table, try_parse_grid_table, try_parse_multiline_table,
    try_parse_pipe_table, try_parse_simple_table,
};
use text_buffer::TextBuffer;

fn init_logger() {
    let _ = env_logger::builder().is_test(true).try_init();
}

pub struct Parser<'a> {
    lines: Vec<&'a str>,
    pos: usize,
    builder: GreenNodeBuilder<'static>,
    containers: ContainerStack,
    config: &'a Config,
    block_registry: BlockParserRegistry,
}

impl<'a> Parser<'a> {
    pub fn new(input: &'a str, config: &'a Config) -> Self {
        // Use split_lines_inclusive to preserve line endings (both LF and CRLF)
        let lines = split_lines_inclusive(input);
        Self {
            lines,
            pos: 0,
            builder: GreenNodeBuilder::new(),
            containers: ContainerStack::new(),
            config,
            block_registry: BlockParserRegistry::new(),
        }
    }

    pub fn parse(mut self) -> SyntaxNode {
        #[cfg(debug_assertions)]
        {
            init_logger();
        }

        self.parse_document_stack();

        SyntaxNode::new_root(self.builder.finish())
    }

    /// Emit buffered PLAIN content if Definition container has open PLAIN.
    /// Close containers down to `keep`, emitting buffered content first.
    fn close_containers_to(&mut self, keep: usize) {
        // Emit buffered PARAGRAPH/PLAIN content before closing
        while self.containers.depth() > keep {
            match self.containers.stack.last() {
                // Handle ListItem with buffering
                Some(Container::ListItem { buffer, .. }) if !buffer.is_empty() => {
                    // Clone buffer to avoid borrow issues
                    let buffer_clone = buffer.clone();

                    log::debug!(
                        "Closing ListItem with buffer (is_empty={}, segment_count={})",
                        buffer_clone.is_empty(),
                        buffer_clone.segment_count()
                    );

                    // Determine if this should be Plain or PARAGRAPH:
                    // 1. Check if parent LIST has blank lines between items (list-level loose)
                    // 2. OR check if this item has blank lines within its content (item-level loose)
                    let parent_list_is_loose = self
                        .containers
                        .stack
                        .iter()
                        .rev()
                        .find_map(|c| match c {
                            Container::List {
                                has_blank_between_items,
                                ..
                            } => Some(*has_blank_between_items),
                            _ => None,
                        })
                        .unwrap_or(false);

                    let use_paragraph =
                        parent_list_is_loose || buffer_clone.has_blank_lines_between_content();

                    log::debug!(
                        "Emitting ListItem buffer: use_paragraph={} (parent_list_is_loose={}, item_has_blanks={})",
                        use_paragraph,
                        parent_list_is_loose,
                        buffer_clone.has_blank_lines_between_content()
                    );

                    // Pop container first
                    self.containers.stack.pop();
                    // Emit buffered content as Plain or PARAGRAPH
                    buffer_clone.emit_as_block(&mut self.builder, use_paragraph, self.config);
                    self.builder.finish_node(); // Close LIST_ITEM
                }
                // Handle ListItem without content
                Some(Container::ListItem { .. }) => {
                    log::debug!("Closing empty ListItem (no buffer content)");
                    // Just close normally (empty list item)
                    self.containers.stack.pop();
                    self.builder.finish_node();
                }
                // Handle Paragraph with buffering
                Some(Container::Paragraph { buffer }) if !buffer.is_empty() => {
                    // Clone buffer to avoid borrow issues
                    let buffer_clone = buffer.clone();
                    // Pop container first
                    self.containers.stack.pop();
                    // Emit buffered content with inline parsing (handles markers)
                    buffer_clone.emit_with_inlines(&mut self.builder, self.config);
                    self.builder.finish_node();
                }
                // Handle Paragraph without content
                Some(Container::Paragraph { .. }) => {
                    // Just close normally
                    self.containers.stack.pop();
                    self.builder.finish_node();
                }
                // Handle Definition with buffered PLAIN
                Some(Container::Definition {
                    plain_open: true,
                    plain_buffer,
                    ..
                }) if !plain_buffer.is_empty() => {
                    // Emit PLAIN node with buffered inline-parsed content
                    self.builder.start_node(SyntaxKind::PLAIN.into());
                    let text = plain_buffer.get_accumulated_text();
                    inline_emission::emit_inlines(&mut self.builder, &text, self.config);
                    self.builder.finish_node();

                    // Mark PLAIN as closed and clear buffer
                    if let Some(Container::Definition {
                        plain_open,
                        plain_buffer,
                        ..
                    }) = self.containers.stack.last_mut()
                    {
                        plain_buffer.clear();
                        *plain_open = false;
                    }

                    // Pop container and finish node
                    self.containers.stack.pop();
                    self.builder.finish_node();
                }
                // Handle Definition with PLAIN open but empty buffer
                Some(Container::Definition {
                    plain_open: true, ..
                }) => {
                    // Mark PLAIN as closed
                    if let Some(Container::Definition {
                        plain_open,
                        plain_buffer,
                        ..
                    }) = self.containers.stack.last_mut()
                    {
                        plain_buffer.clear();
                        *plain_open = false;
                    }

                    // Pop container and finish node
                    self.containers.stack.pop();
                    self.builder.finish_node();
                }
                // All other containers
                _ => {
                    self.containers.stack.pop();
                    self.builder.finish_node();
                }
            }
        }
    }

    /// Emit buffered PLAIN content if there's an open PLAIN in a Definition.
    /// This is used when we need to close PLAIN but keep the Definition container open.
    fn emit_buffered_plain_if_needed(&mut self) {
        // Check if we have an open PLAIN with buffered content
        if let Some(Container::Definition {
            plain_open: true,
            plain_buffer,
            ..
        }) = self.containers.stack.last()
            && !plain_buffer.is_empty()
        {
            // Emit PLAIN node with buffered inline-parsed content
            self.builder.start_node(SyntaxKind::PLAIN.into());
            let text = plain_buffer.get_accumulated_text();
            inline_emission::emit_inlines(&mut self.builder, &text, self.config);
            self.builder.finish_node();
        }

        // Mark PLAIN as closed and clear buffer
        if let Some(Container::Definition {
            plain_open,
            plain_buffer,
            ..
        }) = self.containers.stack.last_mut()
            && *plain_open
        {
            plain_buffer.clear();
            *plain_open = false;
        }
    }

    /// Close blockquotes down to a target depth.
    ///
    /// Must use `Parser::close_containers_to` (not `ContainerStack::close_to`) so list/paragraph
    /// buffers are emitted for losslessness.
    fn close_blockquotes_to_depth(&mut self, target_depth: usize) {
        let mut current = self.current_blockquote_depth();
        while current > target_depth {
            while !matches!(self.containers.last(), Some(Container::BlockQuote { .. })) {
                if self.containers.depth() == 0 {
                    break;
                }
                self.close_containers_to(self.containers.depth() - 1);
            }
            if matches!(self.containers.last(), Some(Container::BlockQuote { .. })) {
                self.close_containers_to(self.containers.depth() - 1);
                current -= 1;
            } else {
                break;
            }
        }
    }

    /// Emit buffered list item content if we're in a ListItem and it has content.
    /// This is used before starting block-level elements inside list items.
    fn emit_list_item_buffer_if_needed(&mut self) {
        if let Some(Container::ListItem { buffer, .. }) = self.containers.stack.last_mut()
            && !buffer.is_empty()
        {
            let buffer_clone = buffer.clone();
            buffer.clear();
            let use_paragraph = buffer_clone.has_blank_lines_between_content();
            buffer_clone.emit_as_block(&mut self.builder, use_paragraph, self.config);
        }
    }

    /// Check if a paragraph is currently open.
    fn is_paragraph_open(&self) -> bool {
        matches!(self.containers.last(), Some(Container::Paragraph { .. }))
    }

    /// Close paragraph if one is currently open.
    fn close_paragraph_if_open(&mut self) {
        if self.is_paragraph_open() {
            self.close_containers_to(self.containers.depth() - 1);
        }
    }

    /// Prepare for a block-level element by flushing buffers and closing paragraphs.
    /// This is a common pattern before starting tables, code blocks, divs, etc.
    fn prepare_for_block_element(&mut self) {
        self.emit_list_item_buffer_if_needed();
        self.close_paragraph_if_open();
    }

    /// Get current blockquote depth from container stack.
    fn current_blockquote_depth(&self) -> usize {
        blockquotes::current_blockquote_depth(&self.containers)
    }

    /// Emit or buffer a blockquote marker depending on parser state.
    ///
    /// If a paragraph is open and we're using integrated parsing, buffer the marker.
    /// Otherwise emit it directly to the builder.
    fn emit_or_buffer_blockquote_marker(
        &mut self,
        leading_spaces: usize,
        has_trailing_space: bool,
    ) {
        // If paragraph is open, buffer the marker (it will be emitted at correct position)
        if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
            // Buffer the marker in the paragraph
            paragraphs::append_paragraph_marker(
                &mut self.containers,
                leading_spaces,
                has_trailing_space,
            );
        } else {
            // Emit directly
            blockquotes::emit_one_blockquote_marker(
                &mut self.builder,
                leading_spaces,
                has_trailing_space,
            );
        }
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

        self.close_containers_to(0);
        self.builder.finish_node(); // DOCUMENT
    }

    /// Returns true if the line was consumed.
    fn parse_line(&mut self, line: &str) -> bool {
        // Count blockquote markers on this line
        let (bq_depth, inner_content) = count_blockquote_markers(line);
        let current_bq_depth = self.current_blockquote_depth();

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
            self.close_paragraph_if_open();

            // Close Plain node in Definition if open
            // Blank lines should close Plain, allowing subsequent content to be siblings
            // Emit buffered PLAIN content before continuing
            self.emit_buffered_plain_if_needed();

            // Note: Blank lines between terms and definitions are now preserved
            // and emitted as part of the term parsing logic

            // For blank lines inside blockquotes, we need to handle them at the right depth
            // First, adjust blockquote depth if needed
            if bq_depth > current_bq_depth {
                // Open blockquotes
                for _ in current_bq_depth..bq_depth {
                    self.builder.start_node(SyntaxKind::BLOCKQUOTE.into());
                    self.containers.push(Container::BlockQuote {});
                }
            } else if bq_depth < current_bq_depth {
                // Close blockquotes down to bq_depth (must use Parser close to emit buffers)
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

            // Check if blank line should be buffered in a ListItem BEFORE closing containers

            // Close containers down to the level we want to keep
            while self.containers.depth() > levels_to_keep {
                match self.containers.last() {
                    Some(Container::ListItem { .. }) => {
                        // levels_to_keep wants to close the ListItem - blank line is between items
                        log::debug!(
                            "Closing ListItem at blank line (levels_to_keep={} < depth={})",
                            levels_to_keep,
                            self.containers.depth()
                        );
                        self.close_containers_to(self.containers.depth() - 1);
                    }
                    Some(Container::List { .. })
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

                        self.close_containers_to(self.containers.depth() - 1);
                    }
                    _ => break,
                }
            }

            // If we kept a list item open, its first-line text may still be buffered.
            // Flush it *before* emitting the blank line node (and its blockquote markers)
            // so byte order matches the source.
            if matches!(self.containers.last(), Some(Container::ListItem { .. })) {
                self.emit_list_item_buffer_if_needed();
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

            self.builder.start_node(SyntaxKind::BLANK_LINE.into());
            self.builder
                .token(SyntaxKind::BLANK_LINE.into(), inner_content);
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
                paragraphs::append_paragraph_line(
                    &mut self.containers,
                    &mut self.builder,
                    line,
                    self.config,
                );
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
                        self.emit_or_buffer_blockquote_marker(
                            info.leading_spaces,
                            info.has_trailing_space,
                        );
                    }
                }

                if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                    // Lazy continuation with the extra > as content
                    paragraphs::append_paragraph_line(
                        &mut self.containers,
                        &mut self.builder,
                        content_at_current_depth,
                        self.config,
                    );
                    self.pos += 1;
                    return true;
                } else {
                    // Start new paragraph with the extra > as content
                    paragraphs::start_paragraph_if_needed(&mut self.containers, &mut self.builder);
                    paragraphs::append_paragraph_line(
                        &mut self.containers,
                        &mut self.builder,
                        content_at_current_depth,
                        self.config,
                    );
                    self.pos += 1;
                    return true;
                }
            }

            // Close paragraph before opening blockquote
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.close_containers_to(self.containers.depth() - 1);
            }

            // Parse marker information for all levels
            let marker_info = parse_blockquote_marker_info(line);

            // First, emit markers for existing blockquote levels (before opening new ones)
            for level in 0..current_bq_depth {
                if let Some(info) = marker_info.get(level) {
                    self.emit_or_buffer_blockquote_marker(
                        info.leading_spaces,
                        info.has_trailing_space,
                    );
                }
            }

            // Then open new blockquotes and emit their markers
            for level in current_bq_depth..bq_depth {
                self.builder.start_node(SyntaxKind::BLOCKQUOTE.into());

                // Emit the marker for this new level
                if let Some(info) = marker_info.get(level) {
                    blockquotes::emit_one_blockquote_marker(
                        &mut self.builder,
                        info.leading_spaces,
                        info.has_trailing_space,
                    );
                }

                self.containers.push(Container::BlockQuote {});
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
                    paragraphs::append_paragraph_line(
                        &mut self.containers,
                        &mut self.builder,
                        line,
                        self.config,
                    );
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
                        // Close containers to the target level, emitting buffers properly
                        self.close_containers_to(level + 1);

                        // Close any open paragraph or list item at this level
                        if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                            self.close_containers_to(self.containers.depth() - 1);
                        }
                        if matches!(self.containers.last(), Some(Container::ListItem { .. })) {
                            self.close_containers_to(self.containers.depth() - 1);
                        }

                        // Check if content is a nested bullet marker
                        if let Some(nested_marker) =
                            is_content_nested_bullet_marker(line, marker_len, spaces_after)
                        {
                            lists::add_list_item_with_nested_empty_list(
                                &mut self.containers,
                                &mut self.builder,
                                line,
                                marker_len,
                                spaces_after,
                                indent_cols,
                                indent_bytes,
                                nested_marker,
                            );
                        } else {
                            lists::add_list_item(
                                &mut self.containers,
                                &mut self.builder,
                                line,
                                marker_len,
                                spaces_after,
                                indent_cols,
                                indent_bytes,
                            );
                        }
                        self.pos += 1;
                        return true;
                    }
                }
            }

            // Not lazy continuation - close paragraph if open
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.close_containers_to(self.containers.depth() - 1);
            }

            // Close blockquotes down to the new depth (must use Parser close to emit buffers)
            self.close_blockquotes_to_depth(bq_depth);

            // Parse the inner content at the new depth
            if bq_depth > 0 {
                // Emit markers at current depth before parsing content
                let marker_info = parse_blockquote_marker_info(line);
                for i in 0..bq_depth {
                    if let Some(info) = marker_info.get(i) {
                        self.emit_or_buffer_blockquote_marker(
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
                Some(Container::ListItem { content_col: _, .. })
            ) {
                let (indent_cols, _) = leading_indent(inner_content);
                let content_indent = self.content_container_indent_to_strip();
                let effective_indent = indent_cols.saturating_sub(content_indent);
                let content_col = match self.containers.last() {
                    Some(Container::ListItem { content_col, .. }) => *content_col,
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
                    self.close_containers_to(self.containers.depth() - 1);
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
                    self.emit_or_buffer_blockquote_marker(
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
                paragraphs::append_paragraph_line(
                    &mut self.containers,
                    &mut self.builder,
                    line,
                    self.config,
                );
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
                    // Close containers to the target level, emitting buffers properly
                    self.close_containers_to(level + 1);

                    // Close any open paragraph or list item at this level
                    if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                        self.close_containers_to(self.containers.depth() - 1);
                    }
                    if matches!(self.containers.last(), Some(Container::ListItem { .. })) {
                        self.close_containers_to(self.containers.depth() - 1);
                    }

                    // Check if content is a nested bullet marker
                    if let Some(nested_marker) =
                        is_content_nested_bullet_marker(line, marker_len, spaces_after)
                    {
                        lists::add_list_item_with_nested_empty_list(
                            &mut self.containers,
                            &mut self.builder,
                            line,
                            marker_len,
                            spaces_after,
                            indent_cols,
                            indent_bytes,
                            nested_marker,
                        );
                    } else {
                        lists::add_list_item(
                            &mut self.containers,
                            &mut self.builder,
                            line,
                            marker_len,
                            spaces_after,
                            indent_cols,
                            indent_bytes,
                        );
                    }
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
        let current_bq_depth = self.current_blockquote_depth();

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
                    ..
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
                                Container::ListItem { content_col, .. } => Some(*content_col),
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
                Container::ListItem { content_col, .. } => {
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

        // Check if we're in a Definition container (with or without an open PLAIN)
        // Continuation lines should be added to PLAIN, not treated as new blocks
        // BUT: Don't treat lines with block element markers as continuations
        if matches!(self.containers.last(), Some(Container::Definition { .. })) {
            // Check if this line starts with any block element marker
            // Use stripped_content so we check AFTER removing footnote/definition indent
            let is_block_element = try_parse_definition_marker(stripped_content).is_some()
                || try_parse_list_marker(stripped_content, self.config).is_some()
                || count_blockquote_markers(stripped_content).0 > 0
                || try_parse_fence_open(stripped_content).is_some()
                || try_parse_div_fence_open(stripped_content).is_some()
                || try_parse_horizontal_rule(stripped_content).is_some()
                || try_parse_atx_heading(stripped_content).is_some()
                || (self.config.extensions.raw_html
                    && try_parse_html_block_start(stripped_content).is_some())
                || (self.config.extensions.raw_tex
                    && try_parse_latex_env_begin(stripped_content).is_some());

            if is_block_element {
                // Close any open Plain block before processing the block element
                // Buffered PLAIN content will be emitted by emit_buffered_plain_if_needed()
                // Fall through to parse the block element
            } else {
                // This is a continuation line - add to PLAIN (start one if needed)
                let full_line = self.lines[self.pos];
                let (text_without_newline, newline_str) = strip_newline(full_line);

                // Buffer the line for later inline parsing
                if let Some(Container::Definition {
                    plain_open,
                    plain_buffer,
                    ..
                }) = self.containers.stack.last_mut()
                {
                    // Include the newline in the buffered text for losslessness
                    let line_with_newline = if !newline_str.is_empty() {
                        format!("{}{}", text_without_newline, newline_str)
                    } else {
                        text_without_newline.to_string()
                    };
                    plain_buffer.push_line(line_with_newline);
                    *plain_open = true; // Mark that we now have an open PLAIN
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
        let at_document_start = self.pos == 0 && self.current_blockquote_depth() == 0;
        let prev_line_blank = if self.pos > 0 {
            let prev_line = self.lines[self.pos - 1];
            let (prev_bq_depth, prev_inner) = count_blockquote_markers(prev_line);
            prev_line.trim().is_empty() || (prev_bq_depth > 0 && prev_inner.trim().is_empty())
        } else {
            false
        };
        let has_blank_before_strict = at_document_start || prev_line_blank;

        // Check for HTML block (if raw_html extension is enabled)
        if self.config.extensions.raw_html
            && let Some(block_type) = try_parse_html_block_start(content)
        {
            log::debug!("Parsed HTML block at line {}: {:?}", self.pos, block_type);

            // Prepare for HTML block
            self.prepare_for_block_element();

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

        // Check if this line looks like a table caption followed by a table
        // If so, try to parse the table (which will include the caption)
        if is_caption_followed_by_table(&self.lines, self.pos) {
            log::debug!("Found caption followed by table at line {}", self.pos);

            // Prepare for table
            self.prepare_for_block_element();

            let caption_start = self.pos;

            // The caption is at self.pos. We need to find where the actual table starts.
            // Skip non-blank lines (caption continuation) and one blank line
            let mut table_pos = self.pos + 1;
            while table_pos < self.lines.len() && !self.lines[table_pos].trim().is_empty() {
                table_pos += 1;
            }
            // Skip one blank line if present
            if table_pos < self.lines.len() && self.lines[table_pos].trim().is_empty() {
                table_pos += 1;
            }

            // Now table_pos should be at the table start (separator, header, or grid fence)
            // Try to parse the table from this position
            if table_pos < self.lines.len() {
                if let Some(lines_consumed) =
                    try_parse_grid_table(&self.lines, table_pos, &mut self.builder, self.config)
                {
                    log::debug!(
                        "Parsed grid table (with caption) starting at line {} ({} lines total from caption)",
                        table_pos,
                        lines_consumed
                    );
                    // lines_consumed is from table_pos, but includes the caption found by find_caption_before_table
                    // So we advance from caption_start by lines_consumed
                    self.pos = caption_start + lines_consumed;
                    return true;
                }

                if let Some(lines_consumed) = try_parse_multiline_table(
                    &self.lines,
                    table_pos,
                    &mut self.builder,
                    self.config,
                ) {
                    log::debug!(
                        "Parsed multiline table (with caption) starting at line {} ({} lines total from caption)",
                        table_pos,
                        lines_consumed
                    );
                    self.pos = caption_start + lines_consumed;
                    return true;
                }

                if let Some(lines_consumed) =
                    try_parse_pipe_table(&self.lines, table_pos, &mut self.builder, self.config)
                {
                    log::debug!(
                        "Parsed pipe table (with caption) starting at line {} ({} lines total from caption)",
                        table_pos,
                        lines_consumed
                    );
                    self.pos = caption_start + lines_consumed;
                    return true;
                }

                if let Some(lines_consumed) =
                    try_parse_simple_table(&self.lines, table_pos, &mut self.builder, self.config)
                {
                    log::debug!(
                        "Parsed simple table (with caption) starting at line {} ({} lines total from caption)",
                        table_pos,
                        lines_consumed
                    );
                    self.pos = caption_start + lines_consumed;
                    return true;
                }
            }
        }

        if has_blank_before {
            // Try to parse grid table (check before pipe/simple since + is most specific)
            if let Some(lines_consumed) =
                try_parse_grid_table(&self.lines, self.pos, &mut self.builder, self.config)
            {
                log::debug!(
                    "Parsed grid table at line {} ({} lines)",
                    self.pos,
                    lines_consumed
                );
                // Prepare for grid table
                self.prepare_for_block_element();
                self.pos += lines_consumed;
                return true;
            }

            // Try to parse multiline table (check before pipe/simple since full-width dashes are specific)
            if let Some(lines_consumed) =
                try_parse_multiline_table(&self.lines, self.pos, &mut self.builder, self.config)
            {
                log::debug!(
                    "Parsed multiline table at line {} ({} lines)",
                    self.pos,
                    lines_consumed
                );
                self.prepare_for_block_element();
                self.pos += lines_consumed;
                return true;
            }

            // Try to parse pipe table (check before simple table since pipes are more specific)
            if let Some(lines_consumed) =
                try_parse_pipe_table(&self.lines, self.pos, &mut self.builder, self.config)
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
                try_parse_simple_table(&self.lines, self.pos, &mut self.builder, self.config)
            {
                log::debug!(
                    "Parsed simple table at line {} ({} lines)",
                    self.pos,
                    lines_consumed
                );
                self.pos += lines_consumed;
                return true;
            }

            // Try dispatcher for blocks that need blank line before
            // OR that can interrupt paragraphs (e.g., fenced code blocks)

            // Calculate list indent info for blocks that need it (e.g., fenced code)
            use super::blocks::lists;
            use super::blocks::paragraphs;
            let list_indent_info = if lists::in_list(&self.containers) {
                let content_col = paragraphs::current_content_col(&self.containers);
                if content_col > 0 {
                    Some(super::block_dispatcher::ListIndentInfo { content_col })
                } else {
                    None
                }
            } else {
                None
            };

            // Get next line for lookahead (used by setext headings)
            let next_line = if self.pos + 1 < self.lines.len() {
                Some(self.lines[self.pos + 1])
            } else {
                None
            };

            let block_ctx = BlockContext {
                content,
                has_blank_before,
                at_document_start,
                blockquote_depth: self.current_blockquote_depth(),
                config: self.config,
                containers: &self.containers,
                content_indent,
                list_indent_info,
                next_line,
            };

            if let Some((parser_idx, detection)) =
                self.block_registry
                    .detect(&block_ctx, &self.lines, self.pos)
            {
                // Drop context to release borrow before prepare

                // Handle based on detection result
                match detection {
                    BlockDetectionResult::YesCanInterrupt => {
                        // Block can interrupt paragraphs
                        // Emit list item buffer if needed
                        self.emit_list_item_buffer_if_needed();

                        // Close paragraph if one is open
                        if self.is_paragraph_open() {
                            self.close_containers_to(self.containers.depth() - 1);
                        }

                        // Recreate context for parsing
                        let list_indent_info = if lists::in_list(&self.containers) {
                            let content_col = paragraphs::current_content_col(&self.containers);
                            if content_col > 0 {
                                Some(super::block_dispatcher::ListIndentInfo { content_col })
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        let next_line = if self.pos + 1 < self.lines.len() {
                            Some(self.lines[self.pos + 1])
                        } else {
                            None
                        };

                        let block_ctx = BlockContext {
                            content,
                            has_blank_before,
                            at_document_start,
                            blockquote_depth: self.current_blockquote_depth(),
                            config: self.config,
                            containers: &self.containers,
                            content_indent,
                            list_indent_info,
                            next_line,
                        };

                        let lines_consumed = self.block_registry.parse(
                            parser_idx,
                            &block_ctx,
                            &mut self.builder,
                            &self.lines,
                            self.pos,
                        );
                        self.pos += lines_consumed;
                        return true;
                    }
                    BlockDetectionResult::Yes => {
                        // Block needs blank line before (normal case)
                        // Prepare for block element (flush buffers, close paragraphs)
                        self.prepare_for_block_element();

                        // Recreate context for parsing
                        let list_indent_info = if lists::in_list(&self.containers) {
                            let content_col = paragraphs::current_content_col(&self.containers);
                            if content_col > 0 {
                                Some(super::block_dispatcher::ListIndentInfo { content_col })
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        let next_line = if self.pos + 1 < self.lines.len() {
                            Some(self.lines[self.pos + 1])
                        } else {
                            None
                        };

                        let block_ctx = BlockContext {
                            content,
                            has_blank_before,
                            at_document_start,
                            blockquote_depth: self.current_blockquote_depth(),
                            config: self.config,
                            containers: &self.containers,
                            content_indent,
                            list_indent_info,
                            next_line,
                        };

                        let lines_consumed = self.block_registry.parse(
                            parser_idx,
                            &block_ctx,
                            &mut self.builder,
                            &self.lines,
                            self.pos,
                        );
                        self.pos += lines_consumed;
                        return true;
                    }
                    BlockDetectionResult::No => {
                        // Should not happen since detect() returned Some
                        unreachable!()
                    }
                }
            }
        }

        // Try dispatcher for blocks that can interrupt paragraphs (even without blank line before)
        // This is called OUTSIDE the has_blank_before check
        use super::blocks::lists;
        use super::blocks::paragraphs;
        let list_indent_info = if lists::in_list(&self.containers) {
            let content_col = paragraphs::current_content_col(&self.containers);
            if content_col > 0 {
                Some(super::block_dispatcher::ListIndentInfo { content_col })
            } else {
                None
            }
        } else {
            None
        };

        let next_line = if self.pos + 1 < self.lines.len() {
            Some(self.lines[self.pos + 1])
        } else {
            None
        };

        let block_ctx = BlockContext {
            content,
            has_blank_before,
            at_document_start,
            blockquote_depth: self.current_blockquote_depth(),
            config: self.config,
            containers: &self.containers,
            content_indent,
            list_indent_info,
            next_line,
        };

        if let Some((parser_idx, detection)) =
            self.block_registry
                .detect(&block_ctx, &self.lines, self.pos)
        {
            // Check if this is a block that can interrupt paragraphs
            if matches!(detection, BlockDetectionResult::YesCanInterrupt) {
                // Block can interrupt paragraphs
                // Emit list item buffer if needed
                self.emit_list_item_buffer_if_needed();

                // Close paragraph if one is open
                if self.is_paragraph_open() {
                    self.close_containers_to(self.containers.depth() - 1);
                }

                // Recreate context for parsing
                let list_indent_info = if lists::in_list(&self.containers) {
                    let content_col = paragraphs::current_content_col(&self.containers);
                    if content_col > 0 {
                        Some(super::block_dispatcher::ListIndentInfo { content_col })
                    } else {
                        None
                    }
                } else {
                    None
                };

                let next_line = if self.pos + 1 < self.lines.len() {
                    Some(self.lines[self.pos + 1])
                } else {
                    None
                };

                let block_ctx = BlockContext {
                    content,
                    has_blank_before,
                    at_document_start,
                    blockquote_depth: self.current_blockquote_depth(),
                    config: self.config,
                    containers: &self.containers,
                    content_indent,
                    list_indent_info,
                    next_line,
                };

                let lines_consumed = self.block_registry.parse(
                    parser_idx,
                    &block_ctx,
                    &mut self.builder,
                    &self.lines,
                    self.pos,
                );
                self.pos += lines_consumed;
                return true;
            }
        }

        // Check for footnote definition: [^id]: content
        // Similar to list items - marker followed by content that can span multiple lines
        // Must check BEFORE reference definitions since both start with [
        if let Some((id, content_start)) = try_parse_footnote_marker(content) {
            log::debug!("Parsed footnote definition at line {}: [^{}]", self.pos, id);

            // Close paragraph if one is open
            self.close_paragraph_if_open();

            // Close previous footnote if one is open
            while matches!(
                self.containers.last(),
                Some(Container::FootnoteDefinition { .. })
            ) {
                self.close_containers_to(self.containers.depth() - 1);
            }

            // Start the footnote definition container
            self.builder
                .start_node(SyntaxKind::FOOTNOTE_DEFINITION.into());

            // Emit the marker
            let marker_text = &content[..content_start];
            self.builder
                .token(SyntaxKind::FOOTNOTE_REFERENCE.into(), marker_text);

            // Calculate content column (minimum 4 spaces for continuation)
            // The first line can start right after the marker, but subsequent lines
            // need at least 4 spaces of indentation
            let content_col = 4;
            self.containers
                .push(Container::FootnoteDefinition { content_col });

            // Parse the first line content (if any)
            let first_line_content = &content[content_start..];
            if !first_line_content.trim().is_empty() {
                paragraphs::start_paragraph_if_needed(&mut self.containers, &mut self.builder);
                paragraphs::append_paragraph_line(
                    &mut self.containers,
                    &mut self.builder,
                    first_line_content,
                    self.config,
                );
            }

            self.pos += 1;
            return true;
        }

        // Check for reference definition: [label]: url "title"
        // These can appear anywhere in the document (no blank line needed)
        // Try dispatcher first

        let next_line = if self.pos + 1 < self.lines.len() {
            Some(self.lines[self.pos + 1])
        } else {
            None
        };

        let block_ctx = BlockContext {
            content,
            has_blank_before,
            at_document_start,
            blockquote_depth: self.current_blockquote_depth(),
            config: self.config,
            containers: &self.containers,
            content_indent,
            list_indent_info: None, // Not needed for reference definitions
            next_line,
        };

        if let Some((parser_idx, _detection)) =
            self.block_registry
                .detect(&block_ctx, &self.lines, self.pos)
        {
            // Reference definitions don't need preparation
            let lines_consumed = self.block_registry.parse(
                parser_idx,
                &block_ctx,
                &mut self.builder,
                &self.lines,
                self.pos,
            );
            self.pos += lines_consumed;
            return true;
        }

        // Check for indented code block
        // Inside a footnote, content needs 4 spaces for code (8 total in raw line)
        // BUT: Don't treat as code if it's a list marker (list takes precedence)
        if has_blank_before_strict
            && is_indented_code_line(content)
            && try_parse_list_marker(content, self.config).is_none()
        {
            // Prepare for indented code block
            self.prepare_for_block_element();

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

        // Check for fenced div opening
        if has_blank_before && let Some(div_fence) = try_parse_div_fence_open(content) {
            log::debug!(
                "Parsed fenced div at line {}: {} colons",
                self.pos,
                div_fence.fence_count
            );

            // Prepare for fenced div
            self.prepare_for_block_element();

            // Start FencedDiv node
            self.builder.start_node(SyntaxKind::FENCED_DIV.into());

            // Emit opening fence with attributes as child node to avoid duplication
            self.builder.start_node(SyntaxKind::DIV_FENCE_OPEN.into());

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
            self.builder.start_node(SyntaxKind::DIV_INFO.into());
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
            self.close_paragraph_if_open();

            // Emit closing fence - parse to avoid newline duplication
            self.builder.start_node(SyntaxKind::DIV_FENCE_CLOSE.into());

            // Get original full line
            let full_line = self.lines[self.pos];
            let trimmed = full_line.trim_start();

            // Emit leading whitespace if present
            let leading_ws_len = full_line.len() - trimmed.len();
            if leading_ws_len > 0 {
                self.builder
                    .token(SyntaxKind::WHITESPACE.into(), &full_line[..leading_ws_len]);
            }

            // Emit fence content without newline (handle both CRLF and LF)
            let (content_without_newline, line_ending) = strip_newline(trimmed);

            self.builder
                .token(SyntaxKind::TEXT.into(), content_without_newline);

            // Emit newline separately if present
            if !line_ending.is_empty() {
                self.builder.token(SyntaxKind::NEWLINE.into(), line_ending);
            }
            self.builder.finish_node(); // DivFenceClose

            // Pop the FencedDiv container (this will finish the FencedDiv node)
            self.close_containers_to(self.containers.depth() - 1);

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

            // Prepare for LaTeX environment
            self.prepare_for_block_element();

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
            if indent_cols >= 4 && !lists::in_list(&self.containers) {
                // Code block at top-level, treat as paragraph
                paragraphs::start_paragraph_if_needed(&mut self.containers, &mut self.builder);
                paragraphs::append_paragraph_line(
                    &mut self.containers,
                    &mut self.builder,
                    content,
                    self.config,
                );
                self.pos += 1;
                return true;
            }

            // Lists can only interrupt paragraphs if there was a blank line before
            // (Per Pandoc spec - lists need blank lines to start interrupting paragraphs)
            if self.is_paragraph_open() {
                if !has_blank_before {
                    // List cannot interrupt paragraph without blank line - treat as paragraph content
                    paragraphs::append_paragraph_line(
                        &mut self.containers,
                        &mut self.builder,
                        line_to_append.unwrap_or(content),
                        self.config,
                    );
                    self.pos += 1;
                    return true;
                }

                // Blank line before - can interrupt paragraph
                self.close_containers_to(self.containers.depth() - 1);
            }

            // Close any open PLAIN node in a Definition before starting a list
            // This ensures buffered PLAIN content is emitted before the list
            if matches!(
                self.containers.last(),
                Some(Container::Definition {
                    plain_open: true,
                    ..
                })
            ) {
                // Emit buffered PLAIN content but keep Definition open
                self.emit_buffered_plain_if_needed();
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
                        // Close containers to the target level, emitting buffers properly
                        self.close_containers_to(level + 1);

                        // Close any open paragraph or list item at this level
                        if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                            self.close_containers_to(self.containers.depth() - 1);
                        }
                        if matches!(self.containers.last(), Some(Container::ListItem { .. })) {
                            self.close_containers_to(self.containers.depth() - 1);
                        }

                        if let Some(indent_str) = indent_to_emit {
                            self.builder
                                .token(SyntaxKind::WHITESPACE.into(), indent_str);
                        }

                        // Check if content is a nested bullet marker
                        if let Some(nested_marker) =
                            is_content_nested_bullet_marker(content, marker_len, spaces_after)
                        {
                            lists::add_list_item_with_nested_empty_list(
                                &mut self.containers,
                                &mut self.builder,
                                content,
                                marker_len,
                                spaces_after,
                                indent_cols,
                                indent_bytes,
                                nested_marker,
                            );
                        } else {
                            lists::add_list_item(
                                &mut self.containers,
                                &mut self.builder,
                                content,
                                marker_len,
                                spaces_after,
                                indent_cols,
                                indent_bytes,
                            );
                        }
                        self.pos += 1;
                        return true;
                    }
                }

                // No exact match - start new nested list.
                // Flush buffered item text first so it stays before the nested LIST in source order.
                self.emit_list_item_buffer_if_needed();

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
                // Close containers to the target level, emitting buffers properly
                self.close_containers_to(level + 1);

                // Close any open paragraph or list item at this level
                if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                    self.close_containers_to(self.containers.depth() - 1);
                }
                if matches!(self.containers.last(), Some(Container::ListItem { .. })) {
                    self.close_containers_to(self.containers.depth() - 1);
                }

                if let Some(indent_str) = indent_to_emit {
                    self.builder
                        .token(SyntaxKind::WHITESPACE.into(), indent_str);
                }

                // Check if content is a nested bullet marker
                if let Some(nested_marker) =
                    is_content_nested_bullet_marker(content, marker_len, spaces_after)
                {
                    lists::add_list_item_with_nested_empty_list(
                        &mut self.containers,
                        &mut self.builder,
                        content,
                        marker_len,
                        spaces_after,
                        indent_cols,
                        indent_bytes,
                        nested_marker,
                    );
                } else {
                    lists::add_list_item(
                        &mut self.containers,
                        &mut self.builder,
                        content,
                        marker_len,
                        spaces_after,
                        indent_cols,
                        indent_bytes,
                    );
                }
                self.pos += 1;
                return true;
            }

            // No match and not nested - start new top-level list.
            // Close existing containers via Parser so buffers are emitted.
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.close_containers_to(self.containers.depth() - 1);
            }
            while matches!(self.containers.last(), Some(Container::ListItem { .. })) {
                self.close_containers_to(self.containers.depth() - 1);
            }
            while matches!(self.containers.last(), Some(Container::List { .. })) {
                self.close_containers_to(self.containers.depth() - 1);
            }

            self.builder.start_node(SyntaxKind::LIST.into());
            if let Some(indent_str) = indent_to_emit {
                self.builder
                    .token(SyntaxKind::WHITESPACE.into(), indent_str);
            }
            self.containers.push(Container::List {
                marker: marker.clone(),
                base_indent_cols: indent_cols,
                has_blank_between_items: false,
            });

            // Check if content is a nested bullet marker (e.g., "- *")
            if let Some(nested_marker) =
                is_content_nested_bullet_marker(content, marker_len, spaces_after)
            {
                lists::add_list_item_with_nested_empty_list(
                    &mut self.containers,
                    &mut self.builder,
                    content,
                    marker_len,
                    spaces_after,
                    indent_cols,
                    indent_bytes,
                    nested_marker,
                );
            } else {
                lists::add_list_item(
                    &mut self.containers,
                    &mut self.builder,
                    content,
                    marker_len,
                    spaces_after,
                    indent_cols,
                    indent_bytes,
                );
            }
            self.pos += 1;
            return true;
        }

        // Definition list marker?
        if let Some((marker_char, indent, spaces_after)) = try_parse_definition_marker(content) {
            // Close paragraph before starting definition
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.close_containers_to(self.containers.depth() - 1);
            }

            // Start definition list if not in one
            if !definition_lists::in_definition_list(&self.containers) {
                self.builder.start_node(SyntaxKind::DEFINITION_LIST.into());
                self.containers.push(Container::DefinitionList {});
            }

            // Close previous definition if one is open (but keep DefinitionItem open)
            if matches!(self.containers.last(), Some(Container::Definition { .. })) {
                self.close_containers_to(self.containers.depth() - 1);
            }

            // Start new definition item if not in one
            if !matches!(
                self.containers.last(),
                Some(Container::DefinitionItem { .. })
            ) {
                self.builder.start_node(SyntaxKind::DEFINITION_ITEM.into());
                self.containers.push(Container::DefinitionItem {});
            }

            // Start Definition node
            self.builder.start_node(SyntaxKind::DEFINITION.into());

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

            // Create buffer for accumulating PLAIN content
            let mut plain_buffer = TextBuffer::new();

            if has_content {
                // Buffer content WITH newline, emit later with inline parsing
                let current_line = self.lines[self.pos];
                let (_, newline_str) = strip_newline(current_line);
                let line_with_newline = if !newline_str.is_empty() {
                    format!("{}{}", after_marker_and_spaces.trim_end(), newline_str)
                } else {
                    after_marker_and_spaces.trim_end().to_string()
                };
                plain_buffer.push_line(line_with_newline);
                // PLAIN node will be emitted when Definition closes
            }

            self.containers.push(Container::Definition {
                content_col,
                plain_open: has_content,
                plain_buffer,
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
                self.close_containers_to(self.containers.depth() - 1);
            }

            // Start definition list if not in one
            if !definition_lists::in_definition_list(&self.containers) {
                self.builder.start_node(SyntaxKind::DEFINITION_LIST.into());
                self.containers.push(Container::DefinitionList {});
            }

            // Close previous definition item if exists
            while matches!(
                self.containers.last(),
                Some(Container::Definition { .. }) | Some(Container::DefinitionItem { .. })
            ) {
                self.close_containers_to(self.containers.depth() - 1);
            }

            // Start new definition item
            self.builder.start_node(SyntaxKind::DEFINITION_ITEM.into());
            self.containers.push(Container::DefinitionItem {});

            // Emit term
            emit_term(&mut self.builder, content, self.config);
            self.pos += 1;

            // Emit blank lines between term and definition marker
            for _ in 0..blank_count {
                if self.pos < self.lines.len() {
                    let blank_line = self.lines[self.pos];
                    self.builder.start_node(SyntaxKind::BLANK_LINE.into());
                    self.builder
                        .token(SyntaxKind::BLANK_LINE.into(), blank_line);
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
            self.close_paragraph_if_open();

            let new_pos = parse_line_block(&self.lines, self.pos, &mut self.builder, self.config);
            self.pos = new_pos;
            return true;
        }

        // Paragraph or list item continuation
        // Check if we're inside a ListItem - if so, buffer the content instead of emitting
        if matches!(self.containers.last(), Some(Container::ListItem { .. })) {
            log::debug!(
                "Inside ListItem - buffering content: {:?}",
                line_to_append.unwrap_or(self.lines[self.pos]).trim_end()
            );
            // Inside list item - buffer content for later parsing
            let line = line_to_append.unwrap_or(self.lines[self.pos]);

            // Add line to buffer in the ListItem container
            if let Some(Container::ListItem { buffer, .. }) = self.containers.stack.last_mut() {
                buffer.push_text(line);
            }

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
        paragraphs::append_paragraph_line(
            &mut self.containers,
            &mut self.builder,
            line,
            self.config,
        );
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
