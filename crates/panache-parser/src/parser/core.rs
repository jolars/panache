use crate::options::ParserOptions;
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::GreenNodeBuilder;

use super::block_dispatcher::{
    BlockContext, BlockDetectionResult, BlockEffect, BlockParserRegistry, BlockQuotePrepared,
    PreparedBlockMatch,
};
use super::blocks::blockquotes;
use super::blocks::code_blocks;
use super::blocks::container_prefix::{ContainerPrefix, StrippedLines, strip_content_indent};
use super::blocks::definition_lists;
use super::blocks::fenced_divs;
use super::blocks::figures::paragraph_is_standalone_image;
use super::blocks::headings::{
    emit_atx_heading, emit_setext_heading, emit_setext_heading_body, try_parse_atx_heading,
    try_parse_setext_heading,
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

const GITHUB_ALERT_MARKERS: [&str; 5] = [
    "[!TIP]",
    "[!WARNING]",
    "[!IMPORTANT]",
    "[!CAUTION]",
    "[!NOTE]",
];

/// Outcome of dispatching a line through `parse_line` / `parse_inner_content`
/// and friends. The outer loop in `parse_document_stack` is the only authority
/// that commits `self.pos`; dispatch helpers describe what they consumed
/// rather than side-effecting the position themselves.
#[must_use]
#[derive(Debug, Clone, Copy)]
pub(crate) enum LineDispatch {
    /// A parser claimed the line and consumed `n` lines (`n >= 1`).
    Consumed(usize),
    /// No parser claimed the line; the outer loop should advance by 1.
    Rejected,
}

impl LineDispatch {
    /// Construct a `Consumed(n)` with a debug assertion that `n >= 1`. Use
    /// `Rejected` for zero-consumption rejections so the caller can advance by
    /// a default of 1 line rather than spinning.
    #[inline]
    pub(crate) fn consumed(n: usize) -> Self {
        debug_assert!(n >= 1, "LineDispatch::Consumed requires n >= 1");
        LineDispatch::Consumed(n)
    }
}

/// The shape of the block a definition marker stands over inside the body of
/// the definition it sits in, as
/// [`Parser::definition_marker_over_open_body_block`] reads it.
///
/// Pandoc re-reads the body as its own block sequence, so the buffered block
/// above the marker decides what the marker can be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BufferedBodyBlock {
    /// A one-line block, which is exactly the shape of a term: the marker
    /// below it defines that line, in a definition list nested in the body.
    /// `T\n:   a\n    : def` is
    /// `DefinitionList [(T, [[DefinitionList [(a, [[Plain "def"]])]]])]`.
    Term,
    /// No term precedes the marker — the block above it is longer than a line,
    /// already closed by a blank, or not there at all — so what is left is
    /// another block of the same body: `T\n:   a\n    b\n    : def` is
    /// `DefinitionList [(T, [[Plain "a b", Plain ": def"]])]`.
    Block,
}

/// The line being folded back into an open blockquote by pandoc's laziness
/// rule, plus the marker bookkeeping `marker_info_for_line` needs to re-emit
/// the `>` run this line does carry. Grouped because the fold is one step of
/// `handle_blockquote_line` and would otherwise take seven positional
/// arguments.
struct LazyFold<'l> {
    /// The raw source line, markers and all.
    line: &'l str,
    /// `line` with `bq_depth` markers stripped.
    inner_content: &'l str,
    /// How many `>` markers the line carries (below the open depth).
    bq_depth: usize,
    bq_marker_line: &'l str,
    shifted_bq_prefix: &'l str,
    used_shifted_bq: bool,
}

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
    /// True while `dispatch_bq_after_list_item` is routing the post-marker
    /// content of a `- > <block>` shape through `parse_inner_content`. In
    /// that path the LIST_MARKER + WHITESPACE bytes for `lines[self.pos]`
    /// have just been emitted upstream by `add_list_item`, so the helper
    /// must skip them when computing the dispatch line's inner content.
    /// Toggled false outside that helper — most dispatch paths fire on
    /// continuation lines where the list-indent bytes are inner content,
    /// not upstream-emitted prefix. Threaded into `BlockContext` via
    /// `list_marker_consumed_on_line_0`.
    dispatch_list_marker_consumed: bool,
    /// Syntax errors found in embedded sublanguages (malformed frontmatter /
    /// hashpipe YAML). Threaded to the validation sites via `BlockContext`;
    /// drained by [`Parser::parse_with_errors`]. Empty for pure Markdown.
    diagnostics: Diagnostics,
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
            dispatch_list_marker_consumed: false,
            diagnostics: Diagnostics::new(),
        }
    }

    pub fn parse(self) -> SyntaxNode {
        self.parse_with_errors().0
    }

    /// Parse, returning the CST plus any embedded-sublanguage syntax errors
    /// (host-ranged) collected during the single pass.
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
                    // Snapshot the gobble chain while the item is still on the
                    // stack — the pop below drops the innermost level.
                    let gobble = self.containers.gobble_chain();

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

                    let suppress_footnote_refs = self.in_footnote_definition();
                    // Pop container first
                    self.containers.stack.pop();
                    // Emit buffered content as Plain or PARAGRAPH. This is
                    // the item-close site: the buffer is the item's complete
                    // remaining content, and the close-form dispatcher gate
                    // (`list_item_unclosed_html_block_tag`) has already
                    // accumulated any matched `</div>` into it. So a buffer
                    // ending on a lone `<div>` open is genuinely unclosed
                    // (pandoc closes it implicitly at the item boundary) —
                    // opt into the unclosed-`<div>` lift.
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
                    let suppress_footnote_refs = self.in_footnote_definition();
                    // Pandoc's `implicit_figures` promotes a paragraph whose
                    // entire content is one image to a `Figure`. The image
                    // must be alone in the paragraph, so this can only be
                    // decided here, at close, once the buffer is complete.
                    let wrapper = if paragraph_is_standalone_image(
                        &buffer_clone.get_text_for_parsing(),
                        self.config,
                    ) {
                        SyntaxKind::FIGURE
                    } else {
                        SyntaxKind::PARAGRAPH
                    };
                    // Pop container first
                    self.containers.stack.pop();
                    // Retroactively wrap and emit buffered content
                    self.builder.start_node_at(checkpoint, wrapper.into());
                    buffer_clone.emit_with_inlines(
                        &mut self.builder,
                        self.config,
                        suppress_footnote_refs,
                    );
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
                    let buffer = plain_buffer.clone();
                    let suppress_footnote_refs = self.in_footnote_definition();
                    emit_definition_plain_or_heading(
                        &mut self.builder,
                        &buffer,
                        self.config,
                        suppress_footnote_refs,
                    );

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
            let buffer = plain_buffer.clone();
            let suppress_footnote_refs = self.in_footnote_definition();
            emit_definition_plain_or_heading(
                &mut self.builder,
                &buffer,
                self.config,
                suppress_footnote_refs,
            );
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
            let gobble = self.containers.gobble_chain();
            let use_paragraph = buffer_clone.has_blank_lines_between_content();
            let suppress_footnote_refs = self.in_footnote_definition();
            // Mid-item partial flush before an interrupting block. The buffer
            // is a partial chunk, so a trailing `<div>` open may still be
            // closed by a `</div>` in a later chunk — do NOT lift it as an
            // unclosed div here (would strand the close as a sibling
            // RawBlock).
            buffer_clone.emit_as_block(
                &mut self.builder,
                use_paragraph,
                self.config,
                &gobble,
                suppress_footnote_refs,
                false,
            );
        }
    }

    /// CommonMark §5.2: when a list item's first line (after the marker) is a
    /// fenced code block opener, the content of the item *is* the code block —
    /// not buffered text. The list-item open path normally pushes the
    /// post-marker text into the item's buffer; this helper detects an opening
    /// fence in that buffered first line and converts it into a CODE_BLOCK
    /// When `add_list_item` opens an inner BLOCK_QUOTE on the same line as
    /// the list marker (`- > <content>`), it returns the post-`> ` content
    /// instead of stuffing it into a paragraph; we re-dispatch that content
    /// through the block parser so block-level constructs (HTML blocks,
    /// ATX headings, fenced code, …) on the first line of a bq-in-listitem
    /// are recognized properly.
    ///
    /// Returns the number of *extra* lines consumed beyond the list-marker
    /// line itself. The caller already accounts for the marker line in its
    /// `LineDispatch::Consumed(1 + extras)`; if `result` is `Done`, this
    /// returns 0.
    fn dispatch_bq_after_list_item(
        &mut self,
        result: super::blocks::lists::ListItemFinish,
    ) -> usize {
        let super::blocks::lists::ListItemFinish::BqDispatch { content } = result else {
            return 0;
        };
        let pos_before = self.pos;
        // Tell parse_inner_content that the LIST_MARKER + WHITESPACE bytes
        // for `lines[self.pos]`'s first list-content-col columns have
        // already been emitted upstream by `add_list_item`, so any
        // emission helper that walks raw `lines[..]` must skip them.
        self.dispatch_list_marker_consumed = true;
        let dispatch = self.parse_inner_content(&content, Some(&content));
        self.dispatch_list_marker_consumed = false;
        self.pos = pos_before;
        match dispatch {
            LineDispatch::Consumed(n) => n.saturating_sub(1),
            LineDispatch::Rejected => 0,
        }
    }

    /// inside the LIST_ITEM, consuming subsequent lines until the closing
    /// fence (or end of document under CommonMark dialect, per §4.5).
    ///
    /// Pandoc-markdown also reaches this path: a bare fence still requires a
    /// matching closer to register as a code block, matching
    /// `FencedCodeBlockParser::detect_prepared` (`bare_fence_in_list_with_closer`).
    /// Returns `Some(extras)` when a fence-open is recognized on the buffered
    /// first-line content and the fenced code block was emitted (`extras` is
    /// the number of source lines consumed beyond the list-marker line).
    /// `None` means the helper did not fire and the caller proceeds normally.
    fn maybe_open_fenced_code_in_new_list_item(&mut self) -> Option<usize> {
        let Some(Container::ListItem {
            content_col,
            buffer,
            ..
        }) = self.containers.stack.last()
        else {
            return None;
        };
        let content_col = *content_col;
        let text = buffer.first_text()?;
        if buffer.segment_count() != 1 {
            return None;
        }
        let text_owned = text.to_string();
        let fence = code_blocks::try_parse_fence_open(&text_owned, self.config.dialect)?;
        let common_mark_dialect = self.config.dialect == crate::options::Dialect::CommonMark;
        let has_info = !fence.info_string.trim().is_empty();
        let bq_depth = self.current_blockquote_depth();
        let has_matching_closer = self.has_matching_fence_closer(&fence, bq_depth, content_col);
        if !(has_info || has_matching_closer || common_mark_dialect) {
            return None;
        }
        // Gate fences by extension flags, mirroring the dispatcher.
        if (fence.fence_char == '`' && !self.config.extensions.backtick_code_blocks)
            || (fence.fence_char == '~' && !self.config.extensions.fenced_code_blocks)
        {
            return None;
        }
        if let Some(Container::ListItem { buffer, .. }) = self.containers.stack.last_mut() {
            buffer.clear();
        }
        // Marker-line dispatch: the list marker + indent were emitted
        // upstream (`list_marker_consumed_on_line_0 = true`); blockquotes,
        // if any, are outer of the list.
        let prefix = ContainerPrefix::from_scalars(
            bq_depth,
            content_col,
            bq_depth > 0,
            0,
            true,
            self.config.dialect,
        );
        let window = StrippedLines::new(&self.lines, self.pos, &prefix);
        let new_pos = code_blocks::parse_fenced_code_block(
            &mut self.builder,
            &window,
            fence,
            Some(&text_owned),
            &self.diagnostics,
            self.config.flavor,
        );
        Some(new_pos.saturating_sub(self.pos).saturating_sub(1))
    }

    /// When a new list item's marker line opens a line block (`- | a`), emit
    /// the line block as the item's content instead of buffering the line as
    /// text.
    ///
    /// Pandoc parses a list item's content as a fresh block sequence, so
    /// `lineBlock` sees `| a` at the item's content column and claims it:
    /// `- | a\n  | b` is `BulletList [[LineBlock [[a], [b]]]]`. The
    /// dispatcher's `LineBlockParser` never gets the marker line — the list
    /// parser consumes it first and buffers the post-marker text — so the
    /// item read as a `PLAIN` of two literal `|` lines. Bridge that gap here,
    /// mirroring [`Self::maybe_open_fenced_code_in_new_list_item`].
    ///
    /// Runs *after* the marker-line table helpers: `- | a | b |` is a pipe
    /// table row, and a table start also satisfies `try_parse_line_block_start`.
    ///
    /// Returns the number of source lines consumed beyond the marker line.
    fn maybe_open_line_block_in_new_list_item(&mut self) -> Option<usize> {
        if !self.config.extensions.line_blocks {
            return None;
        }
        let Some(Container::ListItem {
            content_col,
            buffer,
            ..
        }) = self.containers.stack.last()
        else {
            return None;
        };
        // Only the marker line is buffered so far; a multi-segment buffer
        // means this is not a fresh marker line.
        if buffer.segment_count() != 1 {
            return None;
        }
        try_parse_line_block_start(buffer.first_text()?)?;
        let content_col = *content_col;
        let bq_depth = self.current_blockquote_depth();

        // Marker-line dispatch: the list marker + indent were emitted
        // upstream (`list_marker_consumed_on_line_0 = true`); blockquotes,
        // if any, are outer of the list.
        let prefix = ContainerPrefix::from_scalars(
            bq_depth,
            content_col,
            bq_depth > 0,
            0,
            true,
            self.config.dialect,
        );
        let window = StrippedLines::new(&self.lines, self.pos, &prefix);

        // Tables outrank line blocks in the registry (10 vs 13), and a pipe
        // table's header row satisfies `try_parse_line_block_start`. Probe
        // into a throwaway builder and decline on a hit, leaving the
        // marker-line table to the buffer's structural lift at item close.
        // Only pipe tables can collide: grid tables open on `+`, and simple
        // and multiline tables never open on `|`.
        if self.config.extensions.pipe_tables {
            let mut probe = GreenNodeBuilder::new();
            if tables::try_parse_pipe_table(&window, &mut probe, self.config).is_some() {
                return None;
            }
        }

        if let Some(Container::ListItem { buffer, .. }) = self.containers.stack.last_mut() {
            buffer.clear();
        }
        // Always commits the marker line, so this cannot report zero progress.
        let new_pos = parse_line_block(&window, &mut self.builder, self.config);
        Some(new_pos.saturating_sub(self.pos).saturating_sub(1))
    }

    /// When a new list item's marker-line content is a table caption that a
    /// table follows (`- Table: cap\n\n  | a | b |\n  …`), emit the whole
    /// caption-led table as the item's content instead of leaving the caption
    /// line buffered as a paragraph.
    ///
    /// Without this, the caption line is buffered and emitted as a `PLAIN`, and
    /// the table — dispatched later at its grid line — re-claims the same line
    /// via its backward caption scan (`find_caption_before_table`), duplicating
    /// the caption and breaking losslessness. The dispatcher's `TableParser`
    /// never fires on the marker line because the list parser consumes it before
    /// block dispatch runs, so we bridge that gap here, mirroring
    /// `maybe_open_fenced_code_in_new_list_item`. Returns the number of source
    /// lines consumed beyond the list-marker line.
    fn maybe_open_caption_table_in_new_list_item(&mut self) -> Option<usize> {
        if !self.config.extensions.table_captions {
            return None;
        }
        if !(self.config.extensions.simple_tables
            || self.config.extensions.multiline_tables
            || self.config.extensions.grid_tables
            || self.config.extensions.pipe_tables)
        {
            return None;
        }

        let Some(Container::ListItem {
            content_col,
            buffer,
            ..
        }) = self.containers.stack.last()
        else {
            return None;
        };
        // Only the marker line is buffered so far; a multi-segment buffer means
        // more content already accumulated and this is not a fresh marker line.
        if buffer.segment_count() != 1 || buffer.first_text().is_none() {
            return None;
        }
        let content_col = *content_col;

        // Confirm a caption-led table actually follows, reading the marker line
        // and its lookahead through the list-content strip (`content_col`).
        // Bail otherwise, leaving the line buffered for paragraph handling.
        let bq_depth = self.current_blockquote_depth();
        let prefix = ContainerPrefix::from_scalars(
            bq_depth,
            content_col,
            bq_depth > 0,
            0,
            true,
            self.config.dialect,
        );
        let window = StrippedLines::new(&self.lines, self.pos, &prefix);
        if !tables::is_caption_followed_by_table(&window, self.pos) {
            return None;
        }

        // Parse the caption-led table directly at the marker line, trying each
        // enabled kind (Grid → Multiline → Pipe → Simple). A `None` return
        // leaves the builder untouched (every kind validates before its first
        // `start_node`), so the cascade is safe. Mirrors `first_kind_at`.
        //
        // The cheap `is_caption_followed_by_table` probe can accept where the
        // full parse rejects, so parse *before* clearing the buffer: on the
        // miss path nothing was emitted and the caption line stays buffered for
        // normal paragraph handling.
        let mut consumed = None;
        if self.config.extensions.grid_tables {
            consumed = tables::try_parse_grid_table(&window, &mut self.builder, self.config);
        }
        if consumed.is_none() && self.config.extensions.multiline_tables {
            consumed = tables::try_parse_multiline_table(&window, &mut self.builder, self.config);
        }
        if consumed.is_none() && self.config.extensions.pipe_tables {
            consumed = tables::try_parse_pipe_table(&window, &mut self.builder, self.config);
        }
        if consumed.is_none() && self.config.extensions.simple_tables {
            consumed = tables::try_parse_simple_table(&window, &mut self.builder, self.config);
        }
        let consumed = consumed?;

        // Parse succeeded and the table (with its `TABLE_CAPTION`) is emitted;
        // clear the buffered caption line so it isn't also emitted as a `PLAIN`
        // when the list item closes.
        if let Some(Container::ListItem { buffer, .. }) = self.containers.stack.last_mut() {
            buffer.clear();
        }
        Some(consumed.saturating_sub(1))
    }

    /// When a new list item's marker line *begins a table* that is followed by a
    /// trailing caption (`- | a | b |\n  | - | - |\n\n  : cap` or the
    /// `Table:`/`table:` keyword form), parse the whole table-with-caption as the
    /// item's content here, at the marker line.
    ///
    /// A marker-line table is normally buffered and recognized only at item
    /// close via [`crate::parser::utils::list_item_buffer::ListItemBuffer`]'s
    /// structural lift. That lift cannot see a *trailing* caption: the blank line
    /// after the table flushes the buffer (`Table:` form), and a bare `: cap`
    /// line is additionally claimed by the definition-list parser as a term/
    /// definition (`: cap` form) — both before the caption ever reaches the
    /// buffer. Parsing the table at the marker line instead lets the table
    /// parser's own trailing-caption scan (`find_caption_after_table`) absorb the
    /// caption, matching pandoc, which always treats `: cap` after a table as the
    /// table's `Caption`, never a definition list.
    ///
    /// Gated on a caption actually being present so the *no-caption* marker-line
    /// table keeps its existing buffer-lift CST untouched. Returns the number of
    /// source lines consumed beyond the list-marker line.
    fn maybe_open_table_with_trailing_caption_in_new_list_item(&mut self) -> Option<usize> {
        if !self.config.extensions.table_captions {
            return None;
        }
        if !(self.config.extensions.simple_tables
            || self.config.extensions.multiline_tables
            || self.config.extensions.grid_tables
            || self.config.extensions.pipe_tables)
        {
            return None;
        }

        let Some(Container::ListItem {
            content_col,
            buffer,
            ..
        }) = self.containers.stack.last()
        else {
            return None;
        };
        // Only the marker line is buffered so far; a multi-segment buffer means
        // more content already accumulated and this is not a fresh marker line.
        if buffer.segment_count() != 1 {
            return None;
        }
        // Cheap pre-filter: a table on the marker line begins with `|` or `+`.
        let first = buffer.first_text()?;
        if !matches!(
            first.trim_start().as_bytes().first(),
            Some(b'|') | Some(b'+')
        ) {
            return None;
        }
        let content_col = *content_col;

        let bq_depth = self.current_blockquote_depth();
        let prefix = ContainerPrefix::from_scalars(
            bq_depth,
            content_col,
            bq_depth > 0,
            0,
            true,
            self.config.dialect,
        );
        let window = StrippedLines::new(&self.lines, self.pos, &prefix);

        // A caption-led table (`- : cap` / `- Table: cap` then table) is the
        // sibling function's job; never double-handle it here.
        if tables::is_caption_followed_by_table(&window, self.pos) {
            return None;
        }

        // Probe into a throwaway builder: only commit when the marker-line table
        // actually pulls in a trailing caption. Otherwise leave the line buffered
        // so the no-caption structural lift produces its established CST.
        let mut probe = GreenNodeBuilder::new();
        let _ = try_parse_any_table_kind(&window, &mut probe, self.config)?;
        let probe_root = SyntaxNode::new_root(probe.finish());
        let has_caption = probe_root
            .children()
            .any(|c| c.kind() == SyntaxKind::TABLE_CAPTION);
        if !has_caption {
            return None;
        }

        // Commit: emit the table (with its `TABLE_CAPTION`) as the list item's
        // content and drop the buffered marker line so it isn't also emitted.
        let consumed = try_parse_any_table_kind(&window, &mut self.builder, self.config)?;
        if let Some(Container::ListItem { buffer, .. }) = self.containers.stack.last_mut() {
            buffer.clear();
        }
        Some(consumed.saturating_sub(1))
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
        let mut container_scan = code_blocks::ContainerExitScan::new(content_col);
        for raw_line in self.lines.iter().skip(self.pos + 1) {
            let (line_bq_depth, inner) = count_blockquote_markers(raw_line);
            if line_bq_depth < bq_depth {
                break;
            }
            // The item this fence opened in ends at a blank line followed by an
            // under-indented line; a closer past that point is not its own.
            if container_scan.exits(inner) {
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

    /// Whether a fence opened on a *lazy* blockquote line has a matching
    /// closer inside the quote's raw content.
    ///
    /// A lazy twin of [`Self::has_matching_fence_closer`], which breaks on the
    /// first line with fewer `>` markers and so never sees past the opener
    /// here. This one mirrors `FencedCodeBlockParser::detect_prepared`'s scan
    /// instead: a marker-less line keeps the scan alive because the gobble
    /// folds it back into the quote, while a blank line ends the quote and so
    /// ends the scan. Candidates are fully de-indented to match
    /// `lazy_gobble_trim`; `is_closing_fence` tolerates only three spaces of
    /// its own.
    fn lazy_fence_has_matching_closer(&self, fence: &code_blocks::FenceInfo) -> bool {
        self.lines
            .iter()
            .skip(self.pos + 1)
            .take_while(|raw_line| !raw_line.trim().is_empty())
            .any(|raw_line| {
                let (_, inner) = count_blockquote_markers(raw_line);
                code_blocks::is_closing_fence(inner.trim_start_matches([' ', '\t']), fence)
            })
    }

    /// Whether de-indented lazy content opens a fenced code block that would
    /// actually form.
    ///
    /// `rest` must already be gobble-trimmed: `try_parse_fence_open` tolerates
    /// three spaces and no tabs, while the gobble drops every leading byte.
    /// Extension-gated the way [`Self::maybe_open_fenced_code_in_new_list_item`]
    /// is, and closer-gated because pandoc's `codeBlockFenced` fails without
    /// one — `> - a` / `   ``` ` / `   c` stays a single `Plain`. An info
    /// string is *not* an alternative to the closer here: `- a` / ```` ```rust ````
    /// / `c` is lazy text under pandoc.
    fn lazy_content_opens_fence(&self, rest: &str) -> Option<code_blocks::FenceInfo> {
        let fence = code_blocks::try_parse_fence_open(rest, self.config.dialect)?;
        let enabled = match fence.fence_char {
            '`' => self.config.extensions.backtick_code_blocks,
            '~' => self.config.extensions.fenced_code_blocks,
            _ => true,
        };
        if !enabled || !self.lazy_fence_has_matching_closer(&fence) {
            return None;
        }
        Some(fence)
    }

    /// Check if a paragraph is currently open.
    fn is_paragraph_open(&self) -> bool {
        matches!(self.containers.last(), Some(Container::Paragraph { .. }))
    }

    /// Whether the innermost container is a list item whose content is still
    /// buffered.
    ///
    /// A `ListItemBuffer` holds bytes that have *not* been written to the
    /// green builder yet, so it is the analogue of an open paragraph: any
    /// block emitted while it is non-empty lands before the buffered text and
    /// reorders the document. Paragraph-interrupt rules must consult this as
    /// well as [`Self::is_paragraph_open`].
    fn is_list_item_content_open(&self) -> bool {
        matches!(
            self.containers.last(),
            Some(Container::ListItem { buffer, .. }) if !buffer.is_empty()
        )
    }

    /// Whether a definition marker on this line has to *end* the list item
    /// content above it rather than continue it.
    ///
    /// Pandoc reads a definition marker where a block may start, and inside a
    /// list item its `endline` refuses to cross a block start, so the marker
    /// is neither a soft nor a lazy continuation of the text above it. It is
    /// not a definition either: a term is a one-line block, and the block
    /// above this line is still open, so no term precedes the marker. What is
    /// left is an ordinary paragraph, placed by the marker's own indent:
    ///
    /// - `- a\n  b\n  : def` -> `BulletList [[Plain "a b", Plain ": def"]]`,
    ///   the marker reaching the item's content column and staying inside it;
    /// - `- Term\n: def` -> `BulletList [[Plain "Term"]]` + `Para ": def"`,
    ///   the dedented marker landing outside the item altogether.
    ///
    /// Only a list item guards its `endline` this way. At top level and inside
    /// a blockquote pandoc keeps `a\nb\n: def` a single paragraph, so this
    /// asks for a list item whose content is still buffered — the state the
    /// two cases above share, and the one an already-open definition list
    /// never reaches (its marker is claimed as a `Definition` upstream).
    fn definition_marker_breaks_open_list_item_block(&self, content: &str) -> bool {
        if !self.config.extensions.definition_lists || !self.is_list_item_content_open() {
            return false;
        }
        let content_col = paragraphs::current_content_col(&self.containers);
        let Some((marker, ..)) =
            definition_lists::definition_marker_in_list_frame(content, Some(content_col))
        else {
            return false;
        };
        // Same table-caption escape the term lookaheads take: a `:` line that
        // introduces a table is not a definition marker at all.
        !(marker == ':'
            && self.config.extensions.table_captions
            && super::blocks::tables::is_caption_followed_by_table(&self.lines[..], self.pos))
    }

    /// What a definition marker on this line does to the block of the
    /// definition body it sits in, or `None` when the line is not such a
    /// marker.
    ///
    /// This is the definition-body analogue of
    /// [`Self::definition_marker_breaks_open_list_item_block`]. Pandoc re-reads
    /// a definition body as its own block sequence starting at the body's
    /// content column, so a marker reaching that column is read where a block
    /// may start and `endline` refuses to cross it — the marker is neither a
    /// soft nor a lazy continuation of the text above it. What is left depends
    /// on the shape of the block it stands over; see [`BufferedBodyBlock`].
    ///
    /// Reaching the content column is the whole test: an indented marker is
    /// *always* inside the body, so a body whose block is already closed (or
    /// which has no block yet) still keeps it — as text, since there is no
    /// one-line block below it to be its term. Only a marker *dedented* below
    /// the content column is a second definition of the same term
    /// (`T\n:   a\n  : def`), and that shape is deliberately left to the
    /// `Definition` arm of `DefinitionListParser::detect_prepared`.
    ///
    /// `content_indent` is the body's content column, already stripped off
    /// `stripped_content`; `content` still carries it, so the dedent test reads
    /// the original indent.
    fn definition_marker_over_open_body_block(
        &self,
        content: &str,
        stripped_content: &str,
        content_indent: usize,
    ) -> Option<BufferedBodyBlock> {
        if !self.config.extensions.definition_lists || content_indent == 0 {
            return None;
        }
        let Some(Container::Definition {
            plain_open,
            plain_buffer,
            ..
        }) = self.containers.last()
        else {
            return None;
        };
        if leading_indent(content).0 < content_indent {
            return None;
        }
        let (marker, ..) = definition_lists::try_parse_definition_marker(stripped_content)?;
        // Same table-caption escape the term lookaheads take: a `:` line that
        // introduces a table is not a definition marker at all.
        if marker == ':'
            && self.config.extensions.table_captions
            && super::blocks::tables::is_caption_followed_by_table(&self.lines[..], self.pos)
        {
            return None;
        }
        // A closed block is not a term candidate: the blank line that closed
        // it has already detached it from the marker (a term keeps at most one
        // blank, and the one-blank case was promoted by lookahead before the
        // flush). What is left is body text.
        let buffered = if *plain_open {
            plain_buffer.raw_text()
        } else {
            String::new()
        };
        let buffered = buffered.trim_end_matches(['\r', '\n']);
        if buffered.trim().is_empty() || buffered.contains('\n') {
            Some(BufferedBodyBlock::Block)
        } else {
            Some(BufferedBodyBlock::Term)
        }
    }

    /// Whether the blank line at `self.pos` closes a definition body block
    /// that the *next* line promotes to a term.
    ///
    /// The no-blank-line case is [`Self::definition_marker_over_open_body_block`],
    /// which reads the marker line itself. Here the marker is still ahead, so
    /// the same question is asked by lookahead: pandoc lets a term keep one
    /// blank line before its definition, so the blank does not detach the
    /// marker from the block above it, and a one-line block is a term
    /// (`T\n:   a\n\n    :   b` nests a definition list on `a`).
    ///
    /// The lookahead must run *before* the flush: promotion re-opens the
    /// buffered bytes as a `TERM`, and once they reach the builder as a
    /// `PLAIN` there is nothing left to retag.
    fn blank_line_promotes_buffered_definition_term(&self) -> bool {
        if !self.config.extensions.definition_lists {
            return false;
        }
        let content_indent = self.content_container_indent_to_strip();
        // A body with no content column has no frame to read the marker in;
        // the dispatcher's `Definition` arm owns that shape.
        if content_indent == 0 {
            return false;
        }
        let Some(Container::Definition {
            plain_open: true,
            plain_buffer,
            ..
        }) = self.containers.last()
        else {
            return false;
        };
        let buffered = plain_buffer.raw_text();
        let buffered = buffered.trim_end_matches(['\r', '\n']);
        // Only a one-line block is a term. An empty buffer has no block to
        // promote, and a multi-line one stays a `PLAIN`.
        if buffered.trim().is_empty() || buffered.contains('\n') {
            return false;
        }
        // Read the lookahead through the open containers' prefix, so any
        // list/blockquote markers above the body come off first.
        let prefix =
            ContainerPrefix::from_stack(&self.containers.stack, false, self.config.dialect);
        let stripped = StrippedLines::new(&self.lines, self.pos, &prefix);
        // `self.pos` is the blank line itself, so a marker on the very next
        // line reports zero further blanks. Anything more is two blank lines
        // between the term and its definition, which detaches them.
        if definition_lists::next_line_is_definition_marker(&stripped, self.pos) != Some(0) {
            return false;
        }
        // The strip cannot answer the dedent question on its own —
        // `line_carries_list_indent` vets only the *list* component, and
        // `strip_content_indent` degrades gracefully instead of reporting a
        // short line — so measure the content column the way
        // `definition_marker_over_open_body_block` does: on the
        // blockquote-stripped line, against the summed content indent (which
        // is absolute, list indent included). A marker *below* that column is
        // a second definition of the outer term (`T\n:   a\n\n  : b`), not a
        // definition of the block above it.
        let marker_line =
            strip_n_blockquote_markers(self.lines[self.pos + 1], self.current_blockquote_depth());
        leading_indent(marker_line).0 >= content_indent
    }

    /// Turn the single buffered line of the open definition body into the term
    /// of a definition list nested in that body.
    ///
    /// The buffered bytes have not reached the green builder yet, so the nested
    /// `DEFINITION_LIST` can simply be opened around them — no retroactive
    /// wrapping is needed. The marker line itself is left to the dispatcher,
    /// whose `Definition` arm finds the `DEFINITION_ITEM` this opens and hangs
    /// the definition off it.
    fn promote_buffered_definition_term(&mut self) {
        let Some(Container::Definition {
            plain_open,
            plain_buffer,
            ..
        }) = self.containers.stack.last_mut()
        else {
            return;
        };
        let term_line = plain_buffer.raw_text();
        plain_buffer.clear();
        *plain_open = false;

        self.builder.start_node(SyntaxKind::DEFINITION_LIST.into());
        self.containers.push(Container::DefinitionList {});
        self.builder.start_node(SyntaxKind::DEFINITION_ITEM.into());
        self.containers.push(Container::DefinitionItem {});
        definition_lists::emit_term(&mut self.builder, &term_line, self.config);
    }

    /// Append `line` to whichever open text buffer is holding the current
    /// block's content — the paragraph's, or the list item's.
    ///
    /// Used by the paragraph-interrupt guards in the no-blank-before
    /// dispatch arm, which must fold the line into the open block instead of
    /// letting a `Yes` detection emit a sibling.
    fn append_lazy_continuation_line(&mut self, line: &str) {
        if self.is_paragraph_open() {
            paragraphs::append_paragraph_line(
                &mut self.containers,
                &mut self.builder,
                line,
                self.config,
            );
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
    fn try_fold_list_item_buffer_into_setext(&mut self, content: &str) -> Option<LineDispatch> {
        let Some(Container::ListItem {
            buffer,
            content_col,
            ..
        }) = self.containers.stack.last()
        else {
            return None;
        };
        if buffer.segment_count() != 1 {
            return None;
        }
        let text_line = buffer.first_text()?;

        // CommonMark §5.2: the underline must be indented to at least the
        // list item's content column. A bare `---` at column 0 escapes the
        // item and becomes a thematic break (CMark spec example #94/#99); a
        // bare `-` at column 0 is a sibling list marker (#281/#282).
        let content_col = *content_col;
        let (underline_indent_cols, _) = leading_indent(content);
        if underline_indent_cols < content_col {
            return None;
        }

        let lines = [text_line, content];
        let (level, _) = try_parse_setext_heading(&lines, 0)?;

        let (text_no_newline, _) = strip_newline(text_line);
        if text_no_newline.trim().is_empty() {
            return None;
        }
        if try_parse_horizontal_rule(text_no_newline).is_some() {
            return None;
        }

        let text_owned = text_line.to_string();
        if let Some(Container::ListItem { buffer, .. }) = self.containers.stack.last_mut() {
            buffer.clear();
        }
        emit_setext_heading(&mut self.builder, &text_owned, content, level, self.config);
        Some(LineDispatch::consumed(1))
    }

    /// Close paragraph if one is currently open.
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
    fn html_block_demotes_paragraph_to_plain(&self, block_match: &PreparedBlockMatch) -> bool {
        if self.config.dialect != crate::options::Dialect::Pandoc {
            return false;
        }
        if self.block_registry.parser_name(block_match) != "html_block" {
            return false;
        }
        let html_block_type = block_match
            .payload
            .as_ref()
            .and_then(|p| p.downcast_ref::<crate::parser::blocks::html_blocks::HtmlBlockType>());
        matches!(
            html_block_type,
            Some(crate::parser::blocks::html_blocks::HtmlBlockType::BlockTag { .. })
        )
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

    /// Returns the number of extra lines consumed beyond the block parser's
    /// reported `lines_consumed` (currently always 1 for footnote definitions).
    /// Non-zero only on the definition-list-term blank-line lookahead path.
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
            emit_term(&mut self.builder, first_line_content, self.config);
            self.emit_term_lookahead_blank_lines(blank_count);
            return blank_count;
        }

        if let Some(extras) = self.try_dispatch_footnote_html_block(first_line_content, content_col)
        {
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

        // A marker reaching the deepest item's content column opens a sublist —
        // but only within the [content_col, content_col + 4) band. At
        // content_col + 4 or deeper, a sublist would be an indented code block,
        // and (with no blank line) a code block can't interrupt the open
        // paragraph, so pandoc treats the marker as lazy continuation text. Fall
        // through to the continuation path in that case.
        let current_content_col = paragraphs::current_content_col(&self.containers);
        if prepared.indent_cols >= current_content_col
            && prepared.indent_cols < current_content_col + 4
        {
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
                    buffer.push_text(content, self.config);
                    if !content.trim().is_empty() {
                        *marker_only = false;
                    }
                }
                true
            }
            _ => false,
        }
    }

    /// Returns the number of extra lines consumed beyond the block parser's
    /// reported `lines_consumed` (= 1 for list-open). Non-zero when the
    /// list-marker line opens a fenced code block (multi-line fence) or
    /// dispatches into a same-line blockquote whose content spans multiple
    /// source lines.
    fn handle_list_open_effect(
        &mut self,
        block_match: &super::block_dispatcher::PreparedBlockMatch,
        content: &str,
        indent_to_emit: Option<&str>,
    ) -> usize {
        use super::block_dispatcher::ListPrepared;

        let prepared = block_match
            .payload
            .as_ref()
            .and_then(|p| p.downcast_ref::<ListPrepared>());
        let Some(prepared) = prepared else {
            return 0;
        };

        if prepared.indent_cols >= 4 && !lists::in_list(&self.containers) {
            paragraphs::start_paragraph_if_needed(&mut self.containers, &mut self.builder);
            paragraphs::append_paragraph_line(
                &mut self.containers,
                &mut self.builder,
                content,
                self.config,
            );
            return 0;
        }

        if self.is_paragraph_open() {
            if !block_match.detection.eq(&BlockDetectionResult::Yes) {
                paragraphs::append_paragraph_line(
                    &mut self.containers,
                    &mut self.builder,
                    content,
                    self.config,
                );
                return 0;
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

                    let finish = if let Some(nested_marker) = prepared.nested_marker {
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
                    if let Some(extras) = self.maybe_open_fenced_code_in_new_list_item() {
                        return extras;
                    }
                    if let Some(extras) = self.maybe_open_caption_table_in_new_list_item() {
                        return extras;
                    }
                    if let Some(extras) =
                        self.maybe_open_table_with_trailing_caption_in_new_list_item()
                    {
                        return extras;
                    }
                    if let Some(extras) = self.maybe_open_line_block_in_new_list_item() {
                        return extras;
                    }
                    self.maybe_open_indented_code_in_new_list_item();
                    if let Some(extras) = self.maybe_open_definition_term_in_new_list_item() {
                        return extras;
                    }
                    return self.dispatch_bq_after_list_item(finish);
                }
            }

            self.emit_list_item_buffer_if_needed();

            let finish = start_nested_list(
                &mut self.containers,
                &mut self.builder,
                &prepared.marker,
                &list_item,
                indent_to_emit,
                self.config,
            );
            if let Some(extras) = self.maybe_open_fenced_code_in_new_list_item() {
                return extras;
            }
            if let Some(extras) = self.maybe_open_caption_table_in_new_list_item() {
                return extras;
            }
            if let Some(extras) = self.maybe_open_table_with_trailing_caption_in_new_list_item() {
                return extras;
            }
            if let Some(extras) = self.maybe_open_line_block_in_new_list_item() {
                return extras;
            }
            self.maybe_open_indented_code_in_new_list_item();
            if let Some(extras) = self.maybe_open_definition_term_in_new_list_item() {
                return extras;
            }
            return self.dispatch_bq_after_list_item(finish);
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

            let finish = if let Some(nested_marker) = prepared.nested_marker {
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
            if let Some(extras) = self.maybe_open_fenced_code_in_new_list_item() {
                return extras;
            }
            if let Some(extras) = self.maybe_open_caption_table_in_new_list_item() {
                return extras;
            }
            if let Some(extras) = self.maybe_open_table_with_trailing_caption_in_new_list_item() {
                return extras;
            }
            if let Some(extras) = self.maybe_open_line_block_in_new_list_item() {
                return extras;
            }
            self.maybe_open_indented_code_in_new_list_item();
            if let Some(extras) = self.maybe_open_definition_term_in_new_list_item() {
                return extras;
            }
            return self.dispatch_bq_after_list_item(finish);
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

        let finish = if let Some(nested_marker) = prepared.nested_marker {
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
        if let Some(extras) = self.maybe_open_fenced_code_in_new_list_item() {
            return extras;
        }
        if let Some(extras) = self.maybe_open_caption_table_in_new_list_item() {
            return extras;
        }
        if let Some(extras) = self.maybe_open_table_with_trailing_caption_in_new_list_item() {
            return extras;
        }
        if let Some(extras) = self.maybe_open_line_block_in_new_list_item() {
            return extras;
        }
        self.maybe_open_indented_code_in_new_list_item();
        if let Some(extras) = self.maybe_open_definition_term_in_new_list_item() {
            return extras;
        }
        self.dispatch_bq_after_list_item(finish)
    }

    /// Dispatch a leading HTML block on a definition body's marker line
    /// (`:   <html>`).
    ///
    /// The first content line of a definition body otherwise flows into the
    /// buffered-plain path, which only special-cases ATX headings — a raw
    /// HTML block there would be parsed as inline text (`RawInline` inside a
    /// `Para`) instead of a structural block (`RawBlock` / `Div`), diverging
    /// from pandoc-native. This mirrors the blockquote / list / fenced-code
    /// arms of the definition first-content-line cascade. HTML that appears
    /// on a *later* definition-body line already dispatches correctly through
    /// the normal container path.
    ///
    /// Scope: only blocks that CLOSE on the marker line are lifted here. The
    /// extent is probed by parsing a synthetic line window (line 0 = the
    /// already-stripped post-marker bytes; continuation lines = the raw
    /// following lines with the outer container prefix stripped) into a
    /// throwaway builder and checking the block consumes exactly one line.
    /// Multi-line HTML bodies that open on the marker line fall through to
    /// the buffered-plain path (deferred — they need content-indent
    /// strip/re-inject for the continuation lines).
    ///
    /// Returns `Some(0)` when a marker-line HTML block was emitted (no lines
    /// beyond the marker are consumed), or `None` when no leading HTML block
    /// closes on the marker line.
    fn try_dispatch_definition_html_block(
        &mut self,
        content_line: &str,
        content_col: usize,
    ) -> Option<usize> {
        let is_commonmark = self.config.dialect == crate::options::Dialect::CommonMark;
        let (content_no_nl, _) = strip_newline(content_line);
        let block_type = html_blocks::try_parse_html_block_start(content_no_nl, is_commonmark)?;

        // Probe: does the block close on the marker line? Parse a synthetic
        // window into a throwaway builder and read back the line count. The
        // continuation lines carry their outer container prefix; strip it so
        // the probe sees the same content the real dispatch would.
        let bq_depth = self.current_blockquote_depth();
        let content_prefix = ContainerPrefix::from_scalars(
            bq_depth,
            0,
            bq_depth > 0,
            content_col,
            false,
            self.config.dialect,
        );
        let probe_consumed = {
            let mut synthetic: Vec<&str> = Vec::with_capacity(self.lines.len() - self.pos);
            synthetic.push(content_line);
            for line in &self.lines[self.pos + 1..] {
                synthetic.push(content_prefix.strip(line));
            }
            let mut probe = GreenNodeBuilder::new();
            probe.start_node(SyntaxKind::DOCUMENT.into());
            let consumed = html_blocks::parse_html_block_with_wrapper(
                &mut probe,
                &synthetic,
                0,
                block_type.clone(),
                &ContainerPrefix::default(),
                SyntaxKind::HTML_BLOCK,
                html_blocks::SoftbreakFusion::None,
                self.config,
            );
            probe.finish_node();
            consumed
        };
        if probe_consumed == 0 {
            return None;
        }

        let wrapper_kind =
            marker_line_html_block_wrapper_kind(&block_type, content_no_nl, self.config);

        if probe_consumed == 1 {
            // A comment/PI whose close-line carries trailing text
            // (`:   <!-- hi --> t`) softbreak-fuses that text with the
            // following non-blank continuation lines into ONE paragraph
            // (`RawBlock, Para/Plain [t, SoftBreak, more]`), matching pandoc.
            // The plain single-line emit below would leave the continuation
            // to the definition's plain buffer as a separate block. Route the
            // marker line plus its fusible-paragraph continuation lines
            // through the multi-line lift, which reparses the window and lets
            // the parser's own paragraph-continuation rules fuse them. Gated
            // to Pandoc + `bq_depth == 0`, mirroring the multi-line body lift
            // below.
            if self.config.dialect == crate::options::Dialect::Pandoc
                && bq_depth == 0
                && let Some(extras) = self.try_fuse_definition_comment_trailing(
                    content_line,
                    content_no_nl,
                    content_col,
                )
            {
                return Some(extras);
            }

            // Emit for real using only the marker line's bytes: the block
            // closed on line 0, so no continuation lines are consumed and the
            // CST stays byte-equal to source (marker + spaces already emitted
            // upstream).
            let single = [content_line];
            html_blocks::parse_html_block_with_wrapper(
                &mut self.builder,
                &single,
                0,
                block_type,
                &ContainerPrefix::default(),
                wrapper_kind,
                html_blocks::SoftbreakFusion::None,
                self.config,
            );
            return Some(0);
        }

        // Multi-line HTML body opening on the marker line (`<div>\n  x\n
        // </div>`). The single-line emit path can't handle this — the body
        // lines carry the definition's content indent, which the plain block
        // parser would preserve verbatim (parsing the body as an indented code
        // block). Reuse the list-item lift: build the block text from the
        // marker-line post-marker bytes plus the raw continuation lines, strip
        // `content_col` of leading indent from the continuation lines before
        // the inner reparse (so the body parses as markdown, not code), and
        // re-inject the stripped indent during graft so the CST stays
        // byte-equal to source. Gated to Pandoc — CommonMark keeps the opaque
        // shape (the lift hardcodes the Pandoc HTML-block grammar). Also gated
        // to `bq_depth == 0`: inside a blockquote the continuation lines carry
        // `> ` markers that the list-item lift doesn't strip, so a nested
        // definition falls through to the pre-existing path.
        if self.config.dialect != crate::options::Dialect::Pandoc || bq_depth != 0 {
            return None;
        }
        let mut text = String::from(content_line);
        for line in &self.lines[self.pos + 1..self.pos + probe_consumed] {
            text.push_str(line);
        }
        // A definition is loose (body paragraphs render as `Para`) when a blank
        // line separates the term from the marker line; tight otherwise
        // (`Plain`). Only affects the trailing-text split shape.
        let use_paragraph = self.pos > 0 && is_blank_line(self.lines[self.pos - 1]);
        let lifted = super::utils::list_item_buffer::try_emit_html_block_lift(
            &mut self.builder,
            &text,
            self.config,
            &[content_col],
            use_paragraph,
            "",
            false,
        );
        if !lifted {
            return None;
        }
        Some(probe_consumed.saturating_sub(1))
    }

    /// Dispatch an HTML block that opens on a *later* (non-marker) line of a
    /// content-container body (definition, footnote, admonition) whose lines
    /// carry a `content_col` indent. The general dispatcher's
    /// `parse_html_block_with_wrapper` ignores the `ContentIndent` prefix op,
    /// dropping the stripped indent (losslessness fail) and reparsing the body
    /// with its indent intact (an indented `CodeBlock` instead of markdown).
    /// This routes the block through the list-item lift, which strips
    /// `content_col`, reparses the dedented body as markdown, and re-injects
    /// the indent per line. The line-0 indent is injected *inside* the lifted
    /// block (as the open tag's leading `WHITESPACE`, via the lift's
    /// `line0_prefix` arg) rather than as a sibling — the formatter dumps HTML
    /// blocks verbatim and the `DEFINITION` formatter drops direct `WHITESPACE`
    /// children, so a sibling indent would vanish on format.
    ///
    /// `stripped_content` is the current line with its content indent already
    /// removed; `content_col` is that indent width; `indent_to_emit` is the
    /// stripped indent bytes for line 0. Returns the number of lines consumed
    /// on success. The caller has established Pandoc dialect, top-level
    /// blockquote depth 0, and an open content container.
    fn try_dispatch_content_indent_html_block(
        &mut self,
        stripped_content: &str,
        content_col: usize,
        indent_to_emit: Option<&str>,
    ) -> Option<usize> {
        let (content_no_nl, _) = strip_newline(stripped_content);
        let block_type = html_blocks::try_parse_html_block_start(content_no_nl, false)?;

        // Probe how many lines the block spans by reparsing a synthetic window
        // (line 0 already dedented, continuation lines content-indent-stripped)
        // into a throwaway builder.
        let content_prefix =
            ContainerPrefix::from_scalars(0, 0, false, content_col, false, self.config.dialect);
        let probe_consumed = {
            let mut synthetic: Vec<&str> = Vec::with_capacity(self.lines.len() - self.pos);
            synthetic.push(stripped_content);
            for line in &self.lines[self.pos + 1..] {
                synthetic.push(content_prefix.strip(line));
            }
            let mut probe = GreenNodeBuilder::new();
            probe.start_node(SyntaxKind::DOCUMENT.into());
            let consumed = html_blocks::parse_html_block_with_wrapper(
                &mut probe,
                &synthetic,
                0,
                block_type.clone(),
                &ContainerPrefix::default(),
                SyntaxKind::HTML_BLOCK,
                html_blocks::SoftbreakFusion::None,
                self.config,
            );
            probe.finish_node();
            consumed
        };
        if probe_consumed == 0 {
            return None;
        }

        // Build the block text: line 0 dedented, continuation lines raw (the
        // lift strips `content_col` from them). Loose (blank line before) ->
        // `Para`, tight -> `Plain`; only affects the trailing-text split shape.
        let mut text = String::from(stripped_content);
        for line in &self.lines[self.pos + 1..self.pos + probe_consumed] {
            text.push_str(line);
        }
        let use_paragraph = self.pos > 0 && is_blank_line(self.lines[self.pos - 1]);
        // The line-0 content indent is injected *inside* the lifted block (as
        // the open tag's leading WHITESPACE) rather than as a sibling: the
        // formatter dumps HTML blocks verbatim and the DEFINITION formatter
        // drops direct WHITESPACE children, so the indent must live in the
        // block's own bytes to survive a round-trip.
        let line0_prefix = indent_to_emit.unwrap_or("");

        // The lift both validates and emits, so probe it into a throwaway
        // builder first — if it can't cleanly lift (shape the lift rejects),
        // fall back to the general dispatch without mutating the real tree.
        let lift_ok = {
            let mut probe = GreenNodeBuilder::new();
            probe.start_node(SyntaxKind::DOCUMENT.into());
            let ok = super::utils::list_item_buffer::try_emit_html_block_lift(
                &mut probe,
                &text,
                self.config,
                &[content_col],
                use_paragraph,
                line0_prefix,
                true,
            );
            probe.finish_node();
            ok
        };
        if !lift_ok {
            return None;
        }

        // Flush any buffered plain / list-item content and close the open
        // paragraph so block order stays lossless, then graft the lifted block.
        self.emit_buffered_plain_if_needed();
        self.prepare_for_block_element();
        super::utils::list_item_buffer::try_emit_html_block_lift(
            &mut self.builder,
            &text,
            self.config,
            &[content_col],
            use_paragraph,
            line0_prefix,
            true,
        );
        Some(probe_consumed)
    }

    /// Blockquote-nested variant of
    /// [`Self::try_dispatch_content_indent_html_block`]. When the
    /// content-container body sits inside one or more blockquotes
    /// (`> :   text\n>\n>     <div>\n>     x\n>     </div>`), the later-line
    /// HTML block's continuation lines carry both the `> ` markers and the
    /// content indent. The `bq_depth == 0` path can't handle them (its lift
    /// strips only spaces), so the general dispatcher used to fall through
    /// and silently drop the line-0 content indent (a losslessness
    /// violation) while reparsing the body as an indented `CodeBlock`.
    ///
    /// Here we pre-strip every continuation line with the full container
    /// prefix (bq markers + content indent), reparse the dedented body, and
    /// re-inject the captured `>     ` prefix bytes per line during graft so
    /// the CST stays byte-equal to source and the body lifts to `Div [Para
    /// x]`, matching pandoc's block structure. Line 0's outer `> ` marker was
    /// already emitted upstream by the blockquote container, so only its
    /// content indent is re-injected inside the lifted block.
    ///
    /// Gated to Pandoc. Returns the number of lines consumed on success.
    fn try_dispatch_bq_content_indent_html_block(
        &mut self,
        stripped_content: &str,
        content_col: usize,
        indent_to_emit: Option<&str>,
    ) -> Option<usize> {
        use super::blocks::container_prefix::ContainerPrefixLine;

        if self.config.dialect != crate::options::Dialect::Pandoc {
            return None;
        }
        let bq_depth = self.current_blockquote_depth();
        if bq_depth == 0 {
            return None;
        }

        let (content_no_nl, _) = strip_newline(stripped_content);
        let block_type = html_blocks::try_parse_html_block_start(content_no_nl, false)?;

        // Strip the full container prefix (bq markers outermost, then the
        // content indent) from continuation lines.
        let content_prefix = ContainerPrefix::from_scalars(
            bq_depth,
            0,
            true,
            content_col,
            false,
            self.config.dialect,
        );

        // Probe how many lines the block spans by reparsing a synthetic
        // window (line 0 already dedented, continuation lines fully stripped).
        let probe_consumed = {
            let mut synthetic: Vec<&str> = Vec::with_capacity(self.lines.len() - self.pos);
            synthetic.push(stripped_content);
            for line in &self.lines[self.pos + 1..] {
                synthetic.push(content_prefix.strip(line));
            }
            let mut probe = GreenNodeBuilder::new();
            probe.start_node(SyntaxKind::DOCUMENT.into());
            let consumed = html_blocks::parse_html_block_with_wrapper(
                &mut probe,
                &synthetic,
                0,
                block_type,
                &ContainerPrefix::default(),
                SyntaxKind::HTML_BLOCK,
                html_blocks::SoftbreakFusion::None,
                self.config,
            );
            probe.finish_node();
            consumed
        };
        if probe_consumed == 0 {
            return None;
        }

        // Build the dedented block text plus the per-line prefixes to
        // re-inject during graft. Line 0 keeps its content indent (the outer
        // `> ` was already emitted upstream); continuation lines re-inject
        // their full stripped `>     ` prefix.
        let mut parse_text = String::from(stripped_content);
        let mut prefix_lines: Vec<ContainerPrefixLine> = vec![ContainerPrefixLine::list_only(
            indent_to_emit.unwrap_or("").to_string(),
        )];
        for line in &self.lines[self.pos + 1..self.pos + probe_consumed] {
            let stripped = content_prefix.strip(line);
            let captured = &line[..line.len() - stripped.len()];
            parse_text.push_str(stripped);
            prefix_lines.push(ContainerPrefixLine::bq_only(captured.to_string()));
        }
        let use_paragraph = self.pos > 0 && is_blank_line(self.lines[self.pos - 1]);

        // Probe the lift into a throwaway builder first — if it can't cleanly
        // lift the shape, fall back to the general dispatch without mutating
        // the real tree.
        let lift_ok = {
            let mut probe = GreenNodeBuilder::new();
            probe.start_node(SyntaxKind::DOCUMENT.into());
            let ok = super::utils::list_item_buffer::emit_html_block_lift_from_stripped(
                &mut probe,
                &parse_text,
                self.config,
                prefix_lines.clone(),
                use_paragraph,
                true,
            );
            probe.finish_node();
            ok
        };
        if !lift_ok {
            return None;
        }

        self.emit_buffered_plain_if_needed();
        self.prepare_for_block_element();
        super::utils::list_item_buffer::emit_html_block_lift_from_stripped(
            &mut self.builder,
            &parse_text,
            self.config,
            prefix_lines,
            use_paragraph,
            true,
        );
        Some(probe_consumed)
    }

    /// Fuse a definition-body comment/PI close-line trailing text with its
    /// following non-blank continuation lines into one paragraph, matching
    /// pandoc (`:   <!-- --> t\n    more` -> `RawBlock, Para/Plain [t,
    /// SoftBreak, more]`). Returns the number of continuation lines consumed
    /// on success (grafted as siblings of the definition), or `None` when
    /// there is nothing to fuse (no trailing text, no continuation, or the
    /// window doesn't reparse to a clean `RawBlock` + single-paragraph split).
    /// The caller has already established Pandoc dialect + `bq_depth == 0`.
    fn try_fuse_definition_comment_trailing(
        &mut self,
        content_line: &str,
        content_no_nl: &str,
        content_col: usize,
    ) -> Option<usize> {
        // Only comment (`<!-- -->`) and PI (`<? ?>`) blocks take the
        // trailing-split-with-fusion path; their close markers are `-->`
        // and `?>`.
        let trimmed = content_no_nl.trim_start();
        let marker = if trimmed.starts_with("<!--") {
            "-->"
        } else if trimmed.starts_with("<?") {
            "?>"
        } else {
            return None;
        };
        // Require non-whitespace text after the close marker on the marker
        // line — without trailing text there is nothing to fuse (a bare
        // comment plus a following line stays two separate blocks).
        let close = content_no_nl.find(marker)?;
        let trailing = &content_no_nl[close + marker.len()..];
        if trailing.trim().is_empty() {
            return None;
        }

        // Fusible-paragraph extent: consecutive non-blank continuation lines
        // after the marker line, up to the first blank line (a blank line
        // ends the paragraph in both pandoc and the inner reparse). Most
        // interrupting blocks (heading, fence, blockquote, HTML) interrupt a
        // paragraph at both the document top level and inside a def body, so
        // the inner reparse (which works in top-level coordinates) already
        // yields more than two children and the lift's 2-child gate rejects
        // the window. A list item is the exception: pandoc-markdown lists
        // cannot interrupt a paragraph at the top level (so the reparse would
        // wrongly fuse one), but a list at the definition's content indent IS
        // a separate block in the def body. Stop the scan at such a line so
        // the definition container emits the list on its own.
        let mut fuse_count = 0usize;
        while self.pos + 1 + fuse_count < self.lines.len() {
            let line = self.lines[self.pos + 1 + fuse_count];
            if is_blank_line(line) {
                break;
            }
            let stripped = strip_leading_spaces_n(line, content_col);
            if lists::try_parse_list_marker(stripped, self.config, lists::OpenListHint::None)
                .is_some()
            {
                break;
            }
            fuse_count += 1;
        }
        if fuse_count == 0 {
            return None;
        }

        let mut text = String::from(content_line);
        for line in &self.lines[self.pos + 1..self.pos + 1 + fuse_count] {
            text.push_str(line);
        }
        // Loose (blank line before the marker) -> `Para`; tight -> `Plain`.
        let use_paragraph = self.pos > 0 && is_blank_line(self.lines[self.pos - 1]);
        let lifted = super::utils::list_item_buffer::try_emit_html_block_lift(
            &mut self.builder,
            &text,
            self.config,
            &[content_col],
            use_paragraph,
            "",
            false,
        );
        if !lifted {
            return None;
        }
        Some(fuse_count)
    }

    /// Dispatch an HTML block that opens AND closes on a footnote body's first
    /// content line (`[^1]: <div>x</div>`). Mirrors
    /// `try_dispatch_definition_html_block`, but only tags that can interrupt a
    /// paragraph lift: pandoc keeps comments, PIs, `<span>`, and void
    /// inline-block tags (`<embed>`) inline inside footnote bodies (unlike
    /// definition bodies, where a leading comment lifts to a `RawBlock`). Gated
    /// on Pandoc dialect so GFM/CommonMark footnotes stay byte-identical.
    /// Returns `Some(0)` when the block was emitted (no extra lines consumed).
    fn try_dispatch_footnote_html_block(
        &mut self,
        first_line_content: &str,
        content_col: usize,
    ) -> Option<usize> {
        if self.config.dialect != crate::options::Dialect::Pandoc {
            return None;
        }
        let (content_no_nl, _) = strip_newline(first_line_content);
        let block_type = html_blocks::try_parse_html_block_start(content_no_nl, false)?;
        if super::block_dispatcher::html_block_cannot_interrupt(&block_type, content_no_nl, true) {
            return None;
        }

        // Probe: does the block close on the marker line? Parse a synthetic
        // window (line 0 = post-marker bytes; continuation lines stripped of
        // the footnote's 4-space content indent) into a throwaway builder.
        let closes_on_marker_line = {
            let bq_depth = self.current_blockquote_depth();
            let prefix = ContainerPrefix::from_scalars(
                bq_depth,
                0,
                bq_depth > 0,
                content_col,
                false,
                self.config.dialect,
            );
            let mut synthetic: Vec<&str> = Vec::with_capacity(self.lines.len() - self.pos);
            synthetic.push(first_line_content);
            for line in &self.lines[self.pos + 1..] {
                synthetic.push(prefix.strip(line));
            }
            let mut probe = GreenNodeBuilder::new();
            probe.start_node(SyntaxKind::DOCUMENT.into());
            let consumed = html_blocks::parse_html_block_with_wrapper(
                &mut probe,
                &synthetic,
                0,
                block_type.clone(),
                &ContainerPrefix::default(),
                SyntaxKind::HTML_BLOCK,
                html_blocks::SoftbreakFusion::None,
                self.config,
            );
            probe.finish_node();
            consumed == 1
        };
        if !closes_on_marker_line {
            return None;
        }

        // Emit for real using only the marker line's bytes (byte-lossless: the
        // block closed on line 0, no continuation consumed).
        let wrapper_kind =
            marker_line_html_block_wrapper_kind(&block_type, content_no_nl, self.config);
        let single = [first_line_content];
        html_blocks::parse_html_block_with_wrapper(
            &mut self.builder,
            &single,
            0,
            block_type,
            &ContainerPrefix::default(),
            wrapper_kind,
            html_blocks::SoftbreakFusion::None,
            self.config,
        );
        Some(0)
    }

    /// Returns the number of extra lines consumed beyond the block parser's
    /// reported `lines_consumed` (= 1 for definition list). Non-zero when
    /// the Definition arm opens a fenced code block on the marker line
    /// (multi-line fence consumes additional source lines) or dispatches
    /// into a same-line blockquote, and on the Term arm when blank lines
    /// are absorbed between term and definition.
    fn handle_definition_list_effect(
        &mut self,
        block_match: &super::block_dispatcher::PreparedBlockMatch,
        content: &str,
        indent_to_emit: Option<&str>,
    ) -> usize {
        use super::block_dispatcher::DefinitionPrepared;

        let prepared = block_match
            .payload
            .as_ref()
            .and_then(|p| p.downcast_ref::<DefinitionPrepared>());
        let Some(prepared) = prepared else {
            return 0;
        };

        let mut extras: usize = 0;
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

                let indent_bytes = byte_index_at_column(content, *indent);
                emit_definition_marker(&mut self.builder, *marker_char, &content[..indent_bytes]);
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
                let mut plain_buffer = ParagraphBuffer::new();
                let mut definition_pushed = false;

                if *has_content {
                    let current_line = self.lines[self.pos];
                    let (trimmed_content, _) = strip_newline(content);

                    // Slice the container-stripped `content` (not the raw
                    // `current_line`) — otherwise the post-marker view still
                    // carries the outer blockquote/list prefix and
                    // `count_blockquote_markers` fabricates a phantom inner
                    // blockquote (audit finding: see TODO.md
                    // "Audit other multi-line-lookahead block parsers").
                    let content_start = content_start_bytes.min(trimmed_content.len());
                    let content_slice = &trimmed_content[content_start..];
                    let content_line = &content[content_start_bytes.min(content.len())..];

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
                            plain_buffer: ParagraphBuffer::new(),
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
                    } else if let Some(marker_match) = try_parse_list_marker(
                        content_slice,
                        self.config,
                        lists::open_list_hint_at_indent(
                            &self.containers,
                            leading_indent(content_slice).0,
                        ),
                    ) && should_start_list_from_first_line
                    {
                        self.containers.push(Container::Definition {
                            content_col,
                            plain_open: false,
                            plain_buffer: ParagraphBuffer::new(),
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

                        let finish = if let Some(nested_marker) = is_content_nested_bullet_marker(
                            content_line,
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
                        extras = self.dispatch_bq_after_list_item(finish);
                    } else if let Some(fence) =
                        code_blocks::try_parse_fence_open(content_slice, self.config.dialect)
                    {
                        self.containers.push(Container::Definition {
                            content_col,
                            plain_open: false,
                            plain_buffer: ParagraphBuffer::new(),
                        });
                        definition_pushed = true;

                        let bq_depth = self.current_blockquote_depth();
                        if let Some(indent_str) = indent_to_emit {
                            self.builder
                                .token(SyntaxKind::WHITESPACE.into(), indent_str);
                        }
                        let fence_line = content[content_start..].to_string();
                        // Definition-marker dispatch: no list advance here
                        // (`list_content_col = 0`); the definition's base
                        // indent is the content indent; bq, if any, is outer.
                        let prefix = ContainerPrefix::from_scalars(
                            bq_depth,
                            0,
                            bq_depth > 0,
                            content_col,
                            false,
                            self.config.dialect,
                        );
                        let window = StrippedLines::new(&self.lines, self.pos, &prefix);
                        let new_pos = if self.config.extensions.tex_math_gfm
                            && code_blocks::is_gfm_math_fence(&fence)
                        {
                            code_blocks::parse_fenced_math_block(
                                &mut self.builder,
                                &window,
                                fence,
                                Some(&fence_line),
                                self.config.dialect,
                            )
                        } else {
                            code_blocks::parse_fenced_code_block(
                                &mut self.builder,
                                &window,
                                fence,
                                Some(&fence_line),
                                &self.diagnostics,
                                self.config.flavor,
                            )
                        };
                        extras = new_pos.saturating_sub(self.pos).saturating_sub(1);
                    } else if let Some(html_extras) =
                        self.try_dispatch_definition_html_block(content_line, content_col)
                    {
                        self.containers.push(Container::Definition {
                            content_col,
                            plain_open: false,
                            plain_buffer: ParagraphBuffer::new(),
                        });
                        definition_pushed = true;
                        extras = html_extras;
                    } else {
                        let (_, newline_str) = strip_newline(current_line);
                        let (content_without_newline, _) = strip_newline(after_marker_and_spaces);
                        // The marker line's leading columns are owned by the
                        // marker and its trailing spaces, already emitted, so
                        // there is nothing to hold out here.
                        plain_buffer.push_text(content_without_newline);
                        plain_buffer.push_text(newline_str);
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
                self.emit_term_lookahead_blank_lines(*blank_count);
                extras = *blank_count;
            }
        };
        extras
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

    /// Build a `BlockContext` describing the current line *as if* the
    /// container stack already carried `bq_depth` blockquotes.
    ///
    /// Field-for-field mirror of the context `parse_inner_content` builds
    /// (see the `BlockContext { .. }` literal and the blank/doc-start
    /// fill-in that follows it) — the two must stay in sync, because a
    /// verdict this probe reaches has to survive re-detection there. The
    /// deliberate differences:
    ///
    /// - `blockquote_depth` is the hypothetical `bq_depth`, not the stack's.
    /// - `next_line` is stripped of *every* marker, matching the inner
    ///   context. `parse_line`'s own context passes the raw next line
    ///   instead, which would make `SetextHeadingParser`'s leading-byte
    ///   gate reject any underline still carrying a `>`.
    /// - `has_blank_before`'s blockquote clause also fires when this probe
    ///   would open a level, since that is what the stack looks like at
    ///   re-detection time.
    /// - `after_metadata_block` is read, never taken: a probe must not
    ///   consume parser state.
    fn probe_block_context(
        &self,
        bq_depth: usize,
        current_bq_depth: usize,
        content: &'a str,
    ) -> BlockContext<'a> {
        let has_blank_before = if self.pos == 0 || self.after_metadata_block {
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

            is_blank_line(prev_line)
                || prev_is_fenced_div_open
                || bq_depth > current_bq_depth
                || matches!(self.containers.last(), Some(Container::BlockQuote { .. }))
                || !self.previous_block_requires_blank_before_heading()
        };

        let at_document_start = self.pos == 0 && bq_depth == 0;
        let prev_line_blank = self.pos > 0 && {
            let prev_line = self.lines[self.pos - 1];
            let (prev_bq_depth, prev_inner) = count_blockquote_markers(prev_line);
            is_blank_line(prev_line) || (prev_bq_depth > 0 && is_blank_line(prev_inner))
        };

        BlockContext {
            has_blank_before,
            has_blank_before_strict: at_document_start || prev_line_blank,
            at_document_start,
            in_fenced_div: self.in_fenced_div(),
            fenced_div_open_indent: self.innermost_fenced_div_open_indent(),
            fenced_div_wraps_list: self.fenced_div_wraps_innermost_list(),
            myst_directive_closer: self.innermost_myst_directive_closer(),
            blockquote_depth: bq_depth,
            config: self.config,
            diags: self.diagnostics.clone(),
            content_indent: 0,
            indent_to_emit: None,
            list_indent_info: if lists::in_list(&self.containers) {
                let content_col = paragraphs::current_content_col(&self.containers);
                (content_col > 0).then_some(super::block_dispatcher::ListIndentInfo { content_col })
            } else {
                None
            },
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
            next_line: (self.pos + 1 < self.lines.len())
                .then(|| count_blockquote_markers(self.lines[self.pos + 1]).1),
            open_alpha_hint: lists::open_list_hint_at_indent(
                &self.containers,
                leading_indent(content).0,
            ),
        }
    }

    /// How many blockquote levels this line may open, when fewer than its
    /// `>` count.
    ///
    /// Pandoc's `blockQuote` strips exactly *one* `>` per line of a quoted
    /// run and recursively re-parses the remainder, so every parser ahead
    /// of `blockQuote` in the reader order gets a shot at content that
    /// still begins with `>`. `setextHeader` is one of those, which is why
    /// `pandoc -f markdown -t native` reads `> > a\n> ---\n` as
    /// `BlockQuote [Header 2 [Str ">", Space, Str "a"]]` — the underline's
    /// single marker caps the quote at depth 1 and the surplus `>` becomes
    /// literal heading text.
    ///
    /// Counting markers per line, as this parser does, would open both
    /// quotes. So probe the registry at each depth the line would pass
    /// through and stop at the first one whose winner outranks
    /// `BlockQuoteParser`; that depth is the cap. `k == bq_depth` is
    /// excluded, so "nothing claims" is bit-identical to counting.
    ///
    /// Pandoc-dialect only, and this one *is* a dialect difference rather
    /// than a parser's own gate. Capping presumes the surplus markers are
    /// literal text, which is true only under Pandoc. CommonMark reads
    /// them as real containers, and `SetextHeadingParser` says so by
    /// folding the content's own markers into its same-container
    /// comparison — so at probe depth `k` it answers about depth
    /// `k + own_markers`, and its "yes" cannot be read as a verdict for
    /// `k`. Without this gate `> > a\n> > ---\n` collapses to a single
    /// quote under CommonMark, where `cmark` nests two.
    fn blockquote_depth_cap(&self, current_bq_depth: usize, bq_depth: usize) -> Option<usize> {
        if self.config.dialect != crate::options::Dialect::Pandoc {
            return None;
        }

        // A capped line re-enters `parse_inner_content` with its surplus
        // markers still in the content, where the content-indent blockquote
        // path would consume them a second time. Leave those stacks alone.
        if self.content_container_indent_to_strip() != 0 {
            return None;
        }

        let base = ContainerPrefix::from_stack(
            &self.containers.stack,
            self.dispatch_list_marker_consumed,
            self.config.dialect,
        );
        for k in current_bq_depth.max(1)..bq_depth {
            let prefix = base.with_extra_blockquotes(k - current_bq_depth);
            let stripped = StrippedLines::new(&self.lines, self.pos, &prefix);
            let ctx = self.probe_block_context(k, current_bq_depth, stripped.first());
            if let Some(block_match) = self.block_registry.detect_prepared(&ctx, &stripped)
                && self.block_registry.outranks_blockquote(&block_match)
            {
                return Some(k);
            }
        }
        None
    }

    /// Detect blockquote markers that begin at list-content indentation instead
    /// of column 0 on the physical line.
    fn shifted_blockquote_from_list<'b>(
        &self,
        line: &'b str,
    ) -> Option<(usize, &'b str, &'b str, &'b str)> {
        // Only the innermost `ListItem`'s content_col counts here — content
        // containers (footnotes/definitions) are accounted for separately by
        // `content_container_indent_to_strip`. Mixing them via
        // `paragraphs::current_content_col` (which returns the innermost
        // ListItem-or-FootnoteDef content_col) double-counts the footnote
        // indent for stacks like `[FootnoteDef, BlockQuote, Paragraph]`,
        // pushing `marker_col` past the actual `>` column and stranding
        // continuation-line markers as paragraph text.
        let list_content_col = self
            .containers
            .stack
            .iter()
            .rev()
            .find_map(|c| match c {
                Container::ListItem { content_col, .. } => Some(*content_col),
                _ => None,
            })
            .unwrap_or(0);
        let content_container_indent = self.content_container_indent_to_strip();
        // Don't probe for a "new" blockquote inside a footnote/definition that
        // has no list and no open blockquote — paragraph continuation lines
        // there can legitimately start with `>` (e.g. an angle-link variant
        // `>url>`), and `parse_inner_content` already gates real bq opens
        // via `blank_before_blockquote`. Only fire here when there's an
        // open `BlockQuote` (so we're continuing an existing quote) or a
        // `ListItem` providing the column offset.
        if list_content_col == 0 && self.current_blockquote_depth() == 0 {
            return None;
        }
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

    /// Emit the blank lines a definition-list term look-ahead skipped over.
    ///
    /// The look-ahead runs on container-stripped lines, so inside a blockquote
    /// a "blank" line still carries its `>` markers in the source. Split them
    /// off as `BLOCK_QUOTE_MARKER` tokens the way the main blank-line path
    /// does, instead of burying them in the `BLANK_LINE` token.
    /// Open a definition list whose term is the list item's own first content
    /// line, i.e. the text on the list-marker line.
    ///
    /// `- Term\n  : def\n` is `BulletList [[DefinitionList [(Term, [[Plain
    /// def]])]]]` for pandoc: it reparses item contents as a fresh block sequence,
    /// so the term is found there rather than by the block dispatcher, which never
    /// sees the marker line (`ListParser` claims it first). This is the list
    /// analogue of the footnote branch in `handle_footnote_open_effect`.
    ///
    /// Declines whenever pandoc's reader would reach a block before
    /// `definitionList`: an ATX heading or a thematic break on the marker line
    /// keeps it, and so does more than one buffered line — a term is always a
    /// one-line block. An empty buffer means the item's content went down the
    /// blockquote-dispatch path, which has its own definition handling.
    ///
    /// Returns the number of source lines consumed beyond the marker line.
    fn maybe_open_definition_term_in_new_list_item(&mut self) -> Option<usize> {
        if !self.config.extensions.definition_lists {
            return None;
        }
        let Some(Container::ListItem {
            content_col,
            buffer,
            marker_only,
            ..
        }) = self.containers.stack.last()
        else {
            return None;
        };
        if *marker_only || buffer.segment_count() != 1 {
            return None;
        }
        let content_col = *content_col;
        let text = buffer.first_text()?.to_string();

        // A term is a one-line block; more than one buffered line is a paragraph.
        let mut lines_it = text.split_inclusive('\n');
        let first_line = lines_it.next()?;
        if lines_it.next().is_some() {
            return None;
        }
        let (detect, _) = strip_newline(first_line);
        if detect.trim().is_empty()
            || try_parse_atx_heading(detect).is_some()
            || try_parse_horizontal_rule(detect).is_some()
            || html_blocks::try_parse_html_block_start(detect, false).is_some()
        {
            return None;
        }

        // `content_col` is absolute in the frame left by every container
        // *below* this item, so the lookahead has to strip exactly those —
        // blockquote markers, and any footnote/definition content indent.
        let prefix =
            ContainerPrefix::from_stack(&self.containers.stack, false, self.config.dialect)
                .without_innermost_list_advance();
        let window = StrippedLines::new(&self.lines, self.pos, &prefix);
        let blank_count = first_content_line_term_lookahead(
            &window,
            self.pos,
            content_col,
            self.config.extensions.table_captions,
        )?;

        if let Some(Container::ListItem { buffer, .. }) = self.containers.stack.last_mut() {
            buffer.clear();
        }
        self.builder.start_node(SyntaxKind::DEFINITION_LIST.into());
        self.containers.push(Container::DefinitionList {});
        self.builder.start_node(SyntaxKind::DEFINITION_ITEM.into());
        self.containers.push(Container::DefinitionItem {});
        emit_term(&mut self.builder, &text, self.config);
        self.emit_term_lookahead_blank_lines(blank_count);
        Some(blank_count)
    }

    fn emit_term_lookahead_blank_lines(&mut self, blank_count: usize) {
        let bq_depth = self.current_blockquote_depth();
        for i in 0..blank_count {
            let blank_pos = self.pos + 1 + i;
            if blank_pos >= self.lines.len() {
                continue;
            }
            let blank_line = self.lines[blank_pos];
            let (line_depth, _) = blockquotes::count_blockquote_markers(blank_line);
            let depth = line_depth.min(bq_depth);
            let content = if depth > 0 {
                let marker_info = parse_blockquote_marker_info(blank_line);
                self.emit_blockquote_markers(&marker_info, depth);
                strip_n_blockquote_markers(blank_line, depth)
            } else {
                blank_line
            };
            self.builder.start_node(SyntaxKind::BLANK_LINE.into());
            self.builder.token(SyntaxKind::BLANK_LINE.into(), content);
            self.builder.finish_node();
        }
    }

    fn current_blockquote_depth(&self) -> usize {
        blockquotes::current_blockquote_depth(&self.containers)
    }

    /// Whether pandoc's blockquote gobble stops at this line instead of
    /// folding it into the quote.
    ///
    /// `emailBlockQuote` keeps eating lines through the reader's `endline`
    /// guards, and those guards run against the raw next line — *before* the
    /// leading whitespace the fold otherwise skips. So each guard here is
    /// anchored at byte 0 even where the construct itself tolerates up to
    /// three spaces of indent, which is why `> para` / `# head` ends the
    /// quote under `-blank_before_header` while `> para` / ` # head` does
    /// not.
    ///
    /// `notFollowedBy emailBlockQuoteStart` (the `-blank_before_blockquote`
    /// guard) has no counterpart below: a line whose markers this branch has
    /// not already consumed cannot start with `>`.
    fn blockquote_gobble_ends_at(&self, line: &str, inner_content: &str) -> bool {
        // `notFollowedBy (inList >> listStart)`: a list start ends the gobble
        // only while the quote is itself being read inside a list item — on
        // the stack, a ListItem *below* the innermost quote. A list opened
        // inside the quote is quote content, so `> - a` / `- b` is one list.
        // `Ext_lists_without_preceding_blankline` drops the `inList` half.
        let quote_is_in_list_item = self
            .containers
            .stack
            .iter()
            .rposition(|c| matches!(c, Container::BlockQuote { .. }))
            .is_some_and(|quote| {
                self.containers.stack[..quote]
                    .iter()
                    .any(|c| matches!(c, Container::ListItem { .. }))
            });
        if (self.config.extensions.lists_without_preceding_blankline || quote_is_in_list_item)
            && try_parse_list_marker(
                inner_content,
                self.config,
                lists::open_list_hint_at_indent(&self.containers, leading_indent(inner_content).0),
            )
            .is_some()
        {
            return true;
        }

        // `guardEnabled Ext_blank_before_header <|> (notFollowedBy . char =<< atxChar)`.
        if !self.config.extensions.blank_before_header && inner_content.starts_with('#') {
            return true;
        }

        // `guardDisabled Ext_backtick_code_blocks <|> notFollowedBy
        // (lookAhead (char '`') >> codeBlockFenced)` — backtick-anchored, so
        // a `~~~` fence on the lazy line, or an indented backtick one, is
        // gobbled into the quote instead of ending it.
        if self.config.extensions.backtick_code_blocks
            && inner_content.starts_with('`')
            && code_blocks::try_parse_fence_open(inner_content, self.config.dialect).is_some()
        {
            return true;
        }

        self.div_closer_ends_blockquote(line)
    }

    /// Whether a fenced-div closing fence ends the open blockquote rather
    /// than closing a div inside it.
    ///
    /// Pandoc extracts a quote's raw content from wherever the quote starts,
    /// so a `:::` line is quote content only when the div it closes was
    /// opened inside the quote. A div opened *outside* it is still open at
    /// extraction time and its closer ends the quote: `::: a` / `> text` /
    /// `:::` closes the div at the top level and leaves the quote holding
    /// just the paragraph (issue #310). `> ::: a` / `> x` / `:::` is the
    /// other way round — the div lives in the quote, so its closer does too.
    fn div_closer_ends_blockquote(&self, line: &str) -> bool {
        if !self.config.extensions.fenced_divs || !fenced_divs::is_div_closing_fence(line) {
            return false;
        }
        let innermost_div = self
            .containers
            .stack
            .iter()
            .rposition(|c| matches!(c, Container::FencedDiv { .. }));
        let innermost_quote = self
            .containers
            .stack
            .iter()
            .rposition(|c| matches!(c, Container::BlockQuote { .. }));
        match (innermost_div, innermost_quote) {
            (Some(div), Some(quote)) => div < quote,
            (Some(_), None) => true,
            (None, _) => false,
        }
    }

    /// Fold a lazy line back into the open blockquote, pandoc-style.
    ///
    /// Emits the `>` markers the line carries, then the leading whitespace
    /// pandoc's gobble skips as a bare `WHITESPACE` token, and parses what is
    /// left one level down. Dropping the indent before the inner parse is the
    /// point: the quote's raw content never sees it, so it can neither
    /// continue a line block nor open an indented code block.
    fn fold_lazy_line_into_blockquote(
        &mut self,
        fold: LazyFold<'a>,
        blockquote_payload: Option<&BlockQuotePrepared>,
    ) -> LineDispatch {
        // The gates above already ruled this line out as a continuation of the
        // open paragraph, so close it — inside the quote, not around it.
        if matches!(self.containers.last(), Some(Container::Paragraph { .. })) {
            self.close_containers_to(self.containers.depth() - 1);
        }

        if fold.bq_depth > 0 {
            let marker_info = self.marker_info_for_line(
                blockquote_payload,
                fold.line,
                fold.bq_marker_line,
                fold.shifted_bq_prefix,
                fold.used_shifted_bq,
            );
            for i in 0..fold.bq_depth {
                if let Some(info) = marker_info.get(i) {
                    self.emit_or_buffer_blockquote_marker(
                        info.leading_spaces,
                        info.has_trailing_space,
                    );
                }
            }
        }

        let rest = fold.inner_content.trim_start_matches([' ', '\t']);
        let indent = &fold.inner_content[..fold.inner_content.len() - rest.len()];
        if matches!(self.containers.last(), Some(Container::ListItem { .. })) {
            // A list item still buffering its first line would otherwise take
            // the indent bytes out of source order; flush it the way a new
            // block inside the item does. This runs first so the item's
            // Plain-vs-Para choice still comes from `ListItemBuffer`.
            self.emit_list_item_buffer_if_needed();
            // The gobble has already dropped this line's indent, so from the
            // quote's view the fence is at column 0 and every open item sits
            // below it — pandoc's `rawListItem` stops there. Closing here
            // rather than in `parse_inner_content` is forced, not cosmetic: a
            // `~~~` fence only detects as a block when `has_blank_before`,
            // which is true for a `BlockQuote` on top of the stack but false
            // for a `ListItem`.
            if self.lazy_content_opens_fence(rest).is_some() {
                self.close_lists_above_indent(0);
            }
        }
        if !indent.is_empty() {
            self.builder.token(SyntaxKind::WHITESPACE.into(), indent);
        }

        // `parse_inner_content` hands the dispatcher `rest`, but the block
        // parsers that need a multi-line view build it from `self.lines`
        // through the container prefix, which knows nothing about a fold. Put
        // the gobbled line back so both views agree — otherwise `> # h` /
        // `    indented` reaches the window still indented and is claimed as
        // an indented code block whose content the dispatcher never emits.
        // The bytes taken out are already in the tree as the marker and
        // whitespace tokens above.
        self.lines[self.pos] = rest;

        self.parse_inner_content(rest, Some(rest))
    }

    /// Look up the immediate enclosing `Container::ListItem`'s buffer for an
    /// unclosed Pandoc matched-pair HTML open tag. See
    /// [`crate::parser::utils::list_item_buffer::ListItemBuffer::unclosed_pandoc_matched_pair_tag`]
    /// for the gate; used to populate
    /// `BlockContext::list_item_unclosed_html_block_tag` so the dispatcher
    /// can suppress the close-form match that would otherwise interrupt
    /// `- <div>\n  body\n  </div>` and friends.
    fn list_item_unclosed_html_block_tag(&self) -> Option<String> {
        let Container::ListItem { buffer, .. } = self.containers.stack.last()? else {
            return None;
        };
        buffer.unclosed_pandoc_matched_pair_tag(self.config)
    }

    /// Backtick runs the innermost buffering container is still waiting on a
    /// closer for, to populate `BlockContext::open_code_span_openers`.
    ///
    /// Gated on the current line opening with a backtick (after the container
    /// prefix) because that is the only line that can close such a run, and the
    /// lookup itself costs a scan of the whole buffer.
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

            match self.parse_line(line) {
                LineDispatch::Consumed(n) => self.pos += n,
                LineDispatch::Rejected => self.pos += 1,
            }
        }

        self.close_containers_to(0);
        self.builder.finish_node(); // DOCUMENT
    }

    /// Dispatch a single source line. Returns `LineDispatch::Consumed(n)`
    /// when the line was claimed and `n` lines should be committed, or
    /// `LineDispatch::Rejected` for the outer loop to advance by 1.
    fn parse_line(&mut self, line: &'a str) -> LineDispatch {
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

        // Handle blank lines specially (including blank lines inside blockquotes)
        // A line like ">" with nothing after is a blank line inside a blockquote —
        // but only when we're already inside one (or one can legitimately start
        // here under the active blank_before_blockquote rule). Otherwise treating
        // it as blank would silently open a blockquote mid-paragraph, diverging
        // from pandoc which keeps the whole thing as one paragraph.
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

            // Close paragraph if open
            self.close_paragraph_if_open();

            // Close Plain node in Definition if open
            // Blank lines should close Plain, allowing subsequent content to be siblings
            // Emit buffered PLAIN content before continuing.
            //
            // Unless a definition marker is waiting just past this blank line:
            // a blank line does not detach a term from its definition, so the
            // one buffered line is a term and must be promoted *here*, while
            // it is still buffered. Once flushed it is a `PLAIN` in the
            // builder, which cannot be retagged.
            if self.blank_line_promotes_buffered_definition_term() {
                self.promote_buffered_definition_term();
            }
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

            return LineDispatch::consumed(1);
        }

        // The registry ran on this raw line and claimed it for something that
        // is not a blockquote. Its order mirrors pandoc's reader order, so
        // that verdict outranks the raw `>` count: pandoc reads `> a\n---\n`
        // as `Header 2 [Str ">", Space, Str "a"]`, no blockquote at all,
        // because `setextHeader` runs before `blockQuote`. Opening a
        // blockquote off the marker count here would override the verdict and
        // then re-dispatch the stripped content, where the same parser matches
        // a second time and re-emits the marker run it already consumed — the
        // bytes are duplicated. Hand the raw line to the dispatcher instead.
        //
        // Dialect differences belong to the parsers, not here: under
        // CommonMark a setext underline never reaches across a container
        // boundary (spec examples #92, #101, #234), and
        // `SetextHeadingParser::detect_prepared` declines for that reason, so
        // the blockquote parser takes the line and this branch never fires.
        //
        // The shifted-blockquote case is excluded because the verdict is not
        // trustworthy there: `dispatcher_ctx` carries no `list_indent_info`,
        // so a `>` at a list's content column looks like an indented code
        // block to the registry rather than a quote inside the item.
        if registry_claims_raw_line && bq_depth > 0 && !used_shifted_bq {
            return self.parse_inner_content(line, None);
        }

        // Same rule as the hatch above, generalized past depth 0: pandoc
        // strips one `>` per line and re-parses, so a higher-ranked parser
        // can claim the line at any depth this one would pass through, not
        // just at the depth the stack currently sits at. Cap the open here
        // and the surplus markers stay in `inner_content`, where the
        // claiming parser re-emits them as its own bytes.
        if bq_depth > current_bq_depth
            && !used_shifted_bq
            && let Some(cap) = self.blockquote_depth_cap(current_bq_depth, bq_depth)
        {
            bq_depth = cap;
            inner_content = blockquotes::strip_n_blockquote_markers(line, cap);
            // The dispatcher fork below emits from `blockquote_match`'s own
            // boxed payload, which still carries the uncapped depth, and
            // pushes one container per level of it. Force the manual path.
            blockquote_match = None;
            if let Some(payload) = blockquote_payload.as_mut() {
                payload.depth = cap;
                // The only depth-dependent term of the nesting gate.
                payload.can_nest |= cap <= 1;
            }
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
                    .unwrap_or_else(|| {
                        blockquotes::can_start_blockquote(
                            self.pos,
                            &self.lines,
                            self.config.extensions.fenced_divs,
                        )
                    })
            {
                // Can't start blockquote without blank line - treat as paragraph
                // text. When a list item's content is still buffered, that
                // buffer is the open block, so the line is lazy continuation of
                // it: pandoc reads `- a` / `  > q` as a single
                // `Plain [Str "a", SoftBreak, Str ">", Space, Str "q"]`.
                // Flushing the buffer here would emit the item's text as one
                // block and this line as a sibling.
                if self.is_list_item_content_open() {
                    self.append_lazy_continuation_line(line);
                    return LineDispatch::consumed(1);
                }
                // Otherwise flush any pending list-item inline buffer first so
                // this line stays in source order relative to buffered list text.
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
                    return LineDispatch::consumed(1);
                } else {
                    // Start new paragraph with the extra > as content
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

            // Now parse the inner content. When the bq was a "shifted" one
            // (detected at the list content column inside a list), the
            // bq marker emission above absorbed the outer list-indent
            // bytes (the cols BEFORE the `>`). If the innermost ListItem
            // in the stack sits *below* the BlockQuote we just opened
            // (i.e. there's no inner LI above the BQ), its content_col
            // IS the outer list-indent that was upstream-emitted, so
            // line 0's ListAdvance must be applied — toggle the flag.
            // When an inner LI sits *above* the BQ on the stack, the
            // innermost LA represents inner list-indent that wasn't
            // emitted by the bq marker, so leave the flag false.
            // Pass inner_content as line_to_append since markers are already stripped
            let prev_flag = self.dispatch_list_marker_consumed;
            if used_shifted_bq && !self.innermost_li_above_bq() {
                self.dispatch_list_marker_consumed = true;
            }
            let dispatch = self.parse_inner_content(inner_content, Some(inner_content));
            self.dispatch_list_marker_consumed = prev_flag;
            return dispatch;
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
                // The interrupt checks run on `inner_content` (markers
                // stripped; identical to `line` for zero-marker lines): a
                // reduced-marker line like `> # head` under a depth-2 quote
                // is not lazy at its own level, so the stripped content
                // decides the interruption (issue #429).
                let is_commonmark = self.config.dialect == crate::options::Dialect::CommonMark;
                let interrupts_via_hr =
                    is_commonmark && try_parse_horizontal_rule(inner_content).is_some();
                // Under Pandoc this is the `endline` guard
                // `notFollowedBy (lookAhead (char '`') >> codeBlockFenced)`.
                // The quote's content is de-indented before it runs, so the
                // guard sees the *folded* line: `> a` / `   ``` ` breaks the
                // paragraph even though the source line is indented. It is
                // anchored on a literal backtick, so `   ~~~` keeps continuing
                // the paragraph — unlike the list-item gate below, where any
                // fence ends the item.
                //
                // Separate from `blockquote_gobble_ends_at`, which asks
                // whether the *quote* ends and reads the raw line at byte 0.
                let interrupts_via_fence = if is_commonmark {
                    code_blocks::try_parse_fence_open(inner_content, self.config.dialect).is_some()
                } else {
                    self.lazy_content_opens_fence(inner_content.trim_start_matches([' ', '\t']))
                        .is_some_and(|fence| fence.fence_char == '`')
                };
                // An ATX heading interrupts a paragraph under CommonMark §4.2,
                // and under Pandoc when `blank_before_header` is disabled
                // (`markdown-blank_before_header`) — the same predicate as
                // `can_interrupt` in `AtxHeadingParser::detect_prepared`. A
                // heading-shaped lazy line then ends the paragraph rather
                // than being swallowed as its text (issue #428).
                //
                // This is a separate question from whether the *quote* ends:
                // CommonMark asks it of the line as written, Pandoc of the
                // line the fold below hands to the quote, indent dropped. So
                // under `-blank_before_header` both `# head` and ` # head`
                // end the paragraph, but only the unindented one also ends
                // the quote (`blockquote_gobble_ends_at`).
                let heading_can_interrupt =
                    is_commonmark || !self.config.extensions.blank_before_header;
                let heading_probe = if is_commonmark {
                    inner_content
                } else {
                    inner_content.trim_start_matches([' ', '\t'])
                };
                let interrupts_via_heading =
                    heading_can_interrupt && try_parse_atx_heading(heading_probe).is_some();
                // A fenced-div closing fence terminates the paragraph rather
                // than being swallowed as lazy text — but only while we're
                // actually inside an open div. At the top level a lone `:::`
                // is just text, which is what pandoc does (issue #310). Note
                // this says nothing about the *quote*: a div opened inside it
                // closes inside it, which the fold below works out.
                // This one stays on the raw `line`: the #310 shape was
                // calibrated against zero-marker lines and the reduced-marker
                // form is unverified against pandoc.
                let interrupts_via_div_close = self.config.extensions.fenced_divs
                    && self.in_fenced_div()
                    && fenced_divs::is_div_closing_fence(line);
                // Under Pandoc the rest of the question is the one the fold
                // below asks: does the reader's gobble stop here? An ATX
                // heading with `blank_before_header` off and a backtick code
                // fence both end it, and neither is lazy paragraph text.
                let ends_gobble =
                    !is_commonmark && self.blockquote_gobble_ends_at(line, inner_content);
                if !interrupts_via_hr
                    && !interrupts_via_fence
                    && !interrupts_via_heading
                    && !interrupts_via_div_close
                    && !ends_gobble
                {
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
                    return LineDispatch::consumed(1);
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
                && try_parse_list_marker(
                    line,
                    self.config,
                    lists::open_list_hint_at_indent(&self.containers, leading_indent(line).0),
                )
                .is_none()
            {
                // Same interrupt rules as the paragraph gate above, including
                // the `inner_content` check for reduced-marker lines (issues
                // #428, #429); see `AtxHeadingParser::detect_prepared`.
                let is_commonmark = self.config.dialect == crate::options::Dialect::CommonMark;
                let interrupts_via_hr =
                    is_commonmark && try_parse_horizontal_rule(inner_content).is_some();
                // Pandoc's `rawListItem` stops collecting at a line that opens
                // a fenced code block, so the fence ends the item instead of
                // becoming its text. Unlike the paragraph guard above this is
                // fence-char agnostic — `> - a` / `   ~~~` is a `CodeBlock`
                // sibling of the list, while `> a` / `   ~~~` is not.
                let interrupts_via_fence = if is_commonmark {
                    code_blocks::try_parse_fence_open(inner_content, self.config.dialect).is_some()
                } else {
                    self.lazy_content_opens_fence(inner_content.trim_start_matches([' ', '\t']))
                        .is_some()
                };
                let heading_can_interrupt =
                    is_commonmark || !self.config.extensions.blank_before_header;
                let heading_probe = if is_commonmark {
                    inner_content
                } else {
                    inner_content.trim_start_matches([' ', '\t'])
                };
                let interrupts_via_heading =
                    heading_can_interrupt && try_parse_atx_heading(heading_probe).is_some();
                let ends_gobble =
                    !is_commonmark && self.blockquote_gobble_ends_at(line, inner_content);
                if !interrupts_via_hr
                    && !interrupts_via_fence
                    && !interrupts_via_heading
                    && !ends_gobble
                {
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
            // Lazy continuation of a definition list open inside the quote.
            // Pandoc's blockquote reader folds lazy lines into the quote's raw
            // content before parsing blocks, so `> a` / `: b` is one
            // `BlockQuote [DefinitionList ...]`, not a quote followed by a
            // top-level paragraph. The open DEFINITION_LIST is the analogue of
            // an open Paragraph for the gate above: a `:` line belongs to it
            // whether or not it repeats the `>` markers.
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
            // CommonMark §5.1: a no-`>` line that begins a list marker
            // closes the blockquote and starts a fresh list at the outer
            // level rather than continuing the inner list. Pandoc keeps
            // the inner list going (lazy list continuation across
            // blockquote depth).
            if bq_depth == 0 && self.config.dialect != crate::options::Dialect::CommonMark {
                // Check for lazy list continuation - if we're in a list item and
                // this line looks like a list item with matching marker
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

            // General pandoc blockquote laziness. Pandoc's reader gobbles every
            // non-blank line into the quote's raw content and only stops at a
            // blank line, so whatever block is open inside, the line stays in
            // the quote: `> # h` / ` b |` is one quote holding a `Header` and a
            // `Para`, not a quote followed by a top-level paragraph. The gates
            // above are the cases where the line joins an *open* block; this is
            // the rest, where it opens a new block one level down.
            //
            // The gobble skips the lazy line's leading whitespace, so the
            // indentation is gone before the inner parse sees it: an
            // under-indented line cannot continue a line block, and four
            // leading spaces are not an indented code block. Only the bytes
            // survive, as a `WHITESPACE` token inside the quote.
            //
            // CommonMark laziness is paragraph-only (spec §5.1), so this is
            // dialect-gated, not extension-gated.
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
            if let Some(marker_match) = try_parse_list_marker(
                inner_content,
                self.config,
                lists::open_list_hint_at_indent(&self.containers, inner_indent_cols_raw),
            ) {
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
            // See the "new-depth shifted-bq" path above for the rationale.
            // Only set the flag when the innermost LI sits below the BQ
            // on the stack — its cols are then the ones the bq marker
            // emission absorbed; otherwise the innermost LA represents
            // inner-list indent that wasn't upstream-emitted.
            let prev_flag = self.dispatch_list_marker_consumed;
            if used_shifted_bq && !self.innermost_li_above_bq() {
                self.dispatch_list_marker_consumed = true;
            }
            let dispatch = self.parse_inner_content(inner_content, line_to_append);
            self.dispatch_list_marker_consumed = prev_flag;
            return dispatch;
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
                return LineDispatch::consumed(1);
            }

            // Check for lazy list continuation
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

        // No blockquote markers - use original line
        self.parse_inner_content(line, None)
    }

    /// Close open admonition containers that the current (non-blank) line is
    /// no longer indented into. python-markdown / pymdownx end an admonition
    /// at the first non-indented line; footnotes (lazy continuation) don't, so
    /// this is admonition-specific.
    ///
    /// Conservative: skipped when a `ListItem` is on the stack, since
    /// list-item indentation isn't a content-container strip and would make
    /// the cumulative threshold below incorrect.
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
    fn close_dedented_definition_lists(&mut self, content: &str) {
        if !self.config.extensions.definition_lists
            || self
                .containers
                .stack
                .iter()
                .filter(|c| matches!(c, Container::DefinitionList { .. }))
                .count()
                < 2
        {
            return;
        }

        let (without_newline, _) = strip_newline(content);
        let (indent_cols, _) = leading_indent(without_newline);

        // The column each open definition list is read at: the content column
        // of the body holding it. A list item's column is already cumulative
        // within its enclosing content container, so it replaces rather than
        // adds to the item column (mirroring `ContainerStack::gobble_chain`).
        let mut content_frame = 0usize;
        let mut item_frame = 0usize;
        let mut levels: Vec<(usize, usize)> = Vec::new();
        for (idx, container) in self.containers.stack.iter().enumerate() {
            match container {
                Container::FootnoteDefinition { content_col }
                | Container::Admonition { content_col }
                | Container::Definition { content_col, .. } => {
                    content_frame += *content_col;
                    item_frame = 0;
                }
                Container::ListItem { content_col, .. } => item_frame = *content_col,
                Container::DefinitionList { .. } => levels.push((idx, content_frame + item_frame)),
                _ => {}
            }
        }

        // The marker belongs to the innermost level it both reaches and stays
        // within the 0-3 space allowance of, read in that level's own frame.
        let target = levels.iter().rposition(|(_, frame)| {
            indent_cols >= *frame
                && definition_lists::try_parse_definition_marker(
                    &without_newline[byte_index_at_column(without_newline, *frame)..],
                )
                .is_some()
        });

        if let Some(target) = target
            && let Some((first_closed, _)) = levels.get(target + 1)
        {
            self.close_containers_to(*first_closed);
        }
    }

    /// Get the total indentation to strip from content containers
    /// (footnotes + definitions). Delegates to
    /// [`ContainerStack::content_container_indent`], where the
    /// `content_col` convention is documented.
    fn content_container_indent_to_strip(&self) -> usize {
        self.containers.content_container_indent()
    }

    /// Walk the container stack from top (innermost) toward bottom and
    /// return `true` iff a `ListItem` is encountered before a
    /// `BlockQuote`. Used by the shifted-bq dispatch in `parse_line` to
    /// decide whether the innermost `ListAdvance` op corresponds to
    /// outer-list-indent already absorbed by the bq marker emission,
    /// or to inner-list-indent that is still part of the line's content.
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

    /// Parse content inside blockquotes (or at top level).
    ///
    /// `content` - The content to parse (may have indent/markers stripped)
    /// `line_to_append` - Optional line to use when appending to paragraphs.
    ///                    If None, uses self.lines[self.pos]
    fn parse_inner_content(&mut self, content: &str, line_to_append: Option<&str>) -> LineDispatch {
        log::trace!(
            "parse_inner_content [{}]: depth={}, last={:?}, content={:?}",
            self.pos,
            self.containers.depth(),
            self.containers.last(),
            content.trim_end()
        );
        // Admonitions end at the first non-indented line (unlike footnotes,
        // which allow lazy continuation). Close any open admonition whose
        // content indent the current line no longer meets, before stripping.
        self.close_dedented_admonitions(content);

        // A definition marker dedented out of a nested definition list belongs
        // to an outer one, so unwind to that level before the strip below is
        // measured from the levels this line is actually in.
        self.close_dedented_definition_lists(content);

        // Calculate how much indentation should be stripped for content containers
        // (definitions, footnotes) FIRST, so we can check for block markers correctly.
        // Shared helper mirrors `ContainerPrefix::strip` (post-bq path) so the
        // dispatcher's `StrippedLines::first()` and `ctx.content` agree.
        let content_indent = self.content_container_indent_to_strip();
        let (stripped_content, indent_to_emit) = strip_content_indent(content, content_indent);

        // Pandoc re-reads a content container's body from its content column:
        // `noteBlock` strips `indentSpaces` (4) off every continuation line of
        // a footnote definition before parsing the body, the same rule
        // `listLine` applies inside a list item and `defListIndent` inside a
        // definition body. Hold those bytes out of the text handed to the
        // inline parser so a construct that preserves interior whitespace
        // (a code span, inline math) measures from the content column instead
        // of from column 0; they are spliced back as `WHITESPACE` at emission,
        // so the parse stays byte-lossless.
        //
        // A *lazy* line never reaches the content column and pandoc takes
        // nothing off it, so its whitespace stays payload. A tab straddling
        // the content column has no byte boundary to split on, so it stays in
        // the payload whole and the projector subtracts its gobbled columns
        // by column instead.
        let paragraph_held = {
            let append_line = line_to_append.unwrap_or(self.lines[self.pos]);
            if content_indent > 0 && leading_indent(content).0 >= content_indent {
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

        // A definition marker that reaches the body's own content column
        // cannot continue the block above it, so it either ends that block or
        // defines it. Settling that here — before the dispatcher gets a look
        // and its `Definition` arm claims the marker as a second definition of
        // the outer term — is what keeps the line inside this body.
        let body_block =
            self.definition_marker_over_open_body_block(content, stripped_content, content_indent);
        if body_block == Some(BufferedBodyBlock::Term) {
            self.promote_buffered_definition_term();
        }
        // Flushing the buffered PLAIN makes the marker line the body's next
        // block rather than a second definition.
        let definition_block_breaks = body_block == Some(BufferedBodyBlock::Block);
        if definition_block_breaks {
            self.emit_buffered_plain_if_needed();
        }

        // Check if we're in a Definition container (with or without an open PLAIN)
        // Continuation lines should be added to PLAIN, not treated as new blocks
        // BUT: Don't treat lines with block element markers as continuations
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
                // Definition markers at top-level should start a new definition.
            } else {
                let policy = ContinuationPolicy::new(self.config, &self.block_registry);

                if definition_block_breaks
                    || policy.definition_plain_can_continue(
                        stripped_content,
                        content,
                        content_indent,
                        &BlockContext {
                            has_blank_before: self.pos == 0
                                || is_blank_line(self.lines[self.pos - 1]),
                            has_blank_before_strict: self.pos == 0
                                || is_blank_line(self.lines[self.pos - 1]),
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

                    // Pandoc re-reads a definition body from its content
                    // column — `defListIndent` gobbles those columns off every
                    // continuation line before the body is parsed, the same
                    // rule `listLine` applies inside a list item. Hold them out
                    // of the text handed to the inline parser so a construct
                    // that preserves interior whitespace measures from the
                    // content column instead of from column 0; they are
                    // spliced back as `WHITESPACE` at emission, so the parse
                    // stays byte-lossless.
                    //
                    // A *lazy* line never reaches the content column and pandoc
                    // takes nothing off it, so its whitespace stays payload.
                    let reaches_content_col =
                        content_indent > 0 && leading_indent(content).0 >= content_indent;
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
                        // A tab straddling the content column has no byte
                        // boundary to split on, so it stays in the payload and
                        // the projector subtracts its gobbled columns instead.
                        plain_buffer.push_text(&indent_prefix[held..]);
                        plain_buffer.push_text(text_without_newline);
                        plain_buffer.push_text(newline_str);
                        *plain_open = true;
                    }

                    return LineDispatch::consumed(1);
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
                    && !blockquotes::can_start_blockquote(
                        self.pos,
                        &self.lines,
                        self.config.extensions.fenced_divs,
                    )
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

                    if bq_depth < current_bq_depth {
                        self.close_blockquotes_to_depth(bq_depth);
                    } else {
                        let marker_info = parse_blockquote_marker_info(stripped_content);

                        if bq_depth > current_bq_depth {
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
                        } else {
                            // Same depth: emit markers for losslessness.
                            self.emit_blockquote_markers(&marker_info, bq_depth);
                        }
                    }

                    return self.parse_inner_content(inner_content, Some(inner_content));
                }
            }
        }

        // Later-line HTML block inside a content-container body (definition,
        // footnote, admonition). The general block dispatcher's
        // `parse_html_block_with_wrapper` ignores the `ContentIndent` prefix
        // op, so it silently drops the stripped content indent (losslessness
        // fail) and reparses the body with its indent intact (an indented
        // `CodeBlock` instead of markdown). Route the block through the
        // content-indent-normalizing lift before the general dispatch reaches
        // it. Gated Pandoc + top-level blockquote depth 0 (the lift hardcodes
        // the Pandoc HTML grammar and doesn't strip `> ` markers); CommonMark
        // keeps the opaque shape.
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

        // Blockquote-nested variant of the above: the content-container body
        // sits inside one or more blockquotes, so continuation lines carry
        // `> ` markers on top of the content indent. Handled separately
        // because the plain lift strips only spaces.
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

        // Store the stripped content for later use
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

        // List-item analogue of the paragraph hold above: while the item's
        // buffered lines leave a display-math region open, keep buffering so
        // block detection (e.g. `\begin{...}` -> TEX_BLOCK) cannot split the
        // region. Exception: a sibling list marker of an open list still
        // interrupts — pandoc splits items before scanning math — while
        // indented nested markers and dedented lazy text are math content.
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
        };

        // We'll update these two fields shortly (after they are computed), but we can still
        // use this ctx shape to avoid rebuilding repeated context objects.
        let mut dispatcher_ctx = dispatcher_ctx;

        // Build a stack-aware prefix once; reused across the
        // dispatcher's multiple detect_prepared calls below. The
        // `list_marker_consumed_on_line_0` flag is sourced directly from
        // the parser's `dispatch_list_marker_consumed` field — it never
        // lived on `BlockContext` after the trait migration since no
        // `BlockParser` impl reads it.
        let dispatcher_prefix = ContainerPrefix::from_stack(
            &self.containers.stack,
            self.dispatch_list_marker_consumed,
            self.config.dialect,
        );

        // Setext heading folded over a list item's buffered first-line text.
        // Must run before block detection so that an HR-shaped underline like
        // `---` doesn't get claimed by the thematic-break parser.
        if let Some(dispatch) = self.try_fold_list_item_buffer_into_setext(stripped_content) {
            return dispatch;
        }

        // Initial detection (before blank/doc-start are computed). Note: this can
        // match reference definitions, but footnotes are handled explicitly later.
        let dispatcher_match = {
            let stripped = StrippedLines::new(&self.lines, self.pos, &dispatcher_prefix);
            self.block_registry
                .detect_prepared(&dispatcher_ctx, &stripped)
        };

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
                    BlockEffect::OpenBlockQuote => {
                        // Detection only for now; keep core blockquote handling intact.
                        0
                    }
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
                        // CommonMark §5.2: bullet lists and ordered lists with
                        // start = 1 may interrupt a paragraph; ordered lists
                        // with any other start cannot. Pandoc-markdown forbids
                        // *any* list from interrupting a paragraph without a
                        // blank line. Footnote-definition bodies are also
                        // strict in pandoc-native: even `1.` is treated as
                        // paragraph text, not a sublist (verified via
                        // `pandoc -f markdown -t native`).
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

                    // CommonMark spec example #312: a "list marker" at indent
                    // ≥ 4 isn't actually a marker when it can't reach the
                    // deepest item's content column AND no list level matches
                    // at that indent. Treat as lazy paragraph continuation of
                    // the deepest open list item or paragraph rather than
                    // flushing the buffer and opening a new sibling list.
                    if matches!(block_match.effect, BlockEffect::OpenList)
                        && self.try_lazy_list_continuation(block_match, content)
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

                    // CommonMark §5.2: a thematic break / ATX heading /
                    // fenced code at column 0 cannot continue an open list
                    // item whose content column is greater than the line's
                    // indent — close the surrounding list before emitting.
                    // OpenList is excluded so that a same-level marker still
                    // continues the list rather than closing it.
                    //
                    // Pandoc closes on a *fence only*. `rawListItem` stops
                    // collecting at a line `codeBlockFenced` would claim, so
                    // `- a` / ```` ```rust ```` is `BulletList [[Plain a]]`
                    // plus a sibling `CodeBlock`; but a heading or thematic
                    // break under the content column is lazy item text, which
                    // is why the CommonMark `!OpenList` predicate is too wide
                    // here. No closer check is needed: `detect_prepared`
                    // already declines a closer-less fence under Pandoc, so
                    // one never reaches this arm.
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
                    // CommonMark multi-line setext: when an open paragraph is
                    // followed by a setext underline, the entire paragraph
                    // becomes the heading content. The dispatcher reports
                    // setext at the line *before* the underline (the last text
                    // line); fold the buffered paragraph + this line into a
                    // single HEADING. Pandoc-markdown disagrees (it never
                    // forms a setext heading mid-paragraph, even with
                    // `blank_before_header` disabled), so this branch is
                    // dialect-gated; under Pandoc the unconditional
                    // blank-before gate in `SetextHeadingParser::detect_prepared`
                    // keeps the detection from reaching this point.
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

                    // Keep ambiguous fenced-div openers from interrupting an
                    // active paragraph — or a list item's still-buffered
                    // content — without a blank line. Pandoc folds
                    // `1. a\n|\n::: note\n` into the item's `Plain`.
                    if parser_name == "fenced_div_open"
                        && (self.is_paragraph_open() || self.is_list_item_content_open())
                    {
                        self.append_lazy_continuation_line(
                            line_to_append.unwrap_or(self.lines[self.pos]),
                        );
                        return LineDispatch::consumed(1);
                    }

                    // Reference definitions cannot interrupt a paragraph
                    // (CommonMark §4.7 / Pandoc-markdown agree), nor a list
                    // item's still-buffered content — pandoc folds
                    // `- a\n[x]: /url\n` into the item's `Plain`, and emitting
                    // the definition here would put it *before* the buffered
                    // `a` in the CST.
                    if parser_name == "reference_definition"
                        && (self.is_paragraph_open() || self.is_list_item_content_open())
                    {
                        self.append_lazy_continuation_line(
                            line_to_append.unwrap_or(self.lines[self.pos]),
                        );
                        return LineDispatch::consumed(1);
                    }

                    // Contract: a `Yes` detection in this no-blank-before
                    // branch must not reach emission while a paragraph or a
                    // list item's buffered content is open — the fall-through
                    // below emits the block before those buffered bytes,
                    // silently reordering the CST. Detectors must gate
                    // themselves (return `None` to stay paragraph text, like
                    // setext under Pandoc) or return `YesCanInterrupt` (which
                    // flushes the buffers first), or be special-cased above.
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
                    BlockEffect::OpenBlockQuote => {
                        // Detection only for now; keep core blockquote handling intact.
                        0
                    }
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

            // Legacy fallback path: dispatcher-based `LineBlockParser` handles
            // nesting (list+blockquote container prefixes); this fallback runs
            // only when the dispatcher rejected the line and the raw source
            // line is itself a top-level line-block start (see guard above),
            // so threading zero container params is correct here.
            let prefix = ContainerPrefix::default();
            let window = StrippedLines::new(&self.lines, self.pos, &prefix);
            let new_pos = parse_line_block(&window, &mut self.builder, self.config);
            if new_pos > self.pos {
                return LineDispatch::consumed(new_pos - self.pos);
            }
        }

        // A definition marker cannot be swallowed as continuation text of the
        // item content above it; flush that content first so this line starts
        // its own block, and drop the containers its indent no longer reaches.
        if self.definition_marker_breaks_open_list_item_block(content) {
            self.emit_list_item_buffer_if_needed();
            self.close_lists_above_indent(leading_indent(content).0);
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
        // Not in list item - create paragraph as usual
        paragraphs::start_paragraph_if_needed(&mut self.containers, &mut self.builder);
        // For lossless parsing: use line_to_append if provided (e.g., for blockquotes
        // where markers have been stripped), otherwise use the original line
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

    /// Push a `Container::FencedDiv`, recovering the opener's indent from the
    /// prepared payload so `FencedDivCloseParser` can reject a closer more
    /// indented than the opener.
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

    /// Indent (columns) of the innermost open fenced div's opening fence, or
    /// `None` when not inside a fenced div. Consulted by `FencedDivCloseParser`.
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

    /// Close any containers nested inside the innermost MyST directive, leaving
    /// the directive itself open so its closing fence is emitted as its child.
    fn close_containers_to_myst_directive(&mut self) {
        if let Some(index) = self.myst_directive_container_index() {
            self.close_containers_to(index + 1);
        }
    }

    /// Close (pop) the innermost MyST directive container, finishing its node.
    fn close_myst_directive(&mut self) {
        if let Some(index) = self.myst_directive_container_index() {
            self.close_containers_to(index);
        }
    }

    /// Push a `Container::MystDirective`, recovering the opener's fence
    /// character and count from the prepared payload so the matching closer can
    /// be recognized.
    fn push_myst_directive_container(&mut self, block_match: &PreparedBlockMatch) {
        use crate::parser::blocks::myst_directives::DirectiveOpen;
        let open = block_match
            .payload
            .as_ref()
            .and_then(|p| p.downcast_ref::<DirectiveOpen>());
        // Verbatim directives (`{code}`, `{math}`, ...) consume their whole body
        // and closer in `parse_prepared` and finish the `MYST_DIRECTIVE` node
        // there, so there is no open container to push.
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

    /// The `(fence_char, min_count)` closer of the innermost open MyST
    /// directive, consulted by `MystDirectiveCloseParser`.
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

/// Emit buffered Definition content as either Heading-then-Plain (when the
/// first line is an ATX heading) or as a single Plain block.
///
/// Pandoc parses `Term\n: # Heading\n  Some text` as DefinitionList where the
/// definition contains [Header, Plain]; the `# Heading` line is a real Header
/// inside the definition, not text that happens to start with `#`.
/// Try each enabled table kind in turn (Grid → Multiline → Pipe → Simple),
/// emitting the first match into `builder` and returning the lines consumed.
/// Every kind validates before its first `start_node`, so on a full miss the
/// builder is left untouched and `None` is returned. Mirrors the dispatcher's
/// `first_kind_at` cascade for the list-item marker-line table paths.
fn try_parse_any_table_kind(
    window: &StrippedLines,
    builder: &mut GreenNodeBuilder<'static>,
    config: &ParserOptions,
) -> Option<usize> {
    let mut consumed = None;
    if config.extensions.grid_tables {
        consumed = tables::try_parse_grid_table(window, builder, config);
    }
    if consumed.is_none() && config.extensions.multiline_tables {
        consumed = tables::try_parse_multiline_table(window, builder, config);
    }
    if consumed.is_none() && config.extensions.pipe_tables {
        consumed = tables::try_parse_pipe_table(window, builder, config);
    }
    if consumed.is_none() && config.extensions.simple_tables {
        consumed = tables::try_parse_simple_table(window, builder, config);
    }
    consumed
}

/// Wrapper kind for a marker-line HTML block in a definition body. Mirrors
/// the dispatcher's `HtmlBlockParser::parse_prepared` div-lift gate: a
/// `<div ...>` whose `>` closes on the line retags to `HTML_BLOCK_DIV` under
/// Pandoc (with `native_divs`) so the projector emits `Block::Div` and the
/// salsa anchor index reads the open tag's id. Everything else keeps the
/// opaque `HTML_BLOCK` shape (which itself may retag to `HTML_BLOCK_RAW` for
/// single-construct opaque shapes inside `parse_html_block_with_wrapper`).
/// Wrapper `SyntaxKind` for an HTML block lifted from a container marker line
/// (definition body or footnote body). `<div>` under Pandoc + `native_divs`
/// retags to `HTML_BLOCK_DIV`; everything else stays opaque `HTML_BLOCK`.
fn marker_line_html_block_wrapper_kind(
    block_type: &html_blocks::HtmlBlockType,
    content_no_nl: &str,
    config: &ParserOptions,
) -> SyntaxKind {
    match block_type {
        html_blocks::HtmlBlockType::BlockTag {
            tag_name,
            is_closing: false,
            ..
        } if tag_name == "div"
            && config.dialect == crate::options::Dialect::Pandoc
            && config.extensions.native_divs
            && html_blocks::probe_open_tag_line_has_close_gt(content_no_nl, "div") =>
        {
            SyntaxKind::HTML_BLOCK_DIV
        }
        _ => SyntaxKind::HTML_BLOCK,
    }
}

/// Emit a definition body's buffered PLAIN content.
///
/// Block-shape tests (ATX heading, standalone image) read the buffer's *raw*
/// bytes, so they are measured on the original line the way they always were.
/// The inline emission goes through the buffer instead, so a continuation
/// line's gobbled indent stays out of the text the inline parser sees and is
/// spliced back as `WHITESPACE`.
fn emit_definition_plain_or_heading(
    builder: &mut GreenNodeBuilder<'static>,
    buffer: &ParagraphBuffer,
    config: &ParserOptions,
    suppress_footnote_refs: bool,
) {
    let text = buffer.raw_text();
    let text = text.as_str();
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
            buffer.split_at_raw(first_nl + 1).emit_with_inlines(
                builder,
                config,
                suppress_footnote_refs,
            );
            builder.finish_node();
            return;
        }
    }

    // A definition body that is only an image is a `Figure` under pandoc's
    // `implicit_figures`, same as a paragraph or a list item that is.
    let block_kind = if paragraph_is_standalone_image(text, config) {
        SyntaxKind::FIGURE
    } else {
        SyntaxKind::PLAIN
    };
    builder.start_node(block_kind.into());
    buffer.emit_with_inlines(builder, config, suppress_footnote_refs);
    builder.finish_node();
}

/// Look ahead from `pos+1` past at most one blank line for a definition marker
/// line at `content_col` indent. Returns the blank-line count consumed before
/// the marker, or `None` if no marker is found at the next non-blank line.
///
/// The one-blank limit is the term rule
/// [`next_line_is_definition_marker`](definition_lists::next_line_is_definition_marker)
/// applies, in the container's own frame: `- T\n\n\n  : b` is a bullet item
/// holding two paragraphs, not a term and its definition.
///
/// Used by `handle_footnote_open_effect` and
/// `maybe_open_definition_term_in_new_list_item` to decide whether a
/// container's *first content line* should open a definition-list term:
/// pandoc reparses container contents as a fresh block sequence, so it treats
/// `[^1]: Term\n\n    :   Definition\n` as a `Note [DefinitionList ...]` and
/// `- Term\n  : def\n` as a `BulletList [[DefinitionList ...]]`, not as a
/// paragraph followed by a separate def list with no term.
///
/// `lines` is absolute-indexed. Pass a [`StrippedLines`] when a blockquote
/// prefix is open, so `> - Term` / `>   : def` is measured on the quote's
/// content rather than on its markers.
fn first_content_line_term_lookahead(
    lines: &(impl LineView + ?Sized),
    pos: usize,
    content_col: usize,
    table_captions_enabled: bool,
) -> Option<usize> {
    let mut check_pos = pos + 1;
    let mut blank_count = 0;
    while check_pos < lines.line_count() {
        let line = lines.line(check_pos);
        let (trimmed, _) = strip_newline(line);
        if trimmed.trim().is_empty() {
            blank_count += 1;
            if blank_count > definition_lists::MAX_BLANKS_BEFORE_DEFINITION {
                return None;
            }
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
            // `next_line_is_definition_marker`. This lookahead strips the
            // footnote/list `content_col` indent (not a container prefix), so
            // the raw-line caption gate is appropriate here.
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
