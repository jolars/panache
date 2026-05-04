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
use super::blocks::headings::{
    emit_atx_heading, emit_setext_heading, emit_setext_heading_body, try_parse_atx_heading,
    try_parse_setext_heading,
};
use super::blocks::horizontal_rules::try_parse_horizontal_rule;
use super::blocks::line_blocks;
use super::blocks::lists;
use super::blocks::paragraphs;
use super::blocks::raw_blocks::{extract_environment_name, is_inline_math_environment};
use super::utils::container_stack;
use super::utils::helpers::{is_blank_line, split_lines_inclusive, strip_newline};
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

    /// Close enclosing list items (and their containing list) whose
    /// `content_col` exceeds the given indent. Used by CommonMark when an
    /// interrupting block (HR, ATX heading, fenced code, ...) appears at a
    /// column shallower than the current list-item content column — per
    /// §5.2 the line cannot continue the item, so the item and the
    /// surrounding list close before the new block is emitted at the
    /// outer level. Pandoc-markdown reaches this branch only when the
    /// dispatcher already required a blank line before the interrupter,
    /// at which point the blank-line handler has already closed the list;
    /// gating on dialect at the call site keeps Pandoc unaffected.
    fn close_lists_above_indent(&mut self, indent_cols: usize) {
        while let Some(Container::ListItem { content_col, .. }) = self.containers.last() {
            if indent_cols >= *content_col {
                break;
            }
            self.close_containers_to(self.containers.depth() - 1);
            if matches!(self.containers.last(), Some(Container::List { .. })) {
                self.close_containers_to(self.containers.depth() - 1);
            }
        }
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

                    log::trace!(
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

                    log::trace!(
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
                    log::trace!("Closing empty ListItem (no buffer content)");
                    // Just close normally (empty list item)
                    self.containers.stack.pop();
                    self.builder.finish_node();
                }
                // Handle Paragraph with buffering
                Some(Container::Paragraph {
                    buffer,
                    start_checkpoint,
                    ..
                }) if !buffer.is_empty() => {
                    // Clone buffer to avoid borrow issues
                    let buffer_clone = buffer.clone();
                    let checkpoint = *start_checkpoint;
                    // Pop container first
                    self.containers.stack.pop();
                    // Retroactively wrap as PARAGRAPH and emit buffered content
                    self.builder
                        .start_node_at(checkpoint, SyntaxKind::PARAGRAPH.into());
                    buffer_clone.emit_with_inlines(&mut self.builder, self.config);
                    self.builder.finish_node();
                }
                // Handle Paragraph without content
                Some(Container::Paragraph {
                    start_checkpoint, ..
                }) => {
                    let checkpoint = *start_checkpoint;
                    // Just close normally — emit empty PARAGRAPH wrapper
                    self.containers.stack.pop();
                    self.builder
                        .start_node_at(checkpoint, SyntaxKind::PARAGRAPH.into());
                    self.builder.finish_node();
                }
                // Handle Definition with buffered PLAIN
                Some(Container::Definition {
                    plain_open: true,
                    plain_buffer,
                    ..
                }) if !plain_buffer.is_empty() => {
                    let text = plain_buffer.get_accumulated_text();
                    emit_definition_plain_or_heading(&mut self.builder, &text, self.config);

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
            emit_definition_plain_or_heading(&mut self.builder, &text, self.config);
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

    fn previous_block_requires_blank_before_heading(&self) -> bool {
        matches!(
            self.containers.last(),
            Some(Container::Paragraph { .. })
                | Some(Container::ListItem { .. })
                | Some(Container::Definition { .. })
                | Some(Container::DefinitionItem { .. })
                | Some(Container::FootnoteDefinition { .. })
        )
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

    /// CommonMark §5.2: when a list item's first line (after the marker) is a
    /// fenced code block opener, the content of the item *is* the code block —
    /// not buffered text. The list-item open path normally pushes the
    /// post-marker text into the item's buffer; this helper detects an opening
    /// fence in that buffered first line and converts it into a CODE_BLOCK
    /// inside the LIST_ITEM, consuming subsequent lines until the closing
    /// fence (or end of document under CommonMark dialect, per §4.5).
    ///
    /// Pandoc-markdown also reaches this path: a bare fence still requires a
    /// matching closer to register as a code block, matching
    /// `FencedCodeBlockParser::detect_prepared` (`bare_fence_in_list_with_closer`).
    fn maybe_open_fenced_code_in_new_list_item(&mut self) {
        let Some(Container::ListItem {
            content_col,
            buffer,
            ..
        }) = self.containers.stack.last()
        else {
            return;
        };
        let content_col = *content_col;
        let Some(text) = buffer.first_text() else {
            return;
        };
        if buffer.segment_count() != 1 {
            return;
        }
        let text_owned = text.to_string();
        let Some(fence) = code_blocks::try_parse_fence_open(&text_owned) else {
            return;
        };
        let common_mark_dialect = self.config.dialect == crate::options::Dialect::CommonMark;
        let has_info = !fence.info_string.trim().is_empty();
        let bq_depth = self.current_blockquote_depth();
        let has_matching_closer = self.has_matching_fence_closer(&fence, bq_depth, content_col);
        if !(has_info || has_matching_closer || common_mark_dialect) {
            return;
        }
        // Gate fences by extension flags, mirroring the dispatcher.
        if (fence.fence_char == '`' && !self.config.extensions.backtick_code_blocks)
            || (fence.fence_char == '~' && !self.config.extensions.fenced_code_blocks)
        {
            return;
        }
        if let Some(Container::ListItem { buffer, .. }) = self.containers.stack.last_mut() {
            buffer.clear();
        }
        let new_pos = code_blocks::parse_fenced_code_block(
            &mut self.builder,
            &self.lines,
            self.pos,
            fence,
            bq_depth,
            content_col,
            Some(&text_owned),
        );
        // The dispatcher caller will advance pos by lines_consumed (= 1 from
        // the list parser) after we return. Compensate so the final pos lands
        // on `new_pos`.
        self.pos = new_pos.saturating_sub(1);
    }

    /// CommonMark §5.2 rule #2: when a list marker is followed by ≥ 5 columns
    /// of whitespace and non-empty content, the content begins as an indented
    /// code block on the marker line. The marker parser collapses the post-
    /// marker whitespace to "marker + 1 (possibly virtual) space" and leaves
    /// the surplus in the post-marker text. This helper detects such a single-
    /// line indented-code first-line and converts the buffered text into a
    /// CODE_BLOCK > CODE_CONTENT inside the LIST_ITEM.
    ///
    /// Multi-line accumulation (subsequent indented-code lines on continuation
    /// lines) is handled by the regular block-detection path.
    fn maybe_open_indented_code_in_new_list_item(&mut self) {
        let Some(Container::ListItem {
            content_col,
            buffer,
            marker_only,
            virtual_marker_space,
        }) = self.containers.stack.last()
        else {
            return;
        };
        if *marker_only {
            return;
        }
        if buffer.segment_count() != 1 {
            return;
        }
        let Some(text) = buffer.first_text() else {
            return;
        };
        let content_col = *content_col;
        let virtual_marker_space = *virtual_marker_space;
        let text_owned = text.to_string();

        // Single-line content only for now.
        let mut iter = text_owned.split_inclusive('\n');
        let line_with_nl = iter.next().unwrap_or("").to_string();
        if iter.next().is_some() {
            return;
        }

        let line_no_nl = line_with_nl
            .strip_suffix("\r\n")
            .or_else(|| line_with_nl.strip_suffix('\n'))
            .unwrap_or(&line_with_nl);
        let nl_suffix = &line_with_nl[line_no_nl.len()..];

        let buffer_start_col = if virtual_marker_space {
            content_col.saturating_sub(1)
        } else {
            content_col
        };

        let target = content_col + 4;
        let (cols_walked, ws_bytes) =
            super::utils::container_stack::leading_indent_from(line_no_nl, buffer_start_col);

        if buffer_start_col + cols_walked < target {
            return;
        }
        if ws_bytes >= line_no_nl.len() {
            return;
        }

        if let Some(Container::ListItem { buffer, .. }) = self.containers.stack.last_mut() {
            buffer.clear();
        }

        self.builder.start_node(SyntaxKind::CODE_BLOCK.into());
        self.builder.start_node(SyntaxKind::CODE_CONTENT.into());
        if ws_bytes > 0 {
            self.builder
                .token(SyntaxKind::WHITESPACE.into(), &line_no_nl[..ws_bytes]);
        }
        let rest = &line_no_nl[ws_bytes..];
        if !rest.is_empty() {
            self.builder.token(SyntaxKind::TEXT.into(), rest);
        }
        if !nl_suffix.is_empty() {
            self.builder.token(SyntaxKind::NEWLINE.into(), nl_suffix);
        }
        self.builder.finish_node();
        self.builder.finish_node();
    }

    fn has_matching_fence_closer(
        &self,
        fence: &code_blocks::FenceInfo,
        bq_depth: usize,
        content_col: usize,
    ) -> bool {
        for raw_line in self.lines.iter().skip(self.pos + 1) {
            let (line_bq_depth, inner) = count_blockquote_markers(raw_line);
            if line_bq_depth < bq_depth {
                break;
            }
            let candidate = if content_col > 0 && !inner.is_empty() {
                let idx = byte_index_at_column(inner, content_col);
                if idx <= inner.len() {
                    &inner[idx..]
                } else {
                    inner
                }
            } else {
                inner
            };
            if code_blocks::is_closing_fence(candidate, fence) {
                return true;
            }
        }
        false
    }

    /// Check if a paragraph is currently open.
    fn is_paragraph_open(&self) -> bool {
        matches!(self.containers.last(), Some(Container::Paragraph { .. }))
    }

    /// Fold an open paragraph's buffered content into a setext heading and emit it.
    ///
    /// Used for CommonMark multi-line setext: when a setext underline is matched
    /// and a paragraph is already open with buffered text, the entire paragraph
    /// (buffer + current text line) becomes the heading content. The HEADING node
    /// is wrapped retroactively from the paragraph's start checkpoint so the
    /// emitted bytes appear in source order.
    fn emit_setext_heading_folding_paragraph(
        &mut self,
        text_line: &str,
        underline_line: &str,
        level: usize,
    ) {
        let (buffered_text, checkpoint) = match self.containers.stack.last() {
            Some(Container::Paragraph {
                buffer,
                start_checkpoint,
                ..
            }) => (buffer.get_text_for_parsing(), Some(*start_checkpoint)),
            _ => (String::new(), None),
        };

        if checkpoint.is_some() {
            self.containers.stack.pop();
        }

        let combined_text = if buffered_text.is_empty() {
            text_line.to_string()
        } else {
            format!("{}{}", buffered_text, text_line)
        };

        let cp = checkpoint.expect(
            "emit_setext_heading_folding_paragraph requires an open paragraph; \
             single-line setext should go through the regular dispatcher path",
        );
        self.builder.start_node_at(cp, SyntaxKind::HEADING.into());
        emit_setext_heading_body(
            &mut self.builder,
            &combined_text,
            underline_line,
            level,
            self.config,
        );
        self.builder.finish_node();
    }

    /// Try to fold a list item's buffered first-line text and the current line
    /// into a setext HEADING node, returning true on success.
    ///
    /// CommonMark §4.3 / Pandoc-markdown both treat the marker line of a list
    /// item as a fresh start for setext detection — i.e. `- Bar\n  ---\n` is a
    /// setext h2 inside the list item. The dispatcher path can't see this
    /// because the list parser consumes the marker line and buffers the
    /// post-marker text; by the time `  ---` reaches the dispatcher, the
    /// candidate text line is already inside the buffer rather than the line
    /// stream. This helper bridges that gap: when the innermost container is a
    /// `ListItem` with a single buffered text segment and the current
    /// (list-item-content-stripped) line is a setext underline, emit the
    /// folded heading directly and clear the buffer.
    ///
    /// Multi-line setext (multiple buffered text segments) is *not* handled
    /// here because Pandoc-markdown disagrees with CommonMark on whether
    /// `- Foo\n  Bar\n  ---\n` forms a setext heading.
    fn try_fold_list_item_buffer_into_setext(&mut self, content: &str) -> bool {
        let Some(Container::ListItem {
            buffer,
            content_col,
            ..
        }) = self.containers.stack.last()
        else {
            return false;
        };
        if buffer.segment_count() != 1 {
            return false;
        }
        let Some(text_line) = buffer.first_text() else {
            return false;
        };

        // CommonMark §5.2: the underline must be indented to at least the
        // list item's content column. A bare `---` at column 0 escapes the
        // item and becomes a thematic break (CMark spec example #94/#99); a
        // bare `-` at column 0 is a sibling list marker (#281/#282).
        let content_col = *content_col;
        let (underline_indent_cols, _) = leading_indent(content);
        if underline_indent_cols < content_col {
            return false;
        }

        let lines = [text_line, content];
        let Some((level, _)) = try_parse_setext_heading(&lines, 0) else {
            return false;
        };

        let (text_no_newline, _) = strip_newline(text_line);
        if text_no_newline.trim().is_empty() {
            return false;
        }
        if try_parse_horizontal_rule(text_no_newline).is_some() {
            return false;
        }

        let text_owned = text_line.to_string();
        if let Some(Container::ListItem { buffer, .. }) = self.containers.stack.last_mut() {
            buffer.clear();
        }
        emit_setext_heading(&mut self.builder, &text_owned, content, level, self.config);
        self.pos += 1;
        true
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

    /// Close any open `FootnoteDefinition` container before a new footnote definition
    /// is emitted into the green tree. Without this, a back-to-back `[^a]:`/`[^b]:`
    /// pair would nest the second `FOOTNOTE_DEFINITION` node inside the first.
    fn close_open_footnote_definition(&mut self) {
        while matches!(
            self.containers.last(),
            Some(Container::FootnoteDefinition { .. })
        ) {
            self.close_containers_to(self.containers.depth() - 1);
        }
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

        let content_col = 4;
        self.containers
            .push(Container::FootnoteDefinition { content_col });

        if content_start == 0 {
            return;
        }
        let first_line_content = &content[content_start..];
        if first_line_content.trim().is_empty() {
            let (_, newline_str) = strip_newline(content);
            if !newline_str.is_empty() {
                self.builder.token(SyntaxKind::NEWLINE.into(), newline_str);
            }
            return;
        }

        if self.config.extensions.definition_lists
            && let Some(blank_count) = footnote_first_line_term_lookahead(
                &self.lines,
                self.pos,
                content_col,
                self.config.extensions.table_captions,
            )
        {
            self.builder.start_node(SyntaxKind::DEFINITION_LIST.into());
            self.containers.push(Container::DefinitionList {});
            self.builder.start_node(SyntaxKind::DEFINITION_ITEM.into());
            self.containers.push(Container::DefinitionItem {});
            emit_term(&mut self.builder, first_line_content, self.config);
            for i in 0..blank_count {
                let blank_pos = self.pos + 1 + i;
                if blank_pos < self.lines.len() {
                    let blank_line = self.lines[blank_pos];
                    self.builder.start_node(SyntaxKind::BLANK_LINE.into());
                    self.builder
                        .token(SyntaxKind::BLANK_LINE.into(), blank_line);
                    self.builder.finish_node();
                }
            }
            self.pos += blank_count;
            return;
        }

        paragraphs::start_paragraph_if_needed(&mut self.containers, &mut self.builder);
        paragraphs::append_paragraph_line(
            &mut self.containers,
            &mut self.builder,
            first_line_content,
            self.config,
        );
    }

    /// CommonMark spec example #312: handle a detected list marker that's
    /// actually lazy continuation rather than a new list item. Returns true
    /// when the line was consumed as continuation (caller should advance pos
    /// without calling `handle_list_open_effect`).
    ///
    /// A marker line whose leading indent is ≥ 4 columns isn't a real list
    /// marker when (a) the indent doesn't reach the deepest open list item's
    /// content column (so it can't open a child list), and (b) no open list
    /// level matches the indent (so it can't be a sibling). In that case the
    /// content is just text that lazily extends the deepest open paragraph
    /// or list item.
    fn try_lazy_list_continuation(
        &mut self,
        block_match: &super::block_dispatcher::PreparedBlockMatch,
        content: &str,
    ) -> bool {
        use super::block_dispatcher::ListPrepared;

        let Some(prepared) = block_match
            .payload
            .as_ref()
            .and_then(|p| p.downcast_ref::<ListPrepared>())
        else {
            return false;
        };

        if prepared.indent_cols < 4 || !lists::in_list(&self.containers) {
            return false;
        }

        let current_content_col = paragraphs::current_content_col(&self.containers);
        if prepared.indent_cols >= current_content_col {
            return false;
        }

        if lists::find_matching_list_level(
            &self.containers,
            &prepared.marker,
            prepared.indent_cols,
            self.config.dialect,
        )
        .is_some()
        {
            return false;
        }

        match self.containers.last() {
            Some(Container::Paragraph { .. }) => {
                paragraphs::append_paragraph_line(
                    &mut self.containers,
                    &mut self.builder,
                    content,
                    self.config,
                );
                true
            }
            Some(Container::ListItem { .. }) => {
                if let Some(Container::ListItem {
                    buffer,
                    marker_only,
                    ..
                }) = self.containers.stack.last_mut()
                {
                    buffer.push_text(content);
                    if !content.trim().is_empty() {
                        *marker_only = false;
                    }
                }
                true
            }
            _ => false,
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
            self.config.dialect,
        );
        let list_item = ListItemEmissionInput {
            content,
            marker_len: prepared.marker_len,
            spaces_after_cols: prepared.spaces_after_cols,
            spaces_after_bytes: prepared.spaces_after,
            indent_cols: prepared.indent_cols,
            indent_bytes: prepared.indent_bytes,
            virtual_marker_space: prepared.virtual_marker_space,
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
                        lists::add_list_item(
                            &mut self.containers,
                            &mut self.builder,
                            &list_item,
                            self.config,
                        );
                    }
                    self.maybe_open_fenced_code_in_new_list_item();
                    self.maybe_open_indented_code_in_new_list_item();
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
                self.config,
            );
            self.maybe_open_fenced_code_in_new_list_item();
            self.maybe_open_indented_code_in_new_list_item();
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
                lists::add_list_item(
                    &mut self.containers,
                    &mut self.builder,
                    &list_item,
                    self.config,
                );
            }
            self.maybe_open_fenced_code_in_new_list_item();
            self.maybe_open_indented_code_in_new_list_item();
            return;
        }

        if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
            self.close_containers_to(self.containers.depth() - 1);
        }
        while matches!(
            self.containers.last(),
            Some(Container::ListItem { .. } | Container::List { .. })
        ) {
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
            lists::add_list_item(
                &mut self.containers,
                &mut self.builder,
                &list_item,
                self.config,
            );
        }
        self.maybe_open_fenced_code_in_new_list_item();
        self.maybe_open_indented_code_in_new_list_item();
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
                                return true;
                            }

                            let (next_indent_cols, _) = leading_indent(next_without_newline);
                            next_indent_cols >= content_col
                        })
                        .unwrap_or(true);

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
                            virtual_marker_space: marker_match.virtual_marker_space,
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
                                self.config,
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

    /// Build blockquote marker metadata for the current source line.
    ///
    /// When a blockquote marker is detected at a shifted list content column
    /// (e.g. `    > ...` inside a list item), the prefix indentation must be
    /// folded into the first marker's leading spaces for lossless emission.
    fn marker_info_for_line(
        &self,
        payload: Option<&BlockQuotePrepared>,
        raw_line: &str,
        marker_line: &str,
        shifted_prefix: &str,
        used_shifted: bool,
    ) -> Vec<marker_utils::BlockQuoteMarkerInfo> {
        let mut marker_info = if used_shifted {
            parse_blockquote_marker_info(marker_line)
        } else {
            self.blockquote_marker_info(payload, raw_line)
        };
        if used_shifted && !shifted_prefix.is_empty() {
            let (prefix_cols, _) = leading_indent(shifted_prefix);
            if let Some(first) = marker_info.first_mut() {
                first.leading_spaces += prefix_cols;
            }
        }
        marker_info
    }

    /// Detect blockquote markers that begin at list-content indentation instead
    /// of column 0 on the physical line.
    fn shifted_blockquote_from_list<'b>(
        &self,
        line: &'b str,
    ) -> Option<(usize, &'b str, &'b str, &'b str)> {
        let list_content_col = paragraphs::current_content_col(&self.containers);
        let content_container_indent = self.content_container_indent_to_strip();
        let marker_col = list_content_col.saturating_add(content_container_indent);
        if marker_col == 0 {
            return None;
        }

        let (indent_cols, _) = leading_indent(line);
        if indent_cols < marker_col {
            return None;
        }

        let idx = byte_index_at_column(line, marker_col);
        if idx > line.len() {
            return None;
        }

        let candidate = &line[idx..];
        let (candidate_depth, candidate_inner) = count_blockquote_markers(candidate);
        if candidate_depth == 0 {
            return None;
        }

        Some((candidate_depth, candidate_inner, candidate, &line[..idx]))
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
        if let Some(Container::ListItem {
            buffer,
            marker_only,
            ..
        }) = self.containers.stack.last_mut()
        {
            buffer.push_blockquote_marker(leading_spaces, has_trailing_space);
            *marker_only = false;
            return;
        }

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

        log::trace!("Starting document parse");

        // Pandoc title block is handled via the block dispatcher.

        while self.pos < self.lines.len() {
            let line = self.lines[self.pos];

            log::trace!("Parsing line {}: {}", self.pos + 1, line);

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
        // Count blockquote markers on this line. Inside list items, blockquotes can begin
        // at the list content column (e.g. `    > ...` after `1. `), not at column 0.
        let (mut bq_depth, mut inner_content) = count_blockquote_markers(line);
        let mut bq_marker_line = line;
        let mut shifted_bq_prefix = "";
        let mut used_shifted_bq = false;
        if bq_depth == 0
            && let Some((candidate_depth, candidate_inner, candidate_line, candidate_prefix)) =
                self.shifted_blockquote_from_list(line)
        {
            bq_depth = candidate_depth;
            inner_content = candidate_inner;
            bq_marker_line = candidate_line;
            shifted_bq_prefix = candidate_prefix;
            used_shifted_bq = true;
        }
        let current_bq_depth = self.current_blockquote_depth();

        let has_blank_before = self.pos == 0 || is_blank_line(self.lines[self.pos - 1]);
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
                in_marker_only_list_item: matches!(
                    self.containers.last(),
                    Some(Container::ListItem {
                        marker_only: true,
                        ..
                    })
                ),
                paragraph_open: self.is_paragraph_open(),
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

        log::trace!(
            "parse_line [{}]: bq_depth={}, current_bq={}, depth={}, line={:?}",
            self.pos,
            bq_depth,
            current_bq_depth,
            self.containers.depth(),
            line.trim_end()
        );

        // Handle blank lines specially (including blank lines inside blockquotes)
        // A line like ">" with nothing after is a blank line inside a blockquote
        let is_blank = is_blank_line(line) || (bq_depth > 0 && is_blank_line(inner_content));

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

            // For blank lines inside blockquotes, we need to handle them at the right depth.
            // If a shifted blockquote marker was detected in list-item content, preserve the
            // leading shifted indentation before the first marker for losslessness.
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

            // Peek ahead to determine what containers to keep open. Skip
            // truly blank lines and, when this blank line is inside a
            // blockquote, blank-inside-blockquote lines too (e.g. `>` or
            // `>   `) so multiple consecutive `>`-blank lines don't make
            // the next non-blank line look like it's outside the
            // blockquote's continuation context.
            let mut peek = self.pos + 1;
            while peek < self.lines.len() {
                let peek_line = self.lines[peek];
                if is_blank_line(peek_line) {
                    peek += 1;
                    continue;
                }
                if bq_depth > 0 {
                    let (peek_bq, _) = count_blockquote_markers(peek_line);
                    if peek_bq >= bq_depth {
                        let peek_inner =
                            blockquotes::strip_n_blockquote_markers(peek_line, bq_depth);
                        if is_blank_line(peek_inner) {
                            peek += 1;
                            continue;
                        }
                    }
                }
                break;
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
                        log::trace!(
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
                        log::trace!(
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
                let marker_info = self.marker_info_for_line(
                    blockquote_payload.as_ref(),
                    line,
                    bq_marker_line,
                    shifted_bq_prefix,
                    used_shifted_bq,
                );
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
                && !used_shifted_bq
                && !blockquote_payload
                    .as_ref()
                    .map(|payload| payload.can_start)
                    .unwrap_or_else(|| blockquotes::can_start_blockquote(self.pos, &self.lines))
            {
                // Can't start blockquote without blank line - treat as paragraph
                // Flush any pending list-item inline buffer first so this line
                // stays in source order relative to buffered list text.
                self.emit_list_item_buffer_if_needed();
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
                            prev_bq_depth >= current_bq_depth && is_blank_line(prev_inner)
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
                let marker_info = self.marker_info_for_line(
                    blockquote_payload.as_ref(),
                    line,
                    bq_marker_line,
                    shifted_bq_prefix,
                    used_shifted_bq,
                );
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

            // Preserve source order when a deeper blockquote line arrives while
            // list-item text is still buffered (e.g. issue #174).
            self.emit_list_item_buffer_if_needed();

            // Close paragraph before opening blockquote
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.close_containers_to(self.containers.depth() - 1);
            }

            // Parse marker information for all levels
            let marker_info = self.marker_info_for_line(
                blockquote_payload.as_ref(),
                line,
                bq_marker_line,
                shifted_bq_prefix,
                used_shifted_bq,
            );

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
            // Lazy continuation: line with fewer (or zero) > markers continues
            // a paragraph that started at a deeper blockquote level. CommonMark
            // §5.1 explicitly allows this regardless of how many `>` markers
            // are on the lazy line.
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                // CommonMark §5.1: lazy continuation does *not* fire if
                // the line would itself be a paragraph-interrupting block
                // (e.g. a thematic break) — instead the paragraph closes,
                // any open blockquotes close, and the line opens that
                // block at the outer level. Pandoc keeps the lazy text
                // append in this case.
                let is_commonmark = self.config.dialect == crate::options::Dialect::CommonMark;
                let interrupts_via_hr = is_commonmark && try_parse_horizontal_rule(line).is_some();
                let interrupts_via_fence =
                    is_commonmark && code_blocks::try_parse_fence_open(line).is_some();
                if !interrupts_via_hr && !interrupts_via_fence {
                    if bq_depth > 0 {
                        // Buffer the explicit `>` markers we have into the
                        // paragraph (it's at the deeper blockquote level, so
                        // structurally the markers belong to outer levels but
                        // they're tucked inside the paragraph for losslessness;
                        // the formatter re-emits prefixes from container nesting).
                        let marker_info = self.marker_info_for_line(
                            blockquote_payload.as_ref(),
                            line,
                            bq_marker_line,
                            shifted_bq_prefix,
                            used_shifted_bq,
                        );
                        for i in 0..bq_depth {
                            if let Some(info) = marker_info.get(i) {
                                paragraphs::append_paragraph_marker(
                                    &mut self.containers,
                                    info.leading_spaces,
                                    info.has_trailing_space,
                                );
                            }
                        }
                        paragraphs::append_paragraph_line(
                            &mut self.containers,
                            &mut self.builder,
                            inner_content,
                            self.config,
                        );
                    } else {
                        paragraphs::append_paragraph_line(
                            &mut self.containers,
                            &mut self.builder,
                            line,
                            self.config,
                        );
                    }
                    self.pos += 1;
                    return true;
                }
            }
            // Lazy continuation of a list item's open content (its
            // Plain/Para). Pandoc and CommonMark both fold a no-`>`
            // (or short-`>`) plain-text line into the deepest open
            // ListItem when the line is not itself a list marker or a
            // paragraph-interrupting block. The ListItemBuffer is the
            // analogue of an open Paragraph for items whose content
            // hasn't been wrapped yet.
            if matches!(self.containers.last(), Some(Container::ListItem { .. }))
                && lists::in_blockquote_list(&self.containers)
                && try_parse_list_marker(line, self.config).is_none()
            {
                let is_commonmark = self.config.dialect == crate::options::Dialect::CommonMark;
                let interrupts_via_hr = is_commonmark && try_parse_horizontal_rule(line).is_some();
                let interrupts_via_fence =
                    is_commonmark && code_blocks::try_parse_fence_open(line).is_some();
                if !interrupts_via_hr && !interrupts_via_fence {
                    if bq_depth > 0 {
                        let marker_info = self.marker_info_for_line(
                            blockquote_payload.as_ref(),
                            line,
                            bq_marker_line,
                            shifted_bq_prefix,
                            used_shifted_bq,
                        );
                        if let Some(Container::ListItem {
                            buffer,
                            marker_only,
                            ..
                        }) = self.containers.stack.last_mut()
                        {
                            for i in 0..bq_depth {
                                if let Some(info) = marker_info.get(i) {
                                    buffer.push_blockquote_marker(
                                        info.leading_spaces,
                                        info.has_trailing_space,
                                    );
                                }
                            }
                            buffer.push_text(inner_content);
                            if !inner_content.trim().is_empty() {
                                *marker_only = false;
                            }
                        }
                    } else if let Some(Container::ListItem {
                        buffer,
                        marker_only,
                        ..
                    }) = self.containers.stack.last_mut()
                    {
                        buffer.push_text(line);
                        if !line.trim().is_empty() {
                            *marker_only = false;
                        }
                    }
                    self.pos += 1;
                    return true;
                }
            }
            // CommonMark §5.1: a no-`>` line that begins a list marker
            // closes the blockquote and starts a fresh list at the outer
            // level rather than continuing the inner list. Pandoc keeps
            // the inner list going (lazy list continuation across
            // blockquote depth).
            if bq_depth == 0 && self.config.dialect != crate::options::Dialect::CommonMark {
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
                        self.config.dialect,
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
                                virtual_marker_space: marker_match.virtual_marker_space,
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
                                virtual_marker_space: marker_match.virtual_marker_space,
                            };
                            lists::add_list_item(
                                &mut self.containers,
                                &mut self.builder,
                                &list_item,
                                self.config,
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
                let marker_info = self.marker_info_for_line(
                    blockquote_payload.as_ref(),
                    line,
                    bq_marker_line,
                    shifted_bq_prefix,
                    used_shifted_bq,
                );
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
            let same_depth_marker_info = self.marker_info_for_line(
                blockquote_payload.as_ref(),
                line,
                bq_marker_line,
                shifted_bq_prefix,
                used_shifted_bq,
            );
            let has_explicit_same_depth_marker = same_depth_marker_info.len() >= bq_depth;

            // Sibling-list-marker continuation across BQ prefix: when the
            // BQ-stripped content is a list marker that matches an open
            // inner LIST in the container stack, add a sibling LIST_ITEM
            // at that level. Pandoc tracks columns through BQ markers, so
            // a line like `   > - 2:` (column-aligned) and `> - 2:` (lazy,
            // dropped outer continuation indent) are both siblings of an
            // open inner LIST inside the BQ. Without this, the dispatcher
            // sees the post-strip `- 2:` at column 0 and incorrectly
            // opens a new outer-level LIST_ITEM. The lazy form is what
            // our own formatter emits — without this branch round-trips
            // would not be idempotent.
            let (inner_indent_cols_raw, inner_indent_bytes) = leading_indent(inner_content);
            if let Some(marker_match) = try_parse_list_marker(inner_content, self.config) {
                // Don't steal lines whose leading whitespace inside the BQ
                // would push the marker into the previous inner LIST_ITEM's
                // content area — those are nested lists, not siblings.
                let inner_content_threshold =
                    marker_match.marker_len + marker_match.spaces_after_cols;
                let is_sibling_candidate = inner_indent_cols_raw < inner_content_threshold;
                let sibling_list_level = if is_sibling_candidate {
                    self.containers
                        .stack
                        .iter()
                        .enumerate()
                        .rev()
                        .find_map(|(i, c)| match c {
                            Container::List { marker, .. }
                                if lists::markers_match(
                                    &marker_match.marker,
                                    marker,
                                    self.config.dialect,
                                ) && self.containers.stack[..i]
                                    .iter()
                                    .filter(|x| matches!(x, Container::BlockQuote { .. }))
                                    .count()
                                    == bq_depth =>
                            {
                                Some(i)
                            }
                            _ => None,
                        })
                } else {
                    None
                };
                if let Some(list_level) = sibling_list_level {
                    // Read the matched LIST's base column before mutating
                    // the stack. We use it as the new sibling item's
                    // `indent_cols` so subsequent lines can match by
                    // source column even when the current line was lazy
                    // (its source column wouldn't have lined up).
                    let sibling_base_indent_cols = match self.containers.stack.get(list_level) {
                        Some(Container::List {
                            base_indent_cols, ..
                        }) => *base_indent_cols,
                        _ => 0,
                    };

                    // Flush any pending ListItem buffer before closing.
                    self.emit_list_item_buffer_if_needed();
                    // Close down to the inner LIST level (closing the open
                    // inner LIST_ITEM and anything nested inside it).
                    self.close_containers_to(list_level + 1);

                    // Emit the BQ markers as direct children of the inner
                    // LIST node (the builder is currently positioned inside
                    // it).
                    for i in 0..bq_depth {
                        if let Some(info) = same_depth_marker_info.get(i) {
                            self.emit_or_buffer_blockquote_marker(
                                info.leading_spaces,
                                info.has_trailing_space,
                            );
                        }
                    }

                    // Add the new sibling LIST_ITEM to the inner LIST.
                    let list_item = ListItemEmissionInput {
                        content: inner_content,
                        marker_len: marker_match.marker_len,
                        spaces_after_cols: marker_match.spaces_after_cols,
                        spaces_after_bytes: marker_match.spaces_after_bytes,
                        indent_cols: sibling_base_indent_cols,
                        indent_bytes: inner_indent_bytes,
                        virtual_marker_space: marker_match.virtual_marker_space,
                    };
                    lists::add_list_item(
                        &mut self.containers,
                        &mut self.builder,
                        &list_item,
                        self.config,
                    );
                    self.maybe_open_fenced_code_in_new_list_item();
                    self.maybe_open_indented_code_in_new_list_item();
                    self.pos += 1;
                    return true;
                }
            }

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
                if is_new_item_at_outer_level
                    || (effective_indent < content_col && !has_explicit_same_depth_marker)
                {
                    log::trace!(
                        "Closing ListItem: is_new_item={}, effective_indent={} < content_col={}",
                        is_new_item_at_outer_level,
                        effective_indent,
                        content_col
                    );
                    self.close_containers_to(self.containers.depth() - 1);
                } else {
                    log::trace!(
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

            let continuation_has_explicit_marker = list_item_continuation && {
                if has_explicit_same_depth_marker {
                    for i in 0..bq_depth {
                        if let Some(info) = same_depth_marker_info.get(i) {
                            self.emit_or_buffer_blockquote_marker(
                                info.leading_spaces,
                                info.has_trailing_space,
                            );
                        }
                    }
                    true
                } else {
                    false
                }
            };

            if !list_item_continuation {
                let marker_info = self.marker_info_for_line(
                    blockquote_payload.as_ref(),
                    line,
                    bq_marker_line,
                    shifted_bq_prefix,
                    used_shifted_bq,
                );
                for i in 0..bq_depth {
                    if let Some(info) = marker_info.get(i) {
                        self.emit_or_buffer_blockquote_marker(
                            info.leading_spaces,
                            info.has_trailing_space,
                        );
                    }
                }
            }
            let line_to_append = if list_item_continuation {
                if continuation_has_explicit_marker {
                    Some(inner_content)
                } else {
                    Some(line)
                }
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
                    self.config.dialect,
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
                            virtual_marker_space: marker_match.virtual_marker_space,
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
                            virtual_marker_space: marker_match.virtual_marker_space,
                        };
                        lists::add_list_item(
                            &mut self.containers,
                            &mut self.builder,
                            &list_item,
                            self.config,
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
        log::trace!(
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
                        has_blank_before: self.pos == 0 || is_blank_line(self.lines[self.pos - 1]),
                        has_blank_before_strict: self.pos == 0
                            || is_blank_line(self.lines[self.pos - 1]),
                        at_document_start: self.pos == 0 && self.current_blockquote_depth() == 0,
                        in_fenced_div: self.in_fenced_div(),
                        blockquote_depth: self.current_blockquote_depth(),
                        config: self.config,
                        content_indent,
                        indent_to_emit: None,
                        list_indent_info: None,
                        in_list: lists::in_list(&self.containers),
                        in_marker_only_list_item: matches!(
                            self.containers.last(),
                            Some(Container::ListItem {
                                marker_only: true,
                                ..
                            })
                        ),
                        paragraph_open: self.is_paragraph_open(),
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
            let in_footnote_definition = self
                .containers
                .stack
                .iter()
                .any(|container| matches!(container, Container::FootnoteDefinition { .. }));

            if bq_depth > 0 {
                if in_footnote_definition
                    && self.config.extensions.blank_before_blockquote
                    && current_bq_depth == 0
                    && !blockquotes::can_start_blockquote(self.pos, &self.lines)
                {
                    // Respect blank_before_blockquote even when `>` appears only
                    // after stripping content-container indentation (e.g. footnotes).
                    // In that case the marker should be treated as paragraph text.
                } else {
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
        }

        // Store the stripped content for later use
        let content = stripped_content;

        if self.is_paragraph_open()
            && (paragraphs::has_open_inline_math_environment(&self.containers)
                || paragraphs::has_open_display_math_dollars(&self.containers))
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
            in_marker_only_list_item: matches!(
                self.containers.last(),
                Some(Container::ListItem {
                    marker_only: true,
                    ..
                })
            ),
            paragraph_open: self.is_paragraph_open(),
            next_line,
        };

        // We'll update these two fields shortly (after they are computed), but we can still
        // use this ctx shape to avoid rebuilding repeated context objects.
        let mut dispatcher_ctx = dispatcher_ctx;

        // Setext heading folded over a list item's buffered first-line text.
        // Must run before block detection so that an HR-shaped underline like
        // `---` doesn't get claimed by the thematic-break parser.
        if self.try_fold_list_item_buffer_into_setext(stripped_content) {
            return true;
        }

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

            let prev_line_blank = is_blank_line(prev_line);
            prev_line_blank
                || prev_is_fenced_div_open
                || matches!(self.containers.last(), Some(Container::BlockQuote { .. }))
                || !self.previous_block_requires_blank_before_heading()
        };

        // For indented code blocks, we need a stricter condition - only actual blank lines count
        // Being at document start (pos == 0) is OK only if we're not inside a blockquote
        let at_document_start = self.pos == 0 && current_bq_depth == 0;

        let prev_line_blank = if self.pos > 0 {
            let prev_line = self.lines[self.pos - 1];
            let (prev_bq_depth, prev_inner) = count_blockquote_markers(prev_line);
            is_blank_line(prev_line) || (prev_bq_depth > 0 && is_blank_line(prev_inner))
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
                && is_inline_math_environment(env_name)
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

                if matches!(block_match.effect, BlockEffect::OpenFootnoteDefinition) {
                    self.close_open_footnote_definition();
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
                        // CommonMark §5.2: bullet lists and ordered lists with
                        // start = 1 may interrupt a paragraph; ordered lists
                        // with any other start cannot. Pandoc-markdown forbids
                        // *any* list from interrupting a paragraph without a
                        // blank line.
                        let allow_interrupt =
                            self.config.dialect == crate::options::Dialect::CommonMark && {
                                use super::block_dispatcher::ListPrepared;
                                use super::blocks::lists::OrderedMarker;
                                let prepared = block_match
                                    .payload
                                    .as_ref()
                                    .and_then(|p| p.downcast_ref::<ListPrepared>());
                                match prepared.map(|p| &p.marker) {
                                    Some(ListMarker::Bullet(_)) => true,
                                    Some(ListMarker::Ordered(OrderedMarker::Decimal {
                                        number,
                                        ..
                                    })) => number == "1",
                                    _ => false,
                                }
                            };
                        if !allow_interrupt {
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

                    // CommonMark spec example #312: a "list marker" at indent
                    // ≥ 4 isn't actually a marker when it can't reach the
                    // deepest item's content column AND no list level matches
                    // at that indent. Treat as lazy paragraph continuation of
                    // the deepest open list item or paragraph rather than
                    // flushing the buffer and opening a new sibling list.
                    if matches!(block_match.effect, BlockEffect::OpenList)
                        && self.try_lazy_list_continuation(block_match, content)
                    {
                        self.pos += 1;
                        return true;
                    }

                    self.emit_list_item_buffer_if_needed();
                    if self.is_paragraph_open() {
                        self.close_containers_to(self.containers.depth() - 1);
                    }

                    // CommonMark §5.2: a thematic break / ATX heading /
                    // fenced code at column 0 cannot continue an open list
                    // item whose content column is greater than the line's
                    // indent — close the surrounding list before emitting.
                    // OpenList is excluded so that a same-level marker still
                    // continues the list rather than closing it.
                    if self.config.dialect == crate::options::Dialect::CommonMark
                        && !matches!(block_match.effect, BlockEffect::OpenList)
                    {
                        let (indent_cols, _) = leading_indent(content);
                        self.close_lists_above_indent(indent_cols);
                    }
                }
                BlockDetectionResult::Yes => {
                    // CommonMark multi-line setext: when an open paragraph is
                    // followed by a setext underline, the entire paragraph
                    // becomes the heading content. The dispatcher reports
                    // setext at the line *before* the underline (the last text
                    // line); fold the buffered paragraph + this line into a
                    // single HEADING. Pandoc-markdown disagrees (it never
                    // forms a multi-line setext), so this branch is dialect-
                    // gated; under Pandoc, a setext detection while a
                    // paragraph is open never reaches this point because
                    // `blank_before_header` is on by default and gates out the
                    // detection earlier in `SetextHeadingParser::detect_prepared`.
                    if parser_name == "setext_heading"
                        && self.is_paragraph_open()
                        && self.config.dialect == crate::options::Dialect::CommonMark
                    {
                        let text_line = self.lines[self.pos];
                        let underline_line = self.lines[self.pos + 1];
                        let underline_char = underline_line.trim().chars().next().unwrap_or('=');
                        let level = if underline_char == '=' { 1 } else { 2 };
                        self.emit_setext_heading_folding_paragraph(
                            text_line,
                            underline_line,
                            level,
                        );
                        self.pos += 2;
                        return true;
                    }

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

                    // Reference definitions cannot interrupt a paragraph
                    // (CommonMark §4.7 / Pandoc-markdown agree).
                    if parser_name == "reference_definition" && self.is_paragraph_open() {
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

                if matches!(block_match.effect, BlockEffect::OpenFootnoteDefinition) {
                    self.close_open_footnote_definition();
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
            log::trace!("Parsed line block at line {}", self.pos);
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
            log::trace!(
                "Inside ListItem - buffering content: {:?}",
                line_to_append.unwrap_or(self.lines[self.pos]).trim_end()
            );
            // Inside list item - buffer content for later parsing
            let line = line_to_append.unwrap_or(self.lines[self.pos]);

            // Add line to buffer in the ListItem container
            if let Some(Container::ListItem {
                buffer,
                marker_only,
                ..
            }) = self.containers.stack.last_mut()
            {
                buffer.push_text(line);
                if !is_blank_line(line) {
                    *marker_only = false;
                }
            }

            self.pos += 1;
            return true;
        }

        log::trace!(
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

/// Emit buffered Definition content as either Heading-then-Plain (when the
/// first line is an ATX heading) or as a single Plain block.
///
/// Pandoc parses `Term\n: # Heading\n  Some text` as DefinitionList where the
/// definition contains [Header, Plain]; the `# Heading` line is a real Header
/// inside the definition, not text that happens to start with `#`.
fn emit_definition_plain_or_heading(
    builder: &mut GreenNodeBuilder<'static>,
    text: &str,
    config: &ParserOptions,
) {
    let line_without_newline = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'));
    if let Some(line) = line_without_newline
        && !line.contains('\n')
        && !line.contains('\r')
        && let Some(level) = try_parse_atx_heading(line)
    {
        emit_atx_heading(builder, text, level, config);
        return;
    }

    // Multi-line: first line is heading, rest is plain continuation.
    if let Some(first_nl) = text.find('\n') {
        let first_line = &text[..first_nl];
        let after_first = &text[first_nl + 1..];
        if !after_first.is_empty()
            && let Some(level) = try_parse_atx_heading(first_line)
        {
            let heading_bytes = &text[..first_nl + 1];
            emit_atx_heading(builder, heading_bytes, level, config);
            builder.start_node(SyntaxKind::PLAIN.into());
            inline_emission::emit_inlines(builder, after_first, config);
            builder.finish_node();
            return;
        }
    }

    builder.start_node(SyntaxKind::PLAIN.into());
    inline_emission::emit_inlines(builder, text, config);
    builder.finish_node();
}

/// Look ahead from `pos+1` past blank lines for a definition marker line at
/// `content_col` indent. Returns the blank-line count consumed before the
/// marker, or `None` if no marker is found at the next non-blank line.
///
/// Used by `handle_footnote_open_effect` to decide whether the first content
/// line of a footnote body should open a definition-list term: pandoc treats
/// `[^1]: Term\n\n    :   Definition\n` as a `Note [DefinitionList ...]`,
/// not as a paragraph followed by a separate def list with no term.
fn footnote_first_line_term_lookahead(
    lines: &[&str],
    pos: usize,
    content_col: usize,
    table_captions_enabled: bool,
) -> Option<usize> {
    let mut check_pos = pos + 1;
    let mut blank_count = 0;
    while check_pos < lines.len() {
        let line = lines[check_pos];
        let (trimmed, _) = strip_newline(line);
        if trimmed.trim().is_empty() {
            blank_count += 1;
            check_pos += 1;
            continue;
        }
        let (line_indent_cols, _) = leading_indent(trimmed);
        if line_indent_cols < content_col {
            return None;
        }
        let strip_bytes = byte_index_at_column(trimmed, content_col);
        if strip_bytes > trimmed.len() {
            return None;
        }
        let stripped = &trimmed[strip_bytes..];
        if let Some((marker, ..)) = definition_lists::try_parse_definition_marker(stripped) {
            // A `:` line that is actually a table caption shouldn't open a
            // definition list. Mirror the gate from
            // `next_line_is_definition_marker`.
            if marker == ':'
                && table_captions_enabled
                && super::blocks::tables::is_caption_followed_by_table(lines, check_pos)
            {
                return None;
            }
            return Some(blank_count);
        }
        return None;
    }
    None
}
