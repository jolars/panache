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
use crate::options::{Dialect, Extensions, Flavor, ParserOptions};
use crate::parser::blocks::container_prefix::{
    ContainerPrefix, FrameVerdict, PrefixPieceKind, StripOp, StrippedLines,
    advance_emitted_marker_columns, strip_content_indent, strip_list_indent,
};
use crate::parser::blocks::definition_lists::{
    definition_marker_in_list_frame, next_line_is_definition_marker,
};
use crate::parser::blocks::tables::is_caption_followed_by_table;
use crate::parser::utils::container_stack::{
    Container, byte_index_at_column, content_container_indent, gobbled_indent_prefix_len,
};
use crate::syntax::SyntaxKind;

fn opts(dialect: Dialect) -> ParserOptions {
    let flavor = match dialect {
        Dialect::Pandoc => Flavor::Pandoc,
        Dialect::CommonMark => Flavor::CommonMark,
    };
    ParserOptions {
        flavor,
        dialect,
        extensions: Extensions::for_flavor(flavor),
        ..Default::default()
    }
}

struct PinRow {
    name: &'static str,
    ops: &'static [StripOp],
    line: &'static str,
    strip: &'static str,
    /// `strip_line_0_for_emission` with `list_marker_consumed_on_line_0`
    /// false (the innermost `ListAdvance` is preserved).
    strip_line_0_unmarked: &'static str,
    peek_prefix: &'static str,
    /// `(content_col, expected)` for `gobbled_indent_prefix_len`, where
    /// meaningful for the row's innermost content column.
    gobbled_len: Option<(usize, usize)>,
    /// What `ContainerPrefix::resolve` reports for the row — the typed
    /// verdict every migrated caller reads. Documents, per disagreement
    /// row, which legacy helper the verdict sides with.
    verdict: FrameVerdict<'static>,
    /// Which commit of the consolidation is expected to flip which
    /// column. Empty = frozen: these values must survive the refactor.
    expected_to_change: &'static str,
}

const PINS: &[PinRow] = &[
    PinRow {
        name: "tab_straddles_list_content_col",
        ops: &[StripOp::ListAdvance(2)],
        line: "\t: def",
        strip: ": def",
        strip_line_0_unmarked: "\t: def",
        peek_prefix: ": def",
        gobbled_len: Some((2, 0)),
        verdict: FrameVerdict::StraddlingTab {
            rest: "\t: def",
            cols_before_tab: 0,
        },
        expected_to_change: "",
    },
    PinRow {
        name: "short_line_content_not_sliced_as_indent",
        ops: &[StripOp::ListAdvance(2)],
        line: "c :",
        strip: "c :",
        strip_line_0_unmarked: "c :",
        peek_prefix: "c :",
        gobbled_len: Some((2, 0)),
        verdict: FrameVerdict::FakedIndent {
            rest: "c :",
            op_index: 0,
        },
        expected_to_change: "",
    },
    PinRow {
        name: "real_indent_reaches_content_col",
        ops: &[StripOp::ListAdvance(2)],
        line: "  : def",
        strip: ": def",
        strip_line_0_unmarked: "  : def",
        peek_prefix: ": def",
        gobbled_len: Some((2, 2)),
        verdict: FrameVerdict::Inside { rest: ": def" },
        expected_to_change: "",
    },
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
        peek_prefix: "b",
        gobbled_len: None,
        verdict: FrameVerdict::Inside { rest: "b" },
        expected_to_change: "",
    },
    PinRow {
        name: "short_content_indent_waved_through",
        ops: &[StripOp::ContentIndent(4)],
        line: "----",
        strip: "----",
        strip_line_0_unmarked: "----",
        peek_prefix: "----",
        gobbled_len: Some((4, 0)),
        verdict: FrameVerdict::Dedented {
            rest: "----",
            op_index: 0,
            cols_short: 4,
        },
        expected_to_change: "",
    },
    PinRow {
        name: "content_indent_lazy_short_line",
        ops: &[StripOp::ContentIndent(4)],
        line: "  x",
        strip: "x",
        strip_line_0_unmarked: "x",
        peek_prefix: "x",
        gobbled_len: Some((4, 2)),
        verdict: FrameVerdict::Dedented {
            rest: "x",
            op_index: 0,
            cols_short: 2,
        },
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
        assert_eq!(
            prefix.resolve(row.line),
            row.verdict,
            "{}: resolve",
            row.name
        );
        let _ = row.expected_to_change;
    }
}

#[test]
fn tab_straddle_primitives_disagree() {
    let line = "\t: def";
    assert_eq!(advance_emitted_marker_columns(line, 2), "\t: def");
    assert_eq!(strip_list_indent(line, 2), ": def");
    assert_eq!(byte_index_at_column(line, 2), 1);
    assert_eq!(gobbled_indent_prefix_len(line, 2), 0);
}

/// The blind column walk is reserved for the bytes the parser core has
/// already emitted: on a marker-line dispatch the innermost
/// `ListAdvance` is the item's `- ` marker, which is not whitespace and
/// must still be skipped.
#[test]
fn line_0_list_advance_skips_the_emitted_marker() {
    let bullet = ContainerPrefix::from_ops(&[StripOp::ListAdvance(2)], true);
    assert_eq!(bullet.strip_line_0_for_emission("- x"), "x");
    let ordered = ContainerPrefix::from_ops(&[StripOp::ListAdvance(3)], true);
    assert_eq!(ordered.strip_line_0_for_emission("1. x"), "x");
}

#[test]
fn list_advance_never_eats_content_bytes() {
    let prefix = ContainerPrefix::from_ops(&[StripOp::ListAdvance(2)], false);
    assert_eq!(prefix.strip("b |"), "b |");
    assert_eq!(prefix.strip("- x"), "- x");
    assert_eq!(prefix.strip(" b |"), "b |");

    let nested = ContainerPrefix::from_ops(
        &[
            StripOp::ListAdvance(2),
            StripOp::BlockQuoteMarker,
            StripOp::ListAdvance(2),
        ],
        true,
    );
    assert_eq!(nested.strip_line_0_for_emission("  > - x"), "x");
    assert_eq!(nested.strip_line_0_for_emission("x > - y"), "> - y");
}

/// The invariant behind the two rules above, over every op shape the
/// stack can produce: what the continuation strip consumes is container
/// prefix — indent and blockquote markers — never content. A caller can
/// therefore classify a line off `strip` and emit from the same bytes
/// without the two disagreeing.
#[test]
fn strip_consumes_only_container_prefix_bytes() {
    let shapes: &[&[StripOp]] = &[
        &[StripOp::ListAdvance(2)],
        &[StripOp::ListAdvance(4), StripOp::BlockQuoteMarker],
        &[StripOp::BlockQuoteMarker, StripOp::ListAdvance(2)],
        &[StripOp::ContentIndent(4), StripOp::ListAdvance(2)],
        &[
            StripOp::ListAdvance(2),
            StripOp::BlockQuoteMarker,
            StripOp::ListAdvance(2),
        ],
        &[
            StripOp::ContentIndent(4),
            StripOp::ListAdvance(2),
            StripOp::BlockQuoteMarker,
        ],
    ];
    let lines = [
        "\n",
        "  \n",
        "\r\n",
        "b |",
        " b |",
        "  b |",
        "\tb |",
        "- x",
        "c :",
        "> x",
        "  > x",
        "x > y",
        "----",
        "",
        "😄 x\n",
        "├── x\n",
        "  a\n",
    ];
    for ops in shapes {
        let prefix = ContainerPrefix::from_ops(ops, false);
        for line in lines {
            let rest = prefix.strip(line);
            assert!(
                line.ends_with(rest),
                "{ops:?} on {line:?}: {rest:?} is not a suffix"
            );
            let consumed = &line[..line.len() - rest.len()];
            assert!(
                consumed.chars().all(|c| matches!(c, ' ' | '\t' | '>')),
                "{ops:?} on {line:?}: consumed {consumed:?}, which is not container prefix"
            );
        }
    }
}

/// `split_pieces` captures the list-indent bytes for re-injection as a
/// `WHITESPACE` token, so it must never claim content bytes either.
#[test]
fn split_pieces_captures_whitespace_only_list_indent() {
    let prefix =
        ContainerPrefix::from_ops(&[StripOp::ListAdvance(2), StripOp::BlockQuoteMarker], false);
    let (line, inner) = prefix.split_pieces("  > x");
    assert_eq!(
        line.pieces(),
        &[
            (PrefixPieceKind::Indent, "  ".to_string()),
            (PrefixPieceKind::BqRun, "> ".to_string()),
        ]
    );
    assert_eq!(inner, "x");

    let (line, inner) = prefix.split_pieces("b > x");
    assert!(line.is_empty());
    assert_eq!(inner, "b > x");
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

#[test]
fn from_stack_and_from_scalars_orderings_diverge() {
    let stack_order = ContainerPrefix::from_ops(
        &[
            StripOp::ContentIndent(4),
            StripOp::ListAdvance(2),
            StripOp::BlockQuoteMarker,
        ],
        false,
    );
    let scalar_order = ContainerPrefix::from_scalars(1, 2, false, 4, false, Dialect::CommonMark);
    assert_eq!(stack_order.strip("    x"), "x");
    assert_eq!(scalar_order.strip("    x"), "x");
    assert_eq!(stack_order.strip("      > b"), "b");
    assert_eq!(scalar_order.strip("      > b"), "> b");
}

fn definition(content_col: usize) -> Container {
    use crate::parser::utils::text_buffer::ParagraphBuffer;
    Container::Definition {
        content_col,
        plain_open: false,
        plain_buffer: ParagraphBuffer::new(),
    }
}

fn footnote(content_col: usize) -> Container {
    Container::FootnoteDefinition { content_col }
}

fn admonition(content_col: usize) -> Container {
    Container::Admonition { content_col }
}

fn list() -> Container {
    use crate::parser::blocks::lists::ListMarker;
    Container::List {
        marker: ListMarker::Bullet('-'),
        base_indent_cols: 0,
        has_blank_between_items: false,
    }
}

fn list_item(content_col: usize) -> Container {
    use crate::parser::utils::list_item_buffer::ListItemBuffer;
    Container::ListItem {
        content_col,
        buffer: ListItemBuffer::new(),
        marker_only: false,
        virtual_marker_space: false,
    }
}

/// `from_stack` over a real container stack produces the stack-order
/// ops the scalar path cannot express: [Definition, List, ListItem]
/// yields `[ContentIndent(4), ListAdvance(2)]`, content indent first.
#[test]
fn from_stack_definition_above_list_orders_content_indent_first() {
    let stack = vec![definition(4), list(), list_item(2)];
    let p = ContainerPrefix::from_stack(&stack, false, &opts(Dialect::CommonMark));
    assert!(matches!(
        p.ops(),
        [StripOp::ContentIndent(4), StripOp::ListAdvance(2)]
    ));
    assert_eq!(p.strip("      a"), "a");
}

#[test]
fn from_stack_content_indent_ops_sum_to_the_stack_accessor() {
    let stacks: Vec<Vec<Container>> = vec![
        vec![definition(4)],
        vec![footnote(4), definition(4)],
        vec![list(), list_item(2), definition(4)],
        vec![
            definition(4),
            list(),
            list_item(2),
            Container::BlockQuote { start_line: None },
        ],
        vec![admonition(4), footnote(4)],
        vec![
            Container::BlockQuote { start_line: None },
            footnote(4),
            list(),
            list_item(6),
        ],
    ];
    for stack in &stacks {
        let p = ContainerPrefix::from_stack(stack, false, &opts(Dialect::CommonMark));
        assert_eq!(
            p.content_indent(),
            content_container_indent(stack),
            "ContentIndent ops must sum to the content-container column for {stack:?}"
        );
    }
}

#[test]
fn current_content_col_puts_a_footnote_width_in_the_list_slot() {
    use crate::parser::blocks::paragraphs::current_content_col;
    use crate::parser::utils::container_stack::ContainerStack;

    let mut containers = ContainerStack::new();
    containers.push(footnote(4));
    assert_eq!(current_content_col(&containers), 4);
    assert_eq!(containers.content_container_indent(), 4);

    containers.push(list());
    containers.push(list_item(2));
    assert_eq!(current_content_col(&containers), 2);
    assert_eq!(containers.content_container_indent(), 4);
}

/// The Pandoc lazy-blockquote gobble is dialect-gated: a lazy line
/// (fewer `>` markers than the quote is deep) loses its whole leading
/// whitespace under Pandoc and keeps it under CommonMark. Any faithful
/// verdict walk must preserve this interplay.
#[test]
fn lazy_blockquote_gobble_is_dialect_gated() {
    let stack = vec![Container::BlockQuote { start_line: None }];
    let pandoc = ContainerPrefix::from_stack(&stack, false, &opts(Dialect::Pandoc));
    let commonmark = ContainerPrefix::from_stack(&stack, false, &opts(Dialect::CommonMark));
    assert_eq!(pandoc.strip("    deep"), "deep");
    assert_eq!(commonmark.strip("    deep"), "    deep");
    assert_eq!(pandoc.strip("   \n"), "   \n");
}

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

#[test]
fn definition_marker_frame_depends_on_callers_content_col() {
    let line = "      : def";
    assert_eq!(
        definition_marker_in_list_frame(line, Some(4)),
        Some((':', 6, 1, 1))
    );
    assert_eq!(definition_marker_in_list_frame(line, Some(2)), None);
    assert_eq!(
        definition_marker_in_list_frame("\t: def", Some(2)),
        Some((':', 4, 1, 1))
    );
}

/// The lookahead agrees with the dispatch side pinned above: a marker
/// behind a tab straddling the content column is read off the
/// `StraddlingTab` verdict, in the frame the tab's stop lands on.
/// (Flipped from `None` by fix(parser): recognize definition markers
/// behind a straddling tab; pandoc reads the shape as a definition
/// list.)
#[test]
fn definition_lookahead_sees_marker_behind_straddling_tab() {
    let prefix = ContainerPrefix::from_ops(&[StripOp::ListAdvance(2)], true);
    let raw = ["- a", "\t: def"];
    let lines = StrippedLines::new(&raw, 0, &prefix);
    assert_eq!(next_line_is_definition_marker(&lines, 0, true), Some(0));
}

fn assert_lossless(input: &str) {
    let tree = parse_blocks(input);
    assert_eq!(tree.text().to_string(), input, "losslessness of {input:?}");
}

/// The tab-straddle shape end to end: pandoc reads a definition list
/// (`- a` item, `b` term, `: def` definition), and so does panache
/// since the lookahead reads the marker off the `StraddlingTab`
/// verdict. (Flipped from three paragraphs by fix(parser): recognize
/// definition markers behind a straddling tab.)
#[test]
fn tab_straddle_definition_marker_end_to_end() {
    let input = "- a\n\n  b\n\n\t: def\n";
    let tree = parse_blocks(input);
    assert_eq!(
        find_all(&tree, SyntaxKind::DEFINITION_LIST).len(),
        1,
        "the marker behind the straddling tab defines `b` (matches pandoc)"
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

/// What the caption probe's container bound deliberately does *not*
/// reject. The bound settles indent, and indent only decides a dedent
/// that follows a blank line: every line the grid lookahead reads past
/// the vetted one is contiguous with it, and pandoc admits a contiguous
/// dedent into the container as a lazy continuation. Both shapes below
/// parse as a table inside the definition body under
/// `pandoc -f markdown -t native`, so widening the frame bound over the
/// lookahead tail would be a false negative, not a fix.
#[test]
fn caption_probe_frame_bound_admits_lazy_continuations() {
    let prefix = ContainerPrefix::from_ops(&[StripOp::ContentIndent(4)], false);

    let header_sep = [
        "    : cap",
        "",
        "    A    B",
        "--- ---",
        "x    y",
        "--- ---",
    ];
    let view = StrippedLines::new(&header_sep, 0, &prefix);
    assert!(is_caption_followed_by_table(&view, 0));

    let single_col = ["    : cap", "", "    --", "    x", "--"];
    let view = StrippedLines::new(&single_col, 0, &prefix);
    assert!(is_caption_followed_by_table(&view, 0));
}

/// `div_closer_ends_lines` tracks presence: a div outside a
/// line-collected container fences that container's raw collection
/// (`notFollowedByDivCloser` — the div level was already set when the
/// lines were collected), and an innermost div's own content parse
/// carries the same guard, so a block sitting directly in the div stops
/// at the closer too. Both probed against `pandoc -f markdown -t
/// native`: a table directly in a div, and in a div nested inside a
/// definition body, both end at the `:::`.
#[test]
fn div_closer_ends_lines_needs_a_div_on_the_stack() {
    let p = |stack: &[Container]| {
        ContainerPrefix::from_stack(stack, false, &opts(Dialect::Pandoc)).div_closer_ends_lines()
    };
    assert!(p(&[fenced_div(0), definition(4)]));
    assert!(p(&[
        fenced_div(0),
        Container::BlockQuote { start_line: None }
    ]));
    assert!(p(&[fenced_div(0), list(), list_item(2)]));
    assert!(p(&[definition(4), fenced_div(0)]));
    assert!(p(&[fenced_div(0)]));
    assert!(!p(&[]));
}

fn fenced_div(open_indent_cols: usize) -> Container {
    Container::FencedDiv { open_indent_cols }
}

/// `note_marker_op_bound` tracks *presence*, unlike the div flag's
/// ordering: pandoc's note-marker fence lives in `noteBlock`'s
/// `rawLine`, and everything above the note on the stack sits inside
/// the note's collected raw, so any footnote below is enough. With no
/// footnote at all there is nothing to fence -- `listLine` and
/// `definitionListItem` collect a stray `[^x]:` as ordinary content.
#[test]
fn note_marker_bound_needs_a_footnote_on_the_stack() {
    let p = |stack: &[Container]| {
        ContainerPrefix::from_stack(stack, false, &opts(Dialect::Pandoc)).note_marker_op_bound()
    };
    assert_eq!(p(&[footnote(4)]), Some(0));
    assert_eq!(
        p(&[Container::BlockQuote { start_line: None }, footnote(4)]),
        Some(1)
    );
    assert_eq!(
        p(&[footnote(4), Container::BlockQuote { start_line: None }]),
        Some(0)
    );
    assert_eq!(p(&[list(), list_item(2), footnote(4)]), Some(1));
    assert_eq!(p(&[footnote(4), list(), list_item(4)]), Some(0));
    assert_eq!(p(&[list(), list_item(2)]), None);
    assert_eq!(p(&[definition(4)]), None);
    assert_eq!(p(&[]), None);
}

/// The marker test runs on the frame verdict's tail, not the raw line:
/// inside a blockquote the raw line still carries its `>` prefix, which
/// `try_parse_footnote_marker` (anchored at byte 0) would never match.
/// And a marker that *reaches* the note's content column is note
/// content -- `pandoc -f markdown -t native` collects it as a table
/// row (the fence is `skipNonindentSpaces >> noteMarker`, at most 3
/// spaces in the note's outer frame).
#[test]
fn note_marker_ends_container_lines_on_the_stripped_tail() {
    use crate::parser::blocks::tables::LineView;

    let quoted_note = ContainerPrefix::from_stack(
        &[Container::BlockQuote { start_line: None }, footnote(4)],
        false,
        &opts(Dialect::Pandoc),
    );
    let raw = [
        "> [^1]: body",
        ">     x    y",
        "> [^2]: two",
        ">     [^3]: not new",
    ];
    let view = StrippedLines::new(&raw, 0, &quoted_note);
    assert!(
        !view.ends_container_lines(1),
        "a line inside the note's frame is content"
    );
    assert!(
        view.ends_container_lines(2),
        "the marker terminates through the quote's own prefix"
    );
    assert!(
        !view.ends_container_lines(3),
        "a marker at the note's content column is note content"
    );
}

/// Dropping the innermost list advance renumbers the remaining ops, so
/// the op-index bound must be remapped with them.
#[test]
fn note_marker_bound_survives_without_innermost_list_advance() {
    let inner_list_first = ContainerPrefix::from_stack(
        &[list(), list_item(2), footnote(4)],
        false,
        &opts(Dialect::Pandoc),
    );
    assert_eq!(inner_list_first.note_marker_op_bound(), Some(1));
    assert_eq!(
        inner_list_first
            .without_innermost_list_advance()
            .note_marker_op_bound(),
        Some(0),
        "the removed ListAdvance sat before the bound"
    );

    let footnote_first = ContainerPrefix::from_stack(
        &[footnote(4), list(), list_item(4)],
        false,
        &opts(Dialect::Pandoc),
    );
    assert_eq!(footnote_first.note_marker_op_bound(), Some(0));
    assert_eq!(
        footnote_first
            .without_innermost_list_advance()
            .note_marker_op_bound(),
        Some(0),
        "the removed ListAdvance sat after the bound"
    );
}

/// `list_start_detect` tracks presence of an open list item: pandoc's
/// `listStart` fence lives in `listContinuationLine`, so with no item
/// run being collected there is nothing to fence.
#[test]
fn list_start_detect_needs_a_list_item_on_the_stack() {
    let p = |stack: &[Container]| {
        ContainerPrefix::from_stack(stack, false, &opts(Dialect::Pandoc))
            .list_start_detect()
            .is_some()
    };
    assert!(p(&[list(), list_item(2)]));
    assert!(p(&[
        Container::BlockQuote { start_line: None },
        list(),
        list_item(2)
    ]));
    assert!(p(&[footnote(4), list(), list_item(4)]));
    assert!(!p(&[]));
    assert!(!p(&[footnote(4)]));
    assert!(!p(&[definition(4)]));
}

/// The three-way split, verified against `pandoc -f markdown -t
/// native` over `10.  item` (content column 5, so a lazy band exists
/// between pandoc's 3-column `nonindentSpaces` tolerance and the
/// content column): a marker within 3 columns of the outer frame ends
/// the run, one in the lazy band is collected as content (a table
/// row), and one at the content column is nested-list content.
#[test]
fn list_start_ends_container_lines_three_way_split() {
    use crate::parser::blocks::tables::LineView;

    let prefix =
        ContainerPrefix::from_stack(&[list(), list_item(5)], false, &opts(Dialect::Pandoc));
    let raw = [
        "10.  item",
        "     x    y",
        "   - sib",
        "    - lazy",
        "     - nested",
    ];
    let view = StrippedLines::new(&raw, 0, &prefix);
    assert!(
        !view.ends_container_lines(1),
        "a line inside the item's frame is content"
    );
    assert!(
        view.ends_container_lines(2),
        "a marker within 3 columns of the outer frame ends the run"
    );
    assert!(
        !view.ends_container_lines(3),
        "the lazy band is a continuation, not a fence"
    );
    assert!(
        !view.ends_container_lines(4),
        "a marker at the content column is nested-list content"
    );
}

/// The fence tolerance is per nesting level: pandoc checks `listStart`
/// in the frame each nested list was parsed in — the outer item's
/// content reparse — so a marker's 3-column tolerance is measured from
/// the start of whichever band `[c_{j-1}, c_j)` of the section's
/// item-column ladder it falls in, not from the section frame's column
/// 0. Verified against `pandoc -f markdown -t native` over
/// `- outer` / `  10.  item` (ladder 2, 7): markers at columns 3-5
/// (within 3 of the enclosing item's column 2) end the run, 6 is a
/// lazy continuation, 7 is nested-list content.
#[test]
fn list_start_fence_tolerance_is_per_band_of_the_item_ladder() {
    use crate::parser::blocks::tables::LineView;

    let prefix = ContainerPrefix::from_stack(
        &[list(), list_item(2), list(), list_item(7)],
        false,
        &opts(Dialect::Pandoc),
    );
    let raw = [
        "10.  item",
        "       x    y",
        "   - sib",
        "    - band",
        "     - band",
        "      - lazy",
        "       - nested",
    ];
    let view = StrippedLines::new(&raw, 0, &prefix);
    assert!(
        !view.ends_container_lines(1),
        "a line inside the item's frame is content"
    );
    assert!(
        view.ends_container_lines(2),
        "a marker within 3 columns of the section frame ends the run"
    );
    assert!(
        view.ends_container_lines(3),
        "a marker within 3 columns of the enclosing item's column ends the run"
    );
    assert!(
        view.ends_container_lines(4),
        "the band tolerance reaches the enclosing item's column + 3"
    );
    assert!(
        !view.ends_container_lines(5),
        "past the band tolerance is a lazy continuation, not a fence"
    );
    assert!(
        !view.ends_container_lines(6),
        "a marker at the content column is nested-list content"
    );
}

/// The captured detection bits come from the caller's config: a fancy
/// `a.` marker is a fence under Pandoc options but plain text under
/// CommonMark ones, where `fancy_lists` is off.
#[test]
fn list_start_fence_respects_captured_config_bits() {
    use crate::parser::blocks::tables::LineView;

    let raw = ["- item", "  x    y", "a. alpha"];
    for (dialect, fancy_fence) in [(Dialect::Pandoc, true), (Dialect::CommonMark, false)] {
        let prefix = ContainerPrefix::from_stack(&[list(), list_item(2)], false, &opts(dialect));
        let view = StrippedLines::new(&raw, 0, &prefix);
        assert_eq!(
            view.ends_container_lines(2),
            fancy_fence,
            "fancy-list fence under {dialect:?}"
        );
    }
}

fn list_item_with_open_tag(content_col: usize, first_line: &str) -> Container {
    use crate::parser::utils::list_item_buffer::ListItemBuffer;
    let mut buffer = ListItemBuffer::new();
    buffer.push_text(first_line, &opts(Dialect::Pandoc));
    Container::ListItem {
        content_col,
        buffer,
        marker_only: false,
        virtual_marker_space: false,
    }
}

/// `html_closer_tag` tracks *ordering* like the div flag, anchored at
/// the list item whose buffer holds the open tag: pandoc's
/// `stateInHtmlBlock` is set when the open tag is parsed, which fences
/// only the containers whose lines are collected after that — the
/// item's own run is not fenced (pandoc slices the closer into the
/// item's own table), and a quote below the item collected its lines
/// before the tag existed.
#[test]
fn html_closer_tag_needs_a_container_above_the_tag_holding_item() {
    let p = |stack: &[Container]| {
        ContainerPrefix::from_stack(stack, false, &opts(Dialect::Pandoc))
            .html_closer_tag()
            .map(|(tag, col)| (tag.to_string(), col))
    };
    let armed = Some(("div".to_string(), 2));
    assert_eq!(
        p(&[
            list(),
            list_item_with_open_tag(2, "<div>"),
            Container::BlockQuote { start_line: None }
        ]),
        armed
    );
    assert_eq!(
        p(&[list(), list_item_with_open_tag(2, "<div>"), definition(4)]),
        armed
    );
    assert_eq!(p(&[list(), list_item_with_open_tag(2, "<div>")]), None);
    assert_eq!(
        p(&[
            Container::BlockQuote { start_line: None },
            list(),
            list_item_with_open_tag(2, "<div>")
        ]),
        None
    );
    assert_eq!(
        p(&[
            list(),
            list_item(2),
            Container::BlockQuote { start_line: None }
        ]),
        None
    );
    assert_eq!(p(&[]), None);
}

#[test]
fn html_closer_tag_survives_the_mid_item_flush() {
    use crate::parser::utils::list_item_buffer::ListItemBuffer;

    let config = opts(Dialect::Pandoc);
    let item = |buffer| Container::ListItem {
        content_col: 2,
        buffer,
        marker_only: false,
        virtual_marker_space: false,
    };

    let mut flushed = ListItemBuffer::new();
    flushed.push_text("<div>\n", &config);
    flushed.clear();
    let prefix = ContainerPrefix::from_stack(
        &[
            list(),
            item(flushed),
            Container::BlockQuote { start_line: None },
        ],
        false,
        &config,
    );
    assert_eq!(prefix.html_closer_tag(), Some(("div", 2)));

    let mut closed = ListItemBuffer::new();
    closed.push_text("<div>\n", &config);
    closed.clear();
    closed.push_text("</div>\n", &config);
    closed.clear();
    let prefix = ContainerPrefix::from_stack(
        &[
            list(),
            item(closed),
            Container::BlockQuote { start_line: None },
        ],
        false,
        &config,
    );
    assert_eq!(prefix.html_closer_tag(), None);
}

/// The closer fires at up to the tag-holding item's content column of
/// indent and not one space more: pandoc's item gobble eats at most
/// `content_col` columns before the guard's `htmlTag` runs, and
/// `htmlTag` skips no whitespace (probed: `  </div>` under a 2-column
/// item ends the quoted run, `   </div>` stays lazy quote content).
#[test]
fn html_closer_ends_container_lines_at_the_items_frame_position() {
    use crate::parser::blocks::tables::LineView;

    let prefix = ContainerPrefix::from_stack(
        &[
            list(),
            list_item_with_open_tag(2, "<div>"),
            Container::BlockQuote { start_line: None },
        ],
        false,
        &opts(Dialect::Pandoc),
    );
    let raw = [
        "  > col1  col2",
        "  > ----- -----",
        "  > a     b",
        "  </div>",
        "</div>",
        "   </div>",
        "  </span>",
        "  > </div>",
    ];
    let view = StrippedLines::new(&raw, 0, &prefix);
    assert!(
        !view.ends_container_lines(2),
        "a quoted row is quote content"
    );
    assert!(
        view.ends_container_lines(3),
        "the closer at the content column ends the run"
    );
    assert!(
        view.ends_container_lines(4),
        "the closer at column 0 ends the run"
    );
    assert!(
        !view.ends_container_lines(5),
        "one space past the content column is lazy content"
    );
    assert!(
        !view.ends_container_lines(6),
        "a different tag's closer is ordinary content"
    );
    assert!(
        !view.ends_container_lines(7),
        "a closer behind a quote marker is quote content"
    );
}

/// The open-tag signal is Pandoc-only: under CommonMark list items keep
/// their inline-HTML behavior and no fence arms.
#[test]
fn html_closer_fence_is_pandoc_only() {
    for (dialect, armed) in [(Dialect::Pandoc, true), (Dialect::CommonMark, false)] {
        let config = opts(dialect);
        use crate::parser::utils::list_item_buffer::ListItemBuffer;
        let mut buffer = ListItemBuffer::new();
        buffer.push_text("<div>\n", &config);
        let item = Container::ListItem {
            content_col: 2,
            buffer,
            marker_only: false,
            virtual_marker_space: false,
        };
        let prefix = ContainerPrefix::from_stack(
            &[list(), item, Container::BlockQuote { start_line: None }],
            false,
            &config,
        );
        assert_eq!(
            prefix.html_closer_tag().is_some(),
            armed,
            "fence armed under {dialect:?}"
        );
    }
}
