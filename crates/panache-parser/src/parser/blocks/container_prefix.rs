//! Outer-container prefix vocabulary shared by block parsers that need
//! multi-line lookahead and per-line graft re-injection.
//!
//! [`ContainerPrefix`] captures the bytes the dispatcher / upstream
//! container code has already accounted for on each line — list-item
//! indent or marker, then blockquote markers. Block-level helpers that
//! walk raw `lines[..]` (e.g. `pandoc_html_open_tag_closes` and the HTML
//! block body-lift family) call [`ContainerPrefix::strip`] to skip past
//! those bytes before scanning.
//!
//! [`ContainerPrefixState`] is the graft-time re-injection counterpart.
//! When body content is reparsed from prefix-stripped text, the captured
//! per-line prefix bytes are re-emitted as kind-tagged tokens at line
//! starts so the resulting CST stays byte-equal to source. Folds the
//! older `BqPrefixState` (`html_blocks.rs`) and `LinePrefixState`
//! (`utils/list_item_buffer.rs`) — bq + list-indent on the same line
//! both round-trip cleanly under one structure.
//!
//! Every re-emitted prefix byte carries the `LINE_PREFIX` kind — inside a
//! content node, prefix is structurally distinguishable from content — with
//! token boundaries preserved from the legacy tokenization:
//!
//! - List-indent is emitted as a *single* `LINE_PREFIX` token (matching
//!   the legacy `LinePrefixState` behavior).
//! - Blockquote prefix is emitted byte-by-byte as 1-byte `LINE_PREFIX`
//!   tokens (matching the legacy `BqPrefixState` byte-walker), so a
//!   consumer that needs the marker-vs-padding split can still read it
//!   off the token texts.

use std::ops::Range;

use rowan::GreenNodeBuilder;
use smallvec::SmallVec;

use crate::options::{Dialect, ParserOptions};
use crate::syntax::SyntaxKind;

use super::super::block_dispatcher::BlockContext;
use super::super::utils::container_stack::{
    Container, byte_index_at_column, content_container_indent, leading_indent,
};
use super::blockquotes::{strip_blockquote_markers_counted, strip_n_blockquote_markers};
use super::lists::ListMarkerDetect;

/// A single strip operation applied during the dispatcher's
/// container-stack walk. Ops are applied in order; each consumes some
/// leading bytes of the line and the next op operates on what remains.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum StripOp {
    /// Advance N columns (tab-aware). Mirrors the legacy `list_content_col`
    /// strip.
    ///
    /// The columns are the list item's content indent, so the strip is
    /// whitespace-only and stops at the first content byte — a line
    /// short of the column keeps its bytes rather than having them
    /// counted as indent it never had. The one exception is the
    /// *innermost* op on the dispatch line of a marker-line dispatch,
    /// where the columns are the marker the parser core already emitted
    /// (`- `, `1. `); see [`ContainerPrefix::strip_line_0_for_emission`]
    /// and [`ContainerPrefix::strip_dispatch_line`].
    ListAdvance(u32),
    /// Strip one `>` marker (up to 3 leading spaces allowed per CommonMark).
    BlockQuoteMarker,
    /// Advance N columns when leading indent ≥ N; otherwise lazy-strip
    /// whatever leading whitespace exists. Mirrors the footnote/definition
    /// `content_indent` strip in `parse_inner_content`. `N` is the
    /// container's own relative width (see the `content_col` convention
    /// on [`Container`]); nested content containers contribute one op
    /// each, applied sequentially, summing to the absolute column.
    ContentIndent(u32),
}

const INLINE_STRIP_OPS: usize = 8;

/// Outer-container prefix on every line at the dispatcher level.
///
/// Captured as an ordered sequence of strip ops produced by walking
/// the container stack from bottom (outermost) to top (innermost).
/// Each container contributes one op (with `List` and most non-strip
/// containers skipped); the order matches the stack-walk order, so
/// nested cases like [Definition, List, ListItem, BlockQuote] produce
/// the correct content_indent → list_advance → bq cascade.
///
/// Only the innermost ListItem *per section* contributes a `ListAdvance`
/// op (matching `paragraphs::current_content_col`'s single-value
/// semantics for adjacent nested lists). FootnoteDefinition and
/// Definition each push one `ContentIndent`. BlockQuote pushes one
/// `BlockQuoteMarker`.
#[derive(Clone, Debug, Default)]
pub(crate) struct ContainerPrefix {
    ops: SmallVec<[StripOp; INLINE_STRIP_OPS]>,
    /// True iff the line at dispatch position (`lines[start_pos]`) is
    /// the LIST-MARKER line — i.e. the LIST_MARKER + WHITESPACE tokens
    /// for the innermost list item's `content_col` columns have just
    /// been emitted upstream and must be skipped by the helper. False
    /// (default) when the dispatch fires on a continuation line: those
    /// leading-indent bytes are NOT upstream-emitted and must be
    /// preserved inside the block's content for losslessness.
    ///
    /// Affects only the line-0 strip semantics. Lookahead helpers and
    /// the continuation-line strip always apply every op.
    pub list_marker_consumed_on_line_0: bool,
    /// True under the Pandoc dialect, where the blockquote reader extracts
    /// the quote's raw content line by line and skips the leading
    /// whitespace of every *lazy* line — one carrying fewer `>` markers
    /// than the quote is deep. See [`lazy_gobble_trim`].
    ///
    /// This is the multi-line counterpart of
    /// `Parser::fold_lazy_line_into_blockquote`, which drops the indent of
    /// the single line it dispatches. A construct opened on a lazy line
    /// reads its body straight out of `lines[..]` through this prefix, so
    /// without the gobble here only its first line is de-indented.
    pub lazy_blockquote_gobble: bool,
    /// True when a fenced div is open anywhere on the stack. Pandoc
    /// bounds the block a scan tracks at a bare `:::` through two
    /// guards, and one of them always applies while a div is open: a
    /// div *outside* a line-collected container (list item,
    /// definition/footnote body, blockquote) fences that container's
    /// raw collection (`notFollowedByDivCloser` — the div's level was
    /// already set when the lines were collected), and the innermost
    /// div's *own* content parse carries the same guard, so a block
    /// sitting directly in the div stops at the closer too (probed: a
    /// table directly in a div, and in a div nested inside a
    /// definition body, both end there). Lookaheads that scan for a
    /// block's end (see [`LineView::ends_container_lines`]) stop
    /// instead of swallowing the fence. With no div open a stray `:::`
    /// is ordinary content — a table row in pandoc (probed).
    ///
    /// [`LineView::ends_container_lines`]: super::tables::LineView::ends_container_lines
    div_closer_ends_lines: bool,
    /// Op index of the outermost `FootnoteDefinition`'s `ContentIndent`
    /// op, when a footnote is on the stack. Pandoc collects a note's
    /// raw lines in `noteBlock`, whose `rawLine` stops at a new note
    /// marker reached by `skipNonindentSpaces >> noteMarker` — at most
    /// 3 spaces in the note's *outer* frame. So a `[^x]:` line whose
    /// frame verdict fails at or before this op ends the line run of
    /// every container inside the note (their lines are part of the
    /// note's collected raw), while a marker that reaches the note's
    /// content column — a verdict failing only *past* this op, or not
    /// at all — is note content, which pandoc collects as ordinary
    /// text (e.g. a table row).
    ///
    /// Like [`Self::div_closer_ends_lines`] this tracks presence, not
    /// ordering: everything pushed above the note sits inside its
    /// collected raw either way. With no footnote on the stack there
    /// is nothing to fence — pandoc's `listLine` and
    /// `definitionListItem` have no note-marker guard, so a stray
    /// marker there is ordinary content.
    note_marker_op_bound: Option<usize>,
    /// Captured when a list item is on the stack: the marker-detection
    /// bits a line must match to be pandoc's `listStart` fence. Pandoc
    /// collects a list item's continuation lines with
    /// `listContinuationLine = notFollowedBy blankline >>
    /// notFollowedBy' listStart >> ...`, where `listStart` runs in the
    /// frame the list was parsed in and tolerates `nonindentSpaces`
    /// (at most 3). So a marker line within 3 columns of that frame
    /// ends the run, one that reaches the item's content column is
    /// nested-list content, and one in between is a lazy continuation
    /// (all verified against `pandoc -f markdown -t native`; see the
    /// `list_start` pins). Like [`Self::note_marker_op_bound`] this
    /// tracks presence: with no list item on the stack there is no
    /// item run to fence.
    ///
    /// The bits are captured here because the predicate has no config
    /// access at scan time (the same pattern as
    /// `lazy_blockquote_gobble`).
    list_start_detect: Option<ListMarkerDetect>,
    /// Content columns of the open list items in the innermost list
    /// section (the run of `ListItem`s whose collapsed advance is the
    /// recipe's last `ListAdvance`), outermost first, cumulative in the
    /// section frame. Pandoc parses each nested list inside the
    /// enclosing item's content reparse, so `listStart`'s 3-column
    /// tolerance is measured from that frame — in the section frame,
    /// from the start of whichever band `[c_{j-1}, c_j)` of this
    /// ladder a marker's indent falls in (see
    /// [`Self::list_start_band_start`]; verified against `pandoc -f
    /// markdown -t native`, the `list_start` band pins). Empty
    /// whenever the last advance was flushed by a section-breaking
    /// container, which keeps those exotic stacks on the flat
    /// 3-column rule.
    list_item_ladder: SmallVec<[u32; 4]>,
    /// The matched-pair HTML tag left open by a list item *below* some
    /// container this prefix strips. `Some` arms pandoc's
    /// `notFollowedByHtmlCloser` fence: inside markdown-in-html, a line
    /// opening with the tag's close form ends a collected line run — the
    /// closer belongs to the HTML block, not to whatever block the
    /// container's content was in the middle of.
    ///
    /// Ordering-gated, unlike the presence-tracked flags above, and
    /// anchored at the list item whose buffer holds the open tag
    /// (markdown-in-html is not a stack transition here, so there is no
    /// `Container` to anchor on): only containers pushed *after* that
    /// item are fenced.
    /// The item's own line run is not — pandoc itself collects the closer
    /// as e.g. a table row there (verified: `- <div>` + blank + indented
    /// simple table + `  </div>` slices the closer into cells in pandoc).
    /// The innermost tag-holding item wins, matching pandoc's
    /// `stateInHtmlBlock`.
    ///
    /// The second element is that item's `content_col`: pandoc's item
    /// gobble eats at most that many columns before the guard's `htmlTag`
    /// runs, and `htmlTag` skips no whitespace — so the closer fires at
    /// up to the content column of indent and not one space more (both
    /// probed). Kept as a column bound rather than resolved through the
    /// ops because the lazy blockquote gobble skips a marker-less line's
    /// indent wholesale, erasing exactly the distinction pandoc draws.
    html_closer_tag: Option<(String, u32)>,
    /// Whether the run a bare `:::` closer terminates supplies, on
    /// reparse, the trailing blank line pandoc's headered simple-table
    /// footer (`blanklines`) needs.
    ///
    /// A terminator contiguous with a table's last row cuts the
    /// terminated container's raw off without a blank line, so the
    /// footer fails and the lines reparse as a paragraph — unless some
    /// container in the terminated chain restores the blank when its
    /// content is reparsed: pandoc appends `"\n\n"` to a blockquote's
    /// collected raw and `"\n"` to a note's, and list items and
    /// definition bodies keep a trailing blank present in the outer raw.
    /// So the blank survives iff a blockquote or footnote body sits
    /// inside the innermost open div (or nothing line-collected does at
    /// all — a table directly in the div keeps its footer; probed).
    /// Meaningful only when [`Self::div_closer_ends_lines`] is set.
    div_closer_supplies_blank: bool,
    /// Same question for a run ended by pandoc's `listStart` fence: a
    /// blockquote or footnote body pushed after the innermost list item
    /// supplies the blank; the item's own raw ends contiguous at the
    /// marker otherwise. Meaningful only when [`Self::list_start_detect`]
    /// is set. (A new note marker needs no such flag: it terminates the
    /// note's own raw, which `noteBlock` reparses with `"\n"` appended,
    /// so the blank always materializes.)
    list_start_supplies_blank: bool,
    /// Same question for a run ended by the markdown-in-html closer
    /// fence: a blockquote or footnote body pushed after the tag-holding
    /// item supplies the blank. Meaningful only when
    /// [`Self::html_closer_tag`] is set.
    html_closer_supplies_blank: bool,
}

impl ContainerPrefix {
    /// Build a strip recipe by walking the container stack from bottom
    /// (outermost) to top (innermost).
    ///
    /// Each strip-contributing container pushes one op in stack order:
    /// `BlockQuote` → `BlockQuoteMarker`, `FootnoteDefinition` /
    /// `Definition` → `ContentIndent(content_col)`. Nested `ListItem`s
    /// are collapsed *per section* — for each run of adjacent
    /// `ListItem`s with no intervening strip-contributing container,
    /// only the innermost contributes a `ListAdvance`. This matches
    /// today's `paragraphs::current_content_col` semantics for nested
    /// same-section lists (inner.content_col is cumulative) while still
    /// applying outer-section list strips before an intervening
    /// blockquote or content-indent container.
    pub fn from_stack(
        stack: &[Container],
        list_marker_consumed_on_line_0: bool,
        config: &ParserOptions,
    ) -> Self {
        struct PendingListAdvance {
            content_col: u32,
            div_open: bool,
            html_open: Option<(String, u32)>,
        }
        let mut ops: SmallVec<[StripOp; INLINE_STRIP_OPS]> = SmallVec::new();
        let mut pending_list_advance: Option<PendingListAdvance> = None;
        let mut saw_list_advance = false;
        let mut div_open = false;
        let mut div_closer_ends_lines = false;
        let mut html_open: Option<(String, u32)> = None;
        let mut html_closer_tag: Option<(String, u32)> = None;
        let mut note_marker_op_bound = None;
        let mut div_chain_collects = false;
        let mut div_chain_appends = false;
        let mut list_start_supplies_blank = false;
        let mut html_closer_supplies_blank = false;
        let mut list_item_ladder: SmallVec<[u32; 4]> = SmallVec::new();
        for c in stack {
            match c {
                Container::BlockQuote { .. } => {
                    if let Some(p) = pending_list_advance.take() {
                        ops.push(StripOp::ListAdvance(p.content_col));
                        saw_list_advance = true;
                        div_closer_ends_lines |= p.div_open;
                        html_closer_tag = p.html_open.or(html_closer_tag);
                    }
                    list_item_ladder.clear();
                    ops.push(StripOp::BlockQuoteMarker);
                    div_closer_ends_lines |= div_open;
                    html_closer_tag = html_open.clone().or(html_closer_tag);
                    div_chain_collects = true;
                    div_chain_appends = true;
                    list_start_supplies_blank = true;
                    html_closer_supplies_blank = true;
                }
                Container::FootnoteDefinition { content_col } => {
                    if let Some(p) = pending_list_advance.take() {
                        ops.push(StripOp::ListAdvance(p.content_col));
                        saw_list_advance = true;
                        div_closer_ends_lines |= p.div_open;
                        html_closer_tag = p.html_open.or(html_closer_tag);
                    }
                    list_item_ladder.clear();
                    note_marker_op_bound.get_or_insert(ops.len());
                    ops.push(StripOp::ContentIndent(*content_col as u32));
                    div_closer_ends_lines |= div_open;
                    html_closer_tag = html_open.clone().or(html_closer_tag);
                    div_chain_collects = true;
                    div_chain_appends = true;
                    list_start_supplies_blank = true;
                    html_closer_supplies_blank = true;
                }
                Container::Definition { content_col, .. }
                | Container::Admonition { content_col } => {
                    if let Some(p) = pending_list_advance.take() {
                        if matches!(c, Container::Admonition { .. }) {
                            ops.push(StripOp::ListAdvance(p.content_col));
                        }
                        saw_list_advance = true;
                        div_closer_ends_lines |= p.div_open;
                        html_closer_tag = p.html_open.or(html_closer_tag);
                    }
                    list_item_ladder.clear();
                    ops.push(StripOp::ContentIndent(*content_col as u32));
                    div_closer_ends_lines |= div_open;
                    html_closer_tag = html_open.clone().or(html_closer_tag);
                    div_chain_collects = true;
                }
                Container::ListItem {
                    content_col,
                    buffer,
                    ..
                } => {
                    pending_list_advance = Some(PendingListAdvance {
                        content_col: *content_col as u32,
                        div_open,
                        html_open: html_open.clone(),
                    });
                    list_item_ladder.push(*content_col as u32);
                    div_chain_collects = true;
                    list_start_supplies_blank = false;
                    if let Some(tag) = buffer.open_matched_pair_tag(config) {
                        html_open = Some((tag, *content_col as u32));
                        html_closer_supplies_blank = false;
                    }
                }
                Container::FencedDiv { .. } => {
                    div_open = true;
                    div_chain_collects = false;
                    div_chain_appends = false;
                }
                _ => {}
            }
        }
        if let Some(p) = pending_list_advance {
            ops.push(StripOp::ListAdvance(p.content_col));
            saw_list_advance = true;
            div_closer_ends_lines |= p.div_open;
            html_closer_tag = p.html_open.or(html_closer_tag);
        }
        div_closer_ends_lines |= div_open;
        debug_assert_eq!(
            ops.iter()
                .map(|op| match op {
                    StripOp::ContentIndent(n) => *n as usize,
                    _ => 0,
                })
                .sum::<usize>(),
            content_container_indent(stack),
            "from_stack's ContentIndent ops must mirror the stack's content-container sum"
        );
        let list_start_detect = saw_list_advance.then(|| ListMarkerDetect::from_options(config));
        Self {
            ops,
            list_marker_consumed_on_line_0,
            lazy_blockquote_gobble: config.dialect == Dialect::Pandoc,
            div_closer_ends_lines,
            note_marker_op_bound,
            list_start_detect,
            list_item_ladder,
            html_closer_tag,
            div_closer_supplies_blank: !div_chain_collects || div_chain_appends,
            list_start_supplies_blank,
            html_closer_supplies_blank,
        }
    }

    /// Build from a `BlockContext`. Equivalent to a stack with at most
    /// one ListAdvance + one BlockQuote run + one ContentIndent, in
    /// the order `[ListAdvance?, BlockQuote*, ContentIndent?]`. Use
    /// this only when the caller doesn't have stack access; it is
    /// correct for the common container shapes but may diverge from
    /// [`Self::from_stack`] for exotic orderings (Definition above
    /// List, FootnoteDef interleaved with BlockQuote, etc.).
    ///
    /// `list_marker_consumed_on_line_0` is hard-wired to `false`. The
    /// dispatcher in `parse_inner_content` is the only path that needs
    /// the flag set true (the marker-line-after-`add_list_item` strip),
    /// and it always builds the prefix via [`Self::from_stack`] with the
    /// flag threaded explicitly from `dispatch_list_marker_consumed`.
    /// Every `from_ctx` call site runs in a continuation or detection
    /// context where the flag would be false anyway.
    pub fn from_ctx(ctx: &BlockContext) -> Self {
        let list_content_col = ctx
            .list_indent_info
            .as_ref()
            .map(|i| i.content_col)
            .unwrap_or(0);
        let bq_depth = ctx.blockquote_depth;
        let content_indent = ctx.content_indent;

        let mut ops: SmallVec<[StripOp; INLINE_STRIP_OPS]> = SmallVec::new();
        if list_content_col > 0 {
            ops.push(StripOp::ListAdvance(list_content_col as u32));
        }
        for _ in 0..bq_depth {
            ops.push(StripOp::BlockQuoteMarker);
        }
        if content_indent > 0 {
            ops.push(StripOp::ContentIndent(content_indent as u32));
        }
        Self {
            ops,
            list_marker_consumed_on_line_0: false,
            lazy_blockquote_gobble: ctx.config.dialect == Dialect::Pandoc,
            div_closer_ends_lines: false,
            note_marker_op_bound: None,
            list_start_detect: None,
            list_item_ladder: SmallVec::new(),
            html_closer_tag: None,
            div_closer_supplies_blank: true,
            list_start_supplies_blank: true,
            html_closer_supplies_blank: true,
        }
    }

    /// Bq-only convenience for callers that don't have a `BlockContext`.
    /// The Pandoc lazy gobble is off; callers needing it build from a stack
    /// or a `BlockContext`.
    #[allow(dead_code)]
    pub fn bq_only(bq_depth: usize) -> Self {
        let mut ops: SmallVec<[StripOp; INLINE_STRIP_OPS]> = SmallVec::new();
        for _ in 0..bq_depth {
            ops.push(StripOp::BlockQuoteMarker);
        }
        Self {
            ops,
            list_marker_consumed_on_line_0: false,
            lazy_blockquote_gobble: false,
            div_closer_ends_lines: false,
            note_marker_op_bound: None,
            list_start_detect: None,
            list_item_ladder: SmallVec::new(),
            html_closer_tag: None,
            div_closer_supplies_blank: true,
            list_start_supplies_blank: true,
            html_closer_supplies_blank: true,
        }
    }

    /// Clone this prefix with `n` additional `BlockQuoteMarker` ops
    /// appended — the prefix the stack *would* produce if `n` more
    /// blockquotes were opened on top of it.
    ///
    /// Appending is faithful to [`Self::from_stack`] because a newly
    /// opened blockquote always enters at the top of the container
    /// stack, so it strips last: a stack ending in `ListItem` flushes
    /// its pending `ListAdvance` before the `BlockQuoteMarker` either
    /// way. Used by the blockquote depth-cap probe in `core.rs`, which
    /// asks the block registry what it would match at a hypothetical
    /// depth before committing to opening that many levels.
    pub fn with_extra_blockquotes(&self, n: usize) -> Self {
        let mut next = self.clone();
        for _ in 0..n {
            next.ops.push(StripOp::BlockQuoteMarker);
        }
        if n > 0 {
            next.list_item_ladder.clear();
        }
        next
    }

    /// Build a prefix that reproduces an explicit set of container scalars
    /// — the inverse of the [`Self::bq_depth`] / [`Self::list_content_col`]
    /// / [`Self::content_indent`] accessors and [`bq_outer_of_list`]. Used
    /// by the marker-line fenced-code dispatch paths in `core.rs`, which
    /// compute these scalars by hand from a mid-transition container state
    /// (a `ListItem`/`Definition` just pushed, the marker bytes just
    /// emitted) rather than from a settled stack the other constructors
    /// read.
    ///
    /// `bq_outer` is honored only when both a blockquote run and a list
    /// advance are present (it picks which leads); with at most one of the
    /// two, the op order is forced and `bq_outer_of_list` reconstructs it
    /// faithfully. The content indent is always last, matching
    /// [`content_line_prefix_tail`]'s scalar strip order.
    pub fn from_scalars(
        bq_depth: usize,
        list_content_col: usize,
        bq_outer: bool,
        content_indent: usize,
        list_marker_consumed_on_line_0: bool,
        dialect: Dialect,
    ) -> Self {
        let mut ops: SmallVec<[StripOp; INLINE_STRIP_OPS]> = SmallVec::new();
        if bq_outer {
            for _ in 0..bq_depth {
                ops.push(StripOp::BlockQuoteMarker);
            }
            if list_content_col > 0 {
                ops.push(StripOp::ListAdvance(list_content_col as u32));
            }
        } else {
            if list_content_col > 0 {
                ops.push(StripOp::ListAdvance(list_content_col as u32));
            }
            for _ in 0..bq_depth {
                ops.push(StripOp::BlockQuoteMarker);
            }
        }
        if content_indent > 0 {
            ops.push(StripOp::ContentIndent(content_indent as u32));
        }
        Self {
            ops,
            list_marker_consumed_on_line_0,
            lazy_blockquote_gobble: dialect == Dialect::Pandoc,
            div_closer_ends_lines: false,
            note_marker_op_bound: None,
            list_start_detect: None,
            list_item_ladder: SmallVec::new(),
            html_closer_tag: None,
            div_closer_supplies_blank: true,
            list_start_supplies_blank: true,
            html_closer_supplies_blank: true,
        }
    }

    pub fn ops(&self) -> &[StripOp] {
        &self.ops
    }

    /// See the field of the same name: whether a bare `:::` ends the
    /// line run of the container this prefix strips.
    pub fn div_closer_ends_lines(&self) -> bool {
        self.div_closer_ends_lines
    }

    /// See the field of the same name: the op a `[^x]:` line must fail
    /// at (or before) to end the line run of the container this prefix
    /// strips, when a footnote is on the stack.
    pub fn note_marker_op_bound(&self) -> Option<usize> {
        self.note_marker_op_bound
    }

    /// See the field of the same name: the matched-pair HTML tag whose
    /// close form ends the line run of the container this prefix
    /// strips, when a list item below that container holds the tag
    /// open, with that item's content column as the closer's maximum
    /// leading indent.
    pub fn html_closer_tag(&self) -> Option<(&str, u32)> {
        self.html_closer_tag
            .as_ref()
            .map(|(tag, col)| (tag.as_str(), *col))
    }

    /// See the field of the same name: whether the run a bare `:::`
    /// terminates supplies the headered simple-table footer's blank
    /// line on reparse.
    pub fn div_closer_supplies_blank(&self) -> bool {
        self.div_closer_supplies_blank
    }

    /// See the field of the same name: the blank-supply question for a
    /// run ended by pandoc's `listStart` fence.
    pub fn list_start_supplies_blank(&self) -> bool {
        self.list_start_supplies_blank
    }

    /// See the field of the same name: the blank-supply question for a
    /// run ended by the markdown-in-html closer fence.
    pub fn html_closer_supplies_blank(&self) -> bool {
        self.html_closer_supplies_blank
    }

    /// See the field of the same name: the marker-detection bits a
    /// line must match to be pandoc's `listStart` fence, when a list
    /// item is on the stack.
    pub fn list_start_detect(&self) -> Option<ListMarkerDetect> {
        self.list_start_detect
    }

    /// The start of the [`Self::list_item_ladder`] band a marker line's
    /// section-frame indent falls in — the content column of the item
    /// whose reparse frame pandoc would check `listStart` in. Pandoc's
    /// 3-column tolerance is measured from here, so a marker fences iff
    /// `indent_cols <= band_start + 3` (and the innermost advance
    /// failed, which bounds `indent_cols` below the innermost content
    /// column). Returns 0 — the flat `nonindentSpaces` rule — with no
    /// ladder captured.
    pub fn list_start_band_start(&self, indent_cols: usize) -> usize {
        self.list_item_ladder
            .iter()
            .map(|c| *c as usize)
            .take_while(|c| *c <= indent_cols)
            .last()
            .unwrap_or(0)
    }

    /// Total number of `BlockQuoteMarker` ops. Kept as a back-compat
    /// accessor for callers that previously read `prefix.bq_depth`.
    pub fn bq_depth(&self) -> usize {
        self.ops()
            .iter()
            .filter(|op| matches!(op, StripOp::BlockQuoteMarker))
            .count()
    }

    /// Innermost (last) `ListAdvance` op's column count, or 0 when
    /// the prefix contains no list-advance op. Kept as a back-compat
    /// accessor for callers that previously read
    /// `prefix.list_content_col`.
    pub fn list_content_col(&self) -> usize {
        self.ops()
            .iter()
            .rev()
            .find_map(|op| match op {
                StripOp::ListAdvance(n) => Some(*n as usize),
                _ => None,
            })
            .unwrap_or(0)
    }

    /// Sum of `ContentIndent` ops' column counts. Kept as a back-compat
    /// accessor for callers that previously read `prefix.content_indent`.
    #[allow(dead_code)]
    pub fn content_indent(&self) -> usize {
        self.ops()
            .iter()
            .map(|op| match op {
                StripOp::ContentIndent(n) => *n as usize,
                _ => 0,
            })
            .sum()
    }

    /// Build a `ContainerPrefix` directly from a sequence of strip ops.
    /// Intended for tests; production code should use
    /// [`Self::from_stack`] or [`Self::from_ctx`].
    #[cfg(test)]
    pub fn from_ops(ops_slice: &[StripOp], list_marker_consumed_on_line_0: bool) -> Self {
        Self {
            ops: SmallVec::from_slice(ops_slice),
            list_marker_consumed_on_line_0,
            lazy_blockquote_gobble: false,
            div_closer_ends_lines: false,
            note_marker_op_bound: None,
            list_start_detect: None,
            list_item_ladder: SmallVec::new(),
            html_closer_tag: None,
            div_closer_supplies_blank: true,
            list_start_supplies_blank: true,
            html_closer_supplies_blank: true,
        }
    }

    /// Strip every op in order, reading every `ListAdvance` as the
    /// item's content indent — so the strip stops at the first
    /// non-whitespace byte and can never claim a column the line does
    /// not have. Used for continuation lines (lines 1+) in multi-line
    /// lookahead.
    ///
    /// The dispatch line is different: its innermost `ListAdvance` may
    /// name the list marker the core already emitted, which is not
    /// whitespace. Strip that line with
    /// [`Self::strip_line_0_for_emission`] (emission, keeps an
    /// unconsumed marker's indent) or [`Self::strip_dispatch_line`]
    /// (detection, always consumes it).
    pub fn strip<'a>(&self, line: &'a str) -> &'a str {
        let ops = self.ops();
        let mut s = line;
        let mut i = 0;
        while i < ops.len() {
            match ops[i] {
                StripOp::BlockQuoteMarker => {
                    let run = blockquote_run_len(&ops[i..]);
                    s = strip_bq_with_gobble(s, run, self.lazy_blockquote_gobble);
                    i += run;
                }
                op => {
                    s = apply_op(s, op);
                    i += 1;
                }
            }
        }
        s
    }

    /// This recipe minus the innermost list item's own `ListAdvance`.
    ///
    /// That op is the item's `content_col`, and `content_col` is absolute in
    /// the frame the *remaining* ops produce — inside a footnote body it is
    /// measured after the footnote's content indent is stripped, but two
    /// nested bullets contribute only the inner one's advance, already
    /// absolute. So a lookahead that compares a line's indent against
    /// `content_col` has to strip with this, not with the full recipe.
    pub fn without_innermost_list_advance(&self) -> Self {
        let mut ops = self.ops.clone();
        let mut note_marker_op_bound = self.note_marker_op_bound;
        if let Some(i) = ops
            .iter()
            .rposition(|op| matches!(op, StripOp::ListAdvance(_)))
        {
            ops.remove(i);
            if let Some(bound) = &mut note_marker_op_bound
                && i < *bound
            {
                *bound -= 1;
            }
        }
        Self {
            ops,
            list_marker_consumed_on_line_0: self.list_marker_consumed_on_line_0,
            lazy_blockquote_gobble: self.lazy_blockquote_gobble,
            div_closer_ends_lines: self.div_closer_ends_lines,
            note_marker_op_bound,
            list_start_detect: self.list_start_detect,
            list_item_ladder: self.list_item_ladder.clone(),
            html_closer_tag: self.html_closer_tag.clone(),
            div_closer_supplies_blank: self.div_closer_supplies_blank,
            list_start_supplies_blank: self.list_start_supplies_blank,
            html_closer_supplies_blank: self.html_closer_supplies_blank,
        }
    }

    /// Strip semantics for the dispatch line (line 0). Identical to
    /// [`Self::strip`] except that the *innermost* (last)
    /// `ListAdvance` op is skipped when
    /// `list_marker_consumed_on_line_0` is false — that's the
    /// "continuation-line dispatch where the leading indent belongs to
    /// inner content" case.
    pub fn strip_line_0_for_emission<'a>(&self, line: &'a str) -> &'a str {
        self.strip_line_0_with_indent_emit(line).0
    }

    /// Detection-mode strip of the dispatch line: every op applied, with
    /// the innermost `ListAdvance` consumed either way — as the emitted
    /// marker's columns when `list_marker_consumed_on_line_0` says the
    /// core put a marker there, and as ordinary list indent otherwise.
    ///
    /// This is [`Self::strip`]'s counterpart for line 0, for scans whose
    /// shape test is anchored at column 0 of the item's content (grid
    /// borders, tag balance). [`Self::strip_line_0_for_emission`] keeps
    /// an unconsumed marker line's indent because those bytes still have
    /// to land in the block's content; a detection scan wants them gone.
    pub fn strip_dispatch_line<'a>(&self, line: &'a str) -> &'a str {
        let ops = self.ops();
        let mut s = line;
        let mut i = 0;
        while i < ops.len() {
            match ops[i] {
                StripOp::BlockQuoteMarker => {
                    let run = blockquote_run_len(&ops[i..]);
                    s = strip_bq_with_gobble(s, run, self.lazy_blockquote_gobble);
                    i += run;
                }
                StripOp::ListAdvance(n) if self.is_innermost_list_advance(i) => {
                    s = if self.list_marker_consumed_on_line_0 {
                        advance_emitted_marker_columns(s, n as usize)
                    } else {
                        strip_list_indent(s, n as usize)
                    };
                    i += 1;
                }
                op => {
                    s = apply_op(s, op);
                    i += 1;
                }
            }
        }
        s
    }

    fn is_innermost_list_advance(&self, i: usize) -> bool {
        self.ops()
            .iter()
            .rposition(|op| matches!(op, StripOp::ListAdvance(_)))
            == Some(i)
    }

    /// Like [`Self::strip_line_0_for_emission`] but also returns the
    /// bytes consumed by the trailing run of `ContentIndent` ops (for
    /// re-emission as WHITESPACE when a nested BlockQuote opens inside a
    /// footnote/definition).
    ///
    /// Nested content containers — a definition body inside a footnote,
    /// say — each take their own columns off the same line, so the run
    /// is merged into one slice. Reporting only the innermost op's bytes
    /// drops every outer container's columns on the dispatch line: the
    /// caller emits one token, and nothing else claims those bytes.
    /// Adjacency is required, since a strip in between (a list indent,
    /// which the core already emitted) would otherwise be swept into the
    /// same token and written twice.
    pub fn strip_line_0_with_indent_emit<'a>(&self, line: &'a str) -> (&'a str, Option<&'a str>) {
        let last_list_idx = self
            .ops()
            .iter()
            .rposition(|op| matches!(op, StripOp::ListAdvance(_)));
        let ops = self.ops();
        let offset = |s: &str| line.len() - s.len();
        let mut s = line;
        let mut emit: Option<Range<usize>> = None;
        let mut i = 0;
        while i < ops.len() {
            match ops[i] {
                StripOp::ListAdvance(n) => {
                    if Some(i) != last_list_idx {
                        s = strip_list_indent(s, n as usize);
                    } else if self.list_marker_consumed_on_line_0 {
                        s = advance_emitted_marker_columns(s, n as usize);
                    }
                    i += 1;
                }
                StripOp::BlockQuoteMarker => {
                    let run = blockquote_run_len(&ops[i..]);
                    s = strip_bq_with_gobble(s, run, self.lazy_blockquote_gobble);
                    emit = None;
                    i += run;
                }
                StripOp::ContentIndent(n) => {
                    let (next, e) = strip_content_indent(s, n as usize);
                    if e.is_some() {
                        let range = offset(s)..offset(next);
                        emit = Some(match emit {
                            Some(prev) if prev.end == range.start => prev.start..range.end,
                            _ => range,
                        });
                    }
                    s = next;
                    i += 1;
                }
            }
        }
        (s, emit.map(|range| &line[range]))
    }

    /// Split a continuation line into its captured container prefix — as
    /// ordered [`ContainerPrefixLine`] pieces — and the remaining inner
    /// content, walking every strip op in stack order. Used by graft
    /// helpers that need to capture the consumed prefix bytes for
    /// re-injection.
    ///
    /// Replaces the legacy two-slot `split`, whose phase machine stopped
    /// at the first op past one list advance and one bq run, leaking a
    /// deeper container's prefix bytes into the inner content the graft
    /// hands to the reparse.
    ///
    /// The Pandoc lazy gobble lands in the bq run (the legacy `split`
    /// tokenization): on a line short of its `>` markers the skipped
    /// whitespace is prefix, not content, so the graft re-injects it as
    /// prefix tokens instead of handing it to the reparse. Keeping it in
    /// the reparse would turn a lazy `<div>` body line indented four
    /// columns into an indented code block where pandoc reads a
    /// paragraph.
    pub fn split_pieces<'a>(&self, line: &'a str) -> (ContainerPrefixLine, &'a str) {
        let (pieces, tail) = prefix_pieces(self, line);
        let mut out = ContainerPrefixLine::default();
        for (kind, range) in pieces {
            match kind {
                RawPieceKind::Indent => out.push_indent(&line[range]),
                RawPieceKind::BqRun | RawPieceKind::LazyGobble => out.push_bq(&line[range]),
            }
        }
        (out, tail)
    }
}

/// The authoritative answer to "which container frame is this line in,
/// and does it reach the content column?".
///
/// The guiding principle: a strip must not be able to report *what is
/// left of a line* without also reporting *whether the container indent
/// it consumed was real*. Every variant carries the residual tail, so a
/// caller cannot read one without seeing the other.
///
/// Scope bounds, by design:
///
/// - Blockquote laziness is not classified here. The Pandoc gobble
///   (`lazy_blockquote_gobble`) is applied exactly as `strip` applies
///   it; a line lazy at some quote depth still resolves relative to
///   whatever the bq strip left. Room is left for a `LazyInQuote`
///   variant if a caller ever needs the distinction.
/// - Blank lines are not special-cased: an all-whitespace line short of
///   a column reports `Dedented`/`FakedIndent` with whatever tail its
///   whitespace leaves. Callers gate blanks with `is_blank_line` first,
///   as every current lookahead already does.
///
/// The walk stops at the first op the line fails — a straddling tab
/// included, though it is not a failure — so `rest` is measured in the
/// frame of the ops applied up to that point, `op_index` names the
/// failing op, and any ops after the stop were never applied (see
/// [`Self::reaches_frame`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrameVerdict<'a> {
    /// Every op's columns and markers are covered by real prefix bytes,
    /// with the content column on a byte boundary. `rest` is the tail
    /// in the innermost frame — what emission-side strips produce.
    Inside { rest: &'a str },
    /// Real whitespace fell short of a `ContentIndent` op: the line has
    /// dedented out of that content container. `rest` is the byte-honest
    /// lazily trimmed tail (what [`strip_content_indent`] degrades to);
    /// `cols_short` is the shortfall against that op's width.
    Dedented {
        rest: &'a str,
        op_index: usize,
        cols_short: u32,
    },
    /// A `ListAdvance` op not covered by real whitespace: the
    /// column-blind strip would have faked the indent by eating content
    /// bytes. The line is an under-indented lazy continuation, not
    /// inside the item. `rest` is the byte-honest whitespace-only tail.
    FakedIndent { rest: &'a str, op_index: usize },
    /// The line *does* reach the op's column, but a tab straddles it:
    /// there is no byte boundary to split on. Distinct from a genuinely
    /// short line — the columns are all there. `rest` starts at the
    /// straddling tab; `cols_before_tab` is the column where the tab
    /// begins, so `line.len() - rest.len()` matches
    /// [`gobbled_indent_prefix_len`]'s stop-before-the-tab byte count
    /// when the straddled op is the first.
    ///
    /// [`gobbled_indent_prefix_len`]: super::super::utils::container_stack::gobbled_indent_prefix_len
    StraddlingTab { rest: &'a str, cols_before_tab: u32 },
}

impl<'a> FrameVerdict<'a> {
    /// Whether the walk consumed every op it applied — the typed
    /// replacement for the hand-rolled
    /// `leading_indent(line).0 >= content_col` tests, which are true
    /// for a straddling tab as well (tabs count in columns).
    ///
    /// Caveat for multi-op frames: a straddle ends the walk, so for
    /// [`FrameVerdict::StraddlingTab`] this vouches only for the ops
    /// *up to* the straddled one — ops after it (an inner blockquote
    /// marker, say) were never applied. Current callers tolerate the
    /// overclaim: the caption probe's container bound uses it as a
    /// suppression gate, and the definition lookahead re-reads the
    /// straddle's `rest` with the marker test, which fails on any
    /// unconsumed marker byte. A caller needing the inner ops settled
    /// must read the variant instead of this shortcut.
    pub fn reaches_frame(&self) -> bool {
        matches!(
            self,
            FrameVerdict::Inside { .. } | FrameVerdict::StraddlingTab { .. }
        )
    }

    /// The residual tail, whichever frame the walk ended in.
    #[allow(dead_code)]
    pub fn rest(&self) -> &'a str {
        match self {
            FrameVerdict::Inside { rest }
            | FrameVerdict::Dedented { rest, .. }
            | FrameVerdict::FakedIndent { rest, .. }
            | FrameVerdict::StraddlingTab { rest, .. } => rest,
        }
    }
}

enum ColumnAdvance<'a> {
    /// The target column falls on a byte boundary; holds the tail.
    Reached(&'a str),
    /// A tab spans the target column; holds the tail starting at the
    /// tab and the column where the tab begins.
    Straddled { rest: &'a str, cols_before_tab: u32 },
    /// The leading whitespace ends short of the target; holds the
    /// columns it does cover.
    Short { cols_have: u32 },
}

fn advance_ws_columns(s: &str, target: u32) -> ColumnAdvance<'_> {
    let mut col = 0u32;
    let mut bytes = 0usize;
    for &b in s.as_bytes() {
        if col >= target {
            break;
        }
        match b {
            b' ' => {
                col += 1;
                bytes += 1;
            }
            b'\t' => {
                let next = (col / 4 + 1) * 4;
                if next > target {
                    return ColumnAdvance::Straddled {
                        rest: &s[bytes..],
                        cols_before_tab: col,
                    };
                }
                col = next;
                bytes += 1;
            }
            _ => break,
        }
    }
    if col >= target {
        ColumnAdvance::Reached(&s[bytes..])
    } else {
        ColumnAdvance::Short { cols_have: col }
    }
}

impl ContainerPrefix {
    /// Resolve `line` against this prefix's frame: the typed
    /// counterpart of [`Self::strip`], for continuation lines and
    /// lookahead. Reports whether every consumed column was real
    /// whitespace instead of guessing.
    pub fn resolve<'a>(&self, line: &'a str) -> FrameVerdict<'a> {
        self.resolve_with_skip(line, None)
    }

    /// Resolve the dispatch line (line 0): the typed counterpart of
    /// [`Self::strip_line_0_for_emission`]. The innermost `ListAdvance`
    /// is skipped when `list_marker_consumed_on_line_0` is false, since
    /// those bytes are the upstream-emitted marker, not indent.
    #[allow(dead_code)]
    pub fn resolve_line_0<'a>(&self, line: &'a str) -> FrameVerdict<'a> {
        let skip = if self.list_marker_consumed_on_line_0 {
            None
        } else {
            self.ops()
                .iter()
                .rposition(|op| matches!(op, StripOp::ListAdvance(_)))
        };
        self.resolve_with_skip(line, skip)
    }

    fn resolve_with_skip<'a>(&self, line: &'a str, skip_op: Option<usize>) -> FrameVerdict<'a> {
        let ops = self.ops();
        let mut s = line;
        let mut i = 0;
        while i < ops.len() {
            match ops[i] {
                StripOp::BlockQuoteMarker => {
                    let run = blockquote_run_len(&ops[i..]);
                    s = strip_bq_with_gobble(s, run, self.lazy_blockquote_gobble);
                    i += run;
                    continue;
                }
                StripOp::ListAdvance(n) => {
                    if skip_op != Some(i) {
                        match advance_ws_columns(s, n) {
                            ColumnAdvance::Reached(rest) => s = rest,
                            ColumnAdvance::Straddled {
                                rest,
                                cols_before_tab,
                            } => {
                                return FrameVerdict::StraddlingTab {
                                    rest,
                                    cols_before_tab,
                                };
                            }
                            ColumnAdvance::Short { .. } => {
                                return FrameVerdict::FakedIndent {
                                    rest: strip_list_indent(s, n as usize),
                                    op_index: i,
                                };
                            }
                        }
                    }
                }
                StripOp::ContentIndent(n) => match advance_ws_columns(s, n) {
                    ColumnAdvance::Reached(rest) => s = rest,
                    ColumnAdvance::Straddled {
                        rest,
                        cols_before_tab,
                    } => {
                        return FrameVerdict::StraddlingTab {
                            rest,
                            cols_before_tab,
                        };
                    }
                    ColumnAdvance::Short { cols_have } => {
                        return FrameVerdict::Dedented {
                            rest: strip_content_indent(s, n as usize).0,
                            op_index: i,
                            cols_short: n - cols_have,
                        };
                    }
                },
            }
            i += 1;
        }
        FrameVerdict::Inside { rest: s }
    }
}

/// Resolve a bare content-indent column: the typed twin of
/// [`strip_content_indent`], for callers that hold a pre-stripped line
/// and an absolute column (the `parse_inner_content` family) rather
/// than a [`ContainerPrefix`].
pub(crate) fn resolve_content_indent(line: &str, content_indent: usize) -> FrameVerdict<'_> {
    if content_indent == 0 {
        return FrameVerdict::Inside { rest: line };
    }
    match advance_ws_columns(line, content_indent as u32) {
        ColumnAdvance::Reached(rest) => FrameVerdict::Inside { rest },
        ColumnAdvance::Straddled {
            rest,
            cols_before_tab,
        } => FrameVerdict::StraddlingTab {
            rest,
            cols_before_tab,
        },
        ColumnAdvance::Short { cols_have } => FrameVerdict::Dedented {
            rest: strip_content_indent(line, content_indent).0,
            op_index: 0,
            cols_short: content_indent as u32 - cols_have,
        },
    }
}

fn apply_op(line: &str, op: StripOp) -> &str {
    match op {
        StripOp::ListAdvance(n) => strip_list_indent(line, n as usize),
        StripOp::BlockQuoteMarker => strip_n_blockquote_markers(line, 1),
        StripOp::ContentIndent(n) => strip_content_indent(line, n as usize).0,
    }
}

/// Skip the leading whitespace pandoc's blockquote gobble drops from a lazy
/// line — one carrying fewer `>` markers than the quote is deep.
///
/// Unbounded, and covers tabs: `pandoc -f markdown -t native` on
/// ``> ``` `` / `         deep` / ``> ``` `` yields `CodeBlock "deep"`, so
/// the skip is not capped at the three columns a block construct tolerates.
/// Mirrors `Parser::fold_lazy_line_into_blockquote`'s
/// `trim_start_matches([' ', '\t'])` on the dispatch line.
///
/// A blank line is left alone: it carries no markers, so it would look lazy
/// to every caller, but in pandoc's reader it ends the quote instead of
/// being gobbled into it.
fn lazy_gobble_trim(line: &str) -> &str {
    if line.trim().is_empty() {
        return line;
    }
    line.trim_start_matches([' ', '\t'])
}

fn blockquote_run_len(ops: &[StripOp]) -> usize {
    ops.iter()
        .take_while(|op| matches!(op, StripOp::BlockQuoteMarker))
        .count()
}

fn strip_bq_with_gobble(line: &str, bq_depth: usize, lazy_gobble: bool) -> &str {
    if bq_depth == 0 {
        return line;
    }
    let (rest, consumed) = strip_blockquote_markers_counted(line, bq_depth);
    if lazy_gobble && consumed < bq_depth {
        lazy_gobble_trim(rest)
    } else {
        rest
    }
}

/// Strip up to `content_indent` columns of leading whitespace from
/// `line`, returning the stripped slice and the consumed bytes (or
/// `None` when nothing was stripped).
///
/// Mirrors the strip done by `parse_inner_content` in `core.rs` for
/// footnote/definition base-indent: when the line's leading indent
/// reaches `content_indent`, strip exactly `content_indent` columns;
/// otherwise (lazy continuation) strip whatever leading indent exists.
///
/// The short-line branch measures with [`leading_indent`], which counts
/// spaces and tabs only. A line terminator is content, never container
/// prefix: claiming a blank line's newline tags it `LINE_PREFIX` inside a
/// code block, and the formatter folds that into the next line's indent,
/// drifting it one content column per pass.
pub(crate) fn strip_content_indent(line: &str, content_indent: usize) -> (&str, Option<&str>) {
    if content_indent == 0 {
        return (line, None);
    }
    let (indent_cols, indent_bytes) = leading_indent(line);
    if indent_cols >= content_indent {
        let idx = byte_index_at_column(line, content_indent);
        (&line[idx..], Some(&line[..idx]))
    } else if indent_bytes > 0 {
        (&line[indent_bytes..], Some(&line[..indent_bytes]))
    } else {
        (line, None)
    }
}

/// Lazy stripped view over `&self.lines[base..]`. The dispatcher builds
/// one of these per block dispatch from a [`ContainerPrefix`] and the
/// raw line buffer, then hands it to block parsers in place of the
/// historical `(ctx.content, &[&str], line_pos)` triple.
///
/// Strips are computed on access (no allocation): the returned `&'a str`
/// is always a sub-slice of one of `raw`'s entries, so the lifetime
/// matches the underlying source.
///
/// Three accessors, with deliberately different strip semantics:
///
/// * [`Self::first`] — emission-safe line-0 strip via
///   [`ContainerPrefix::strip_line_0_for_emission`]. For common stack
///   shapes (no nested footnote-inside-list-inside-definition) this
///   matches the byte boundary of `BlockContext::content` exactly;
///   parsers that need a guaranteed match should keep reading
///   `ctx.content` directly.
/// * [`Self::get`] — line `i` from `base`; emission-safe for `i == 0`,
///   unconditional [`ContainerPrefix::strip`] for `i > 0`. Mirrors what
///   parsers used to hand-roll with `prefix.strip(lines[line_pos + i])`.
/// * [`Self::first_unconditional`] — detection-time strip of line 0
///   that always advances past `list_content_col`, regardless of
///   `list_marker_consumed_on_line_0`. Used by parsers that scan for
///   shape (e.g. fenced-code open) where the indent must be skipped on
///   both marker and continuation lines.
///
/// Raw access via [`Self::raw`] / [`Self::raw_at`] is preserved for
/// helpers that need byte positions inside the original source (table
/// scans, indent-rule probes, byte-level lookahead).
pub(crate) struct StrippedLines<'a, 'p> {
    raw: &'a [&'a str],
    base: usize,
    /// Absolute index (into `raw`) of the dispatch line — the line whose
    /// container prefix the parser core already consumed. Equals `base`
    /// unless built via [`Self::with_dispatch`] (e.g. pipe tables, whose
    /// scan start can sit past a caption while dispatch stays at
    /// `line_pos`).
    dispatch: usize,
    prefix: &'p ContainerPrefix,
    hugo_lookahead: Option<&'p std::cell::RefCell<super::hugo::LookaheadCache>>,
}

#[allow(dead_code)]
impl<'a, 'p> StrippedLines<'a, 'p> {
    pub fn new(raw: &'a [&'a str], base: usize, prefix: &'p ContainerPrefix) -> Self {
        Self {
            raw,
            base,
            dispatch: base,
            prefix,
            hugo_lookahead: None,
        }
    }

    /// Like [`Self::new`] but names the dispatch line explicitly (absolute
    /// index into `raw`), for parsers whose dispatch line differs from the
    /// iteration start `base` — e.g. pipe tables scanning past a caption.
    pub fn with_dispatch(
        raw: &'a [&'a str],
        base: usize,
        dispatch: usize,
        prefix: &'p ContainerPrefix,
    ) -> Self {
        Self {
            raw,
            base,
            dispatch,
            prefix,
            hugo_lookahead: None,
        }
    }

    /// Number of lines available from `base` onward (line 0 plus lookahead).
    pub fn remaining(&self) -> usize {
        self.raw.len().saturating_sub(self.base)
    }

    pub(crate) fn with_hugo_lookahead(
        mut self,
        cache: &'p std::cell::RefCell<super::hugo::LookaheadCache>,
    ) -> Self {
        self.hugo_lookahead = Some(cache);
        self
    }

    pub(crate) fn hugo_lookahead(
        &self,
    ) -> Option<&'p std::cell::RefCell<super::hugo::LookaheadCache>> {
        self.hugo_lookahead
    }

    /// Line 0 with emission-safe strip semantics (matches the legacy
    /// `ctx.content` byte boundary for the common container stacks).
    pub fn first(&self) -> &'a str {
        self.prefix.strip_line_0_for_emission(self.raw[self.base])
    }

    /// Line `i` relative to `base`. Uses
    /// [`ContainerPrefix::strip_line_0_for_emission`] when `i == 0` and
    /// [`ContainerPrefix::strip`] otherwise — matching the behaviour
    /// of parsers that previously hand-rolled this split.
    #[allow(dead_code)]
    pub fn get(&self, i: usize) -> &'a str {
        let line = self.raw[self.base + i];
        if i == 0 {
            self.prefix.strip_line_0_for_emission(line)
        } else {
            self.prefix.strip(line)
        }
    }

    /// Detection-mode line-0 strip — always advances past
    /// `list_content_col`. Used when scanning for block shapes (fences,
    /// HRs) where the indent must be skipped regardless of whether the
    /// marker was upstream-emitted.
    #[allow(dead_code)]
    pub fn first_unconditional(&self) -> &'a str {
        self.prefix.strip_dispatch_line(self.raw[self.base])
    }

    /// Raw line buffer (full slice — index with `raw()[base + i]` or
    /// use [`Self::raw_at`]).
    #[allow(dead_code)]
    pub fn raw(&self) -> &'a [&'a str] {
        self.raw
    }

    /// Raw line at offset `i` from `base`, with no stripping.
    #[allow(dead_code)]
    pub fn raw_at(&self, i: usize) -> &'a str {
        self.raw[self.base + i]
    }

    /// Base offset into `raw` — equal to the legacy `line_pos`.
    #[allow(dead_code)]
    pub fn pos(&self) -> usize {
        self.base
    }

    /// Underlying [`ContainerPrefix`].
    #[allow(dead_code)]
    pub fn prefix(&self) -> &ContainerPrefix {
        self.prefix
    }

    /// Absolute index (into `raw`) of the dispatch line. Equals `base`
    /// unless built via [`Self::with_dispatch`].
    pub fn dispatch_pos(&self) -> usize {
        self.dispatch
    }

    /// Peek-strip the line at ABSOLUTE index `i`. Uses
    /// [`ContainerPrefix::strip_line_0_for_emission`] when `i` is the
    /// dispatch line, [`ContainerPrefix::strip`] otherwise. Pure
    /// detection — emits nothing.
    pub fn strip_at(&self, i: usize) -> &'a str {
        let line = self.raw[i];
        if i == self.dispatch {
            self.prefix.strip_line_0_for_emission(line)
        } else {
            self.prefix.strip(line)
        }
    }

    /// Peek-strip the line at ABSOLUTE index `i` with *detection*
    /// semantics: [`ContainerPrefix::strip_dispatch_line`] on the dispatch
    /// line, [`ContainerPrefix::strip`] otherwise.
    ///
    /// The counterpart to [`Self::strip_at`] for scans whose shape test is
    /// anchored at column 0 of the container's content: a continuation-line
    /// dispatch keeps its list indent under the emission strip, which would
    /// defeat such a test.
    pub fn detect_at(&self, i: usize) -> &'a str {
        let line = self.raw[i];
        if i == self.dispatch {
            self.prefix.strip_dispatch_line(line)
        } else {
            self.prefix.strip(line)
        }
    }

    /// Peek the tail [`Self::emit_prefix_at`] would return for the line at
    /// ABSOLUTE index `i`, emitting nothing.
    ///
    /// Use this — not [`Self::strip_at`] — when a decision made during
    /// classification has to hold at emission time. The two agree on
    /// every continuation line (both strip list indent with
    /// `strip_list_indent`), but `strip_at` applies the dispatch line's
    /// marker rule, which emission does not.
    ///
    /// Equals [`Self::emit_prefix_at`]'s tail by construction: both are
    /// the same faithful walk of [`ContainerPrefix::ops`], one with a
    /// builder and one without.
    pub fn peek_prefix_at(&self, i: usize) -> &'a str {
        walk_content_line_prefix(self.prefix, self.raw[i], None)
    }

    /// Emit the continuation-line container prefix for the line at
    /// ABSOLUTE index `i` as kind-tagged tokens, returning the
    /// post-prefix tail. A faithful walk of [`ContainerPrefix::ops`] in
    /// stack order (list/content indent as coalesced `WHITESPACE`,
    /// blockquote prefix byte-by-byte); [`Self::peek_prefix_at`] is the
    /// same walk without the builder, so classify-then-emit callers get
    /// their invariant by construction. Use for continuation lines only;
    /// for the dispatch line use [`Self::dispatch_tail`].
    pub fn emit_prefix_at(&self, builder: &mut GreenNodeBuilder<'static>, i: usize) -> &'a str {
        walk_content_line_prefix(self.prefix, self.raw[i], Some(builder))
    }

    /// Dispatch-line tail for emission — emits no prefix tokens (the core
    /// already emitted them upstream). Equals
    /// `prefix.strip_line_0_for_emission(raw[dispatch])`.
    pub fn dispatch_tail(&self) -> &'a str {
        self.prefix
            .strip_line_0_for_emission(self.raw[self.dispatch])
    }

    /// Emission tail for the line at ABSOLUTE index `i`, picking the right
    /// strategy by position: the dispatch line re-emits only the innermost
    /// `ContentIndent` bytes (list markers and blockquote prefixes were
    /// consumed upstream by the core, but a footnote/definition body's
    /// content indent never is — block parsers own those bytes, mirroring
    /// the paragraph and fenced-code paths); every other line re-emits its
    /// full container prefix as tokens via [`Self::emit_prefix_at`].
    /// Consolidates the `if i == dispatch { … } else { … }` idiom repeated
    /// across the table emitters.
    pub fn emit_or_dispatch_tail(
        &self,
        builder: &mut GreenNodeBuilder<'static>,
        i: usize,
    ) -> &'a str {
        if i == self.dispatch {
            let (mut tail, indent) = self
                .prefix
                .strip_line_0_with_indent_emit(self.raw[self.dispatch]);
            if let Some(ws) = indent
                && !ws.is_empty()
            {
                builder.token(SyntaxKind::LINE_PREFIX.into(), ws);
            }
            if !self.prefix.list_marker_consumed_on_line_0
                && let Some(StripOp::ListAdvance(n)) = self.prefix.ops().last()
            {
                let peeled = strip_list_indent(tail, *n as usize);
                let consumed = tail.len() - peeled.len();
                if consumed > 0 {
                    builder.token(SyntaxKind::LINE_PREFIX.into(), &tail[..consumed]);
                    tail = peeled;
                }
            }
            tail
        } else {
            self.emit_prefix_at(builder, i)
        }
    }

    /// Iterate `(absolute_index, raw_line, peek_stripped)` from `base` to
    /// the end of the buffer. `peek_stripped` follows the same
    /// dispatch-aware rule as [`Self::strip_at`].
    pub fn iter_from_base(&self) -> impl Iterator<Item = (usize, &'a str, &'a str)> + '_ {
        (self.base..self.raw.len()).map(move |i| (i, self.raw[i], self.strip_at(i)))
    }
}

/// Strip up to `list_content_col` columns of leading whitespace,
/// stopping at the first non-whitespace byte (newlines stop the scan
/// rather than being consumed — important on blank lines inside a
/// fenced code block). Mirrors the legacy
/// `byte_index_at_column`-based strip used by the formatter.
pub(crate) fn strip_list_indent(line: &str, list_content_col: usize) -> &str {
    if list_content_col == 0 {
        return line;
    }
    let idx = byte_index_at_column(line, list_content_col);
    &line[idx..]
}

/// Returns `true` iff the outermost active container in `prefix` is a
/// blockquote (i.e. `prefix.ops()` starts with `BlockQuoteMarker`
/// before any `ListAdvance`). Used to pick the bq-vs-list strip order
/// on content/lookahead lines.
pub(crate) fn bq_outer_of_list(prefix: &ContainerPrefix) -> bool {
    for op in prefix.ops() {
        match op {
            StripOp::BlockQuoteMarker => return true,
            StripOp::ListAdvance(_) => return false,
            StripOp::ContentIndent(_) => {}
        }
    }
    false
}

pub(crate) fn emit_blockquote_prefix_tokens(builder: &mut GreenNodeBuilder<'static>, prefix: &str) {
    for ch in prefix.chars() {
        let mut buf = [0u8; 4];
        builder.token(SyntaxKind::LINE_PREFIX.into(), ch.encode_utf8(&mut buf));
    }
}

/// Scalar-driven continuation-line strip, in the fixed order
/// `bq_outer ? bq → list → content-indent : list → bq → content-indent`.
///
/// Retained only for the marker-line (line 0) scalar emitters in
/// `code_blocks.rs` / `line_blocks.rs` (`prepare_fence_open_line`,
/// `emit_open_line_prefixes`, the hashpipe preamble helpers), which
/// derive the same scalars and must stay byte-identical to this strip.
/// Continuation lines go through [`StrippedLines::peek_prefix_at`] /
/// [`StrippedLines::emit_prefix_at`], which walk
/// [`ContainerPrefix::ops`] in true stack order instead of collapsing
/// them to scalars.
pub(crate) fn content_line_prefix_tail<'a>(
    content_line: &'a str,
    bq_depth: usize,
    list_content_col: usize,
    bq_outer: bool,
    content_indent: usize,
    lazy_gobble: bool,
) -> &'a str {
    let mut s = content_line;

    let strip_list = |s: &mut &'a str| {
        if list_content_col > 0 {
            *s = strip_list_indent(s, list_content_col);
        }
    };
    let strip_bq = |s: &mut &'a str| {
        if bq_depth > 0 {
            *s = strip_bq_with_gobble(s, bq_depth, lazy_gobble);
        }
    };

    if bq_outer {
        strip_bq(&mut s);
        strip_list(&mut s);
    } else {
        strip_list(&mut s);
        strip_bq(&mut s);
    }

    if content_indent > 0 {
        let indent_bytes = byte_index_at_column(s, content_indent);
        if s.len() >= indent_bytes && indent_bytes > 0 {
            s = &s[indent_bytes..];
        }
    }

    s
}

/// One raw piece of a continuation line's consumed container prefix,
/// as captured by [`prefix_pieces`]. `LazyGobble` is whitespace a lazy
/// line lost to the Pandoc gobble; it stays distinct because the two
/// consumers tokenize it differently: [`walk_content_line_prefix`]
/// coalesces it into the surrounding `WHITESPACE` run, while
/// [`ContainerPrefix::split_pieces`] appends it to the preceding bq
/// run for byte-by-byte re-injection (the legacy `split` behavior).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RawPieceKind {
    /// List or content indent, emitted as `WHITESPACE`.
    Indent,
    /// A blockquote marker run's bytes. Pushed at every marker-run op,
    /// even when it consumed nothing, because the op is a `WHITESPACE`
    /// coalescing boundary: indent before a quote and gobbled bytes
    /// after it must stay separate tokens.
    BqRun,
    /// Whitespace consumed by the Pandoc lazy gobble on a line short of
    /// its `>` markers.
    LazyGobble,
}

type RawPieces = SmallVec<[(RawPieceKind, Range<usize>); 4]>;

fn prefix_pieces<'a>(prefix: &ContainerPrefix, content_line: &'a str) -> (RawPieces, &'a str) {
    let ops = prefix.ops();
    let mut pieces = RawPieces::new();
    let offset = |s: &str| content_line.len() - s.len();
    let mut s = content_line;
    let mut i = 0;
    while i < ops.len() {
        match ops[i] {
            StripOp::ListAdvance(n) => {
                let stripped = strip_list_indent(s, n as usize);
                if stripped.len() < s.len() {
                    pieces.push((RawPieceKind::Indent, offset(s)..offset(stripped)));
                    s = stripped;
                }
                i += 1;
            }
            StripOp::BlockQuoteMarker => {
                let run = blockquote_run_len(&ops[i..]);
                let (stripped, consumed) = strip_blockquote_markers_counted(s, run);
                pieces.push((RawPieceKind::BqRun, offset(s)..offset(stripped)));
                s = stripped;
                if prefix.lazy_blockquote_gobble && consumed < run {
                    let gobbled = lazy_gobble_trim(s);
                    if gobbled.len() < s.len() {
                        pieces.push((RawPieceKind::LazyGobble, offset(s)..offset(gobbled)));
                        s = gobbled;
                    }
                }
                i += run;
            }
            StripOp::ContentIndent(n) => {
                let (stripped, consumed) = strip_content_indent(s, n as usize);
                if consumed.is_some() {
                    pieces.push((RawPieceKind::Indent, offset(s)..offset(stripped)));
                    s = stripped;
                }
                i += 1;
            }
        }
    }
    (pieces, s)
}

fn walk_content_line_prefix<'a>(
    prefix: &ContainerPrefix,
    content_line: &'a str,
    builder: Option<&mut GreenNodeBuilder<'static>>,
) -> &'a str {
    let (pieces, tail) = prefix_pieces(prefix, content_line);
    if let Some(b) = builder {
        let mut pending_ws: Option<Range<usize>> = None;
        for (kind, range) in pieces {
            match kind {
                RawPieceKind::Indent | RawPieceKind::LazyGobble => {
                    pending_ws = Some(match pending_ws {
                        Some(pending) => pending.start..range.end,
                        None => range,
                    });
                }
                RawPieceKind::BqRun => {
                    if let Some(pending) = pending_ws.take() {
                        b.token(SyntaxKind::LINE_PREFIX.into(), &content_line[pending]);
                    }
                    if !range.is_empty() {
                        emit_blockquote_prefix_tokens(b, &content_line[range]);
                    }
                }
            }
        }
        if let Some(pending) = pending_ws {
            b.token(SyntaxKind::LINE_PREFIX.into(), &content_line[pending]);
        }
    }
    tail
}

/// Advance past `target` columns of `line`, counting **any** character
/// as a column. Tabs round up to the next 4-column stop; a tab that
/// would overshoot the target is left intact (mirrors
/// `strip_list_item_indent`'s tab handling). Newlines / CR short-circuit
/// to the empty string — the line ended before the target was reached.
///
/// Reserved for the one case where the bytes being skipped are known
/// *not* to be whitespace: the list marker the parser core has already
/// emitted upstream on a marker-line dispatch (`- `, `1. `). It is a
/// content-eater by construction, so it must never stand in for an
/// indent strip — a `ListAdvance` op that means "the item's content
/// indent" resolves through [`strip_list_indent`], which stops at the
/// first non-whitespace byte. Conflating the two is how an
/// under-indented lazy line loses its first bytes to a column the line
/// never had.
pub(in crate::parser::blocks) fn advance_emitted_marker_columns(line: &str, target: usize) -> &str {
    if target == 0 {
        return line;
    }
    let mut col = 0usize;
    for (i, ch) in line.char_indices() {
        if col >= target {
            return &line[i..];
        }
        match ch {
            '\n' | '\r' => return "",
            '\t' => {
                let next = (col / 4 + 1) * 4;
                if next > target {
                    return &line[i..];
                }
                col = next;
            }
            _ => {
                col += 1;
            }
        }
    }
    ""
}

/// Per-line container-prefix re-injection state used by graft helpers
/// when content is reparsed from prefix-stripped text. Each entry
/// captures the prefix bytes that were stripped from one source line of
/// the body before the recursive parse; during graft, those bytes are
/// re-emitted as kind-tagged tokens at the start of each grafted line
/// so the result CST stays byte-equal to the source.
///
/// Folds the predecessor `BqPrefixState` (`html_blocks.rs`) and
/// `LinePrefixState` (`utils/list_item_buffer.rs`) — bq + list-indent
/// on the same line both round-trip cleanly under one structure. The
/// emitted tokenization is preserved: list-indent goes out as a single
/// `WHITESPACE` token (legacy `LinePrefixState` behavior), bq prefix
/// goes out byte-by-byte (legacy `BqPrefixState` byte-walker).
pub(crate) struct ContainerPrefixState {
    pub prefixes: Vec<ContainerPrefixLine>,
    pub line_idx: usize,
    pub at_line_start: bool,
}

impl ContainerPrefixState {
    /// Wrap a per-line prefix vector. Returns `None` when every entry
    /// is empty — callers should pass `&mut None` to graft helpers in
    /// that case to skip re-injection entirely.
    pub fn new(prefixes: Vec<ContainerPrefixLine>) -> Option<Self> {
        if prefixes.iter().all(ContainerPrefixLine::is_empty) {
            None
        } else {
            Some(Self {
                prefixes,
                line_idx: 0,
                at_line_start: true,
            })
        }
    }
}

/// One ordered piece of a line's container prefix. The kind decides
/// tokenization at emission time; the piece order is the order the bytes
/// appear in the source line, so nesting depth is not capped by the
/// representation (`- > - a`'s continuation lines carry indent, quote
/// markers, indent — three ordered pieces).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PrefixPieceKind {
    /// Indent bytes (list-item indent, content indent, lazily gobbled
    /// whitespace) — emitted as one coalesced `WHITESPACE` token.
    Indent,
    /// Blockquote prefix bytes (a mix of `>` and inter-marker
    /// whitespace) — emitted byte-by-byte as `BLOCK_QUOTE_MARKER` /
    /// `WHITESPACE` tokens.
    BqRun,
}

/// One per-line entry in [`ContainerPrefixState`]: the line's container
/// prefix as an ordered list of kind-tagged byte runs, captured in
/// source order.
#[derive(Clone, Debug, Default)]
pub(crate) struct ContainerPrefixLine {
    pieces: SmallVec<[(PrefixPieceKind, String); 2]>,
}

impl ContainerPrefixLine {
    pub fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }

    /// Append indent bytes, coalescing into a trailing `Indent` piece so
    /// adjacent indent runs keep emitting as one `WHITESPACE` token.
    pub fn push_indent(&mut self, bytes: &str) {
        self.push(PrefixPieceKind::Indent, bytes);
    }

    /// Append blockquote prefix bytes, coalescing into a trailing `BqRun`.
    pub fn push_bq(&mut self, bytes: &str) {
        self.push(PrefixPieceKind::BqRun, bytes);
    }

    fn push(&mut self, kind: PrefixPieceKind, bytes: &str) {
        if bytes.is_empty() {
            return;
        }
        if let Some((last_kind, run)) = self.pieces.last_mut()
            && *last_kind == kind
        {
            run.push_str(bytes);
        } else {
            self.pieces.push((kind, bytes.to_string()));
        }
    }

    pub fn bq_only(bq_prefix: String) -> Self {
        let mut line = Self::default();
        line.push_bq(&bq_prefix);
        line
    }

    pub fn list_only(list_indent: String) -> Self {
        let mut line = Self::default();
        line.push_indent(&list_indent);
        line
    }

    /// A line inside a quoted list item: the quote's markers, then the
    /// item's own indent.
    pub fn bq_then_list(bq_prefix: String, list_indent: String) -> Self {
        let mut line = Self::default();
        line.push_bq(&bq_prefix);
        line.push_indent(&list_indent);
        line
    }

    /// Test-only view of the ordered pieces.
    #[cfg(test)]
    pub(crate) fn pieces(&self) -> &[(PrefixPieceKind, String)] {
        &self.pieces
    }
}

/// Emit a captured per-line container prefix as `LINE_PREFIX` tokens, in
/// piece order. Indent pieces go out as one token; bq pieces go out
/// byte-by-byte.
///
/// The byte-by-byte split is load-bearing, not cosmetic: the formatter
/// reads these tokens back when rebuilding a quoted line and drops only
/// the marker byte and its own padding space, keeping indent pieces (see
/// the `html_block_div_definition_body_later_line_blockquote` golden
/// case). Coalescing the run would erase that distinction.
pub(crate) fn emit_container_prefix_tokens(
    builder: &mut GreenNodeBuilder<'static>,
    line: &ContainerPrefixLine,
) {
    for (kind, bytes) in &line.pieces {
        match kind {
            PrefixPieceKind::Indent => {
                builder.token(SyntaxKind::LINE_PREFIX.into(), bytes);
            }
            PrefixPieceKind::BqRun => {
                for ch in bytes.chars() {
                    let mut buf = [0u8; 4];
                    builder.token(SyntaxKind::LINE_PREFIX.into(), ch.encode_utf8(&mut buf));
                }
            }
        }
    }
}

/// Emit a single token while optionally injecting the current line's
/// container prefix tokens at line starts. When `prefix` is `None`, this
/// is a plain `builder.token()` passthrough.
///
/// Shared by the graft walkers in `list_item_buffer` and `html_blocks`,
/// which re-inject held-out per-line prefixes when splicing a reparsed
/// subtree back into the host tree.
pub(crate) fn emit_grafted_token(
    builder: &mut GreenNodeBuilder<'static>,
    kind: SyntaxKind,
    text: &str,
    prefix: &mut Option<ContainerPrefixState>,
) {
    if let Some(state) = prefix.as_mut() {
        if state.at_line_start {
            if let Some(line_prefix) = state.prefixes.get(state.line_idx) {
                emit_container_prefix_tokens(builder, line_prefix);
            }
            state.at_line_start = false;
        }
        builder.token(kind.into(), text);
        if kind == SyntaxKind::NEWLINE || kind == SyntaxKind::BLANK_LINE {
            state.line_idx += 1;
            state.at_line_start = true;
        }
    } else {
        builder.token(kind.into(), text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(dialect: Dialect) -> ParserOptions {
        let flavor = match dialect {
            Dialect::Pandoc => crate::options::Flavor::Pandoc,
            Dialect::CommonMark => crate::options::Flavor::CommonMark,
        };
        ParserOptions {
            flavor,
            dialect,
            extensions: crate::options::Extensions::for_flavor(flavor),
            ..Default::default()
        }
    }

    #[test]
    fn strip_bq_only_matches_legacy() {
        let p = ContainerPrefix::bq_only(1);
        assert_eq!(p.strip("> foo"), "foo");
        assert_eq!(p.strip(">> foo"), "> foo");
        assert_eq!(p.strip("> "), "");
        assert_eq!(p.strip("plain"), "plain");
    }

    #[test]
    fn with_extra_blockquotes_matches_a_deeper_stack() {
        let base = ContainerPrefix::from_stack(
            &[Container::BlockQuote {}],
            false,
            &opts(Dialect::CommonMark),
        );
        let deeper = base.with_extra_blockquotes(1);
        assert_eq!(deeper.bq_depth(), 2);
        assert_eq!(deeper.strip("> > a"), "a");
        assert_eq!(base.strip_line_0_for_emission("> > a"), "> a");
        assert_eq!(deeper.strip_line_0_for_emission("> > a"), "a");
        let from_stack = ContainerPrefix::from_stack(
            &[Container::BlockQuote {}, Container::BlockQuote {}],
            false,
            &opts(Dialect::CommonMark),
        );
        assert_eq!(deeper.ops().len(), from_stack.ops().len());
        assert_eq!(deeper.bq_depth(), from_stack.bq_depth());
        assert_eq!(base.with_extra_blockquotes(0).bq_depth(), 1);
    }

    #[test]
    fn from_scalars_round_trips_marker_line_caller_combos() {
        let check = |bq: usize, lcc: usize, ci: usize, lmc0: bool| {
            let bq_outer = bq > 0;
            let p = ContainerPrefix::from_scalars(bq, lcc, bq_outer, ci, lmc0, Dialect::CommonMark);
            assert_eq!(p.bq_depth(), bq, "bq_depth");
            assert_eq!(p.list_content_col(), lcc, "list_content_col");
            assert_eq!(p.content_indent(), ci, "content_indent");
            assert_eq!(bq_outer_of_list(&p), bq_outer, "bq_outer_of_list");
            assert_eq!(
                p.list_marker_consumed_on_line_0, lmc0,
                "list_marker_consumed_on_line_0"
            );
        };

        check(0, 4, 0, true);
        check(1, 4, 0, true);
        check(0, 0, 4, false);
        check(2, 0, 4, false);
    }

    #[test]
    fn strip_list_marker_line() {
        let p =
            ContainerPrefix::from_ops(&[StripOp::ListAdvance(2), StripOp::BlockQuoteMarker], true);
        assert_eq!(p.strip_dispatch_line("- > <div>"), "<div>");
        assert_eq!(p.strip("- > <div>"), "- > <div>");
    }

    #[test]
    fn strip_list_continuation_line() {
        let p =
            ContainerPrefix::from_ops(&[StripOp::ListAdvance(2), StripOp::BlockQuoteMarker], false);
        assert_eq!(p.strip("  > hello"), "hello");
    }

    #[test]
    fn strip_tab_indent_rounds_to_four() {
        let p = ContainerPrefix::from_ops(&[StripOp::ListAdvance(4)], false);
        assert_eq!(p.strip("\tfoo"), "foo");
    }

    #[test]
    fn strip_short_line_keeps_its_bytes() {
        let p = ContainerPrefix::from_ops(&[StripOp::ListAdvance(4)], false);
        assert_eq!(p.strip(""), "");
        assert_eq!(p.strip("\n"), "\n");
    }

    #[test]
    fn advance_emitted_marker_columns_lands_on_char_boundary_for_multibyte() {
        assert_eq!(advance_emitted_marker_columns("├── x", 2), "─ x");
        assert_eq!(advance_emitted_marker_columns("éxy", 1), "xy");
        assert_eq!(advance_emitted_marker_columns("黑x", 1), "x");
        assert_eq!(advance_emitted_marker_columns("😄.", 1), ".");
        assert_eq!(advance_emitted_marker_columns("├──", 5), "");
        assert_eq!(advance_emitted_marker_columns("  > hello", 2), "> hello");
        assert_eq!(advance_emitted_marker_columns("\tfoo", 4), "foo");
    }

    #[test]
    fn stripped_lines_first_matches_strip_line_0_for_emission() {
        let prefix =
            ContainerPrefix::from_ops(&[StripOp::ListAdvance(2), StripOp::BlockQuoteMarker], true);
        let raw = ["- > <div>", "  > foo"];
        let lines = StrippedLines::new(&raw, 0, &prefix);
        assert_eq!(lines.first(), "<div>");
        assert_eq!(lines.first(), prefix.strip_line_0_for_emission(raw[0]));
    }

    #[test]
    fn stripped_lines_first_skips_list_col_only_when_marker_consumed() {
        let prefix_continuation = ContainerPrefix::from_ops(&[StripOp::ListAdvance(2)], false);
        let raw = ["  continuation"];
        let lines = StrippedLines::new(&raw, 0, &prefix_continuation);
        assert_eq!(lines.first(), "  continuation");
        assert_eq!(lines.first_unconditional(), "continuation");

        let prefix_marker = ContainerPrefix::from_ops(&[StripOp::ListAdvance(2)], true);
        let lines = StrippedLines::new(&raw, 0, &prefix_marker);
        assert_eq!(lines.first(), "continuation");
    }

    #[test]
    fn stripped_lines_get_uses_unconditional_strip_after_line_0() {
        let prefix = ContainerPrefix::from_ops(&[StripOp::ListAdvance(2)], false);
        let raw = ["  foo", "  bar", "  baz"];
        let lines = StrippedLines::new(&raw, 0, &prefix);
        assert_eq!(lines.get(0), "  foo");
        assert_eq!(lines.get(1), "bar");
        assert_eq!(lines.get(2), "baz");
    }

    #[test]
    fn stripped_lines_raw_access_is_unstripped() {
        let prefix =
            ContainerPrefix::from_ops(&[StripOp::ListAdvance(2), StripOp::BlockQuoteMarker], true);
        let raw = ["- > foo", "  > bar"];
        let lines = StrippedLines::new(&raw, 0, &prefix);
        assert_eq!(lines.raw_at(0), "- > foo");
        assert_eq!(lines.raw_at(1), "  > bar");
        assert_eq!(lines.raw().len(), 2);
        assert_eq!(lines.pos(), 0);
    }

    #[test]
    fn stripped_lines_respects_base_offset() {
        let prefix = ContainerPrefix::default();
        let raw = ["pre", "first", "second"];
        let lines = StrippedLines::new(&raw, 1, &prefix);
        assert_eq!(lines.first(), "first");
        assert_eq!(lines.get(0), "first");
        assert_eq!(lines.get(1), "second");
        assert_eq!(lines.pos(), 1);
        assert_eq!(lines.raw_at(0), "first");
    }

    #[test]
    fn peek_prefix_at_agrees_with_emit_prefix_at() {
        let prefix = ContainerPrefix::from_ops(&[StripOp::ListAdvance(2)], false);
        let raw = ["  | a", " b |", "   c", "x", "  | d"];
        let lines = StrippedLines::new(&raw, 0, &prefix);
        for (i, raw_line) in raw.iter().enumerate().skip(1) {
            let mut builder = GreenNodeBuilder::new();
            builder.start_node(SyntaxKind::DOCUMENT.into());
            let emitted = lines.emit_prefix_at(&mut builder, i);
            builder.finish_node();
            assert_eq!(lines.peek_prefix_at(i), emitted, "line {i}: {raw_line:?}");
        }
        assert_eq!(lines.strip_at(1), "b |");
        assert_eq!(lines.peek_prefix_at(1), "b |");
    }

    #[test]
    fn strip_at_matches_hand_rolled_table_closure() {
        let prefix =
            ContainerPrefix::from_ops(&[StripOp::ListAdvance(2), StripOp::BlockQuoteMarker], true);
        let raw = ["- > | a |", "  > |---|", "  > | 1 |"];
        let dispatch = 0;
        let lines = StrippedLines::with_dispatch(&raw, 0, dispatch, &prefix);
        for (i, &raw_line) in raw.iter().enumerate() {
            let expected = if i == dispatch {
                prefix.strip_line_0_for_emission(raw_line)
            } else {
                prefix.strip(raw_line)
            };
            assert_eq!(lines.strip_at(i), expected, "strip_at({i})");
        }
    }

    #[test]
    fn strip_at_honors_dispatch_offset_past_base() {
        let prefix =
            ContainerPrefix::from_ops(&[StripOp::ListAdvance(2), StripOp::BlockQuoteMarker], true);
        let raw = [": caption", "- > header", "  > sep"];
        let dispatch = 1;
        let lines = StrippedLines::with_dispatch(&raw, 0, dispatch, &prefix);
        assert_eq!(lines.strip_at(0), prefix.strip(raw[0]));
        assert_eq!(lines.strip_at(2), prefix.strip(raw[2]));
        assert_eq!(
            lines.strip_at(dispatch),
            prefix.strip_line_0_for_emission(raw[dispatch])
        );
        assert_eq!(
            lines.dispatch_tail(),
            prefix.strip_line_0_for_emission(raw[dispatch])
        );
        assert_eq!(lines.dispatch_pos(), dispatch);
    }

    #[test]
    fn iter_from_base_yields_absolute_index_and_peek() {
        let prefix = ContainerPrefix::default();
        let raw = ["pre", "first", "second"];
        let lines = StrippedLines::new(&raw, 1, &prefix);
        let collected: Vec<(usize, &str, &str)> = lines.iter_from_base().collect();
        assert_eq!(
            collected,
            vec![(1, "first", "first"), (2, "second", "second")]
        );
    }

    #[test]
    fn emit_prefix_at_returns_continuation_tail() {
        let prefix =
            ContainerPrefix::from_ops(&[StripOp::ListAdvance(2), StripOp::BlockQuoteMarker], true);
        let raw = ["- > header", "  > hello"];
        let lines = StrippedLines::new(&raw, 0, &prefix);
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(SyntaxKind::DOCUMENT.into());
        let tail = lines.emit_prefix_at(&mut builder, 1);
        builder.finish_node();
        assert_eq!(tail, "hello");
        assert_eq!(tail, lines.peek_prefix_at(1));
    }

    fn emitted_tokens(line: &ContainerPrefixLine) -> Vec<(SyntaxKind, String)> {
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(SyntaxKind::DOCUMENT.into());
        emit_container_prefix_tokens(&mut builder, line);
        builder.finish_node();
        let node = crate::syntax::SyntaxNode::new_root(builder.finish());
        node.children_with_tokens()
            .filter_map(|c| c.into_token())
            .map(|t| (t.kind(), t.text().to_string()))
            .collect()
    }

    #[test]
    fn container_prefix_line_emits_pieces_in_capture_order() {
        let line = ContainerPrefixLine::bq_then_list("> ".to_string(), "  ".to_string());
        assert_eq!(
            emitted_tokens(&line),
            vec![
                (SyntaxKind::LINE_PREFIX, ">".to_string()),
                (SyntaxKind::LINE_PREFIX, " ".to_string()),
                (SyntaxKind::LINE_PREFIX, "  ".to_string()),
            ]
        );

        let mut line = ContainerPrefixLine::default();
        line.push_indent("  ");
        line.push_bq("> ");
        assert_eq!(
            emitted_tokens(&line),
            vec![
                (SyntaxKind::LINE_PREFIX, "  ".to_string()),
                (SyntaxKind::LINE_PREFIX, ">".to_string()),
                (SyntaxKind::LINE_PREFIX, " ".to_string()),
            ]
        );
    }

    #[test]
    fn container_prefix_line_expresses_three_pieces() {
        let mut line = ContainerPrefixLine::default();
        line.push_indent("  ");
        line.push_bq("> ");
        line.push_indent("  ");
        assert_eq!(
            emitted_tokens(&line),
            vec![
                (SyntaxKind::LINE_PREFIX, "  ".to_string()),
                (SyntaxKind::LINE_PREFIX, ">".to_string()),
                (SyntaxKind::LINE_PREFIX, " ".to_string()),
                (SyntaxKind::LINE_PREFIX, "  ".to_string()),
            ]
        );
    }

    #[test]
    fn container_prefix_line_coalesces_adjacent_same_kind_pushes() {
        let mut line = ContainerPrefixLine::default();
        line.push_indent("  ");
        line.push_indent("  ");
        assert_eq!(
            emitted_tokens(&line),
            vec![(SyntaxKind::LINE_PREFIX, "    ".to_string())]
        );

        let mut line = ContainerPrefixLine::default();
        line.push_indent("");
        line.push_bq("");
        assert!(line.is_empty());
    }

    #[test]
    fn resolve_rejects_faked_indent() {
        let p = ContainerPrefix::from_ops(&[StripOp::ListAdvance(2)], false);
        assert_eq!(p.strip("c :\n"), "c :\n");
        assert!(!p.resolve("c :\n").reaches_frame());
        assert!(!p.resolve(" c :\n").reaches_frame());
        assert!(p.resolve("  : def\n").reaches_frame());
        assert!(p.resolve("    : def\n").reaches_frame());
        assert!(p.resolve("\t: def\n").reaches_frame());
        assert!(ContainerPrefix::default().resolve("c :\n").reaches_frame());
    }

    #[test]
    fn resolve_measures_list_advance_after_outer_ops() {
        let p =
            ContainerPrefix::from_ops(&[StripOp::BlockQuoteMarker, StripOp::ListAdvance(2)], false);
        assert!(!p.resolve("> c :\n").reaches_frame());
        assert!(p.resolve(">   : def\n").reaches_frame());
    }

    #[test]
    fn strip_content_indent_only() {
        let p = ContainerPrefix::from_ops(&[StripOp::ContentIndent(4)], false);
        assert_eq!(p.strip("    continuation"), "continuation");
        assert_eq!(
            p.strip_line_0_for_emission("    continuation"),
            "continuation"
        );
    }

    #[test]
    fn strip_content_indent_inside_blockquote() {
        let p = ContainerPrefix::from_ops(
            &[StripOp::BlockQuoteMarker, StripOp::ContentIndent(4)],
            false,
        );
        assert_eq!(p.strip(">     continuation"), "continuation");
    }

    #[test]
    fn strip_blockquote_inside_content_indent() {
        let p = ContainerPrefix::from_ops(
            &[StripOp::ContentIndent(4), StripOp::BlockQuoteMarker],
            false,
        );
        assert_eq!(p.strip("    >quoted"), "quoted");
    }

    #[test]
    fn strip_definition_above_list_above_bq() {
        let p = ContainerPrefix::from_ops(
            &[
                StripOp::ContentIndent(4),
                StripOp::ListAdvance(2),
                StripOp::BlockQuoteMarker,
            ],
            false,
        );
        assert_eq!(p.strip("    - > a"), "- > a");
        let marked = ContainerPrefix::from_ops(
            &[
                StripOp::ContentIndent(4),
                StripOp::ListAdvance(2),
                StripOp::BlockQuoteMarker,
            ],
            true,
        );
        assert_eq!(marked.strip_dispatch_line("    - > a"), "a");
    }

    #[test]
    fn strip_content_indent_lazy_continuation() {
        let p = ContainerPrefix::from_ops(&[StripOp::ContentIndent(4)], false);
        let (stripped, emit) = p.strip_line_0_with_indent_emit("  short");
        assert_eq!(stripped, "short");
        assert_eq!(emit, Some("  "));
    }

    #[test]
    fn strip_content_indent_leaves_a_blank_line_intact() {
        for line in ["\n", "\r\n", ""] {
            assert_eq!(strip_content_indent(line, 4), (line, None), "on {line:?}");
        }
        assert_eq!(strip_content_indent("  \n", 4), ("\n", Some("  ")));
    }

    #[test]
    fn strip_content_indent_with_list_marker_consumed() {
        let p =
            ContainerPrefix::from_ops(&[StripOp::ListAdvance(2), StripOp::ContentIndent(4)], true);
        assert_eq!(
            p.strip_line_0_for_emission("-     footnote text"),
            "footnote text"
        );
    }

    #[test]
    fn strip_content_indent_zero_is_passthrough() {
        let p = ContainerPrefix::default();
        assert_eq!(p.strip("no indent here"), "no indent here");
        let (stripped, emit) = p.strip_line_0_with_indent_emit("no indent here");
        assert_eq!(stripped, "no indent here");
        assert_eq!(emit, None);
    }

    #[test]
    fn from_stack_picks_only_innermost_list_item() {
        use crate::parser::blocks::lists::ListMarker;
        use crate::parser::utils::list_item_buffer::ListItemBuffer;
        let stack = vec![
            Container::List {
                marker: ListMarker::Bullet('-'),
                base_indent_cols: 0,
                has_blank_between_items: false,
            },
            Container::ListItem {
                content_col: 2,
                buffer: ListItemBuffer::new(),
                marker_only: false,
                virtual_marker_space: false,
            },
            Container::List {
                marker: ListMarker::Bullet('-'),
                base_indent_cols: 2,
                has_blank_between_items: false,
            },
            Container::ListItem {
                content_col: 4,
                buffer: ListItemBuffer::new(),
                marker_only: false,
                virtual_marker_space: false,
            },
        ];
        let p = ContainerPrefix::from_stack(&stack, true, &opts(Dialect::CommonMark));
        assert_eq!(p.strip_dispatch_line("- - foo"), "foo");
        assert_eq!(p.strip("    foo"), "foo");
    }

    #[test]
    fn resolve_agrees_with_strip_when_indent_is_real() {
        let p = ContainerPrefix::from_ops(
            &[
                StripOp::ContentIndent(4),
                StripOp::ListAdvance(2),
                StripOp::BlockQuoteMarker,
            ],
            false,
        );
        for line in ["      > b", "      >b", "       > b"] {
            assert_eq!(
                p.resolve(line),
                FrameVerdict::Inside {
                    rest: p.strip(line)
                },
                "resolve vs strip on {line:?}"
            );
        }
    }

    #[test]
    fn resolve_line_0_skips_the_unconsumed_marker_advance() {
        let p = ContainerPrefix::from_ops(&[StripOp::ListAdvance(2)], false);
        assert_eq!(
            p.resolve_line_0("continuation"),
            FrameVerdict::Inside {
                rest: "continuation"
            }
        );
        let p = ContainerPrefix::from_ops(&[StripOp::ListAdvance(2)], true);
        assert_eq!(
            p.resolve_line_0("c :"),
            FrameVerdict::FakedIndent {
                rest: "c :",
                op_index: 0
            }
        );
    }

    #[test]
    fn resolve_reports_the_first_failing_op() {
        let p =
            ContainerPrefix::from_ops(&[StripOp::ContentIndent(4), StripOp::ListAdvance(2)], false);
        assert_eq!(
            p.resolve("    x"),
            FrameVerdict::FakedIndent {
                rest: "x",
                op_index: 1
            }
        );
        assert_eq!(
            p.resolve("  x"),
            FrameVerdict::Dedented {
                rest: "x",
                op_index: 0,
                cols_short: 2
            }
        );
    }

    #[test]
    fn resolve_content_indent_matches_the_leading_indent_gate() {
        for (line, col) in [
            ("    x", 4),
            ("   x", 4),
            ("\tx", 4),
            ("\t: def", 2),
            ("  \t x", 8),
            ("x", 4),
            ("", 4),
            ("  x", 0),
        ] {
            let verdict = resolve_content_indent(line, col);
            assert_eq!(
                verdict.reaches_frame(),
                leading_indent(line).0 >= col,
                "reaches_frame vs leading_indent gate on ({line:?}, {col})"
            );
        }
        assert_eq!(
            resolve_content_indent("\t: def", 2),
            FrameVerdict::StraddlingTab {
                rest: "\t: def",
                cols_before_tab: 0
            }
        );
        assert_eq!(resolve_content_indent("    x", 4).rest(), "x");
        assert_eq!(
            resolve_content_indent("  x", 4).rest(),
            strip_content_indent("  x", 4).0
        );
    }

    #[test]
    fn split_pieces_captures_consumed_bytes() {
        let p =
            ContainerPrefix::from_ops(&[StripOp::ListAdvance(2), StripOp::BlockQuoteMarker], false);
        let (line, inner) = p.split_pieces("  > hello");
        assert_eq!(
            line.pieces(),
            &[
                (PrefixPieceKind::Indent, "  ".to_string()),
                (PrefixPieceKind::BqRun, "> ".to_string()),
            ]
        );
        assert_eq!(inner, "hello");
    }

    #[test]
    fn split_pieces_captures_interleaved_ops() {
        let p = ContainerPrefix::from_ops(
            &[
                StripOp::ListAdvance(2),
                StripOp::BlockQuoteMarker,
                StripOp::ListAdvance(2),
            ],
            false,
        );
        let (line, inner) = p.split_pieces("  >   x");
        assert_eq!(
            line.pieces(),
            &[
                (PrefixPieceKind::Indent, "  ".to_string()),
                (PrefixPieceKind::BqRun, "> ".to_string()),
                (PrefixPieceKind::Indent, "  ".to_string()),
            ]
        );
        assert_eq!(inner, "x");
    }

    #[test]
    fn split_pieces_appends_lazy_gobble_to_bq_run() {
        let mut p = ContainerPrefix::from_ops(&[StripOp::BlockQuoteMarker], false);
        p.lazy_blockquote_gobble = true;
        let (line, inner) = p.split_pieces("  x");
        assert_eq!(line.pieces(), &[(PrefixPieceKind::BqRun, "  ".to_string())]);
        assert_eq!(inner, "x");
    }
}
