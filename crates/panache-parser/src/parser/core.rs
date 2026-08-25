use crate::options::ParserOptions;
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::GreenNodeBuilder;

use super::block_dispatcher::{
    BlockContext, BlockDetectionResult, BlockEffect, BlockParserRegistry, BlockQuotePrepared,
    PreparedBlockMatch,
};
use super::blocks::blockquotes;
use super::blocks::code_blocks;
use super::blocks::container_prefix::{
    ContainerPrefix, StrippedLines, resolve_content_indent, strip_content_indent,
};
use super::blocks::definition_lists;
use super::blocks::fenced_divs;
use super::blocks::figures::paragraph_is_standalone_image;
use super::blocks::headings::{
    emit_atx_heading, emit_setext_heading_body, emit_setext_heading_text, emit_setext_underline,
    try_parse_atx_heading, try_parse_setext_heading,
};
use super::blocks::horizontal_rules::try_parse_horizontal_rule;
use super::blocks::html_blocks;
use super::blocks::line_blocks;
use super::blocks::lists;
use super::blocks::paragraphs;
use super::blocks::raw_blocks::{extract_environment_name, is_inline_math_environment};
use super::blocks::tables;
use super::blocks::tables::LineView;
use super::diagnostics::{Diagnostics, SyntaxError};
use super::utils::container_stack;
use super::utils::helpers::{
    is_blank_line, split_lines_inclusive, strip_leading_spaces_n, strip_newline,
};
use super::utils::marker_utils;
use super::utils::text_buffer;

use super::blocks::blockquotes::strip_n_blockquote_markers;
use super::utils::continuation::ContinuationPolicy;
use container_stack::{
    Container, ContainerStack, FOOTNOTE_INDENT_COLUMNS, byte_index_at_column,
    gobbled_indent_prefix_len, leading_indent,
};
use definition_lists::{emit_definition_marker, emit_term};
use line_blocks::{parse_line_block, try_parse_line_block_start};
use lists::{
    ListItemEmissionInput, ListMarker, is_content_nested_bullet_marker, start_nested_list,
    try_parse_list_marker,
};
use marker_utils::{count_blockquote_markers, parse_blockquote_marker_info};
use text_buffer::ParagraphBuffer;

#[path = "core/blockquotes.rs"]
mod blockquote_protocol;
#[path = "core/definition_lists.rs"]
mod definition_list_protocol;
#[path = "core/html_interruptions.rs"]
mod html_interruption_protocol;
#[path = "core/list_items.rs"]
mod list_item_protocol;

use blockquote_protocol::{LazyFold, LazyInterruptContext};
use definition_list_protocol::{
    BufferedBodyBlock, emit_definition_plain_or_heading, first_content_line_term_lookahead,
};

const GITHUB_ALERT_MARKERS: [&str; 5] = [
    "[!TIP]",
    "[!WARNING]",
    "[!IMPORTANT]",
    "[!CAUTION]",
    "[!NOTE]",
];

/// Result of dispatching a line without advancing `self.pos`.
#[must_use]
#[derive(Debug, Clone, Copy)]
pub(crate) enum LineDispatch {
    Consumed(usize),
    Rejected,
}

impl LineDispatch {
    #[inline]
    pub(crate) fn consumed(n: usize) -> Self {
        debug_assert!(n >= 1, "LineDispatch::Consumed requires n >= 1");
        LineDispatch::Consumed(n)
    }
}

pub struct Parser<'a> {
    lines: Vec<&'a str>,
    pos: usize,
    builder: GreenNodeBuilder<'static>,
    containers: ContainerStack,
    config: &'a ParserOptions,
    block_registry: &'static BlockParserRegistry,
    after_metadata_block: bool,
    at_note_body_start: bool,
    dispatch_list_marker_consumed: bool,
    diagnostics: Diagnostics,
    origin: ParseOrigin,
}

/// Whether a parse covers a whole document or a fragment lifted out of one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParseOrigin {
    #[default]
    Document,
    Fragment,
}

impl<'a> Parser<'a> {
    pub fn new(input: &'a str, config: &'a ParserOptions) -> Self {
        Self::with_origin(input, config, ParseOrigin::Document)
    }

    /// Parse a slice that does not begin at the document's byte zero.
    pub fn new_fragment(input: &'a str, config: &'a ParserOptions) -> Self {
        Self::with_origin(input, config, ParseOrigin::Fragment)
    }

    fn with_origin(input: &'a str, config: &'a ParserOptions, origin: ParseOrigin) -> Self {
        let lines = split_lines_inclusive(input);
        Self {
            lines,
            pos: 0,
            builder: GreenNodeBuilder::new(),
            containers: ContainerStack::new(),
            config,
            block_registry: BlockParserRegistry::shared(),
            after_metadata_block: false,
            at_note_body_start: false,
            dispatch_list_marker_consumed: false,
            diagnostics: Diagnostics::new(),
            origin,
        }
    }

    fn origin_allows_document_start(&self) -> bool {
        self.origin == ParseOrigin::Document
    }

    pub fn parse(self) -> SyntaxNode {
        self.parse_with_errors().0
    }

    /// Parse the CST and return host-ranged embedded-language errors.
    pub fn parse_with_errors(mut self) -> (SyntaxNode, Vec<SyntaxError>) {
        self.parse_document_stack();
        let node = SyntaxNode::new_root(self.builder.finish());
        let errors = self.diagnostics.take();
        (node, errors)
    }

    /// Close enclosing list items (and their containing list) whose
    /// `content_col` exceeds the given indent. Under CommonMark this covers
    /// every interrupting block (HR, ATX heading, fenced code, ...): per §5.2
    /// a line shallower than the item's content column cannot continue the
    /// item, so the item and the surrounding list close before the new block
    /// is emitted at the outer level.
    ///
    /// Pandoc uses the same close but for fenced code *only* — its
    /// `rawListItem` stops collecting at a line `codeBlockFenced` would claim
    /// while still swallowing an under-indented heading or thematic break as
    /// lazy item text. Which blocks qualify is therefore the call site's
    /// question, not this helper's.
    ///
    /// The loop stops at any non-`ListItem` container, so a list *outside* an
    /// enclosing blockquote is never reached from inside it.
    fn close_containers_to(&mut self, keep: usize) {
        while self.containers.depth() > keep {
            match self.containers.stack.last() {
                Some(Container::ListItem { buffer, .. }) if !buffer.is_empty() => {
                    let buffer_clone = buffer.clone();
                    let gobble = self.containers.gobble_chain();

                    log::trace!(
                        "Closing ListItem with buffer (is_empty={}, buffered_line_count={})",
                        buffer_clone.is_empty(),
                        buffer_clone.buffered_line_count()
                    );

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

                    let suppress_footnote_refs = self.in_footnote_definition();
                    self.containers.stack.pop();
                    buffer_clone.emit_as_block(
                        &mut self.builder,
                        use_paragraph,
                        self.config,
                        &gobble,
                        suppress_footnote_refs,
                        true,
                    );
                    self.builder.finish_node(); // Close LIST_ITEM
                }
                Some(Container::ListItem { .. }) => {
                    log::trace!("Closing empty ListItem (no buffer content)");
                    self.containers.stack.pop();
                    self.builder.finish_node();
                }
                Some(Container::Paragraph {
                    buffer,
                    start_checkpoint,
                    ..
                }) if !buffer.is_empty() => {
                    let buffer_clone = buffer.clone();
                    let checkpoint = *start_checkpoint;
                    let suppress_footnote_refs = self.in_footnote_definition();
                    let wrapper = if paragraph_is_standalone_image(
                        &buffer_clone.get_text_for_parsing(),
                        self.config,
                    ) {
                        SyntaxKind::FIGURE
                    } else {
                        SyntaxKind::PARAGRAPH
                    };
                    self.containers.stack.pop();
                    self.builder.start_node_at(checkpoint, wrapper.into());
                    buffer_clone.emit_with_inlines(
                        &mut self.builder,
                        self.config,
                        suppress_footnote_refs,
                    );
                    self.builder.finish_node();
                }
                Some(Container::Paragraph {
                    start_checkpoint, ..
                }) => {
                    let checkpoint = *start_checkpoint;
                    self.containers.stack.pop();
                    self.builder
                        .start_node_at(checkpoint, SyntaxKind::PARAGRAPH.into());
                    self.builder.finish_node();
                }
                Some(Container::Definition {
                    plain_open: true,
                    plain_buffer,
                    ..
                }) if !plain_buffer.is_empty() => {
                    let buffer = plain_buffer.clone();
                    let suppress_footnote_refs = self.in_footnote_definition();
                    emit_definition_plain_or_heading(
                        &mut self.builder,
                        &buffer,
                        self.config,
                        suppress_footnote_refs,
                    );

                    if let Some(Container::Definition {
                        plain_open,
                        plain_buffer,
                        ..
                    }) = self.containers.stack.last_mut()
                    {
                        plain_buffer.clear();
                        *plain_open = false;
                    }

                    self.containers.stack.pop();
                    self.builder.finish_node();
                }
                Some(Container::Definition {
                    plain_open: true, ..
                }) => {
                    if let Some(Container::Definition {
                        plain_open,
                        plain_buffer,
                        ..
                    }) = self.containers.stack.last_mut()
                    {
                        plain_buffer.clear();
                        *plain_open = false;
                    }

                    self.containers.stack.pop();
                    self.builder.finish_node();
                }
                _ => {
                    self.containers.stack.pop();
                    self.builder.finish_node();
                }
            }
        }
    }

    fn emit_buffered_plain_if_needed(&mut self) {
        if let Some(Container::Definition {
            plain_open: true,
            plain_buffer,
            ..
        }) = self.containers.stack.last()
            && !plain_buffer.is_empty()
        {
            let buffer = plain_buffer.clone();
            let suppress_footnote_refs = self.in_footnote_definition();
            emit_definition_plain_or_heading(
                &mut self.builder,
                &buffer,
                self.config,
                suppress_footnote_refs,
            );
        }

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

    fn close_paragraph_if_open(&mut self) {
        if self.is_paragraph_open() {
            self.close_containers_to(self.containers.depth() - 1);
        }
    }

    /// Close an open `Container::Paragraph` at the top of the stack, retagging
    /// the wrapper as `PLAIN` instead of `PARAGRAPH`. Mirrors pandoc's
    /// `[Plain[foo], RawBlock<p>]` shape when a paragraph terminates because
    /// the next line opens an HTML strict-block / verbatim block.
    ///
    /// Caller is responsible for ensuring the paragraph is at the top of the
    /// container stack (i.e. no other deeper containers above it). All other
    /// closing-related semantics (list-item buffering, blockquote depth) are
    /// unchanged from `close_paragraph_if_open`; this method only changes the
    /// emitted wrapper kind.
    fn close_paragraph_as_plain_if_open(&mut self) {
        if !self.is_paragraph_open() {
            return;
        }
        let Some(Container::Paragraph {
            buffer,
            start_checkpoint,
            ..
        }) = self.containers.stack.last()
        else {
            return;
        };
        let buffer_clone = buffer.clone();
        let checkpoint = *start_checkpoint;
        let suppress_footnote_refs = self.in_footnote_definition();
        self.containers.stack.pop();
        self.builder
            .start_node_at(checkpoint, SyntaxKind::PLAIN.into());
        if !buffer_clone.is_empty() {
            buffer_clone.emit_with_inlines(&mut self.builder, self.config, suppress_footnote_refs);
        }
        self.builder.finish_node();
    }

    /// Whether an HTML block about to interrupt an open paragraph should
    /// retag the paragraph wrapper as `PLAIN` (pandoc's
    /// `[Plain[foo], RawBlock<p>]` rule). Fires only under Pandoc dialect
    /// when the YesCanInterrupt match is an HTML `BlockTag` — by
    /// construction this is a strict-block (`PANDOC_BLOCK_TAGS`) or
    /// verbatim (`VERBATIM_TAGS`) tag, since inline-block / void block
    /// tags and Type7 / comments take the `cannot_interrupt` path and
    /// never reach this site.
    fn prepare_for_block_element(&mut self) {
        self.emit_list_item_buffer_if_needed();
        self.close_paragraph_if_open();
    }

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
    ) -> usize {
        let content_start = block_match
            .payload
            .as_ref()
            .and_then(|p| p.downcast_ref::<super::block_dispatcher::FootnoteDefinitionPrepared>())
            .map(|p| p.content_start)
            .unwrap_or(0);

        let content_col = FOOTNOTE_INDENT_COLUMNS;
        self.containers
            .push(Container::FootnoteDefinition { content_col });

        if content_start == 0 {
            return 0;
        }
        let first_line_content = &content[content_start..];
        if first_line_content.trim().is_empty() {
            let (_, newline_str) = strip_newline(content);
            if !newline_str.is_empty() {
                self.builder.token(SyntaxKind::NEWLINE.into(), newline_str);
            }
            self.at_note_body_start = true;
            return 0;
        }

        if self.config.extensions.definition_lists
            && let Some(blank_count) = first_content_line_term_lookahead(
                self.lines.as_slice(),
                self.pos,
                content_col,
                self.config.extensions.table_captions,
            )
        {
            self.builder.start_node(SyntaxKind::DEFINITION_LIST.into());
            self.containers.push(Container::DefinitionList {});
            self.builder.start_node(SyntaxKind::DEFINITION_ITEM.into());
            self.containers.push(Container::DefinitionItem {});
            emit_term(&mut self.builder, first_line_content, None, self.config);
            self.emit_term_lookahead_blank_lines(blank_count);
            return blank_count;
        }

        if let Some(extras) = self.try_dispatch_footnote_html_block(first_line_content, content_col)
        {
            return extras;
        }

        if let Some(extras) = self.try_dispatch_footnote_marker_line_block(first_line_content) {
            return extras;
        }

        paragraphs::start_paragraph_if_needed(&mut self.containers, &mut self.builder);
        paragraphs::append_paragraph_line(
            &mut self.containers,
            &mut self.builder,
            first_line_content,
            self.config,
        );
        0
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
    fn try_dispatch_footnote_marker_line_block(
        &mut self,
        first_line_content: &str,
    ) -> Option<usize> {
        if self.config.dialect != crate::options::Dialect::Pandoc {
            return None;
        }

        let mut synthetic: Vec<&str> = Vec::with_capacity(self.lines.len() - self.pos);
        synthetic.push(first_line_content);
        synthetic.extend_from_slice(&self.lines[self.pos + 1..]);
        let prefix = ContainerPrefix::from_stack(&self.containers.stack, false, self.config);
        let window = StrippedLines::new(&synthetic, 0, &prefix);

        let ctx = BlockContext {
            has_blank_before: true,
            has_blank_before_strict: false,
            at_document_start: false,
            in_fenced_div: self.in_fenced_div(),
            fenced_div_open_indent: self.innermost_fenced_div_open_indent(),
            fenced_div_wraps_list: self.fenced_div_wraps_innermost_list(),
            myst_directive_closer: self.innermost_myst_directive_closer(),
            blockquote_depth: self.current_blockquote_depth(),
            config: self.config,
            diags: self.diagnostics.clone(),
            content_indent: self.content_container_indent_to_strip(),
            indent_to_emit: None,
            list_indent_info: None,
            in_list: false,
            in_definition_list: false,
            in_marker_only_list_item: false,
            list_item_unclosed_html_block_tag: None,
            open_code_span_openers: Vec::new(),
            paragraph_open: false,
            list_item_content_open: false,
            next_line: self.lines.get(self.pos + 1).map(|line| prefix.strip(line)),
            open_alpha_hint: lists::OpenListHint::None,
            restricted_ordered_sublist: false,
        };

        let block_match = self.block_registry.detect_prepared(&ctx, &window)?;
        match self.block_registry.parser_name(&block_match) {
            "table" | "horizontal_rule" | "fenced_code_block" | "setext_heading" => {
                let consumed = self.block_registry.parse_prepared(
                    &block_match,
                    &ctx,
                    &mut self.builder,
                    &window,
                );
                Some(consumed.saturating_sub(1))
            }
            "list" => self.open_footnote_marker_line_list(first_line_content),
            "blockquote" => self.open_footnote_marker_line_blockquote(&block_match),
            _ => None,
        }
    }

    fn open_footnote_marker_line_list(&mut self, first_line_content: &str) -> Option<usize> {
        let marker_match =
            try_parse_list_marker(first_line_content, self.config, lists::OpenListHint::None)?;
        let (indent_cols, indent_bytes) = leading_indent(first_line_content);

        self.builder.start_node(SyntaxKind::LIST.into());
        self.containers.push(Container::List {
            marker: marker_match.marker.clone(),
            base_indent_cols: indent_cols,
            has_blank_between_items: false,
        });

        let list_item = ListItemEmissionInput {
            content: first_line_content,
            marker_len: marker_match.marker_len,
            spaces_after_cols: marker_match.spaces_after_cols,
            spaces_after_bytes: marker_match.spaces_after_bytes,
            indent_cols,
            indent_bytes,
            virtual_marker_space: marker_match.virtual_marker_space,
        };

        let finish = if let Some(nested_marker) = is_content_nested_bullet_marker(
            first_line_content,
            marker_match.marker_len,
            marker_match.spaces_after_bytes,
        ) {
            lists::add_list_item_with_nested_empty_list(
                &mut self.containers,
                &mut self.builder,
                &list_item,
                nested_marker,
                self.config,
            );
            lists::ListItemFinish::Done
        } else {
            lists::add_list_item(
                &mut self.containers,
                &mut self.builder,
                &list_item,
                self.config,
            )
        };
        Some(self.dispatch_bq_after_list_item(finish))
    }

    fn open_footnote_marker_line_blockquote(
        &mut self,
        block_match: &super::block_dispatcher::PreparedBlockMatch,
    ) -> Option<usize> {
        let prepared = block_match
            .payload
            .as_ref()?
            .downcast_ref::<BlockQuotePrepared>()?;
        let inner = prepared.inner_content.as_str();
        if strip_newline(inner).0.trim().is_empty() {
            return None;
        }

        for level in 0..prepared.depth {
            self.builder.start_node(SyntaxKind::BLOCK_QUOTE.into());
            if let Some(info) = prepared.marker_info.get(level) {
                blockquotes::emit_one_blockquote_marker(
                    &mut self.builder,
                    info.leading_spaces,
                    info.has_trailing_space,
                );
            }
            self.containers.push(Container::BlockQuote {});
        }

        paragraphs::start_paragraph_if_needed(&mut self.containers, &mut self.builder);
        paragraphs::append_paragraph_line(
            &mut self.containers,
            &mut self.builder,
            inner,
            self.config,
        );
        Some(0)
    }

    fn open_code_span_openers(&self) -> Vec<usize> {
        let Some(line) = self.lines.get(self.pos) else {
            return Vec::new();
        };
        if !line.trim_start_matches([' ', '\t', '>']).starts_with('`') {
            return Vec::new();
        }
        match self.containers.last() {
            Some(Container::Paragraph { buffer, .. }) => buffer.pending_code_span_openers(),
            Some(Container::ListItem { buffer, .. }) => buffer.pending_code_span_openers(),
            Some(Container::Definition { plain_buffer, .. }) => {
                plain_buffer.pending_code_span_openers()
            }
            _ => Vec::new(),
        }
    }

    /// Emit or buffer a blockquote marker depending on parser state.
    ///
    /// If a paragraph is open and we're using integrated parsing, buffer the marker.
    /// Otherwise emit it directly to the builder.
    fn parse_document_stack(&mut self) {
        self.builder.start_node(SyntaxKind::DOCUMENT.into());

        log::trace!("Starting document parse");

        while self.pos < self.lines.len() {
            let line = self.lines[self.pos];

            log::trace!("Parsing line {}: {}", self.pos + 1, line);

            match self.parse_line(line) {
                LineDispatch::Consumed(n) => self.pos += n,
                LineDispatch::Rejected => self.pos += 1,
            }
        }

        self.close_containers_to(0);
        self.builder.finish_node(); // DOCUMENT
    }

    fn parse_line(&mut self, line: &'a str) -> LineDispatch {
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

        if self.restricted_sublist_interrupts(if current_bq_depth > 0 {
            inner_content
        } else {
            line
        }) {
            self.emit_list_item_buffer_if_needed();
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.close_containers_to(self.containers.depth() - 1);
            }
        }

        let has_blank_before = self.pos == 0 || is_blank_line(self.lines[self.pos - 1]);
        let mut blockquote_match: Option<PreparedBlockMatch> = None;
        let dispatcher_ctx = if current_bq_depth == 0 {
            Some(BlockContext {
                has_blank_before,
                has_blank_before_strict: has_blank_before,
                at_document_start: self.pos == 0,
                in_fenced_div: self.in_fenced_div(),
                fenced_div_open_indent: self.innermost_fenced_div_open_indent(),
                fenced_div_wraps_list: self.fenced_div_wraps_innermost_list(),
                myst_directive_closer: self.innermost_myst_directive_closer(),
                blockquote_depth: current_bq_depth,
                config: self.config,
                diags: self.diagnostics.clone(),
                content_indent: 0,
                indent_to_emit: None,
                list_indent_info: None,
                in_list: lists::in_list(&self.containers),
                in_definition_list: definition_lists::in_definition_list(&self.containers),
                in_marker_only_list_item: matches!(
                    self.containers.last(),
                    Some(Container::ListItem {
                        marker_only: true,
                        ..
                    })
                ),
                list_item_unclosed_html_block_tag: self.list_item_unclosed_html_block_tag(),
                open_code_span_openers: self.open_code_span_openers(),
                paragraph_open: self.is_paragraph_open(),
                list_item_content_open: self.is_list_item_content_open(),
                next_line: if self.pos + 1 < self.lines.len() {
                    Some(self.lines[self.pos + 1])
                } else {
                    None
                },
                open_alpha_hint: lists::open_list_hint_at_indent(
                    &self.containers,
                    leading_indent(line).0,
                ),
                restricted_ordered_sublist: self.restricted_ordered_sublist(line),
            })
        } else {
            None
        };

        let mut registry_claims_raw_line = false;
        let mut blockquote_payload = if let Some(dispatcher_ctx) = dispatcher_ctx.as_ref() {
            let prefix = ContainerPrefix::from_ctx(dispatcher_ctx);
            let stripped = StrippedLines::new(&self.lines, self.pos, &prefix);
            self.block_registry
                .detect_prepared(dispatcher_ctx, &stripped)
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
                        registry_claims_raw_line = true;
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

        let inner_blank_in_blockquote = bq_depth > 0
            && is_blank_line(inner_content)
            && (current_bq_depth > 0
                || !self.config.extensions.blank_before_blockquote
                || blockquotes::can_start_blockquote(
                    self.pos,
                    &self.lines,
                    self.config.extensions.fenced_divs,
                ));
        let is_blank = is_blank_line(line) || inner_blank_in_blockquote;

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
                return LineDispatch::consumed(1);
            }

            self.close_paragraph_if_open();

            if self.blank_line_promotes_buffered_definition_term() {
                self.promote_buffered_definition_term();
            }
            self.emit_buffered_plain_if_needed();

            if bq_depth > current_bq_depth {
                for _ in current_bq_depth..bq_depth {
                    self.builder.start_node(SyntaxKind::BLOCK_QUOTE.into());
                    self.containers.push(Container::BlockQuote {});
                }
            } else if bq_depth < current_bq_depth {
                self.close_blockquotes_to_depth(bq_depth);
            }

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

            let levels_to_keep = if peek < self.lines.len() {
                ContinuationPolicy::new(self.config, self.block_registry).compute_levels_to_keep(
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

            while self.containers.depth() > levels_to_keep {
                match self.containers.last() {
                    Some(Container::ListItem { .. }) => {
                        log::trace!(
                            "Closing ListItem at blank line (levels_to_keep={} < depth={})",
                            levels_to_keep,
                            self.containers.depth()
                        );
                        self.close_containers_to(self.containers.depth() - 1);
                    }
                    Some(Container::List { .. })
                    | Some(Container::FootnoteDefinition { .. })
                    | Some(Container::Admonition { .. })
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

            if matches!(self.containers.last(), Some(Container::ListItem { .. })) {
                self.emit_list_item_buffer_if_needed();
            }

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

            return LineDispatch::consumed(1);
        }

        if registry_claims_raw_line && bq_depth > 0 && !used_shifted_bq {
            return self.parse_inner_content(line, None);
        }

        if bq_depth > current_bq_depth
            && !used_shifted_bq
            && let Some(cap) = self.blockquote_depth_cap(current_bq_depth, bq_depth)
        {
            bq_depth = cap;
            inner_content = blockquotes::strip_n_blockquote_markers(line, cap);
            blockquote_match = None;
            if let Some(payload) = blockquote_payload.as_mut() {
                payload.depth = cap;
                payload.can_nest |= cap <= 1;
            }
        }

        if bq_depth > current_bq_depth {
            if self.config.extensions.blank_before_blockquote
                && current_bq_depth == 0
                && !used_shifted_bq
                && !blockquote_payload
                    .as_ref()
                    .map(|payload| payload.can_start)
                    .unwrap_or_else(|| {
                        blockquotes::can_start_blockquote(
                            self.pos,
                            &self.lines,
                            self.config.extensions.fenced_divs,
                        )
                    })
            {
                if self.is_list_item_content_open() {
                    self.append_lazy_continuation_line(line);
                    return LineDispatch::consumed(1);
                }
                self.emit_list_item_buffer_if_needed();
                paragraphs::start_paragraph_if_needed(&mut self.containers, &mut self.builder);
                paragraphs::append_paragraph_line(
                    &mut self.containers,
                    &mut self.builder,
                    line,
                    self.config,
                );
                return LineDispatch::consumed(1);
            }

            let can_nest = if current_bq_depth > 0 {
                if self.config.extensions.blank_before_blockquote {
                    matches!(self.containers.last(), Some(Container::BlockQuote { .. }))
                        || (self.pos > 0 && {
                            let prev_line = self.lines[self.pos - 1];
                            let (prev_bq_depth, prev_inner) = count_blockquote_markers(prev_line);
                            (prev_bq_depth >= current_bq_depth && is_blank_line(prev_inner))
                                || blockquotes::opens_fenced_div_at_depth(
                                    prev_line,
                                    current_bq_depth,
                                    self.config.extensions.fenced_divs,
                                )
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
                let content_at_current_depth =
                    blockquotes::strip_n_blockquote_markers(line, current_bq_depth);

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
                    paragraphs::append_paragraph_line(
                        &mut self.containers,
                        &mut self.builder,
                        content_at_current_depth,
                        self.config,
                    );
                    return LineDispatch::consumed(1);
                } else if self.is_list_item_content_open() {
                    self.append_lazy_continuation_line(content_at_current_depth);
                    return LineDispatch::consumed(1);
                } else {
                    paragraphs::start_paragraph_if_needed(&mut self.containers, &mut self.builder);
                    paragraphs::append_paragraph_line(
                        &mut self.containers,
                        &mut self.builder,
                        content_at_current_depth,
                        self.config,
                    );
                    return LineDispatch::consumed(1);
                }
            }

            self.emit_list_item_buffer_if_needed();

            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.close_containers_to(self.containers.depth() - 1);
            }

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
                let prefix = ContainerPrefix::from_ctx(dispatcher_ctx);
                let stripped = StrippedLines::new(&self.lines, self.pos, &prefix);
                let _ = self.block_registry.parse_prepared(
                    prepared,
                    dispatcher_ctx,
                    &mut self.builder,
                    &stripped,
                );
                for _ in 0..bq_depth {
                    self.containers.push(Container::BlockQuote {});
                }
            } else {
                for level in 0..current_bq_depth {
                    if let Some(info) = marker_info.get(level) {
                        blockquotes::emit_one_blockquote_marker(
                            &mut self.builder,
                            info.leading_spaces,
                            info.has_trailing_space,
                        );
                    }
                }

                for level in current_bq_depth..bq_depth {
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
            }

            let prev_flag = self.dispatch_list_marker_consumed;
            if used_shifted_bq && !self.innermost_li_above_bq() {
                self.dispatch_list_marker_consumed = true;
            }
            let dispatch = self.parse_inner_content(inner_content, Some(inner_content));
            self.dispatch_list_marker_consumed = prev_flag;
            return dispatch;
        } else if bq_depth < current_bq_depth {
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                let interrupts = self.lazy_interrupts(
                    line,
                    inner_content,
                    &LazyInterruptContext::for_paragraph(),
                );
                if !interrupts.any() {
                    if bq_depth > 0 {
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
                    return LineDispatch::consumed(1);
                }
                if interrupts.html
                    && self.config.dialect != crate::options::Dialect::CommonMark
                    && !interrupts.ends_gobble
                {
                    self.close_paragraph_as_plain_if_open();
                }
            }
            let quoted_grid_hold = lists::in_blockquote_list(&self.containers)
                && self.partial_separator_continues_item_table(inner_content);
            if quoted_grid_hold {
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
                        buffer.push_text(inner_content, self.config);
                        *marker_only = false;
                    }
                } else if let Some(Container::ListItem {
                    buffer,
                    marker_only,
                    ..
                }) = self.containers.stack.last_mut()
                {
                    buffer.push_text(line, self.config);
                    *marker_only = false;
                }
                return LineDispatch::consumed(1);
            }

            if matches!(self.containers.last(), Some(Container::ListItem { .. }))
                && lists::in_blockquote_list(&self.containers)
                && try_parse_list_marker(
                    line,
                    self.config,
                    lists::open_list_hint_at_indent(&self.containers, leading_indent(line).0),
                )
                .is_none()
            {
                let interrupts = self.lazy_interrupts(
                    line,
                    inner_content,
                    &LazyInterruptContext::for_list_item(),
                );
                if !interrupts.any() {
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
                            buffer.push_text(inner_content, self.config);
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
                        buffer.push_text(line, self.config);
                        if !line.trim().is_empty() {
                            *marker_only = false;
                        }
                    }
                    return LineDispatch::consumed(1);
                }
            }
            if self.config.extensions.definition_lists
                && definition_lists::in_definition_list(&self.containers)
                && definition_lists::try_parse_definition_marker(inner_content).is_some()
            {
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
                return self.parse_inner_content(inner_content, Some(inner_content));
            }
            if bq_depth == 0 && self.config.dialect != crate::options::Dialect::CommonMark {
                // Pandoc may resume an enclosing quoted list from an unquoted line.
                if lists::in_blockquote_list(&self.containers)
                    && let Some(marker_match) = try_parse_list_marker(
                        line,
                        self.config,
                        lists::open_list_hint_at_indent(&self.containers, leading_indent(line).0),
                    )
                {
                    let (indent_cols, indent_bytes) = leading_indent(line);
                    if let Some(level) = lists::find_matching_list_level(
                        &self.containers,
                        &marker_match.marker,
                        indent_cols,
                        self.config.dialect,
                    ) {
                        self.close_containers_to(level + 1);

                        if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                            self.close_containers_to(self.containers.depth() - 1);
                        }
                        if matches!(self.containers.last(), Some(Container::ListItem { .. })) {
                            self.close_containers_to(self.containers.depth() - 1);
                        }

                        let extras = if let Some(nested_marker) = is_content_nested_bullet_marker(
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
                                self.config,
                            );
                            0
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
                            let finish = lists::add_list_item(
                                &mut self.containers,
                                &mut self.builder,
                                &list_item,
                                self.config,
                            );
                            self.dispatch_bq_after_list_item(finish)
                        };
                        return LineDispatch::consumed(1 + extras);
                    }
                }
            }

            if self.config.dialect != crate::options::Dialect::CommonMark
                && !is_blank_line(inner_content)
                && !self.blockquote_gobble_ends_at(line, inner_content)
            {
                return self.fold_lazy_line_into_blockquote(
                    LazyFold {
                        line,
                        inner_content,
                        bq_depth,
                        bq_marker_line,
                        shifted_bq_prefix,
                        used_shifted_bq,
                    },
                    blockquote_payload.as_ref(),
                );
            }

            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                self.close_containers_to(self.containers.depth() - 1);
            }

            self.close_blockquotes_to_depth(bq_depth);

            if bq_depth > 0 {
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
                return self.parse_inner_content(inner_content, Some(inner_content));
            } else {
                return self.parse_inner_content(line, None);
            }
        } else if bq_depth > 0 {
            let mut list_item_continuation = false;
            let same_depth_marker_info = self.marker_info_for_line(
                blockquote_payload.as_ref(),
                line,
                bq_marker_line,
                shifted_bq_prefix,
                used_shifted_bq,
            );
            let has_explicit_same_depth_marker = same_depth_marker_info.len() >= bq_depth;

            let (inner_indent_cols_raw, inner_indent_bytes) = leading_indent(inner_content);
            if let Some(marker_match) = try_parse_list_marker(
                inner_content,
                self.config,
                lists::open_list_hint_at_indent(&self.containers, inner_indent_cols_raw),
            ) {
                let inner_content_threshold =
                    marker_match.marker_len + marker_match.spaces_after_cols;
                let is_sibling_candidate = inner_indent_cols_raw < inner_content_threshold
                    && !self.partial_separator_continues_item_table(inner_content);
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
                    let sibling_base_indent_cols = match self.containers.stack.get(list_level) {
                        Some(Container::List {
                            base_indent_cols, ..
                        }) => *base_indent_cols,
                        _ => 0,
                    };

                    self.emit_list_item_buffer_if_needed();
                    self.close_containers_to(list_level + 1);

                    for i in 0..bq_depth {
                        if let Some(info) = same_depth_marker_info.get(i) {
                            self.emit_or_buffer_blockquote_marker(
                                info.leading_spaces,
                                info.has_trailing_space,
                            );
                        }
                    }

                    let list_item = ListItemEmissionInput {
                        content: inner_content,
                        marker_len: marker_match.marker_len,
                        spaces_after_cols: marker_match.spaces_after_cols,
                        spaces_after_bytes: marker_match.spaces_after_bytes,
                        indent_cols: sibling_base_indent_cols,
                        indent_bytes: inner_indent_bytes,
                        virtual_marker_space: marker_match.virtual_marker_space,
                    };
                    let finish = lists::add_list_item(
                        &mut self.containers,
                        &mut self.builder,
                        &list_item,
                        self.config,
                    );
                    let extras = if let Some(extras) =
                        self.maybe_open_fenced_code_in_new_list_item()
                    {
                        extras
                    } else if let Some(extras) = self.maybe_open_caption_table_in_new_list_item() {
                        extras
                    } else if let Some(extras) =
                        self.maybe_open_table_with_trailing_caption_in_new_list_item()
                    {
                        extras
                    } else if let Some(extras) = self.maybe_open_line_block_in_new_list_item() {
                        extras
                    } else {
                        self.maybe_open_indented_code_in_new_list_item();
                        if let Some(extras) = self.maybe_open_definition_term_in_new_list_item() {
                            extras
                        } else {
                            self.dispatch_bq_after_list_item(finish)
                        }
                    };
                    return LineDispatch::consumed(1 + extras);
                }
            }

            if matches!(self.containers.last(), Some(Container::ListItem { .. })) {
                let (indent_cols, _) = leading_indent(inner_content);
                let content_indent = self.content_container_indent_to_strip();
                let effective_indent = indent_cols.saturating_sub(content_indent);
                let content_col = match self.containers.last() {
                    Some(Container::ListItem { content_col, .. }) => *content_col,
                    _ => 0,
                };

                let is_new_item_at_outer_level = if try_parse_list_marker(
                    inner_content,
                    self.config,
                    lists::open_list_hint_at_indent(
                        &self.containers,
                        leading_indent(inner_content).0,
                    ),
                )
                .is_some()
                {
                    effective_indent < content_col
                } else {
                    false
                };

                let innermost_band_marker = is_new_item_at_outer_level
                    && self.config.dialect == crate::options::Dialect::Pandoc
                    && self
                        .containers
                        .stack
                        .iter()
                        .rev()
                        .take_while(|c| {
                            !matches!(
                                c,
                                Container::BlockQuote { .. }
                                    | Container::FootnoteDefinition { .. }
                                    | Container::Definition { .. }
                                    | Container::Admonition { .. }
                            )
                        })
                        .filter_map(|c| match c {
                            Container::ListItem { content_col, .. } => Some(*content_col),
                            _ => None,
                        })
                        .nth(1)
                        .is_some_and(|outer_col| effective_indent >= outer_col);

                if innermost_band_marker {
                    log::trace!(
                        "Keeping ladder for innermost-band marker: effective_indent={}, content_col={}",
                        effective_indent,
                        content_col
                    );
                } else if is_new_item_at_outer_level
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

            if list_item_continuation
                && code_blocks::try_parse_fence_open(inner_content, self.config.dialect).is_some()
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
            let prev_flag = self.dispatch_list_marker_consumed;
            if used_shifted_bq && !self.innermost_li_above_bq() {
                self.dispatch_list_marker_consumed = true;
            }
            let dispatch = self.parse_inner_content(inner_content, line_to_append);
            self.dispatch_list_marker_consumed = prev_flag;
            return dispatch;
        }

        if current_bq_depth > 0 {
            if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                paragraphs::append_paragraph_line(
                    &mut self.containers,
                    &mut self.builder,
                    line,
                    self.config,
                );
                return LineDispatch::consumed(1);
            }

            if lists::in_blockquote_list(&self.containers)
                && let Some(marker_match) = try_parse_list_marker(
                    line,
                    self.config,
                    lists::open_list_hint_at_indent(&self.containers, leading_indent(line).0),
                )
            {
                let (indent_cols, indent_bytes) = leading_indent(line);
                if let Some(level) = lists::find_matching_list_level(
                    &self.containers,
                    &marker_match.marker,
                    indent_cols,
                    self.config.dialect,
                ) {
                    self.close_containers_to(level + 1);

                    if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
                        self.close_containers_to(self.containers.depth() - 1);
                    }
                    if matches!(self.containers.last(), Some(Container::ListItem { .. })) {
                        self.close_containers_to(self.containers.depth() - 1);
                    }

                    let extras = if let Some(nested_marker) = is_content_nested_bullet_marker(
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
                            self.config,
                        );
                        0
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
                        let finish = lists::add_list_item(
                            &mut self.containers,
                            &mut self.builder,
                            &list_item,
                            self.config,
                        );
                        self.dispatch_bq_after_list_item(finish)
                    };
                    return LineDispatch::consumed(1 + extras);
                }
            }
        }

        self.parse_inner_content(line, None)
    }

    fn close_dedented_admonitions(&mut self, content: &str) {
        if !self
            .containers
            .stack
            .iter()
            .any(|c| matches!(c, Container::Admonition { .. }))
        {
            return;
        }
        if self
            .containers
            .stack
            .iter()
            .any(|c| matches!(c, Container::ListItem { .. }))
        {
            return;
        }

        let (without_newline, _) = strip_newline(content);
        if without_newline.trim().is_empty() {
            return;
        }
        let (indent_cols, _) = leading_indent(without_newline);

        let mut acc = 0usize;
        let mut close_to: Option<usize> = None;
        for (idx, c) in self.containers.stack.iter().enumerate() {
            match c {
                Container::FootnoteDefinition { content_col, .. }
                | Container::Definition { content_col, .. } => {
                    acc += *content_col;
                }
                Container::Admonition { content_col } => {
                    acc += *content_col;
                    if indent_cols < acc {
                        close_to = Some(idx);
                        break;
                    }
                }
                _ => {}
            }
        }
        if let Some(idx) = close_to {
            self.close_containers_to(idx);
        }
    }

    /// Close nested definition-list levels that a dedented definition marker
    /// on the current line has left.
    ///
    /// A marker is a block start, so pandoc reads it in the frame of whichever
    /// body it lands in. `T\n:   a\n    : def\n  : sibling` puts `: sibling`
    /// back on `T`: column 2 does not reach the frame of the list nested in
    /// `T`'s body, which starts at the body's own content column. Plain text
    /// is *not* a block start and stays a lazy continuation of the innermost
    /// body, so only a marker line unwinds anything.
    ///
    /// Runs before the content-container strip, since the levels it closes are
    /// what that strip is measured from. A single, unnested definition list
    /// cannot be dedented out of this way — its own marker arm handles a
    /// sibling definition — so this needs two levels to do anything.
    fn content_container_indent_to_strip(&self) -> usize {
        self.containers.content_container_indent()
    }

    fn innermost_li_above_bq(&self) -> bool {
        for c in self.containers.stack.iter().rev() {
            match c {
                Container::ListItem { .. } => return true,
                Container::BlockQuote { .. } => return false,
                _ => continue,
            }
        }
        false
    }

    fn parse_inner_content(&mut self, content: &str, line_to_append: Option<&str>) -> LineDispatch {
        log::trace!(
            "parse_inner_content [{}]: depth={}, last={:?}, content={:?}",
            self.pos,
            self.containers.depth(),
            self.containers.last(),
            content.trim_end()
        );
        self.close_dedented_admonitions(content);

        self.close_dedented_definition_lists(content);

        let content_indent = self.content_container_indent_to_strip();
        let (stripped_content, indent_to_emit) = strip_content_indent(content, content_indent);

        let paragraph_held = {
            let append_line = line_to_append.unwrap_or(self.lines[self.pos]);
            if content_indent > 0 && resolve_content_indent(content, content_indent).reaches_frame()
            {
                gobbled_indent_prefix_len(append_line, content_indent)
            } else {
                0
            }
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
            return LineDispatch::consumed(1);
        }

        let body_block =
            self.definition_marker_over_open_body_block(content, stripped_content, content_indent);
        if body_block == Some(BufferedBodyBlock::Term) {
            self.promote_buffered_definition_term();
        }
        let definition_block_breaks = body_block == Some(BufferedBodyBlock::Block);
        if definition_block_breaks {
            self.emit_buffered_plain_if_needed();
        }

        if let Some(Container::Definition {
            plain_open: definition_plain_open,
            ..
        }) = self.containers.last()
        {
            let definition_plain_open = *definition_plain_open;
            let is_definition_marker =
                definition_lists::try_parse_definition_marker(stripped_content).is_some()
                    && !stripped_content.starts_with(':');
            if content_indent == 0 && is_definition_marker {
            } else {
                let policy = ContinuationPolicy::new(self.config, self.block_registry);
                let prev_line_blank = self.pos > 0 && {
                    let prev_line = self.lines[self.pos - 1];
                    let (prev_bq_depth, prev_inner) = count_blockquote_markers(prev_line);
                    is_blank_line(prev_line) || (prev_bq_depth > 0 && is_blank_line(prev_inner))
                };

                if definition_block_breaks
                    || policy.definition_plain_can_continue(
                        stripped_content,
                        content,
                        content_indent,
                        &BlockContext {
                            has_blank_before: self.pos == 0 || prev_line_blank,
                            has_blank_before_strict: self.pos == 0 || prev_line_blank,
                            at_document_start: self.pos == 0
                                && self.current_blockquote_depth() == 0,
                            in_fenced_div: self.in_fenced_div(),
                            fenced_div_open_indent: self.innermost_fenced_div_open_indent(),
                            fenced_div_wraps_list: self.fenced_div_wraps_innermost_list(),
                            myst_directive_closer: self.innermost_myst_directive_closer(),
                            blockquote_depth: self.current_blockquote_depth(),
                            config: self.config,
                            diags: self.diagnostics.clone(),
                            content_indent,
                            indent_to_emit: None,
                            list_indent_info: None,
                            in_list: lists::in_list(&self.containers),
                            in_definition_list: definition_lists::in_definition_list(
                                &self.containers,
                            ),
                            in_marker_only_list_item: matches!(
                                self.containers.last(),
                                Some(Container::ListItem {
                                    marker_only: true,
                                    ..
                                })
                            ),
                            list_item_unclosed_html_block_tag: self
                                .list_item_unclosed_html_block_tag(),
                            open_code_span_openers: self.open_code_span_openers(),
                            paragraph_open: self.is_paragraph_open(),
                            list_item_content_open: self.is_list_item_content_open(),
                            next_line: if self.pos + 1 < self.lines.len() {
                                Some(self.lines[self.pos + 1])
                            } else {
                                None
                            },
                            open_alpha_hint: lists::open_list_hint_at_indent(
                                &self.containers,
                                leading_indent(stripped_content).0,
                            ),
                            restricted_ordered_sublist: self
                                .restricted_ordered_sublist(stripped_content),
                        },
                        &self.lines,
                        self.pos,
                        definition_plain_open,
                    )
                {
                    let content_line = stripped_content;
                    let (text_without_newline, newline_str) = strip_newline(content_line);
                    let indent_prefix = if !text_without_newline.trim().is_empty() {
                        indent_to_emit.unwrap_or("")
                    } else {
                        ""
                    };

                    let reaches_content_col = content_indent > 0
                        && resolve_content_indent(content, content_indent).reaches_frame();
                    let held = if reaches_content_col {
                        gobbled_indent_prefix_len(indent_prefix, content_indent)
                    } else {
                        0
                    };

                    if let Some(Container::Definition {
                        plain_open,
                        plain_buffer,
                        ..
                    }) = self.containers.stack.last_mut()
                    {
                        plain_buffer.push_indent(&indent_prefix[..held]);
                        plain_buffer.push_text(&indent_prefix[held..]);
                        plain_buffer.push_text(text_without_newline);
                        plain_buffer.push_text(newline_str);
                        *plain_open = true;
                    }

                    return LineDispatch::consumed(1);
                }
            }
        }

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
                    && !self.at_note_body_start
                    && !blockquotes::can_start_blockquote(
                        self.pos,
                        &self.lines,
                        self.config.extensions.fenced_divs,
                    )
                {
                } else {
                    self.emit_buffered_plain_if_needed();
                    self.emit_list_item_buffer_if_needed();

                    self.close_paragraph_if_open();

                    if bq_depth < current_bq_depth {
                        self.close_blockquotes_to_depth(bq_depth);
                    } else {
                        let marker_info = parse_blockquote_marker_info(stripped_content);

                        if bq_depth > current_bq_depth {
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
                        } else {
                            self.emit_blockquote_markers(&marker_info, bq_depth);
                        }
                    }

                    return self.parse_inner_content(inner_content, Some(inner_content));
                }
            }
        }

        if content_indent > 0
            && self.config.dialect == crate::options::Dialect::Pandoc
            && self.current_blockquote_depth() == 0
            && matches!(
                self.containers.last(),
                Some(
                    Container::Definition { .. }
                        | Container::FootnoteDefinition { .. }
                        | Container::Admonition { .. }
                )
            )
            && let Some(consumed) = self.try_dispatch_content_indent_html_block(
                stripped_content,
                content_indent,
                indent_to_emit,
            )
        {
            return LineDispatch::consumed(consumed);
        }

        if content_indent > 0
            && self.config.dialect == crate::options::Dialect::Pandoc
            && self.current_blockquote_depth() > 0
            && matches!(
                self.containers.last(),
                Some(
                    Container::Definition { .. }
                        | Container::FootnoteDefinition { .. }
                        | Container::Admonition { .. }
                )
            )
            && let Some(consumed) = self.try_dispatch_bq_content_indent_html_block(
                stripped_content,
                content_indent,
                indent_to_emit,
            )
        {
            return LineDispatch::consumed(consumed);
        }

        let content = stripped_content;

        if self.is_paragraph_open()
            && (paragraphs::has_open_inline_math_environment(&self.containers)
                || paragraphs::has_open_display_math(&self.containers))
        {
            paragraphs::append_paragraph_line_gobbling(
                &mut self.containers,
                &mut self.builder,
                line_to_append.unwrap_or(self.lines[self.pos]),
                paragraph_held,
                self.config,
            );
            return LineDispatch::consumed(1);
        }

        if matches!(
            self.containers.last(),
            Some(Container::ListItem { buffer, .. }) if buffer.has_open_display_math()
        ) {
            use super::blocks::lists;
            let is_sibling_marker = try_parse_list_marker(
                content,
                self.config,
                lists::open_list_hint_at_indent(&self.containers, leading_indent(content).0),
            )
            .is_some_and(|marker_match| {
                lists::find_matching_list_level(
                    &self.containers,
                    &marker_match.marker,
                    leading_indent(content).0,
                    self.config.dialect,
                )
                .is_some()
            });
            if !is_sibling_marker {
                let line = line_to_append.unwrap_or(self.lines[self.pos]);
                if let Some(Container::ListItem {
                    buffer,
                    marker_only,
                    ..
                }) = self.containers.stack.last_mut()
                {
                    buffer.push_text(line, self.config);
                    if !is_blank_line(line) {
                        *marker_only = false;
                    }
                }
                return LineDispatch::consumed(1);
            }
        }

        if self.partial_separator_continues_item_table(content) {
            let line = line_to_append.unwrap_or(self.lines[self.pos]);
            if let Some(Container::ListItem {
                buffer,
                marker_only,
                ..
            }) = self.containers.stack.last_mut()
            {
                buffer.push_text(line, self.config);
                *marker_only = false;
            }
            return LineDispatch::consumed(1);
        }

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
            has_blank_before: false,        // filled in later
            has_blank_before_strict: false, // filled in later
            at_document_start: false,       // filled in later
            in_fenced_div: self.in_fenced_div(),
            fenced_div_open_indent: self.innermost_fenced_div_open_indent(),
            fenced_div_wraps_list: self.fenced_div_wraps_innermost_list(),
            myst_directive_closer: self.innermost_myst_directive_closer(),
            blockquote_depth: current_bq_depth,
            config: self.config,
            diags: self.diagnostics.clone(),
            content_indent,
            indent_to_emit,
            list_indent_info,
            in_list: lists::in_list(&self.containers),
            in_definition_list: definition_lists::in_definition_list(&self.containers),
            in_marker_only_list_item: matches!(
                self.containers.last(),
                Some(Container::ListItem {
                    marker_only: true,
                    ..
                })
            ),
            list_item_unclosed_html_block_tag: self.list_item_unclosed_html_block_tag(),
            open_code_span_openers: self.open_code_span_openers(),
            paragraph_open: self.is_paragraph_open(),
            list_item_content_open: self.is_list_item_content_open(),
            next_line,
            open_alpha_hint: lists::open_list_hint_at_indent(
                &self.containers,
                leading_indent(content).0,
            ),
            restricted_ordered_sublist: self.restricted_ordered_sublist(content),
        };

        let mut dispatcher_ctx = dispatcher_ctx;

        let dispatcher_prefix = ContainerPrefix::from_stack(
            &self.containers.stack,
            self.dispatch_list_marker_consumed,
            self.config,
        );

        if let Some(dispatch) = self.try_fold_list_item_buffer_into_setext(stripped_content) {
            return dispatch;
        }

        let dispatcher_match = {
            let stripped = StrippedLines::new(&self.lines, self.pos, &dispatcher_prefix);
            self.block_registry
                .detect_prepared(&dispatcher_ctx, &stripped)
        };

        let after_metadata_block = std::mem::replace(&mut self.after_metadata_block, false);
        let at_note_body_start = std::mem::replace(&mut self.at_note_body_start, false);
        let has_blank_before = if self.pos == 0 || after_metadata_block || at_note_body_start {
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

            let prev_line_blank = is_blank_line(prev_line)
                || (current_bq_depth > 0 && prev_bq_depth > 0 && is_blank_line(prev_inner));
            prev_line_blank
                || prev_is_fenced_div_open
                || matches!(self.containers.last(), Some(Container::BlockQuote { .. }))
                || !self.previous_block_requires_blank_before_heading()
        };

        let at_line_zero = self.pos == 0 && current_bq_depth == 0;
        let at_document_start = self.origin_allows_document_start() && at_line_zero;

        let prev_line_blank = if self.pos > 0 {
            let prev_line = self.lines[self.pos - 1];
            let (prev_bq_depth, prev_inner) = count_blockquote_markers(prev_line);
            is_blank_line(prev_line) || (prev_bq_depth > 0 && is_blank_line(prev_inner))
        } else {
            false
        };
        let has_blank_before_strict = at_line_zero || prev_line_blank || at_note_body_start;

        dispatcher_ctx.has_blank_before = has_blank_before;
        dispatcher_ctx.has_blank_before_strict = has_blank_before_strict;
        dispatcher_ctx.at_document_start = at_document_start;

        let dispatcher_match =
            if dispatcher_ctx.has_blank_before || dispatcher_ctx.at_document_start {
                let stripped = StrippedLines::new(&self.lines, self.pos, &dispatcher_prefix);
                self.block_registry
                    .detect_prepared(&dispatcher_ctx, &stripped)
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
                paragraphs::append_paragraph_line_gobbling(
                    &mut self.containers,
                    &mut self.builder,
                    line_to_append.unwrap_or(self.lines[self.pos]),
                    paragraph_held,
                    self.config,
                );
                return LineDispatch::consumed(1);
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

                if matches!(block_match.effect, BlockEffect::CloseMystDirective) {
                    self.close_containers_to_myst_directive();
                }

                if matches!(block_match.effect, BlockEffect::OpenFootnoteDefinition) {
                    self.close_open_footnote_definition();
                }

                let lines_consumed = {
                    let stripped = StrippedLines::new(&self.lines, self.pos, &dispatcher_prefix);
                    self.block_registry.parse_prepared(
                        block_match,
                        &dispatcher_ctx,
                        &mut self.builder,
                        &stripped,
                    )
                };

                if matches!(
                    self.block_registry.parser_name(block_match),
                    "yaml_metadata" | "pandoc_title_block" | "mmd_title_block"
                ) {
                    self.after_metadata_block = true;
                }

                let extras = match block_match.effect {
                    BlockEffect::None => 0,
                    BlockEffect::OpenFencedDiv => {
                        self.push_fenced_div_container(block_match);
                        0
                    }
                    BlockEffect::CloseFencedDiv => {
                        self.close_fenced_div();
                        0
                    }
                    BlockEffect::OpenMystDirective => {
                        self.push_myst_directive_container(block_match);
                        0
                    }
                    BlockEffect::CloseMystDirective => {
                        self.close_myst_directive();
                        0
                    }
                    BlockEffect::OpenAdmonition => {
                        self.containers
                            .push(Container::Admonition { content_col: 4 });
                        0
                    }
                    BlockEffect::OpenFootnoteDefinition => {
                        self.handle_footnote_open_effect(block_match, content)
                    }
                    BlockEffect::OpenList => {
                        self.handle_list_open_effect(block_match, content, indent_to_emit)
                    }
                    BlockEffect::OpenDefinitionList => {
                        self.handle_definition_list_effect(block_match, content, indent_to_emit)
                    }
                    BlockEffect::OpenBlockQuote => 0,
                };

                if lines_consumed == 0 {
                    log::warn!(
                        "block parser made no progress at line {} (parser={})",
                        self.pos + 1,
                        self.block_registry.parser_name(block_match)
                    );
                    return LineDispatch::Rejected;
                }

                return LineDispatch::consumed(lines_consumed + extras);
            }
        } else if let Some(block_match) = dispatcher_match.as_ref() {
            let parser_name = self.block_registry.parser_name(block_match);
            match block_match.detection {
                BlockDetectionResult::YesCanInterrupt => {
                    if matches!(block_match.effect, BlockEffect::OpenFencedDiv)
                        && self.is_paragraph_open()
                    {
                        if !self.is_paragraph_open() {
                            paragraphs::start_paragraph_if_needed(
                                &mut self.containers,
                                &mut self.builder,
                            );
                        }
                        paragraphs::append_paragraph_line_gobbling(
                            &mut self.containers,
                            &mut self.builder,
                            line_to_append.unwrap_or(self.lines[self.pos]),
                            paragraph_held,
                            self.config,
                        );
                        return LineDispatch::consumed(1);
                    }

                    if matches!(block_match.effect, BlockEffect::OpenList)
                        && self.is_paragraph_open()
                        && !lists::in_list(&self.containers)
                        && (self.content_container_indent_to_strip() == 0
                            || self.in_footnote_definition())
                    {
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
                            paragraphs::append_paragraph_line_gobbling(
                                &mut self.containers,
                                &mut self.builder,
                                line_to_append.unwrap_or(self.lines[self.pos]),
                                paragraph_held,
                                self.config,
                            );
                            return LineDispatch::consumed(1);
                        }
                    }

                    if matches!(block_match.effect, BlockEffect::OpenList)
                        && self.try_lazy_list_continuation(block_match, content)
                    {
                        return LineDispatch::consumed(1);
                    }

                    if matches!(block_match.effect, BlockEffect::OpenList)
                        && self.try_buffer_marker_line_table_delimiter(content)
                    {
                        return LineDispatch::consumed(1);
                    }

                    self.emit_list_item_buffer_if_needed();
                    if self.is_paragraph_open() {
                        if self.html_block_demotes_paragraph_to_plain(block_match) {
                            self.close_paragraph_as_plain_if_open();
                        } else {
                            self.close_containers_to(self.containers.depth() - 1);
                        }
                    }

                    let closes_list = if self.config.dialect == crate::options::Dialect::CommonMark
                    {
                        !matches!(block_match.effect, BlockEffect::OpenList)
                    } else {
                        parser_name == "fenced_code_block"
                    };
                    if closes_list {
                        let (indent_cols, _) = leading_indent(content);
                        self.close_lists_above_indent(indent_cols);
                    }
                }
                BlockDetectionResult::Yes => {
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
                        return LineDispatch::consumed(2);
                    }

                    if parser_name == "fenced_div_open"
                        && (self.is_paragraph_open() || self.is_list_item_content_open())
                    {
                        self.append_lazy_continuation_line(
                            line_to_append.unwrap_or(self.lines[self.pos]),
                        );
                        return LineDispatch::consumed(1);
                    }

                    if parser_name == "reference_definition"
                        && (self.is_paragraph_open() || self.is_list_item_content_open())
                    {
                        self.append_lazy_continuation_line(
                            line_to_append.unwrap_or(self.lines[self.pos]),
                        );
                        return LineDispatch::consumed(1);
                    }

                    debug_assert!(
                        !self.is_paragraph_open() && !self.is_list_item_content_open(),
                        "block parser `{parser_name}` returned `Yes` while a paragraph or \
                         buffered list-item content is open; this reorders bytes — gate the \
                         detection or return `YesCanInterrupt`"
                    );
                }
                BlockDetectionResult::No => unreachable!(),
            }

            if !matches!(block_match.detection, BlockDetectionResult::No) {
                if matches!(block_match.effect, BlockEffect::CloseFencedDiv) {
                    self.close_containers_to_fenced_div();
                }

                if matches!(block_match.effect, BlockEffect::CloseMystDirective) {
                    self.close_containers_to_myst_directive();
                }

                if matches!(block_match.effect, BlockEffect::OpenFootnoteDefinition) {
                    self.close_open_footnote_definition();
                }

                let lines_consumed = {
                    let stripped = StrippedLines::new(&self.lines, self.pos, &dispatcher_prefix);
                    self.block_registry.parse_prepared(
                        block_match,
                        &dispatcher_ctx,
                        &mut self.builder,
                        &stripped,
                    )
                };

                let extras = match block_match.effect {
                    BlockEffect::None => 0,
                    BlockEffect::OpenFencedDiv => {
                        self.push_fenced_div_container(block_match);
                        0
                    }
                    BlockEffect::CloseFencedDiv => {
                        self.close_fenced_div();
                        0
                    }
                    BlockEffect::OpenMystDirective => {
                        self.push_myst_directive_container(block_match);
                        0
                    }
                    BlockEffect::CloseMystDirective => {
                        self.close_myst_directive();
                        0
                    }
                    BlockEffect::OpenAdmonition => {
                        self.containers
                            .push(Container::Admonition { content_col: 4 });
                        0
                    }
                    BlockEffect::OpenFootnoteDefinition => {
                        self.handle_footnote_open_effect(block_match, content)
                    }
                    BlockEffect::OpenList => {
                        self.handle_list_open_effect(block_match, content, indent_to_emit)
                    }
                    BlockEffect::OpenDefinitionList => {
                        self.handle_definition_list_effect(block_match, content, indent_to_emit)
                    }
                    BlockEffect::OpenBlockQuote => 0,
                };

                if lines_consumed == 0 {
                    log::warn!(
                        "block parser made no progress at line {} (parser={})",
                        self.pos + 1,
                        self.block_registry.parser_name(block_match)
                    );
                    return LineDispatch::Rejected;
                }

                return LineDispatch::consumed(lines_consumed + extras);
            }
        }

        if self.config.extensions.line_blocks
            && (has_blank_before || self.pos == 0)
            && try_parse_line_block_start(content).is_some()
            && try_parse_line_block_start(self.lines[self.pos]).is_some()
        {
            log::trace!("Parsed line block at line {}", self.pos);
            self.close_paragraph_if_open();

            let prefix = ContainerPrefix::default();
            let window = StrippedLines::new(&self.lines, self.pos, &prefix);
            let new_pos = parse_line_block(&window, &mut self.builder, self.config);
            if new_pos > self.pos {
                return LineDispatch::consumed(new_pos - self.pos);
            }
        }

        if self.definition_marker_breaks_open_list_item_block(content) {
            self.emit_list_item_buffer_if_needed();
            self.close_lists_above_indent(leading_indent(content).0);
        }

        if matches!(self.containers.last(), Some(Container::ListItem { .. })) {
            log::trace!(
                "Inside ListItem - buffering content: {:?}",
                line_to_append.unwrap_or(self.lines[self.pos]).trim_end()
            );
            let line = line_to_append.unwrap_or(self.lines[self.pos]);

            if let Some(Container::ListItem {
                buffer,
                marker_only,
                ..
            }) = self.containers.stack.last_mut()
            {
                buffer.push_text(line, self.config);
                if !is_blank_line(line) {
                    *marker_only = false;
                }
            }

            return LineDispatch::consumed(1);
        }

        log::trace!(
            "Not in ListItem - creating paragraph for: {:?}",
            line_to_append.unwrap_or(self.lines[self.pos]).trim_end()
        );
        paragraphs::start_paragraph_if_needed(&mut self.containers, &mut self.builder);
        let line = line_to_append.unwrap_or(self.lines[self.pos]);
        paragraphs::append_paragraph_line_gobbling(
            &mut self.containers,
            &mut self.builder,
            line,
            paragraph_held,
            self.config,
        );
        LineDispatch::consumed(1)
    }

    fn fenced_div_container_index(&self) -> Option<usize> {
        self.containers
            .stack
            .iter()
            .rposition(|c| matches!(c, Container::FencedDiv { .. }))
    }

    /// True when the innermost open fenced div wraps the innermost open list
    /// (the div is outer to the list on the container stack). In that case a
    /// `:::` indented to the list's content column is list content, not the
    /// div's closer — pandoc only closes a div on a fence at the div's own
    /// indentation. When instead the innermost div was opened *inside* a list
    /// item (as a continuation block, so it is inner to the list), a fence at
    /// the content column does close it. See issue #439.
    fn fenced_div_wraps_innermost_list(&self) -> bool {
        let Some(div_idx) = self.fenced_div_container_index() else {
            return false;
        };
        match self
            .containers
            .stack
            .iter()
            .rposition(|c| matches!(c, Container::List { .. }))
        {
            Some(list_idx) => div_idx < list_idx,
            None => false,
        }
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

    fn push_fenced_div_container(&mut self, block_match: &PreparedBlockMatch) {
        use crate::parser::blocks::fenced_divs::DivFenceInfo;
        let open_indent_cols = block_match
            .payload
            .as_ref()
            .and_then(|p| p.downcast_ref::<DivFenceInfo>())
            .map(|info| info.open_indent_cols)
            .unwrap_or(0);
        self.containers
            .push(Container::FencedDiv { open_indent_cols });
    }

    fn innermost_fenced_div_open_indent(&self) -> Option<usize> {
        self.containers.stack.iter().rev().find_map(|c| match c {
            Container::FencedDiv { open_indent_cols } => Some(*open_indent_cols),
            _ => None,
        })
    }

    fn myst_directive_container_index(&self) -> Option<usize> {
        self.containers
            .stack
            .iter()
            .rposition(|c| matches!(c, Container::MystDirective { .. }))
    }

    fn close_containers_to_myst_directive(&mut self) {
        if let Some(index) = self.myst_directive_container_index() {
            self.close_containers_to(index + 1);
        }
    }

    fn close_myst_directive(&mut self) {
        if let Some(index) = self.myst_directive_container_index() {
            self.close_containers_to(index);
        }
    }

    fn push_myst_directive_container(&mut self, block_match: &PreparedBlockMatch) {
        use crate::parser::blocks::myst_directives::DirectiveOpen;
        let open = block_match
            .payload
            .as_ref()
            .and_then(|p| p.downcast_ref::<DirectiveOpen>());
        if open.is_some_and(|open| open.is_verbatim) {
            return;
        }
        let (fence_char, fence_count) = open
            .map(|open| (open.fence_char, open.fence_count))
            .unwrap_or((b'`', 3));
        self.containers.push(Container::MystDirective {
            fence_char,
            fence_count,
        });
    }

    fn innermost_myst_directive_closer(&self) -> Option<(u8, usize)> {
        self.containers.stack.iter().rev().find_map(|c| match c {
            Container::MystDirective {
                fence_char,
                fence_count,
            } => Some((*fence_char, *fence_count)),
            _ => None,
        })
    }

    /// Whether the active container stack has any `FootnoteDefinition`
    /// ancestor. Used to drive `suppress_footnote_refs` when flushing
    /// buffered inline content: pandoc silently drops nested `[^id]`
    /// references inside a reference-style footnote definition body, and
    /// the suppression cascades through every container nested under it
    /// (blockquotes, lists, bracketed spans, emphasis, inline footnotes,
    /// etc.).
    fn in_footnote_definition(&self) -> bool {
        self.containers
            .stack
            .iter()
            .any(|c| matches!(c, Container::FootnoteDefinition { .. }))
    }
}
