//! Behavior pins for container-frame resolution.
//!
//! There is no single authoritative answer to "which container frame is
//! this line in, and does it reach the content column?" — the helpers
//! exercised here each answer it slightly differently, and several are
//! known to disagree on specific `(container stack, line)` shapes. This
//! module pins **today's actual behavior** of every path, including the
//! wrong answers, so the typed-verdict consolidation (see TODO.md,
//! "Consolidate container-frame resolution behind a single typed
//! verdict") can migrate callers one at a time against a fixed baseline.
//!
//! Rows whose helpers disagree carry a `DISAGREES` comment and a
//! non-empty `expected_to_change` note naming the commit expected to
//! flip them. Flipping a pinned value is a deliberate, greppable edit of
//! that row — never silent snapshot churn.

use super::helpers::*;
use crate::options::Dialect;
use crate::parser::blocks::container_prefix::{
    ContainerPrefix, StripOp, StrippedLines, advance_columns, strip_content_indent,
    strip_list_indent,
};
use crate::parser::blocks::definition_lists::{
    definition_marker_in_list_frame, next_line_is_definition_marker,
};
use crate::parser::blocks::tables::is_caption_followed_by_table;
use crate::parser::utils::container_stack::{
    Container, byte_index_at_column, gobbled_indent_prefix_len,
};
use crate::syntax::SyntaxKind;

/// One `(ops, line)` shape pinned across every prefix-level helper.
///
/// The columns have deliberately different semantics — that is the
/// point. `strip` walks list indent column-blind (`advance_columns`),
/// `peek_prefix` goes through the scalar-projection path
/// (`content_line_prefix_tail`), `carries_list_indent` vets only the
/// `ListAdvance` ops, and `reaches_content_column` also vets
/// `ContentIndent`.
struct PinRow {
    name: &'static str,
    ops: &'static [StripOp],
    line: &'static str,
    strip: &'static str,
    /// `strip_line_0_for_emission` with `list_marker_consumed_on_line_0`
    /// false (the innermost `ListAdvance` is preserved).
    strip_line_0_unmarked: &'static str,
    carries_list_indent: bool,
    reaches_content_column: bool,
    peek_prefix: &'static str,
    /// `(content_col, expected)` for `gobbled_indent_prefix_len`, where
    /// meaningful for the row's innermost content column.
    gobbled_len: Option<(usize, usize)>,
    /// Which commit of the consolidation is expected to flip which
    /// column. Empty = frozen: these values must survive the refactor.
    expected_to_change: &'static str,
}

const PINS: &[PinRow] = &[
    // DISAGREES: three primitives give three different tails for a tab
    // straddling the item's content column (see also the dedicated
    // primitive test below). The line *does* reach column 2 — a tab has
    // no byte boundary to split on. `next_line_is_definition_marker`
    // reads through `strip`, so the marker behind the tab is invisible
    // to it today.
    PinRow {
        name: "tab_straddles_list_content_col",
        ops: &[StripOp::ListAdvance(2)],
        line: "\t: def",
        strip: "\t: def",
        strip_line_0_unmarked: "\t: def",
        carries_list_indent: true,
        reaches_content_column: true,
        peek_prefix: ": def",
        gobbled_len: Some((2, 0)),
        expected_to_change: "verdict reports StraddlingTab; lookahead flip lands in \
                             fix(parser): recognize definition markers behind a straddling tab",
    },
    // DISAGREES: `strip` fakes the indent by eating `c ` as two columns;
    // emission-side strips (peek) stop at the first non-whitespace byte.
    PinRow {
        name: "short_line_content_sliced_as_indent",
        ops: &[StripOp::ListAdvance(2)],
        line: "c :",
        strip: ":",
        strip_line_0_unmarked: "c :",
        carries_list_indent: false,
        reaches_content_column: false,
        peek_prefix: "c :",
        gobbled_len: Some((2, 0)),
        expected_to_change: "",
    },
    PinRow {
        name: "real_indent_reaches_content_col",
        ops: &[StripOp::ListAdvance(2)],
        line: "  : def",
        strip: ": def",
        strip_line_0_unmarked: "  : def",
        carries_list_indent: true,
        reaches_content_column: true,
        peek_prefix: ": def",
        gobbled_len: Some((2, 2)),
        expected_to_change: "",
    },
    // DISAGREES: the issue_209 shape ([ContentIndent, ListAdvance,
    // BlockQuoteMarker], a definition body holding a list item holding a
    // blockquote). `strip` walks the ops in true stack order; the peek
    // path collapses to scalars applied list → bq → content-indent, so
    // the bq marker still sits behind four spaces when the bq strip
    // runs (only 3 allowed) and the marker survives into the tail. The
    // doc on `emit_prefix_at` claims the divergence is dormant while
    // `content_indent == 0`; this row is the counterexample.
    PinRow {
        name: "issue_209_content_indent_list_bq",
        ops: &[
            StripOp::ContentIndent(4),
            StripOp::ListAdvance(2),
            StripOp::BlockQuoteMarker,
        ],
        line: "      > b",
        strip: "b",
        strip_line_0_unmarked: "b",
        carries_list_indent: true,
        reaches_content_column: true,
        peek_prefix: "> b",
        gobbled_len: None,
        expected_to_change: "peek converges on the op walk in refactor(parser): make \
                             emission a faithful op walk",
    },
    // The documented `line_carries_list_indent` gap: a dash run at
    // column 0 below a definition body has left the body, but a body
    // contributes only a `ContentIndent` op, which that helper does not
    // vet. `line_reaches_content_column` exists precisely for this.
    PinRow {
        name: "short_content_indent_waved_through",
        ops: &[StripOp::ContentIndent(4)],
        line: "----",
        strip: "----",
        strip_line_0_unmarked: "----",
        carries_list_indent: true,
        reaches_content_column: false,
        peek_prefix: "----",
        gobbled_len: Some((4, 0)),
        expected_to_change: "",
    },
    // `strip_content_indent` degrades gracefully on a short line: it
    // trims whatever whitespace exists and never reports the shortfall.
    PinRow {
        name: "content_indent_lazy_short_line",
        ops: &[StripOp::ContentIndent(4)],
        line: "  x",
        strip: "x",
        strip_line_0_unmarked: "x",
        carries_list_indent: true,
        reaches_content_column: false,
        peek_prefix: "x",
        gobbled_len: Some((4, 2)),
        expected_to_change: "",
    },
];

#[test]
fn pinned_prefix_helper_matrix() {
    for row in PINS {
        let prefix = ContainerPrefix::from_ops(row.ops, false);
        assert_eq!(prefix.strip(row.line), row.strip, "{}: strip", row.name);
        assert_eq!(
            prefix.strip_line_0_for_emission(row.line),
            row.strip_line_0_unmarked,
            "{}: strip_line_0_for_emission (unmarked)",
            row.name
        );
        assert_eq!(
            prefix.line_carries_list_indent(row.line),
            row.carries_list_indent,
            "{}: line_carries_list_indent",
            row.name
        );
        assert_eq!(
            prefix.line_reaches_content_column(row.line),
            row.reaches_content_column,
            "{}: line_reaches_content_column",
            row.name
        );
        let raw = [row.line];
        let lines = StrippedLines::new(&raw, 0, &prefix);
        assert_eq!(
            lines.peek_prefix_at(0),
            row.peek_prefix,
            "{}: peek_prefix_at",
            row.name
        );
        if let Some((col, expected)) = row.gobbled_len {
            assert_eq!(
                gobbled_indent_prefix_len(row.line, col),
                expected,
                "{}: gobbled_indent_prefix_len",
                row.name
            );
        }
        // Documentation only; asserted nothing so a planned flip is an
        // expected-value edit on the row, not a driver change.
        let _ = row.expected_to_change;
    }
}

/// The tab-straddle shape at the primitive level: one line, one column
/// target, three different answers. `advance_columns` leaves the
/// straddling tab in place (any-char column walk), `strip_list_indent`
/// (via `byte_index_at_column`) consumes it whole, and
/// `gobbled_indent_prefix_len` stops before it.
#[test]
fn tab_straddle_primitives_disagree() {
    let line = "\t: def";
    assert_eq!(advance_columns(line, 2), "\t: def");
    assert_eq!(strip_list_indent(line, 2), ": def");
    assert_eq!(byte_index_at_column(line, 2), 1);
    assert_eq!(gobbled_indent_prefix_len(line, 2), 0);
}

/// The step-2 invariant: nested content containers push one
/// `ContentIndent` op each (relative widths applied sequentially), and
/// the absolute-summing consumers (`content_container_indent_to_strip`)
/// collapse them into one column count. For today's whitespace-only
/// strips the two agree; this pins that agreement so the convention can
/// be stated once and asserted.
#[test]
fn sequential_content_indent_ops_agree_with_summed_strip() {
    let sequential = ContainerPrefix::from_ops(
        &[StripOp::ContentIndent(4), StripOp::ContentIndent(4)],
        false,
    );
    for line in ["        x", "      x", "    x", "  x", "x", "    \tx"] {
        assert_eq!(
            sequential.strip(line),
            strip_content_indent(line, 8).0,
            "sequential [CI(4), CI(4)] vs summed CI(8) on {line:?}"
        );
    }
}

/// DISAGREES: `from_stack` preserves true stack order (content indent
/// before the list advance for [Definition, List, ListItem]); the
/// scalar constructors (`from_scalars`, mirroring `from_ctx`) always
/// order list → bq → content-indent. On a line whose indent exactly
/// covers the content container, the orders strip different bytes:
/// stack order spends the indent on the `ContentIndent` op and then
/// `advance_columns` eats content for the `ListAdvance`; scalar order
/// spends part on the list advance and lazy-trims the rest.
#[test]
fn from_stack_and_from_scalars_orderings_diverge() {
    let stack_order =
        ContainerPrefix::from_ops(&[StripOp::ContentIndent(4), StripOp::ListAdvance(2)], false);
    let scalar_order = ContainerPrefix::from_scalars(0, 2, false, 4, false, Dialect::CommonMark);
    let line = "    x";
    assert_eq!(stack_order.strip(line), "");
    assert_eq!(scalar_order.strip(line), "x");
}

/// `from_stack` over a real container stack produces the stack-order
/// ops the scalar path cannot express: [Definition, List, ListItem]
/// yields `[ContentIndent(4), ListAdvance(2)]`, content indent first.
#[test]
fn from_stack_definition_above_list_orders_content_indent_first() {
    use crate::parser::blocks::lists::ListMarker;
    use crate::parser::utils::list_item_buffer::ListItemBuffer;
    use crate::parser::utils::text_buffer::ParagraphBuffer;
    let stack = vec![
        Container::Definition {
            content_col: 4,
            plain_open: false,
            plain_buffer: ParagraphBuffer::new(),
        },
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
    ];
    let p = ContainerPrefix::from_stack(&stack, false, Dialect::CommonMark);
    assert!(matches!(
        p.ops(),
        [StripOp::ContentIndent(4), StripOp::ListAdvance(2)]
    ));
    assert_eq!(p.strip("      a"), "a");
}

/// The Pandoc lazy-blockquote gobble is dialect-gated: a lazy line
/// (fewer `>` markers than the quote is deep) loses its whole leading
/// whitespace under Pandoc and keeps it under CommonMark. Any faithful
/// verdict walk must preserve this interplay.
#[test]
fn lazy_blockquote_gobble_is_dialect_gated() {
    let stack = vec![Container::BlockQuote {}];
    let pandoc = ContainerPrefix::from_stack(&stack, false, Dialect::Pandoc);
    let commonmark = ContainerPrefix::from_stack(&stack, false, Dialect::CommonMark);
    assert_eq!(pandoc.strip("    deep"), "deep");
    assert_eq!(commonmark.strip("    deep"), "    deep");
    // A blank line is exempt from the gobble (it ends the quote instead).
    assert_eq!(pandoc.strip("   \n"), "   \n");
}

/// The caption probe is bounded by the container frame it is called in:
/// a dash run at column 0 below a definition body (content indent 4)
/// has left the body, so nothing below it can be the caption's table.
/// The same dash run *at* the content column still fires.
#[test]
fn caption_probe_bounded_by_container_frame() {
    let prefix = ContainerPrefix::from_ops(&[StripOp::ContentIndent(4)], false);

    let outside = ["    : cap", "", "----", "x", "----"];
    let view = StrippedLines::new(&outside, 0, &prefix);
    assert!(!is_caption_followed_by_table(&view, 0));

    let inside = ["    : cap", "", "    ----", "    x", "    ----"];
    let view = StrippedLines::new(&inside, 0, &prefix);
    assert!(is_caption_followed_by_table(&view, 0));
}

/// `definition_marker_in_list_frame`'s two production callers feed it
/// different notions of "the content column" — the dispatcher passes
/// `ctx.list_indent_info.content_col`, the list-item-break path passes
/// `paragraphs::current_content_col`, which scans `ListItem` *or*
/// `FootnoteDefinition` and can hand it a footnote width instead. The
/// same line parses differently under the two conventions.
#[test]
fn definition_marker_frame_depends_on_callers_content_col() {
    let line = "      : def";
    // Content column 4: the marker sits 2 columns into the item frame,
    // within the 0-3 allowance. `indent_cols` is absolute (6).
    assert_eq!(
        definition_marker_in_list_frame(line, Some(4)),
        Some((':', 6, 1, 1))
    );
    // Content column 2: the marker sits 4 columns into the item frame,
    // past the allowance, and the column-0 fallback also rejects it.
    assert_eq!(definition_marker_in_list_frame(line, Some(2)), None);
    // A tab straddling the content column: `byte_index_at_column`
    // consumes the tab whole and the helper re-reads the column it
    // actually reached, so the dispatch side already sees this marker.
    assert_eq!(
        definition_marker_in_list_frame("\t: def", Some(2)),
        Some((':', 4, 1, 1))
    );
}

/// DISAGREES with the dispatch side pinned above: the *lookahead*
/// (`next_line_is_definition_marker`) reads lines through
/// `ContainerPrefix::strip`, where the straddling tab survives the list
/// strip and then fails the 0-3 space marker gate. The marker behind
/// the tab is invisible to the lookahead while the dispatcher would
/// parse it. Expected to flip to `Some(0)` in fix(parser): recognize
/// definition markers behind a straddling tab.
#[test]
fn definition_lookahead_cannot_see_marker_behind_straddling_tab() {
    let prefix = ContainerPrefix::from_ops(&[StripOp::ListAdvance(2)], true);
    let raw = ["- a", "\t: def"];
    let lines = StrippedLines::new(&raw, 0, &prefix);
    assert_eq!(next_line_is_definition_marker(&lines, 0), None);
}

// --- End-to-end pins for the Parser-coupled paths -------------------------
//
// `content_container_indent_to_strip` and the hand-rolled
// `leading_indent(x).0 >= content_col` sites are private Parser methods
// reading live container state; constructing that state by hand would
// re-derive the logic under test. They are pinned here at the public
// boundary instead: parse, assert structure, assert losslessness.

fn assert_lossless(input: &str) {
    let tree = parse_blocks(input);
    assert_eq!(tree.text().to_string(), input, "losslessness of {input:?}");
}

/// The tab-straddle shape end to end: pandoc reads a definition list
/// (`- a` item, `b` term, `: def` definition); panache's lookahead
/// cannot see the marker behind the tab, so no definition list forms.
/// DISAGREES with pandoc. Expected to flip in fix(parser): recognize
/// definition markers behind a straddling tab.
#[test]
fn tab_straddle_definition_marker_end_to_end() {
    let input = "- a\n\n  b\n\n\t: def\n";
    let tree = parse_blocks(input);
    assert_eq!(
        find_all(&tree, SyntaxKind::DEFINITION_LIST).len(),
        0,
        "pinned: no definition list forms today"
    );
    assert_lossless(input);
}

/// A line ending in ` :` short of the item's content column must not
/// promote the paragraph above it into a term: the "indent" `strip`
/// found was faked from content bytes (`- a\nb\n\nc :` is a bullet list
/// plus `Para "c :"` in pandoc). Pins the `carries_container_prefix`
/// vetting in `next_line_is_definition_marker`.
#[test]
fn faked_indent_marker_does_not_promote_a_term() {
    let input = "- a\nb\n\nc :\n";
    let tree = parse_blocks(input);
    assert_eq!(find_all(&tree, SyntaxKind::DEFINITION_LIST).len(), 0);
    assert_lossless(input);
}

/// A marker at the open definition body's content column belongs to a
/// *nested* definition list; one dedented below it closes the body and
/// reads as a sibling definition of the outer term. Pins the
/// `content_container_indent_to_strip`-based dedent tests in
/// `definition_marker_over_open_body_block` and
/// `blank_line_promotes_buffered_definition_term`.
#[test]
fn definition_marker_nests_at_content_col_and_siblings_below_it() {
    let nested = "T\n\n:   a\n\n    : b\n";
    let tree = parse_blocks(nested);
    assert_eq!(
        find_all(&tree, SyntaxKind::DEFINITION_LIST).len(),
        2,
        "marker at the body's content column opens a nested definition list"
    );
    assert_lossless(nested);

    let sibling = "T\n\n:   a\n\n: b\n";
    let tree = parse_blocks(sibling);
    assert_eq!(
        find_all(&tree, SyntaxKind::DEFINITION_LIST).len(),
        1,
        "dedented marker stays in the outer definition list"
    );
    assert_eq!(find_all(&tree, SyntaxKind::DEFINITION).len(), 2);
    assert_lossless(sibling);
}

/// A definition marker under a list item's marker line forms a nested
/// definition list only when it sits at the item's content column; at
/// column 0 it has left the item and pandoc reads `Para ": def"`
/// instead (verified: `pandoc -f markdown -t native`). Pins
/// `definition_marker_breaks_open_list_item_block` and the
/// `current_content_col` convention it feeds
/// `definition_marker_in_list_frame`.
#[test]
fn definition_marker_under_list_item_first_line() {
    let dedented = "- Term\n: def\n";
    let tree = parse_blocks(dedented);
    assert_eq!(
        find_all(&tree, SyntaxKind::DEFINITION_LIST).len(),
        0,
        "a column-0 marker under an item is not a definition (matches pandoc)"
    );
    assert_lossless(dedented);

    let at_content_col = "- Term\n  : def\n";
    let tree = parse_blocks(at_content_col);
    let item = find_first(&tree, SyntaxKind::LIST_ITEM).expect("list item should parse");
    assert_eq!(
        find_all(&item, SyntaxKind::DEFINITION_LIST).len(),
        1,
        "a marker at the item's content column nests a definition list (matches pandoc)"
    );
    assert_lossless(at_content_col);
}

/// A term/definition pair indented to a footnote body's content column
/// stays inside the footnote (pins the footnote-frame term lookahead,
/// which passes raw lines and `FOOTNOTE_INDENT_COLUMNS`).
#[test]
fn definition_list_nests_inside_footnote_body() {
    let input = "[^1]: Term\n\n    :   def\n";
    let tree = parse_blocks(input);
    let footnote = find_first(&tree, SyntaxKind::FOOTNOTE_DEFINITION)
        .expect("footnote definition should parse");
    assert_eq!(
        find_all(&footnote, SyntaxKind::DEFINITION_LIST).len(),
        1,
        "the definition list belongs to the footnote body"
    );
    assert_lossless(input);
}
