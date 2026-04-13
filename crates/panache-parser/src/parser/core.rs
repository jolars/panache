use crate::options::ParserOptions;
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::GreenNodeBuilder;

use super::block_dispatcher::{
    BlockContext, BlockDetectionResult, BlockEffect, BlockParserRegistry, BlockQuotePrepared,
    PreparedBlockMatch,
};
use super::blocks::blockquotes;
use super::blocks::code_blocks;
use super::blocks::definition_lists;
use super::blocks::fenced_divs;
use super::blocks::headings::{emit_atx_heading, try_parse_atx_heading};
use super::blocks::line_blocks;
use super::blocks::lists;
use super::blocks::paragraphs;
use super::blocks::raw_blocks::{extract_environment_name, is_inline_math_environment};
use super::utils::container_stack;
use super::utils::helpers::{split_lines_inclusive, strip_newline};
use super::utils::inline_emission;
use super::utils::marker_utils;
use super::utils::text_buffer;

use super::blocks::blockquotes::strip_n_blockquote_markers;
use super::utils::continuation::ContinuationPolicy;
use container_stack::{Container, ContainerStack, byte_index_at_column, leading_indent};
use definition_lists::{emit_definition_marker, emit_term};
use line_blocks::{parse_line_block, try_parse_line_block_start};
use lists::{
    ListItemEmissionInput, ListMarker, is_content_nested_bullet_marker, start_nested_list,
    try_parse_list_marker,
};
use marker_utils::{count_blockquote_markers, parse_blockquote_marker_info};
use text_buffer::TextBuffer;

const GITHUB_ALERT_MARKERS: [&str; 5] = [
    "[!TIP]",
    "[!WARNING]",
    "[!IMPORTANT]",
    "[!CAUTION]",
    "[!NOTE]",
];

pub struct Parser<'a> {
    lines: Vec<&'a str>,
    pos: usize,
    builder: GreenNodeBuilder<'static>,
    containers: ContainerStack,
    config: &'a ParserOptions,
    block_registry: BlockParserRegistry,
    /// True when the previous block was a metadata block (YAML, Pandoc title, or MMD title).
    /// The first line after a metadata block is treated as if it has a blank line before it,
    /// matching Pandoc's behavior of allowing headings etc. directly after frontmatter.
    after_metadata_block: bool,
}

impl<'a> Parser<'a> {
    pub fn new(input: &'a str, config: &'a ParserOptions) -> Self {
        // Use split_lines_inclusive to preserve line endings (both LF and CRLF)
        let lines = split_lines_inclusive(input);
        Self {
            lines,
            pos: 0,
            builder: GreenNodeBuilder::new(),
            containers: ContainerStack::new(),
            config,
            block_registry: BlockParserRegistry::new(),
            after_metadata_block: false,
        }
    }

    pub fn parse(mut self) -> SyntaxNode {
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
                Some(Container::Paragraph { buffer, .. }) if !buffer.is_empty() => {
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
                    let text = plain_buffer.get_accumulated_text();
                    let line_without_newline = text
                        .strip_suffix("\r\n")
                        .or_else(|| text.strip_suffix('\n'));
                    if let Some(line) = line_without_newline
                        && !line.contains('\n')
                        && !line.contains('\r')
                        && let Some(level) = try_parse_atx_heading(line)
                    {
                        emit_atx_heading(&mut self.builder, &text, level, self.config);
                    } else {
                        // Emit PLAIN node with buffered inline-parsed content
                        self.builder.start_node(SyntaxKind::PLAIN.into());
                        inline_emission::emit_inlines(&mut self.builder, &text, self.config);
                        self.builder.finish_node();
                    }

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
            let text = plain_buffer.get_accumulated_text();
            let line_without_newline = text
                .strip_suffix("\r\n")
                .or_else(|| text.strip_suffix('\n'));
            if let Some(line) = line_without_newline
                && !line.contains('\n')
                && !line.contains('\r')
                && let Some(level) = try_parse_atx_heading(line)
            {
                emit_atx_heading(&mut self.builder, &text, level, self.config);
            } else {
                // Emit PLAIN node with buffered inline-parsed content
                self.builder.start_node(SyntaxKind::PLAIN.into());
                inline_emission::emit_inlines(&mut self.builder, &text, self.config);
                self.builder.finish_node();
            }
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

    fn active_alert_blockquote_depth(&self) -> Option<usize> {
        self.containers.stack.iter().rev().find_map(|c| match c {
            Container::Alert { blockquote_depth } => Some(*blockquote_depth),
            _ => None,
        })
    }

    fn in_active_alert(&self) -> bool {
        self.active_alert_blockquote_depth().is_some()
    }

    fn alert_marker_from_content(content: &str) -> Option<&'static str> {
        let (without_newline, _) = strip_newline(content);
        let trimmed = without_newline.trim();
        GITHUB_ALERT_MARKERS
            .into_iter()
            .find(|marker| *marker == trimmed)
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

    fn handle_footnote_open_effect(
        &mut self,
        block_match: &super::block_dispatcher::PreparedBlockMatch,
        content: &str,
    ) {
        let content_start = block_match
            .payload
            .as_ref()
            .and_then(|p| p.downcast_ref::<super::block_dispatcher::FootnoteDefinitionPrepared>())
            .map(|p| p.content_start)
            .unwrap_or(0);

        while matches!(
            self.containers.last(),
            Some(Container::FootnoteDefinition { .. })
        ) {
            self.close_containers_to(self.containers.depth() - 1);
        }

        let content_col = 4;
        self.containers
            .push(Container::FootnoteDefinition { content_col });

        if content_start > 0 {
            let first_line_content = &content[content_start..];
            if !first_line_content.trim().is_empty() {
                paragraphs::start_paragraph_if_needed(&mut self.containers, &mut self.builder);
                paragraphs::append_paragraph_line(
                    &mut self.containers,
                    &mut self.builder,
                    first_line_content,
                    self.config,
                );
            } else {
                let (_, newline_str) = strip_newline(content);
                if !newline_str.is_empty() {
                    self.builder.token(SyntaxKind::NEWLINE.into(), newline_str);
                }
            }
        }
    }

    fn handle_list_open_effect(
        &mut self,
        block_match: &super::block_dispatcher::PreparedBlockMatch,
        content: &str,
        indent_to_emit: Option<&str>,
    ) {
        use super::block_dispatcher::ListPrepared;

        let prepared = block_match
            .payload
            .as_ref()
            .and_then(|p| p.downcast_ref::<ListPrepared>());
        let Some(prepared) = prepared else {
            return;
        };

        if prepared.indent_cols >= 4 && !lists::in_list(&self.containers) {
            paragraphs::start_paragraph_if_needed(&mut self.containers, &mut self.builder);
            paragraphs::append_paragraph_line(
                &mut self.containers,
                &mut self.builder,
                content,
                self.config,
            );
            return;
        }

        if self.is_paragraph_open() {
            if !block_match.detection.eq(&BlockDetectionResult::Yes) {
                paragraphs::append_paragraph_line(
                    &mut self.containers,
                    &mut self.builder,
                    content,
                    self.config,
                );
                return;
            }
            self.close_containers_to(self.containers.depth() - 1);
        }

        if matches!(
            self.containers.last(),
            Some(Container::Definition {
                plain_open: true,
                ..
            })
        ) {
            self.emit_buffered_plain_if_needed();
        }

        let matched_level = lists::find_matching_list_level(
            &self.containers,
            &prepared.marker,
            prepared.indent_cols,
        );
        let list_item = ListItemEmissionInput {
            content,
            marker_len: prepared.marker_len,
            spaces_after_cols: prepared.spaces_after_cols,
            spaces_after_bytes: prepared.spaces_after,
            indent_cols: prepared.indent_cols,
            indent_bytes: prepared.indent_bytes,
        };
        let current_content_col = paragraphs::current_content_col(&self.containers);
        let deep_ordered_matched_level = matched_level
            .and_then(|level| self.containers.stack.get(level).map(|c| (level, c)))
            .and_then(|(level, container)| match container {
                Container::List {
                    marker: list_marker,
                    base_indent_cols,
                    ..
                } if matches!(
                    (&prepared.marker, list_marker),
                    (ListMarker::Ordered(_), ListMarker::Ordered(_))
                ) && prepared.indent_cols >= 4
                    && *base_indent_cols >= 4
                    && prepared.indent_cols.abs_diff(*base_indent_cols) <= 3 =>
                {
                    Some(level)
                }
                _ => None,
            });

        if deep_ordered_matched_level.is_none()
            && current_content_col > 0
            && prepared.indent_cols >= current_content_col
        {
            if let Some(level) = matched_level
                && let Some(Container::List {
                    base_indent_cols, ..
                }) = self.containers.stack.get(level)
                && prepared.indent_cols == *base_indent_cols
            {
                let num_parent_lists = self.containers.stack[..level]
                    .iter()
                    .filter(|c| matches!(c, Container::List { .. }))
                    .count();

                if num_parent_lists > 0 {
                    self.close_containers_to(level + 1);

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

                    if let Some(nested_marker) = prepared.nested_marker {
                        lists::add_list_item_with_nested_empty_list(
                            &mut self.containers,
                            &mut self.builder,
                            &list_item,
                            nested_marker,
                        );
                    } else {
                        lists::add_list_item(&mut self.containers, &mut self.builder, &list_item);
                    }
                    return;
                }
            }

            self.emit_list_item_buffer_if_needed();

            start_nested_list(
                &mut self.containers,
                &mut self.builder,
                &prepared.marker,
                &list_item,
                indent_to_emit,
            );
            return;
        }

        if let Some(level) = matched_level {
            self.close_containers_to(level + 1);

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

            if let Some(nested_marker) = prepared.nested_marker {
                lists::add_list_item_with_nested_empty_list(
                    &mut self.containers,
                    &mut self.builder,
                    &list_item,
                    nested_marker,
                );
            } else {
                lists::add_list_item(&mut self.containers, &mut self.builder, &list_item);
            }
            return;
        }

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
            marker: prepared.marker.clone(),
            base_indent_cols: prepared.indent_cols,
            has_blank_between_items: false,
        });

        if let Some(nested_marker) = prepared.nested_marker {
            lists::add_list_item_with_nested_empty_list(
                &mut self.containers,
                &mut self.builder,
                &list_item,
                nested_marker,
            );
        } else {
            lists::add_list_item(&mut self.containers, &mut self.builder, &list_item);
        }
    }

    fn handle_definition_list_effect(
        &mut self,
        block_match: &super::block_dispatcher::PreparedBlockMatch,
        content: &str,
        indent_to_emit: Option<&str>,
    ) {
        use super::block_dispatcher::DefinitionPrepared;

        let prepared = block_match
            .payload
            .as_ref()
            .and_then(|p| p.downcast_ref::<DefinitionPrepared>());
        let Some(prepared) = prepared else {
            return;
        };

        match prepared {
            DefinitionPrepared::Definition {
                marker_char,
                indent,
                spaces_after,
                spaces_after_cols,
                has_content,
            } => {
                self.emit_buffered_plain_if_needed();

                while matches!(self.containers.last(), Some(Container::ListItem { .. })) {
                    self.close_containers_to(self.containers.depth() - 1);
                }
                while matches!(self.containers.last(), Some(Container::List { .. })) {
                    self.close_containers_to(self.containers.depth() - 1);
                }

                if matches!(self.containers.last(), Some(Container::Definition { .. })) {
                    self.close_containers_to(self.containers.depth() - 1);
                }

                if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                    self.close_containers_to(self.containers.depth() - 1);
                }

                // A definition marker cannot start a new definition item without a term.
                // If the preceding term/item was closed by a blank line but we are still
                // inside the same definition list, reopen a definition item for continuation.
                if definition_lists::in_definition_list(&self.containers)
                    && !matches!(
                        self.containers.last(),
                        Some(Container::DefinitionItem { .. })
                    )
                {
                    self.builder.start_node(SyntaxKind::DEFINITION_ITEM.into());
                    self.containers.push(Container::DefinitionItem {});
                }

                if !definition_lists::in_definition_list(&self.containers) {
                    self.builder.start_node(SyntaxKind::DEFINITION_LIST.into());
                    self.containers.push(Container::DefinitionList {});
                }

                if !matches!(
                    self.containers.last(),
                    Some(Container::DefinitionItem { .. })
                ) {
                    self.builder.start_node(SyntaxKind::DEFINITION_ITEM.into());
                    self.containers.push(Container::DefinitionItem {});
                }

                self.builder.start_node(SyntaxKind::DEFINITION.into());

                if let Some(indent_str) = indent_to_emit {
                    self.builder
                        .token(SyntaxKind::WHITESPACE.into(), indent_str);
                }

                emit_definition_marker(&mut self.builder, *marker_char, *indent);
                let indent_bytes = byte_index_at_column(content, *indent);
                if *spaces_after > 0 {
                    let space_start = indent_bytes + 1;
                    let space_end = space_start + *spaces_after;
                    if space_end <= content.len() {
                        self.builder.token(
                            SyntaxKind::WHITESPACE.into(),
                            &content[space_start..space_end],
                        );
                    }
                }

                if !*has_content {
                    let current_line = self.lines[self.pos];
                    let (_, newline_str) = strip_newline(current_line);
                    if !newline_str.is_empty() {
                        self.builder.token(SyntaxKind::NEWLINE.into(), newline_str);
                    }
                }

                let content_col = *indent + 1 + *spaces_after_cols;
                let content_start_bytes = indent_bytes + 1 + *spaces_after;
                let after_marker_and_spaces = content.get(content_start_bytes..).unwrap_or("");
                let mut plain_buffer = TextBuffer::new();
                let mut definition_pushed = false;

                if *has_content {
                    let current_line = self.lines[self.pos];
                    let (trimmed_line, _) = strip_newline(current_line);

                    let content_start = content_start_bytes.min(trimmed_line.len());
                    let content_slice = &trimmed_line[content_start..];
                    let content_line = &current_line[content_start_bytes.min(current_line.len())..];

                    let (blockquote_depth, inner_blockquote_content) =
                        count_blockquote_markers(content_line);

                    let should_start_list_from_first_line = self
                        .lines
                        .get(self.pos + 1)
                        .map(|next_line| {
                            let (next_without_newline, _) = strip_newline(next_line);
                            if next_without_newline.trim().is_empty() {
                                return false;
                            }

                            let (next_indent_cols, _) = leading_indent(next_without_newline);
                            next_indent_cols >= content_col
                        })
                        .unwrap_or(false);

                    if blockquote_depth > 0 {
                        self.containers.push(Container::Definition {
                            content_col,
                            plain_open: false,
                            plain_buffer: TextBuffer::new(),
                        });
                        definition_pushed = true;

                        let marker_info = parse_blockquote_marker_info(content_line);
                        for level in 0..blockquote_depth {
                            self.builder.start_node(SyntaxKind::BLOCK_QUOTE.into());
                            if let Some(info) = marker_info.get(level) {
                                blockquotes::emit_one_blockquote_marker(
                                    &mut self.builder,
                                    info.leading_spaces,
                                    info.has_trailing_space,
                                );
                            }
                            self.containers.push(Container::BlockQuote {});
                        }

                        if !inner_blockquote_content.trim().is_empty() {
                            paragraphs::start_paragraph_if_needed(
                                &mut self.containers,
                                &mut self.builder,
                            );
                            paragraphs::append_paragraph_line(
                                &mut self.containers,
                                &mut self.builder,
                                inner_blockquote_content,
                                self.config,
                            );
                        }
                    } else if let Some(marker_match) =
                        try_parse_list_marker(content_slice, self.config)
                        && should_start_list_from_first_line
                    {
                        self.containers.push(Container::Definition {
                            content_col,
                            plain_open: false,
                            plain_buffer: TextBuffer::new(),
                        });
                        definition_pushed = true;

                        let (indent_cols, indent_bytes) = leading_indent(content_line);
                        self.builder.start_node(SyntaxKind::LIST.into());
                        self.containers.push(Container::List {
                            marker: marker_match.marker.clone(),
                            base_indent_cols: indent_cols,
                            has_blank_between_items: false,
                        });

                        let list_item = ListItemEmissionInput {
                            content: content_line,
                            marker_len: marker_match.marker_len,
                            spaces_after_cols: marker_match.spaces_after_cols,
                            spaces_after_bytes: marker_match.spaces_after_bytes,
                            indent_cols,
                            indent_bytes,
                        };

                        if let Some(nested_marker) = is_content_nested_bullet_marker(
                            content_line,
                            marker_match.marker_len,
                            marker_match.spaces_after_bytes,
                        ) {
                            lists::add_list_item_with_nested_empty_list(
                                &mut self.containers,
                                &mut self.builder,
                                &list_item,
                                nested_marker,
                            );
                        } else {
                            lists::add_list_item(
                                &mut self.containers,
                                &mut self.builder,
                                &list_item,
                            );
                        }
                    } else if let Some(fence) = code_blocks::try_parse_fence_open(content_slice) {
                        self.containers.push(Container::Definition {
                            content_col,
                            plain_open: false,
                            plain_buffer: TextBuffer::new(),
                        });
                        definition_pushed = true;

                        let bq_depth = self.current_blockquote_depth();
                        if let Some(indent_str) = indent_to_emit {
                            self.builder
                                .token(SyntaxKind::WHITESPACE.into(), indent_str);
                        }
                        let fence_line = current_line[content_start..].to_string();
                        let new_pos = if self.config.extensions.tex_math_gfm
                            && code_blocks::is_gfm_math_fence(&fence)
                        {
                            code_blocks::parse_fenced_math_block(
                                &mut self.builder,
                                &self.lines,
                                self.pos,
                                fence,
                                bq_depth,
                                content_col,
                                Some(&fence_line),
                            )
                        } else {
                            code_blocks::parse_fenced_code_block(
                                &mut self.builder,
                                &self.lines,
                                self.pos,
                                fence,
                                bq_depth,
                                content_col,
                                Some(&fence_line),
                            )
                        };
                        self.pos = new_pos - 1;
                    } else {
                        let (_, newline_str) = strip_newline(current_line);
                        let (content_without_newline, _) = strip_newline(after_marker_and_spaces);
                        if content_without_newline.is_empty() {
                            plain_buffer.push_line(newline_str);
                        } else {
                            let line_with_newline = if !newline_str.is_empty() {
                                format!("{}{}", content_without_newline, newline_str)
                            } else {
                                content_without_newline.to_string()
                            };
                            plain_buffer.push_line(line_with_newline);
                        }
                    }
                }

                if !definition_pushed {
                    self.containers.push(Container::Definition {
                        content_col,
                        plain_open: *has_content,
                        plain_buffer,
                    });
                }
            }
            DefinitionPrepared::Term { blank_count } => {
                self.emit_buffered_plain_if_needed();

                if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                    self.close_containers_to(self.containers.depth() - 1);
                }

                if !definition_lists::in_definition_list(&self.containers) {
                    self.builder.start_node(SyntaxKind::DEFINITION_LIST.into());
                    self.containers.push(Container::DefinitionList {});
                }

                while matches!(
                    self.containers.last(),
                    Some(Container::Definition { .. }) | Some(Container::DefinitionItem { .. })
                ) {
                    self.close_containers_to(self.containers.depth() - 1);
                }

                self.builder.start_node(SyntaxKind::DEFINITION_ITEM.into());
                self.containers.push(Container::DefinitionItem {});

                emit_term(&mut self.builder, content, self.config);

                for i in 0..*blank_count {
                    let blank_pos = self.pos + 1 + i;
                    if blank_pos < self.lines.len() {
                        let blank_line = self.lines[blank_pos];
                        self.builder.start_node(SyntaxKind::BLANK_LINE.into());
                        self.builder
                            .token(SyntaxKind::BLANK_LINE.into(), blank_line);
                        self.builder.finish_node();
                    }
                }
                self.pos += *blank_count;
            }
        }
    }

    /// Get current blockquote depth from container stack.
    fn blockquote_marker_info(
        &self,
        payload: Option<&BlockQuotePrepared>,
        line: &str,
    ) -> Vec<marker_utils::BlockQuoteMarkerInfo> {
        payload
            .map(|payload| payload.marker_info.clone())
            .unwrap_or_else(|| parse_blockquote_marker_info(line))
    }

    fn emit_blockquote_markers(
        &mut self,
        marker_info: &[marker_utils::BlockQuoteMarkerInfo],
        depth: usize,
    ) {
        for i in 0..depth {
            if let Some(info) = marker_info.get(i) {
                blockquotes::emit_one_blockquote_marker(
                    &mut self.builder,
                    info.leading_spaces,
                    info.has_trailing_space,
                );
            }
        }
    }

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

        // Pandoc title block is handled via the block dispatcher.

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

        let has_blank_before = self.pos == 0 || self.lines[self.pos - 1].trim().is_empty();
        let mut blockquote_match: Option<PreparedBlockMatch> = None;
        let dispatcher_ctx = if current_bq_depth == 0 {
            Some(BlockContext {
                content: line,
                has_blank_before,
                has_blank_before_strict: has_blank_before,
                at_document_start: self.pos == 0,
                in_fenced_div: self.in_fenced_div(),
                blockquote_depth: current_bq_depth,
                config: self.config,
                content_indent: 0,
                indent_to_emit: None,
                list_indent_info: None,
                in_list: lists::in_list(&self.containers),
                next_line: if self.pos + 1 < self.lines.len() {
                    Some(self.lines[self.pos + 1])
                } else {
                    None
                },
            })
        } else {
            None
        };

        let blockquote_payload = if let Some(dispatcher_ctx) = dispatcher_ctx.as_ref() {
            self.block_registry
                .detect_prepared(dispatcher_ctx, &self.lines, self.pos)
                .and_then(|prepared| {
                    if matches!(prepared.effect, BlockEffect::OpenBlockQuote) {
                        blockquote_match = Some(prepared);
                        blockquote_match.as_ref().and_then(|prepared| {
                            prepared
                                .payload
                                .as_ref()
                                .and_then(|payload| payload.downcast_ref::<BlockQuotePrepared>())
                                .cloned()
                        })
                    } else {
                        None
                    }
                })
        } else {
            None
        };

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
        // TODO: Does this handle CLRF correctly?
        let is_blank = line.trim_end_matches('\n').trim().is_empty()
            || (bq_depth > 0 && inner_content.trim_end_matches('\n').trim().is_empty());

        if is_blank {
            if self.is_paragraph_open()
                && paragraphs::has_open_inline_math_environment(&self.containers)
            {
                paragraphs::append_paragraph_line(
                    &mut self.containers,
                    &mut self.builder,
                    line,
                    self.config,
                );
                self.pos += 1;
                return true;
            }

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
                    self.builder.start_node(SyntaxKind::BLOCK_QUOTE.into());
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
                ContinuationPolicy::new(self.config, &self.block_registry).compute_levels_to_keep(
                    self.current_blockquote_depth(),
                    &self.containers,
                    &self.lines,
                    peek,
                    self.lines[peek],
                )
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
                    | Some(Container::Alert { .. })
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
                let marker_info = self.blockquote_marker_info(blockquote_payload.as_ref(), line);
                self.emit_blockquote_markers(&marker_info, bq_depth);
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
            if self.config.extensions.blank_before_blockquote
                && current_bq_depth == 0
                && !blockquote_payload
                    .as_ref()
                    .map(|payload| payload.can_start)
                    .unwrap_or_else(|| blockquotes::can_start_blockquote(self.pos, &self.lines))
            {
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
                if self.config.extensions.blank_before_blockquote {
                    // Check if we're right after a blank line or at start of blockquote
                    matches!(self.containers.last(), Some(Container::BlockQuote { .. }))
                        || (self.pos > 0 && {
                            let prev_line = self.lines[self.pos - 1];
                            let (prev_bq_depth, prev_inner) = count_blockquote_markers(prev_line);
                            prev_bq_depth >= current_bq_depth && prev_inner.trim().is_empty()
                        })
                } else {
                    true
                }
            } else {
                blockquote_payload
                    .as_ref()
                    .map(|payload| payload.can_nest)
                    .unwrap_or(true)
            };

            if !can_nest {
                // Can't nest deeper - treat extra > as content
                // Only strip markers up to current depth
                let content_at_current_depth =
                    blockquotes::strip_n_blockquote_markers(line, current_bq_depth);

                // Emit blockquote markers for current depth (for losslessness)
                let marker_info = self.blockquote_marker_info(blockquote_payload.as_ref(), line);
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
            let marker_info = self.blockquote_marker_info(blockquote_payload.as_ref(), line);

            if let (Some(dispatcher_ctx), Some(prepared)) =
                (dispatcher_ctx.as_ref(), blockquote_match.as_ref())
            {
                let _ = self.block_registry.parse_prepared(
                    prepared,
                    dispatcher_ctx,
                    &mut self.builder,
                    &self.lines,
                    self.pos,
                );
                for _ in 0..bq_depth {
                    self.containers.push(Container::BlockQuote {});
                }
            } else {
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
                    self.builder.start_node(SyntaxKind::BLOCK_QUOTE.into());

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
                    && let Some(marker_match) = try_parse_list_marker(line, self.config)
                {
                    let (indent_cols, indent_bytes) = leading_indent(line);
                    if let Some(level) = lists::find_matching_list_level(
                        &self.containers,
                        &marker_match.marker,
                        indent_cols,
                    ) {
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
                        if let Some(nested_marker) = is_content_nested_bullet_marker(
                            line,
                            marker_match.marker_len,
                            marker_match.spaces_after_bytes,
                        ) {
                            let list_item = ListItemEmissionInput {
                                content: line,
                                marker_len: marker_match.marker_len,
                                spaces_after_cols: marker_match.spaces_after_cols,
                                spaces_after_bytes: marker_match.spaces_after_bytes,
                                indent_cols,
                                indent_bytes,
                            };
                            lists::add_list_item_with_nested_empty_list(
                                &mut self.containers,
                                &mut self.builder,
                                &list_item,
                                nested_marker,
                            );
                        } else {
                            let list_item = ListItemEmissionInput {
                                content: line,
                                marker_len: marker_match.marker_len,
                                spaces_after_cols: marker_match.spaces_after_cols,
                                spaces_after_bytes: marker_match.spaces_after_bytes,
                                indent_cols,
                                indent_bytes,
                            };
                            lists::add_list_item(
                                &mut self.containers,
                                &mut self.builder,
                                &list_item,
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
            let mut list_item_continuation = false;

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
                let is_new_item_at_outer_level =
                    if try_parse_list_marker(inner_content, self.config).is_some() {
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
                    list_item_continuation = true;
                }
            }

            // Fenced code blocks inside list items need marker emission in this branch.
            // If we keep continuation buffering for these lines, opening fence markers in
            // blockquote contexts can be dropped from CST text.
            if list_item_continuation && code_blocks::try_parse_fence_open(inner_content).is_some()
            {
                list_item_continuation = false;
            }

            if !list_item_continuation {
                let marker_info = parse_blockquote_marker_info(line);
                for i in 0..bq_depth {
                    if let Some(info) = marker_info.get(i) {
                        self.emit_or_buffer_blockquote_marker(
                            info.leading_spaces,
                            info.has_trailing_space,
                        );
                    }
                }
            }
            // When continuing a list item inside a blockquote, keep original line bytes in the
            // list-item buffer and avoid emitting separate marker tokens here.
            let line_to_append = if list_item_continuation {
                Some(line)
            } else {
                Some(inner_content)
            };
            return self.parse_inner_content(inner_content, line_to_append);
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
                && let Some(marker_match) = try_parse_list_marker(line, self.config)
            {
                let (indent_cols, indent_bytes) = leading_indent(line);
                if let Some(level) = lists::find_matching_list_level(
                    &self.containers,
                    &marker_match.marker,
                    indent_cols,
                ) {
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
                    if let Some(nested_marker) = is_content_nested_bullet_marker(
                        line,
                        marker_match.marker_len,
                        marker_match.spaces_after_bytes,
                    ) {
                        let list_item = ListItemEmissionInput {
                            content: line,
                            marker_len: marker_match.marker_len,
                            spaces_after_cols: marker_match.spaces_after_cols,
                            spaces_after_bytes: marker_match.spaces_after_bytes,
                            indent_cols,
                            indent_bytes,
                        };
                        lists::add_list_item_with_nested_empty_list(
                            &mut self.containers,
                            &mut self.builder,
                            &list_item,
                            nested_marker,
                        );
                    } else {
                        let list_item = ListItemEmissionInput {
                            content: line,
                            marker_len: marker_match.marker_len,
                            spaces_after_cols: marker_match.spaces_after_cols,
                            spaces_after_bytes: marker_match.spaces_after_bytes,
                            indent_cols,
                            indent_bytes,
                        };
                        lists::add_list_item(&mut self.containers, &mut self.builder, &list_item);
                    }
                    self.pos += 1;
                    return true;
                }
            }
        }

        // No blockquote markers - use original line
        self.parse_inner_content(line, None)
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

        if self.config.extensions.alerts
            && self.current_blockquote_depth() > 0
            && !self.in_active_alert()
            && !self.is_paragraph_open()
            && let Some(marker) = Self::alert_marker_from_content(stripped_content)
        {
            let (_, newline_str) = strip_newline(stripped_content);
            self.builder.start_node(SyntaxKind::ALERT.into());
            self.builder.token(SyntaxKind::ALERT_MARKER.into(), marker);
            if !newline_str.is_empty() {
                self.builder.token(SyntaxKind::NEWLINE.into(), newline_str);
            }
            self.containers.push(Container::Alert {
                blockquote_depth: self.current_blockquote_depth(),
            });
            self.pos += 1;
            return true;
        }

        // Check if we're in a Definition container (with or without an open PLAIN)
        // Continuation lines should be added to PLAIN, not treated as new blocks
        // BUT: Don't treat lines with block element markers as continuations
        if matches!(self.containers.last(), Some(Container::Definition { .. })) {
            let is_definition_marker =
                definition_lists::try_parse_definition_marker(stripped_content).is_some()
                    && !stripped_content.starts_with(':');
            if content_indent == 0 && is_definition_marker {
                // Definition markers at top-level should start a new definition.
            } else {
                let policy = ContinuationPolicy::new(self.config, &self.block_registry);

                if policy.definition_plain_can_continue(
                    stripped_content,
                    content,
                    content_indent,
                    &BlockContext {
                        content: stripped_content,
                        has_blank_before: self.pos == 0
                            || self.lines[self.pos - 1].trim().is_empty(),
                        has_blank_before_strict: self.pos == 0
                            || self.lines[self.pos - 1].trim().is_empty(),
                        at_document_start: self.pos == 0 && self.current_blockquote_depth() == 0,
                        in_fenced_div: self.in_fenced_div(),
                        blockquote_depth: self.current_blockquote_depth(),
                        config: self.config,
                        content_indent,
                        indent_to_emit: None,
                        list_indent_info: None,
                        in_list: lists::in_list(&self.containers),
                        next_line: if self.pos + 1 < self.lines.len() {
                            Some(self.lines[self.pos + 1])
                        } else {
                            None
                        },
                    },
                    &self.lines,
                    self.pos,
                ) {
                    let content_line = stripped_content;
                    let (text_without_newline, newline_str) = strip_newline(content_line);
                    let indent_prefix = if !text_without_newline.trim().is_empty() {
                        indent_to_emit.unwrap_or("")
                    } else {
                        ""
                    };
                    let content_line = format!("{}{}", indent_prefix, text_without_newline);

                    if let Some(Container::Definition {
                        plain_open,
                        plain_buffer,
                        ..
                    }) = self.containers.stack.last_mut()
                    {
                        let line_with_newline = if !newline_str.is_empty() {
                            format!("{}{}", content_line, newline_str)
                        } else {
                            content_line
                        };
                        plain_buffer.push_line(line_with_newline);
                        *plain_open = true;
                    }

                    self.pos += 1;
                    return true;
                }
            }
        }

        // Handle blockquotes that appear after stripping content-container indentation
        // (e.g. `    > quote` inside a definition list item).
        if content_indent > 0 {
            let (bq_depth, inner_content) = count_blockquote_markers(stripped_content);
            let current_bq_depth = self.current_blockquote_depth();

            if bq_depth > 0 {
                // If definition/list plain text is buffered, flush it before opening nested
                // blockquotes so block order remains lossless and stable across reparse.
                self.emit_buffered_plain_if_needed();
                self.emit_list_item_buffer_if_needed();

                // Blockquotes can nest inside content containers; preserve the stripped indentation
                // as WHITESPACE before the first marker for losslessness.
                self.close_paragraph_if_open();

                if bq_depth > current_bq_depth {
                    let marker_info = parse_blockquote_marker_info(stripped_content);

                    // Open new blockquotes and emit their markers.
                    for level in current_bq_depth..bq_depth {
                        self.builder.start_node(SyntaxKind::BLOCK_QUOTE.into());

                        if level == current_bq_depth
                            && let Some(indent_str) = indent_to_emit
                        {
                            self.builder
                                .token(SyntaxKind::WHITESPACE.into(), indent_str);
                        }

                        if let Some(info) = marker_info.get(level) {
                            blockquotes::emit_one_blockquote_marker(
                                &mut self.builder,
                                info.leading_spaces,
                                info.has_trailing_space,
                            );
                        }

                        self.containers.push(Container::BlockQuote {});
                    }
                } else if bq_depth < current_bq_depth {
                    self.close_blockquotes_to_depth(bq_depth);
                } else {
                    // Same depth: emit markers for losslessness.
                    let marker_info = parse_blockquote_marker_info(stripped_content);
                    self.emit_blockquote_markers(&marker_info, bq_depth);
                }

                return self.parse_inner_content(inner_content, Some(inner_content));
            }
        }

        // Store the stripped content for later use
        let content = stripped_content;

        if self.is_paragraph_open()
            && paragraphs::has_open_inline_math_environment(&self.containers)
        {
            paragraphs::append_paragraph_line(
                &mut self.containers,
                &mut self.builder,
                line_to_append.unwrap_or(self.lines[self.pos]),
                self.config,
            );
            self.pos += 1;
            return true;
        }

        // Precompute dispatcher match once per line (reused by multiple branches below).
        // This covers: blocks requiring blank lines, blocks that can interrupt paragraphs,
        // and blocks that can appear without blank lines (e.g. reference definitions).
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
            // For lookahead-based blocks (e.g. setext headings), the dispatcher expects
            // `ctx.next_line` to be in the same “inner content” form as `ctx.content`.
            Some(count_blockquote_markers(self.lines[self.pos + 1]).1)
        } else {
            None
        };

        let current_bq_depth = self.current_blockquote_depth();
        if let Some(alert_bq_depth) = self.active_alert_blockquote_depth()
            && current_bq_depth < alert_bq_depth
        {
            while matches!(self.containers.last(), Some(Container::Alert { .. })) {
                self.close_containers_to(self.containers.depth() - 1);
            }
        }

        let dispatcher_ctx = BlockContext {
            content,
            has_blank_before: false,        // filled in later
            has_blank_before_strict: false, // filled in later
            at_document_start: false,       // filled in later
            in_fenced_div: self.in_fenced_div(),
            blockquote_depth: current_bq_depth,
            config: self.config,
            content_indent,
            indent_to_emit,
            list_indent_info,
            in_list: lists::in_list(&self.containers),
            next_line,
        };

        // We'll update these two fields shortly (after they are computed), but we can still
        // use this ctx shape to avoid rebuilding repeated context objects.
        let mut dispatcher_ctx = dispatcher_ctx;

        // Initial detection (before blank/doc-start are computed). Note: this can
        // match reference definitions, but footnotes are handled explicitly later.
        let dispatcher_match =
            self.block_registry
                .detect_prepared(&dispatcher_ctx, &self.lines, self.pos);

        // Check for heading (needs blank line before, or at start of container)
        // Note: for fenced div nesting, the line immediately after a div opening fence
        // should be treated like the start of a container (Pandoc allows nested fences
        // without an intervening blank line). Similarly, the first line after a metadata
        // block (YAML/Pandoc title/MMD title) is treated as having a blank before it.
        let after_metadata_block = std::mem::replace(&mut self.after_metadata_block, false);
        let has_blank_before = if self.pos == 0 || after_metadata_block {
            true
        } else {
            let prev_line = self.lines[self.pos - 1];
            let (prev_bq_depth, prev_inner) = count_blockquote_markers(prev_line);
            let (prev_inner_no_nl, _) = strip_newline(prev_inner);
            let prev_is_fenced_div_open = self.config.extensions.fenced_divs
                && fenced_divs::try_parse_div_fence_open(
                    strip_n_blockquote_markers(prev_inner_no_nl, prev_bq_depth).trim_start(),
                )
                .is_some();

            prev_line.trim().is_empty()
                || prev_is_fenced_div_open
                || matches!(self.containers.last(), Some(Container::BlockQuote { .. }))
        };

        // For indented code blocks, we need a stricter condition - only actual blank lines count
        // Being at document start (pos == 0) is OK only if we're not inside a blockquote
        let at_document_start = self.pos == 0 && current_bq_depth == 0;

        let prev_line_blank = if self.pos > 0 {
            let prev_line = self.lines[self.pos - 1];
            let (prev_bq_depth, prev_inner) = count_blockquote_markers(prev_line);
            prev_line.trim().is_empty() || (prev_bq_depth > 0 && prev_inner.trim().is_empty())
        } else {
            false
        };
        let has_blank_before_strict = at_document_start || prev_line_blank;

        dispatcher_ctx.has_blank_before = has_blank_before;
        dispatcher_ctx.has_blank_before_strict = has_blank_before_strict;
        dispatcher_ctx.at_document_start = at_document_start;

        let dispatcher_match =
            if dispatcher_ctx.has_blank_before || dispatcher_ctx.at_document_start {
                // Recompute now that blank/doc-start conditions are known.
                self.block_registry
                    .detect_prepared(&dispatcher_ctx, &self.lines, self.pos)
            } else {
                dispatcher_match
            };

        if has_blank_before {
            if let Some(env_name) = extract_environment_name(content)
                && is_inline_math_environment(&env_name)
            {
                if !self.is_paragraph_open() {
                    paragraphs::start_paragraph_if_needed(&mut self.containers, &mut self.builder);
                }
                paragraphs::append_paragraph_line(
                    &mut self.containers,
                    &mut self.builder,
                    line_to_append.unwrap_or(self.lines[self.pos]),
                    self.config,
                );
                self.pos += 1;
                return true;
            }

            if let Some(block_match) = dispatcher_match.as_ref() {
                let detection = block_match.detection;

                match detection {
                    BlockDetectionResult::YesCanInterrupt => {
                        self.emit_list_item_buffer_if_needed();
                        if self.is_paragraph_open() {
                            self.close_containers_to(self.containers.depth() - 1);
                        }
                    }
                    BlockDetectionResult::Yes => {
                        self.prepare_for_block_element();
                    }
                    BlockDetectionResult::No => unreachable!(),
                }

                if matches!(block_match.effect, BlockEffect::CloseFencedDiv) {
                    self.close_containers_to_fenced_div();
                }

                let lines_consumed = self.block_registry.parse_prepared(
                    block_match,
                    &dispatcher_ctx,
                    &mut self.builder,
                    &self.lines,
                    self.pos,
                );

                if matches!(
                    self.block_registry.parser_name(block_match),
                    "yaml_metadata" | "pandoc_title_block" | "mmd_title_block"
                ) {
                    self.after_metadata_block = true;
                }

                match block_match.effect {
                    BlockEffect::None => {}
                    BlockEffect::OpenFencedDiv => {
                        self.containers.push(Container::FencedDiv {});
                    }
                    BlockEffect::CloseFencedDiv => {
                        self.close_fenced_div();
                    }
                    BlockEffect::OpenFootnoteDefinition => {
                        self.handle_footnote_open_effect(block_match, content);
                    }
                    BlockEffect::OpenList => {
                        self.handle_list_open_effect(block_match, content, indent_to_emit);
                    }
                    BlockEffect::OpenDefinitionList => {
                        self.handle_definition_list_effect(block_match, content, indent_to_emit);
                    }
                    BlockEffect::OpenBlockQuote => {
                        // Detection only for now; keep core blockquote handling intact.
                    }
                }

                if lines_consumed == 0 {
                    log::warn!(
                        "block parser made no progress at line {} (parser={})",
                        self.pos + 1,
                        self.block_registry.parser_name(block_match)
                    );
                    return false;
                }

                self.pos += lines_consumed;
                return true;
            }
        } else if let Some(block_match) = dispatcher_match.as_ref() {
            // Without blank-before, only allow interrupting blocks OR blocks that are
            // explicitly allowed without blank lines (e.g. reference definitions).
            let parser_name = self.block_registry.parser_name(block_match);
            match block_match.detection {
                BlockDetectionResult::YesCanInterrupt => {
                    if matches!(block_match.effect, BlockEffect::OpenFencedDiv)
                        && self.is_paragraph_open()
                    {
                        // Fenced divs must not interrupt paragraphs without a blank line.
                        if !self.is_paragraph_open() {
                            paragraphs::start_paragraph_if_needed(
                                &mut self.containers,
                                &mut self.builder,
                            );
                        }
                        paragraphs::append_paragraph_line(
                            &mut self.containers,
                            &mut self.builder,
                            line_to_append.unwrap_or(self.lines[self.pos]),
                            self.config,
                        );
                        self.pos += 1;
                        return true;
                    }

                    if matches!(block_match.effect, BlockEffect::OpenList)
                        && self.is_paragraph_open()
                        && !lists::in_list(&self.containers)
                        && self.content_container_indent_to_strip() == 0
                    {
                        // Do not let lists interrupt a paragraph without a blank line.
                        paragraphs::append_paragraph_line(
                            &mut self.containers,
                            &mut self.builder,
                            line_to_append.unwrap_or(self.lines[self.pos]),
                            self.config,
                        );
                        self.pos += 1;
                        return true;
                    }

                    self.emit_list_item_buffer_if_needed();
                    if self.is_paragraph_open() {
                        self.close_containers_to(self.containers.depth() - 1);
                    }
                }
                BlockDetectionResult::Yes => {
                    // Keep ambiguous fenced-div openers from interrupting an
                    // active paragraph without a blank line.
                    if parser_name == "fenced_div_open" && self.is_paragraph_open() {
                        if !self.is_paragraph_open() {
                            paragraphs::start_paragraph_if_needed(
                                &mut self.containers,
                                &mut self.builder,
                            );
                        }
                        paragraphs::append_paragraph_line(
                            &mut self.containers,
                            &mut self.builder,
                            line_to_append.unwrap_or(self.lines[self.pos]),
                            self.config,
                        );
                        self.pos += 1;
                        return true;
                    }
                }
                BlockDetectionResult::No => unreachable!(),
            }

            if !matches!(block_match.detection, BlockDetectionResult::No) {
                if matches!(block_match.effect, BlockEffect::CloseFencedDiv) {
                    self.close_containers_to_fenced_div();
                }

                let lines_consumed = self.block_registry.parse_prepared(
                    block_match,
                    &dispatcher_ctx,
                    &mut self.builder,
                    &self.lines,
                    self.pos,
                );

                match block_match.effect {
                    BlockEffect::None => {}
                    BlockEffect::OpenFencedDiv => {
                        self.containers.push(Container::FencedDiv {});
                    }
                    BlockEffect::CloseFencedDiv => {
                        self.close_fenced_div();
                    }
                    BlockEffect::OpenFootnoteDefinition => {
                        self.handle_footnote_open_effect(block_match, content);
                    }
                    BlockEffect::OpenList => {
                        self.handle_list_open_effect(block_match, content, indent_to_emit);
                    }
                    BlockEffect::OpenDefinitionList => {
                        self.handle_definition_list_effect(block_match, content, indent_to_emit);
                    }
                    BlockEffect::OpenBlockQuote => {
                        // Detection only for now; keep core blockquote handling intact.
                    }
                }

                if lines_consumed == 0 {
                    log::warn!(
                        "block parser made no progress at line {} (parser={})",
                        self.pos + 1,
                        self.block_registry.parser_name(block_match)
                    );
                    return false;
                }

                self.pos += lines_consumed;
                return true;
            }
        }

        // Check for line block (if line_blocks extension is enabled)
        if self.config.extensions.line_blocks
            && (has_blank_before || self.pos == 0)
            && try_parse_line_block_start(content).is_some()
            // Guard against context-stripped content (e.g. inside blockquotes) that
            // looks like a line block while the raw source line does not. Calling
            // parse_line_block on raw lines in that state would consume 0 lines.
            && try_parse_line_block_start(self.lines[self.pos]).is_some()
        {
            log::debug!("Parsed line block at line {}", self.pos);
            // Close paragraph before opening line block
            self.close_paragraph_if_open();

            let new_pos = parse_line_block(&self.lines, self.pos, &mut self.builder, self.config);
            if new_pos > self.pos {
                self.pos = new_pos;
                return true;
            }
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

    fn fenced_div_container_index(&self) -> Option<usize> {
        self.containers
            .stack
            .iter()
            .rposition(|c| matches!(c, Container::FencedDiv { .. }))
    }

    fn close_containers_to_fenced_div(&mut self) {
        if let Some(index) = self.fenced_div_container_index() {
            self.close_containers_to(index + 1);
        }
    }

    fn close_fenced_div(&mut self) {
        if let Some(index) = self.fenced_div_container_index() {
            self.close_containers_to(index);
        }
    }

    fn in_fenced_div(&self) -> bool {
        self.containers
            .stack
            .iter()
            .any(|c| matches!(c, Container::FencedDiv { .. }))
    }
}
