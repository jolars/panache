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
//! Tokenization preserved across the migration:
//!
//! - List-indent is emitted as a *single* `WHITESPACE` token (matching
//!   the legacy `LinePrefixState` behavior).
//! - Blockquote prefix is emitted byte-by-byte — `>` as
//!   `BLOCK_QUOTE_MARKER`, anything else as a 1-byte `WHITESPACE`
//!   (matching the legacy `BqPrefixState` byte-walker).

use rowan::GreenNodeBuilder;
use smallvec::SmallVec;

use crate::options::Dialect;
use crate::syntax::SyntaxKind;

use super::super::block_dispatcher::BlockContext;
use super::super::utils::container_stack::{
    Container, byte_index_at_column, content_container_indent, leading_indent,
};
use super::blockquotes::{strip_blockquote_markers_counted, strip_n_blockquote_markers};

/// A single strip operation applied during the dispatcher's
/// container-stack walk. Ops are applied in order; each consumes some
/// leading bytes of the line and the next op operates on what remains.
#[derive(Copy, Clone, Debug)]
pub(crate) enum StripOp {
    /// Advance N columns (tab-aware). Mirrors the legacy `list_content_col`
    /// strip. On line 0, applied only when the marker line is the
    /// upstream-emitted dispatch line (see
    /// [`ContainerPrefix::strip_line_0_for_emission`]).
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

/// Inline capacity for the strip-op sequence. Container stacks are
/// typically ≤ 4 deep; sizes up to this stay stack-allocated. Deeper
/// nesting (legal but rare, e.g. 18-level blockquote chains) spills
/// to the heap automatically via `SmallVec`.
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
        dialect: Dialect,
    ) -> Self {
        let mut ops: SmallVec<[StripOp; INLINE_STRIP_OPS]> = SmallVec::new();
        let mut pending_list_advance: Option<u32> = None;
        for c in stack {
            match c {
                Container::BlockQuote { .. } => {
                    if let Some(la) = pending_list_advance.take() {
                        ops.push(StripOp::ListAdvance(la));
                    }
                    ops.push(StripOp::BlockQuoteMarker);
                }
                Container::FootnoteDefinition { content_col, .. }
                | Container::Definition { content_col, .. }
                | Container::Admonition { content_col } => {
                    if let Some(la) = pending_list_advance.take() {
                        ops.push(StripOp::ListAdvance(la));
                    }
                    ops.push(StripOp::ContentIndent(*content_col as u32));
                }
                Container::ListItem { content_col, .. } => {
                    // Keep only the innermost ListItem within this section
                    // (overwrites any previous pending value).
                    pending_list_advance = Some(*content_col as u32);
                }
                _ => {}
            }
        }
        if let Some(la) = pending_list_advance {
            ops.push(StripOp::ListAdvance(la));
        }
        // The `content_col` convention (see `Container`): content
        // containers carry relative widths, so the ContentIndent ops --
        // applied sequentially -- must sum to the absolute column the
        // scalar consumers (`ContainerStack::content_container_indent`)
        // compute from the same stack.
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
        Self {
            ops,
            list_marker_consumed_on_line_0,
            lazy_blockquote_gobble: dialect == Dialect::Pandoc,
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
        }
    }

    pub fn ops(&self) -> &[StripOp] {
        &self.ops
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
        }
    }

    /// Strip every op in order. Used for continuation lines (lines 1+)
    /// in multi-line lookahead and for callers that need the full
    /// strip regardless of the line-0 marker flag.
    pub fn strip<'a>(&self, line: &'a str) -> &'a str {
        let ops = self.ops();
        let mut s = line;
        let mut i = 0;
        while i < ops.len() {
            match ops[i] {
                // Walk the whole contiguous blockquote run at once: the lazy
                // gobble fires on the run, not on each marker, so a line that
                // is lazy at *some* level de-indents exactly once.
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
        if let Some(i) = ops
            .iter()
            .rposition(|op| matches!(op, StripOp::ListAdvance(_)))
        {
            ops.remove(i);
        }
        Self {
            ops,
            list_marker_consumed_on_line_0: self.list_marker_consumed_on_line_0,
            lazy_blockquote_gobble: self.lazy_blockquote_gobble,
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

    /// Like [`Self::strip_line_0_for_emission`] but also returns the
    /// bytes consumed by the *last* `ContentIndent` op (for re-emission
    /// as WHITESPACE when a nested BlockQuote opens inside a
    /// footnote/definition).
    #[allow(dead_code)]
    pub fn strip_line_0_with_indent_emit<'a>(&self, line: &'a str) -> (&'a str, Option<&'a str>) {
        let last_list_idx = self
            .ops()
            .iter()
            .rposition(|op| matches!(op, StripOp::ListAdvance(_)));
        let ops = self.ops();
        let mut s = line;
        let mut emit: Option<&'a str> = None;
        let mut i = 0;
        while i < ops.len() {
            match ops[i] {
                StripOp::ListAdvance(n) => {
                    if Some(i) == last_list_idx && !self.list_marker_consumed_on_line_0 {
                        // Preserve list-indent on the dispatch line
                        // when the marker wasn't upstream-emitted.
                    } else {
                        s = advance_columns(s, n as usize);
                    }
                    i += 1;
                }
                StripOp::BlockQuoteMarker => {
                    let run = blockquote_run_len(&ops[i..]);
                    s = strip_bq_with_gobble(s, run, self.lazy_blockquote_gobble);
                    i += run;
                }
                StripOp::ContentIndent(n) => {
                    let (next, e) = strip_content_indent(s, n as usize);
                    s = next;
                    if e.is_some() {
                        emit = e;
                    }
                    i += 1;
                }
            }
        }
        (s, emit)
    }

    /// Split a line into `(list_indent, bq_prefix, inner)` — the bytes
    /// consumed by the FIRST `ListAdvance` op, the bytes consumed by
    /// all `BlockQuoteMarker` ops between the list advance and the
    /// next non-bq op, and the remaining inner content. Used by graft
    /// helpers that need to capture the consumed prefix bytes for
    /// re-injection.
    ///
    /// Note: this split mirrors the legacy `(list_indent, bq_prefix,
    /// inner)` shape and does NOT account for `ContentIndent` ops
    /// (graft helpers operate on outer-container prefixes only).
    ///
    /// The Pandoc lazy gobble lands in `bq_prefix`: on a line short of
    /// its `>` markers the skipped whitespace is prefix, not content, so
    /// the graft re-injects it as prefix tokens instead of handing it to
    /// the reparse. Keeping it in the reparse would turn a lazy `<div>`
    /// body line indented four columns into an indented code block where
    /// pandoc reads a paragraph.
    #[allow(dead_code)]
    pub fn split<'a>(&self, line: &'a str) -> (&'a str, &'a str, &'a str) {
        let mut s = line;
        let mut list_consumed = 0usize;
        let mut bq_consumed = 0usize;
        let mut bq_requested = 0usize;
        let mut bq_matched = 0usize;
        let mut phase = 0; // 0 = looking for list, 1 = consuming bqs, 2 = done
        for op in self.ops() {
            match op {
                StripOp::ListAdvance(n) if phase == 0 => {
                    let after = advance_columns(s, *n as usize);
                    list_consumed = s.len() - after.len();
                    s = after;
                    phase = 1;
                }
                StripOp::BlockQuoteMarker if phase <= 1 => {
                    let (after, consumed) = strip_blockquote_markers_counted(s, 1);
                    bq_consumed += s.len() - after.len();
                    bq_requested += 1;
                    bq_matched += consumed;
                    s = after;
                    phase = 1;
                }
                _ => {
                    phase = 2;
                    break;
                }
            }
        }
        let _ = phase;
        if self.lazy_blockquote_gobble && bq_matched < bq_requested {
            let gobbled = lazy_gobble_trim(s);
            bq_consumed += s.len() - gobbled.len();
            s = gobbled;
        }
        (
            &line[..list_consumed],
            &line[list_consumed..list_consumed + bq_consumed],
            s,
        )
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

/// Outcome of advancing over whitespace-only bytes toward a column
/// target. Private engine shared by the `ListAdvance` and
/// `ContentIndent` arms of the verdict walk.
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

/// Advance up to `target` columns over spaces and tabs only (tab stop
/// 4, matching [`leading_indent`]). Unlike [`advance_columns`], content
/// bytes never count as columns, and a straddling tab is reported
/// rather than silently kept or consumed.
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
        StripOp::ListAdvance(n) => advance_columns(line, n as usize),
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

/// Length of the leading run of `BlockQuoteMarker` ops in `ops`.
fn blockquote_run_len(ops: &[StripOp]) -> usize {
    ops.iter()
        .take_while(|op| matches!(op, StripOp::BlockQuoteMarker))
        .count()
}

/// The tail after the blockquote bucket, with the lazy gobble applied when
/// `lazy_gobble` is set and fewer than `bq_depth` markers were consumed.
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
/// otherwise (lazy continuation) strip whatever leading whitespace
/// exists.
pub(crate) fn strip_content_indent(line: &str, content_indent: usize) -> (&str, Option<&str>) {
    if content_indent == 0 {
        return (line, None);
    }
    let (indent_cols, _) = leading_indent(line);
    if indent_cols >= content_indent {
        let idx = byte_index_at_column(line, content_indent);
        (&line[idx..], Some(&line[..idx]))
    } else {
        let trimmed_start = line.trim_start();
        let ws_len = line.len() - trimmed_start.len();
        if ws_len > 0 {
            (trimmed_start, Some(&line[..ws_len]))
        } else {
            (line, None)
        }
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
}

#[allow(dead_code)]
impl<'a, 'p> StrippedLines<'a, 'p> {
    pub fn new(raw: &'a [&'a str], base: usize, prefix: &'p ContainerPrefix) -> Self {
        Self {
            raw,
            base,
            dispatch: base,
            prefix,
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
        }
    }

    /// Number of lines available from `base` onward (line 0 plus lookahead).
    pub fn remaining(&self) -> usize {
        self.raw.len().saturating_sub(self.base)
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
        self.prefix.strip(self.raw[self.base])
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

    /// Peek the tail [`Self::emit_prefix_at`] would return for the line at
    /// ABSOLUTE index `i`, emitting nothing.
    ///
    /// Use this — not [`Self::strip_at`] — when a decision made during
    /// classification has to hold at emission time. `strip_at` walks list
    /// indent with `advance_columns` (any character counts as a column),
    /// emission with `strip_list_indent` (whitespace only); on an
    /// under-indented lazy line such as `" b |"` inside a two-column list
    /// item they return `" |"` and `"b |"` respectively.
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
    /// strategy by position: the dispatch line emits no prefix tokens (the
    /// core consumed them) via [`Self::dispatch_tail`]; every other line
    /// re-emits its container prefix as tokens via [`Self::emit_prefix_at`].
    /// Consolidates the `if i == dispatch { … } else { … }` idiom repeated
    /// across the table emitters.
    pub fn emit_or_dispatch_tail(
        &self,
        builder: &mut GreenNodeBuilder<'static>,
        i: usize,
    ) -> &'a str {
        if i == self.dispatch {
            self.dispatch_tail()
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
        if ch == '>' {
            builder.token(SyntaxKind::BLOCK_QUOTE_MARKER.into(), ">");
        } else {
            let mut buf = [0u8; 4];
            builder.token(SyntaxKind::WHITESPACE.into(), ch.encode_utf8(&mut buf));
        }
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

/// Strip a continuation line's container prefix by walking
/// [`ContainerPrefix::ops`] in stack order, optionally emitting the
/// consumed bytes as kind-tagged tokens.
///
/// Tokenization (matching the legacy scalar emitters): list indent and
/// content indent become `WHITESPACE`, with adjacent runs coalesced
/// into one token for byte-range-equivalent CST stability; blockquote
/// prefix bytes are emitted one by one (`>` as `BLOCK_QUOTE_MARKER`,
/// anything else as 1-byte `WHITESPACE`). With no builder the walk is
/// pure detection, and the tail equals the emitting walk's by
/// construction.
///
/// Every strip is the graceful emission-side one (`strip_list_indent`,
/// `strip_content_indent`, the counted bq strip), so nothing here can
/// eat content bytes as indent; a caller that needs to *know* whether
/// the frame was reached asks [`ContainerPrefix::resolve`] instead.
fn walk_content_line_prefix<'a>(
    prefix: &ContainerPrefix,
    content_line: &'a str,
    mut builder: Option<&mut GreenNodeBuilder<'static>>,
) -> &'a str {
    fn flush_ws(
        builder: &mut Option<&mut GreenNodeBuilder<'static>>,
        content_line: &str,
        pending: &mut Option<usize>,
        current_offset: usize,
    ) {
        if let Some(start) = *pending
            && current_offset > start
        {
            if let Some(b) = builder.as_deref_mut() {
                b.token(
                    SyntaxKind::WHITESPACE.into(),
                    &content_line[start..current_offset],
                );
            }
            *pending = None;
        }
    }

    let ops = prefix.ops();
    let mut s = content_line;
    let mut pending_ws_start: Option<usize> = None;
    let mut i = 0;
    while i < ops.len() {
        match ops[i] {
            StripOp::ListAdvance(n) => {
                let stripped = strip_list_indent(s, n as usize);
                if stripped.len() < s.len() {
                    let start = content_line.len() - s.len();
                    if pending_ws_start.is_none() {
                        pending_ws_start = Some(start);
                    }
                    s = stripped;
                }
                i += 1;
            }
            StripOp::BlockQuoteMarker => {
                let run = blockquote_run_len(&ops[i..]);
                let current_offset = content_line.len() - s.len();
                flush_ws(
                    &mut builder,
                    content_line,
                    &mut pending_ws_start,
                    current_offset,
                );
                let (stripped, consumed) = strip_blockquote_markers_counted(s, run);
                let prefix_len = s.len() - stripped.len();
                if prefix_len > 0
                    && let Some(b) = builder.as_deref_mut()
                {
                    emit_blockquote_prefix_tokens(b, &s[..prefix_len]);
                }
                s = stripped;
                // Pandoc's gobble: a lazy line loses its indent to the
                // quote's raw content. Hand the bytes to the pending
                // WHITESPACE run rather than leaving them in the
                // construct's content — that is what keeps the tree
                // lossless while the block sees a de-indented line.
                if prefix.lazy_blockquote_gobble && consumed < run {
                    let gobbled = lazy_gobble_trim(s);
                    if gobbled.len() < s.len() {
                        let start = content_line.len() - s.len();
                        if pending_ws_start.is_none() {
                            pending_ws_start = Some(start);
                        }
                        s = gobbled;
                    }
                }
                i += run;
            }
            StripOp::ContentIndent(n) => {
                let (stripped, consumed) = strip_content_indent(s, n as usize);
                if consumed.is_some() {
                    let start = content_line.len() - s.len();
                    if pending_ws_start.is_none() {
                        pending_ws_start = Some(start);
                    }
                    s = stripped;
                }
                i += 1;
            }
        }
    }

    let final_offset = content_line.len() - s.len();
    flush_ws(
        &mut builder,
        content_line,
        &mut pending_ws_start,
        final_offset,
    );
    s
}

/// Advance past `target` columns of `line`. Tabs round up to the next
/// 4-column stop; tab that would overshoot the target is left intact
/// (mirrors `strip_list_item_indent`'s tab handling). Newlines / CR
/// short-circuit to the empty string — the line ended before the target
/// was reached.
pub(in crate::parser::blocks) fn advance_columns(line: &str, target: usize) -> &str {
    if target == 0 {
        return line;
    }
    // Walk whole UTF-8 characters, not bytes: each char counts as one
    // column, so the returned slice always starts on a char boundary.
    // (A byte-indexed walk treated every continuation byte of a
    // multibyte char as its own column and could slice mid-char — #314.)
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

/// One per-line entry in [`ContainerPrefixState`].
#[derive(Clone, Debug, Default)]
pub(crate) struct ContainerPrefixLine {
    /// List-indent bytes — emitted as a single `WHITESPACE` token at
    /// line start when non-empty.
    pub list_indent: String,
    /// Blockquote prefix bytes (mix of `>` and inter-marker whitespace),
    /// plus any whitespace the Pandoc lazy gobble skipped on a line short
    /// of its markers. Emitted byte-by-byte after the list-indent token,
    /// except for the run trailing the last `>` — see
    /// [`emit_container_prefix_tokens`].
    pub bq_prefix: String,
}

impl ContainerPrefixLine {
    pub fn is_empty(&self) -> bool {
        self.list_indent.is_empty() && self.bq_prefix.is_empty()
    }

    pub fn bq_only(bq_prefix: String) -> Self {
        Self {
            list_indent: String::new(),
            bq_prefix,
        }
    }

    pub fn list_only(list_indent: String) -> Self {
        Self {
            list_indent,
            bq_prefix: String::new(),
        }
    }
}

/// Emit a captured per-line container prefix as kind-tagged tokens.
/// List-indent (if any) goes out as one `WHITESPACE`; bq prefix bytes
/// go out byte-by-byte as `BLOCK_QUOTE_MARKER` / `WHITESPACE`.
///
/// The byte-by-byte split is load-bearing, not cosmetic: the formatter
/// reads these tokens back when rebuilding a quoted line, and coalescing
/// the run into one `WHITESPACE` makes it drop the whole run rather than
/// the marker's own space (see the
/// `html_block_div_definition_body_later_line_blockquote` golden case).
pub(crate) fn emit_container_prefix_tokens(
    builder: &mut GreenNodeBuilder<'static>,
    line: &ContainerPrefixLine,
) {
    if !line.list_indent.is_empty() {
        builder.token(SyntaxKind::WHITESPACE.into(), &line.list_indent);
    }
    for ch in line.bq_prefix.chars() {
        if ch == '>' {
            builder.token(SyntaxKind::BLOCK_QUOTE_MARKER.into(), ">");
        } else {
            let mut buf = [0u8; 4];
            builder.token(SyntaxKind::WHITESPACE.into(), ch.encode_utf8(&mut buf));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let base =
            ContainerPrefix::from_stack(&[Container::BlockQuote {}], false, Dialect::CommonMark);
        let deeper = base.with_extra_blockquotes(1);
        assert_eq!(deeper.bq_depth(), 2);
        assert_eq!(deeper.strip("> > a"), "a");
        // The probe reads line 0 through the emission-side strip, which is
        // what `StrippedLines::first` and `dispatch_tail` use.
        assert_eq!(base.strip_line_0_for_emission("> > a"), "> a");
        assert_eq!(deeper.strip_line_0_for_emission("> > a"), "a");
        // Equivalent to building the prefix from the stack it models.
        let from_stack = ContainerPrefix::from_stack(
            &[Container::BlockQuote {}, Container::BlockQuote {}],
            false,
            Dialect::CommonMark,
        );
        assert_eq!(deeper.ops().len(), from_stack.ops().len());
        assert_eq!(deeper.bq_depth(), from_stack.bq_depth());
        // Zero extra levels is the identity.
        assert_eq!(base.with_extra_blockquotes(0).bq_depth(), 1);
    }

    #[test]
    fn from_scalars_round_trips_marker_line_caller_combos() {
        // Each case mirrors a `core.rs` marker-line fenced-code caller,
        // which passes `bq_outer = bq_depth > 0`.
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

        // core.rs:491 (list-item-first-line): lcc>0, ci=0, lmc0=true.
        check(0, 4, 0, true);
        check(1, 4, 0, true);
        // core.rs:1443/1456 (definition-marker): lcc=0, ci>0, lmc0=false.
        check(0, 0, 4, false);
        check(2, 0, 4, false);
    }

    #[test]
    fn strip_list_marker_line() {
        // `- > <div>` with content_col=2: advance past `- `, then strip `>`.
        let p =
            ContainerPrefix::from_ops(&[StripOp::ListAdvance(2), StripOp::BlockQuoteMarker], false);
        assert_eq!(p.strip("- > <div>"), "<div>");
    }

    #[test]
    fn strip_list_continuation_line() {
        // `  > hello` with content_col=2: advance past `  `, then strip `>`.
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
    fn strip_short_line_yields_empty() {
        let p = ContainerPrefix::from_ops(&[StripOp::ListAdvance(4)], false);
        assert_eq!(p.strip(""), "");
        assert_eq!(p.strip("\n"), "");
    }

    #[test]
    fn advance_columns_lands_on_char_boundary_for_multibyte() {
        // Regression for #314-322: `advance_columns` counted columns
        // per-byte, so advancing past N columns could slice inside a
        // multibyte char (e.g. box-drawing `├`, accented `é`, CJK,
        // emoji) and panic. Each char must count as one column.
        assert_eq!(advance_columns("├── x", 2), "─ x");
        assert_eq!(advance_columns("éxy", 1), "xy");
        assert_eq!(advance_columns("黑x", 1), "x");
        assert_eq!(advance_columns("😄.", 1), ".");
        // Advancing past the whole multibyte run yields empty.
        assert_eq!(advance_columns("├──", 5), "");
        // ASCII behaviour is unchanged (one column per char).
        assert_eq!(advance_columns("  > hello", 2), "> hello");
        // Tabs still round up to the next 4-column stop.
        assert_eq!(advance_columns("\tfoo", 4), "foo");
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
        // bq absent isolates the list-col strip difference — the bq
        // marker stripper otherwise consumes up to 3 leading spaces by
        // itself, masking the divergence.
        let prefix_continuation = ContainerPrefix::from_ops(&[StripOp::ListAdvance(2)], false);
        let raw = ["  continuation"];
        let lines = StrippedLines::new(&raw, 0, &prefix_continuation);
        // marker_consumed=false → list-indent preserved on line 0.
        assert_eq!(lines.first(), "  continuation");
        // first_unconditional always advances past the list cols.
        assert_eq!(lines.first_unconditional(), "continuation");

        let prefix_marker = ContainerPrefix::from_ops(&[StripOp::ListAdvance(2)], true);
        let lines = StrippedLines::new(&raw, 0, &prefix_marker);
        // marker_consumed=true → list-indent skipped on line 0.
        assert_eq!(lines.first(), "continuation");
    }

    #[test]
    fn stripped_lines_get_uses_unconditional_strip_after_line_0() {
        let prefix = ContainerPrefix::from_ops(&[StripOp::ListAdvance(2)], false);
        let raw = ["  foo", "  bar", "  baz"];
        let lines = StrippedLines::new(&raw, 0, &prefix);
        // Line 0: emission-safe → list-indent preserved.
        assert_eq!(lines.get(0), "  foo");
        // Lines 1+: unconditional → list-indent stripped.
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
        // The invariant a classify-then-emit block parser depends on: what the
        // peek reports is what the emitter hands back. `strip_at` does NOT
        // satisfy it — it walks list indent column-blind — which is how a
        // line block inside a list item used to panic on a lazy line whose
        // trailing `|` the peek mistook for a marker.
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
        // And the specific divergence that motivated the split.
        assert_eq!(lines.strip_at(1), " |");
        assert_eq!(lines.peek_prefix_at(1), "b |");
    }

    #[test]
    fn strip_at_matches_hand_rolled_table_closure() {
        // The per-line stripped view table detection reads through `LineView`
        // (lazily, via `strip_at`):
        //   strip_at(i) == if i == dispatch { strip_line_0_for_emission }
        //                  else             { strip }
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
        // Pipe tables scan from `start_pos` (here 0, a caption line) while
        // the dispatch line (marker-consumed) is `line_pos` (here 1).
        let prefix =
            ContainerPrefix::from_ops(&[StripOp::ListAdvance(2), StripOp::BlockQuoteMarker], true);
        let raw = [": caption", "- > header", "  > sep"];
        let dispatch = 1;
        let lines = StrippedLines::with_dispatch(&raw, 0, dispatch, &prefix);
        // Non-dispatch lines use the full strip.
        assert_eq!(lines.strip_at(0), prefix.strip(raw[0]));
        assert_eq!(lines.strip_at(2), prefix.strip(raw[2]));
        // The dispatch line uses the emission-safe line-0 strip.
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
        // `  > ` stripped (list-col 2, then one bq marker) → "hello".
        assert_eq!(tail, "hello");
        assert_eq!(tail, lines.peek_prefix_at(1));
    }

    #[test]
    fn resolve_rejects_faked_indent() {
        let p = ContainerPrefix::from_ops(&[StripOp::ListAdvance(2)], false);
        // `strip` would return `":\n"` here by counting `c` and the space as
        // columns; the line never reaches the item's content column.
        assert_eq!(p.strip("c :\n"), ":\n");
        assert!(!p.resolve("c :\n").reaches_frame());
        assert!(!p.resolve(" c :\n").reaches_frame());
        // Real indent reaching the content column is accepted.
        assert!(p.resolve("  : def\n").reaches_frame());
        assert!(p.resolve("    : def\n").reaches_frame());
        // A tab reaches column 4, which covers a two-column item — as a
        // straddle, since column 2 has no byte boundary.
        assert!(p.resolve("\t: def\n").reaches_frame());
        // An empty prefix has nothing to fake.
        assert!(ContainerPrefix::default().resolve("c :\n").reaches_frame());
    }

    #[test]
    fn resolve_measures_list_advance_after_outer_ops() {
        // The list advance is measured against what the blockquote strip left.
        let p =
            ContainerPrefix::from_ops(&[StripOp::BlockQuoteMarker, StripOp::ListAdvance(2)], false);
        assert!(!p.resolve("> c :\n").reaches_frame());
        assert!(p.resolve(">   : def\n").reaches_frame());
    }

    #[test]
    fn strip_content_indent_only() {
        // Inside a footnote definition (content_indent=4), the line's
        // leading 4 cols belong to the footnote container and are stripped.
        let p = ContainerPrefix::from_ops(&[StripOp::ContentIndent(4)], false);
        assert_eq!(p.strip("    continuation"), "continuation");
        // Same via `strip_line_0_for_emission` (always strips content_indent).
        assert_eq!(
            p.strip_line_0_for_emission("    continuation"),
            "continuation"
        );
    }

    #[test]
    fn strip_content_indent_inside_blockquote() {
        // Footnote inside a blockquote ([BlockQuote, FootnoteDef]):
        // bq strips first, then content_indent.
        let p = ContainerPrefix::from_ops(
            &[StripOp::BlockQuoteMarker, StripOp::ContentIndent(4)],
            false,
        );
        // `>     continuation` → strip `> ` → `    continuation` → strip 4 cols → `continuation`.
        assert_eq!(p.strip(">     continuation"), "continuation");
    }

    #[test]
    fn strip_blockquote_inside_content_indent() {
        // Blockquote opened *inside* a footnote ([FootnoteDef, BlockQuote]):
        // content_indent strips first, then bq.
        let p = ContainerPrefix::from_ops(
            &[StripOp::ContentIndent(4), StripOp::BlockQuoteMarker],
            false,
        );
        // `    >quoted` → strip 4 cols → `>quoted` → strip bq → `quoted`.
        assert_eq!(p.strip("    >quoted"), "quoted");
    }

    #[test]
    fn strip_definition_above_list_above_bq() {
        // Stack [Definition(4), List, ListItem(2), BlockQuote] for a line
        // shaped like `    - > a` (Definition indent + list marker + bq).
        let p = ContainerPrefix::from_ops(
            &[
                StripOp::ContentIndent(4),
                StripOp::ListAdvance(2),
                StripOp::BlockQuoteMarker,
            ],
            false,
        );
        assert_eq!(p.strip("    - > a"), "a");
    }

    #[test]
    fn strip_content_indent_lazy_continuation() {
        // Less indent than `content_indent` requires: legacy strip
        // consumes whatever leading whitespace exists and reports it via
        // `indent_to_emit`.
        let p = ContainerPrefix::from_ops(&[StripOp::ContentIndent(4)], false);
        let (stripped, emit) = p.strip_line_0_with_indent_emit("  short");
        assert_eq!(stripped, "short");
        assert_eq!(emit, Some("  "));
    }

    #[test]
    fn strip_content_indent_with_list_marker_consumed() {
        // List-marker line with content_indent set (footnote in a list
        // item): list cols stripped, then content_indent.
        let p =
            ContainerPrefix::from_ops(&[StripOp::ListAdvance(2), StripOp::ContentIndent(4)], true);
        // Line: `- ` (list marker, 2 cols) + `    footnote text` (content_indent).
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
        // Nested lists: only the innermost ListItem contributes a
        // ListAdvance, matching `paragraphs::current_content_col`.
        // For `- - foo`, inner.content_col=4 is absolute.
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
        let p = ContainerPrefix::from_stack(&stack, false, Dialect::CommonMark);
        // Only the innermost (content_col=4) is applied.
        assert_eq!(p.strip("- - foo"), "foo");
    }

    #[test]
    fn resolve_agrees_with_strip_when_indent_is_real() {
        // On lines whose prefix bytes are all real, the verdict's tail
        // is exactly what `strip` returns.
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
        // Mirrors `strip_line_0_for_emission`: with the marker not
        // upstream-emitted, the innermost ListAdvance is not applied,
        // so its columns are neither required nor consumed.
        let p = ContainerPrefix::from_ops(&[StripOp::ListAdvance(2)], false);
        assert_eq!(
            p.resolve_line_0("continuation"),
            FrameVerdict::Inside {
                rest: "continuation"
            }
        );
        // With the marker consumed, line 0 resolves like any line.
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
        // [ContentIndent(4), ListAdvance(2)]: a line covering only the
        // content indent fails the list advance, in the content frame.
        let p =
            ContainerPrefix::from_ops(&[StripOp::ContentIndent(4), StripOp::ListAdvance(2)], false);
        assert_eq!(
            p.resolve("    x"),
            FrameVerdict::FakedIndent {
                rest: "x",
                op_index: 1
            }
        );
        // A line short of the content indent never reaches the list op.
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
        // The migration contract for the hand-rolled
        // `leading_indent(line).0 >= content_col` sites:
        // `reaches_frame()` is equivalent, straddling tabs included
        // (leading_indent counts tab *columns*).
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
        // The straddle carries the byte-honest tail and stop column.
        assert_eq!(
            resolve_content_indent("\t: def", 2),
            FrameVerdict::StraddlingTab {
                rest: "\t: def",
                cols_before_tab: 0
            }
        );
        // Reached and short tails match `strip_content_indent`.
        assert_eq!(resolve_content_indent("    x", 4).rest(), "x");
        assert_eq!(
            resolve_content_indent("  x", 4).rest(),
            strip_content_indent("  x", 4).0
        );
    }

    #[test]
    fn split_captures_consumed_bytes() {
        let p =
            ContainerPrefix::from_ops(&[StripOp::ListAdvance(2), StripOp::BlockQuoteMarker], false);
        let (li, bq, inner) = p.split("  > hello");
        assert_eq!(li, "  ");
        assert_eq!(bq, "> ");
        assert_eq!(inner, "hello");
    }
}
