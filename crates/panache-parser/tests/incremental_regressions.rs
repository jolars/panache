//! Minimized reproducers for divergences found by the incremental fuzz
//! harness (`incremental_fuzz.rs`). Each comment names the trap and the
//! fuzz case that found it; a red test here is `#[ignore]`d only until its
//! fix lands (see the "Incremental Parsing" roadmap in `TODO.md`).
//!
//! Three of the finds turned out to be **full-parser bugs**, not incremental
//! ones: the spliced tree faithfully matched the full parse (the debug
//! oracle passed) and the full parse itself reordered bytes or panicked.
//! They are pinned here because this harness found them, but they were
//! fixed in the block parser, not in the incremental machinery.

use panache_parser::parser::{fingerprint, parse, parse_with_errors};

mod common;
use common::reparse_or_full;

fn apply_edit(text: &str, old: (usize, usize), insert: &str) -> String {
    let mut out = String::with_capacity(text.len() - (old.1 - old.0) + insert.len());
    out.push_str(&text[..old.0]);
    out.push_str(insert);
    out.push_str(&text[old.1..]);
    out
}

/// Full-parser losslessness: the parse of `input` must round-trip its bytes.
fn assert_full_parse_lossless(input: &str) {
    let tree = parse(input, None);
    assert_eq!(
        tree.text().to_string(),
        input,
        "full parse is lossy for this input"
    );
}

// Fuzz find: snippet lazy_list, seed 2654436281, single edit #30 (minimized).
// A reference definition on the line after a list item's text is emitted
// *before* the buffered item text, reordering the document's bytes:
// "- a\n[x]: /url\n" round-trips as "- [x]: /url\na\n". Through
// `panache format` the refdef line and the item text swap places. Fixed by
// `fix(parser): keep refdefs from interrupting list items`.
#[test]
fn full_parse_lossless_refdef_after_list_item_line() {
    assert_full_parse_lossless("- a\n[x]: /url\n");
}

// Fuzz find: small.qmd, seed 12648430, single edit #75 (minimized).
// A line-block marker (`| `) as indented list-item continuation, followed
// by a lazy line with a trailing pipe, panics the block parser outright:
// "marker presence verified upstream" in `blocks/line_blocks.rs`. Fixed by
// `fix(parser): match line-block peek to emitted prefix`.
#[test]
fn full_parse_must_not_panic_on_line_block_in_list_item() {
    let input = "- x\n\n  | a\n b |\n";
    let tree = parse(input, None);
    assert_eq!(tree.text().to_string(), input);
}

// Fuzz find: snippet lazy_blockquote, seed 1374496257, batch #0 (minimized).
// A `---` line directly after a blockquote line duplicates the quote
// marker: "> a\n---\nb\n" round-trips as "> > a\n---\nb\n". Through
// `panache format` the input becomes "> ## > a\nb" - marker duplicated and
// the thematic break folded into a setext heading. Fixed by
// `fix(parser): let setext claim a quoted line at top level`; pandoc reads
// the input as a top-level `Header 2 [Str ">", Space, Str "a"]`.
#[test]
fn full_parse_lossless_thematic_break_after_blockquote() {
    assert_full_parse_lossless("> a\n---\nb\n");
}

// Fuzz finds: snippets mid_document_yaml / hashpipe / fenced_code, pandoc and
// quarto tiers (minimized). A setext underline directly after a setext
// heading takes the `follows_setext_heading` escape in
// `SetextHeadingParser::detect_prepared` and returns `Yes` while the next
// paragraph is still buffered, which breaks the block-dispatch contract at
// `core.rs`: the heading is emitted *before* the buffered bytes, reordering
// the CST. Debug builds trip the contract's `debug_assert!`; release builds
// silently produce the reordering, so both shapes are pinned as losslessness
// failures. Pandoc-dialect only -- CommonMark folds the open paragraph into
// the heading instead. Not an incremental bug; needs the detector to return
// `YesCanInterrupt` (which flushes the buffers) or to gate itself.
#[test]
#[ignore = "known full-parser bug: setext-after-setext breaks the open-paragraph contract"]
fn full_parse_lossless_setext_underline_after_setext_heading() {
    assert_full_parse_lossless("a\nb\n---\nc\n---\n");
}

#[test]
#[ignore = "known full-parser bug: setext-after-setext breaks the open-paragraph contract"]
fn full_parse_lossless_setext_pair_after_unterminated_fence() {
    assert_full_parse_lossless("```\nx\n---\ny\n---\n");
}

/// Incremental invariant: the spliced tree must equal a full parse of the
/// edited text, structurally (fingerprint), textually, and in its syntax
/// errors.
fn check_incremental(before: &str, old_edit: (usize, usize), insert: &str) {
    check_incremental_via(before, old_edit, insert, None);
}

/// [`check_incremental`], asserting *which* strategy ran.
///
/// A reproducer whose document is small or whose window is wide gets declined
/// by the guard cascade and falls back to a full parse, at which point the
/// oracle compares a full parse against itself and the test passes without
/// exercising anything. Any case pinning a *splice* bug names its strategy so
/// that it fails loudly instead of going quiet.
fn check_incremental_strategy(
    before: &str,
    old_edit: (usize, usize),
    insert: &str,
    strategy: &'static str,
) {
    check_incremental_via(before, old_edit, insert, Some(strategy));
}

fn check_incremental_via(
    before: &str,
    old_edit: (usize, usize),
    insert: &str,
    expect_strategy: Option<&'static str>,
) {
    let (old_tree, old_errors) = parse_with_errors(before, None);
    let updated = apply_edit(before, old_edit, insert);
    let new_edit = (old_edit.0, old_edit.0 + insert.len());
    let inc = reparse_or_full(&updated, None, &old_tree, &old_errors, old_edit, new_edit);
    if let Some(expected) = expect_strategy {
        assert_eq!(
            inc.strategy, expected,
            "reparse took the {} path, so this case no longer exercises {expected}",
            inc.strategy
        );
    }
    let (full, full_errors) = parse_with_errors(&updated, None);
    assert_eq!(
        inc.tree.text().to_string(),
        full.text().to_string(),
        "splice text diverged from full parse (strategy {})",
        inc.strategy
    );
    assert_eq!(
        fingerprint(&inc.tree),
        fingerprint(&full),
        "structural divergence (strategy {})",
        inc.strategy
    );
    assert_eq!(
        inc.errors, full_errors,
        "syntax errors diverged from full parse (strategy {})",
        inc.strategy
    );
}

// Fuzz find: snippet unterminated_fence, seed 2654436537, single edit #153.
// Inserting at the blank line between an unterminated fence and the next
// paragraph produced a restart offset *after* the insertion point, so the
// splice retained stale bytes and dropped the inserted `\`. Fixed by the
// restart <= edit-start guard in `reparse_ranges`.
#[test]
fn insertion_at_blank_line_after_unterminated_fence() {
    check_incremental("```\ncode\n\npara after\n", (9, 9), "\\");
}

// Fuzz find: snippet use_before_refdef, tier pandoc, seed 1374497537,
// batch #43, chain step #1 (minimized). A suffix starting with a lone `:`
// reaches *backward* across the seam's blank line and promotes the retained
// paragraph into a definition-list `TERM`; parsed standalone it is only a
// paragraph, so the splice had two blocks where a full parse has one
// `DEFINITION_LIST`. Pandoc allows a blank line between term and definition,
// which is why blank-line separation does not decouple them. Fixed by pairing
// `last_retained_block_is_paragraph` with
// `first_nonblank_line_is_definition_marker`.
#[test]
fn definition_marker_suffix_after_a_retained_paragraph() {
    check_incremental("see [x] and [foo] here\n\nmore prose\n", (24, 35), ":");
}

// The `~` spelling of the same marker, which the list-continuation guard's
// character set does not cover.
#[test]
fn tilde_definition_marker_suffix_after_a_retained_paragraph() {
    check_incremental("term line\n\nmore prose\n", (11, 22), "~ definition\n");
}

// Fuzz find: snippet pipe_table, tier pandoc, seed 2654434233, single edit
// #244 (minimized). The same `:` line after a *table* is that table's
// caption rather than a definition term, so the retained table grows to
// swallow the seam.
#[test]
fn caption_marker_suffix_after_a_retained_table() {
    check_incremental("| a | b |\n|---|---|\n| 1 | 2 |\n\npara\n", (31, 35), ":");
}

// Fuzz find: snippet hr_vs_setext, tier pandoc, seed 1374499073, batch #72
// (minimized). The mirror image of the two above: `---` is a multiline-table
// border as well as a thematic break, so suffix content can turn the
// *retained* rule into the top rule of a table spanning the seam.
// `prefix_fence_state_is_stable` cannot carry this one -- dash runs are far
// too common for a parity count -- so the guard is on the retained block kind.
#[test]
fn suffix_content_can_reinterpret_a_retained_thematic_break() {
    check_incremental("- a\n\n---\n\n- b\n", (12, 13), "---\nk: v\n---\n");
}

// Fuzz find: medium_quarto.qmd, seed 12648174, single edit #15.
// A 22-byte deletion inside a `:::` callout body (swallowing the
// `{download="hello.qmd"}` attribute and the line break before the `:::`
// closer) made the section-window reparse diverge from a full parse: the
// window was parsed as a standalone document, and the mangled div context
// leaked across the window boundary. Fixed by parsing the section tail to
// EOF and re-adopting the old suffix only on structural equality.
//
// The reproducer is *synthetic*, not the corpus bytes: `medium_quarto.qmd` is
// gitignored and `benches/documents/download.sh` no longer produces it, so a
// corpus-reading test fails on every clean checkout. What it reconstructs is
// the shape from the fuzz case -- a deletion that glues a callout's closing
// `:::` onto its content line, leaving the div unterminated so it swallows
// everything below the window -- with enough prose above the section heading
// to keep the window under the size cutoff. The strategy it pins is
// `suffix_window`: the section window is *chosen*, and then the fix's
// structural-equality check refuses to re-adopt the old suffix (the
// unterminated div now swallows the section below it), degrading to a
// wholesale suffix splice. Pinning that is what keeps the case honest --
// a fallback to a full parse would compare a full parse against itself,
// and a `section_window` result would mean the leak was re-adopted.
fn callout_document() -> String {
    let mut doc = String::from("---\ntitle: Callouts\n---\n\n# Overview\n\n");
    for i in 0..40 {
        doc.push_str(&format!(
            "Paragraph {i} of the overview, here to keep the section below a\nsmall enough share of the document that the window is admitted.\n\n"
        ));
    }
    doc.push_str("## Downloads\n\n::: {.callout-note}\nGrab the [example](hello.qmd){download=\"hello.qmd\"}\n:::\n\nTrailing prose inside the edited section.\n\n");
    // A section *below* the window is what makes this case a leak rather than
    // a reparse to EOF: the unterminated div has somewhere to escape to.
    doc.push_str("## Afterword\n\nProse the mangled div must not swallow.\n");
    doc
}

#[test]
fn section_window_divergence_on_mangled_div_in_callout() {
    let before = callout_document();
    let attr = before.find("{download=").expect("attribute present");
    // 22 bytes: all but the opening brace of `{download="hello.qmd"}`, plus
    // the newline that separates it from the `:::` closer.
    let old_edit = (attr + 1, attr + 1 + 22);
    assert!(
        before[old_edit.0..old_edit.1].ends_with("\"}\n"),
        "edit must swallow the attribute and the line break before `:::`"
    );
    check_incremental_strategy(&before, old_edit, "_", "suffix_window");
}

// Fuzz find: snippet lazy_list, tier pandoc, seed 1374496001, batch #66,
// chain step #1 (minimized). A window whose first block has a line ending in
// ` :` reaches *backward* past the seam's blank line and promotes the retained
// list item's lazy continuation line into a definition-list `TERM`, swallowing
// the blank line into the item. Parsed standalone the window is only a
// paragraph, so the splice kept the list and the paragraph apart.
//
// The splice was **right** and the full parse wrong -- pandoc reads the input
// as `BulletList [[Plain [item one, SoftBreak, continuat]]]` followed by
// `Para [em, Space, :]` -- but the governing invariant measures the splice
// against the full parse, so the guard declines the shape until the parser bug
// below is fixed. The find is not CRLF-specific: it reproduces byte for byte
// on LF, and predates the line-ending fix that reshuffled the corpus onto it.
#[test]
fn trailing_definition_marker_window_after_a_retained_lazy_list() {
    check_incremental("- item one\ncontinuat\n\nem two\n", (25, 28), ":");
}

// The `~` spelling, and the CRLF twin of the case as the fuzzer found it.
#[test]
fn trailing_tilde_marker_window_after_a_retained_lazy_list() {
    check_incremental("- item one\ncontinuat\n\nem two\n", (25, 28), "~");
}

#[test]
fn trailing_definition_marker_window_under_crlf() {
    check_incremental("- item one\ncontinuat\r\n\r\nem two\n", (27, 30), ":");
}

// The full-parser bug the guard above works around. Not an incremental bug and
// not a losslessness failure (the tree round-trips), which is why the fuzz
// harness's lossy-or-panic skip does not catch it: it is a *divergence from
// pandoc*, so the harness sees a well-formed oracle and demands the splice
// match it. Pandoc:
//
//   [ BulletList [[Plain [Str "a", SoftBreak, Str "b"]]]
//   , Para [Str "c", Space, Str ":"] ]
//
// Fixing it belongs on `main` with the other block-parser finds; when it lands,
// delete `first_block_has_trailing_definition_marker` and its two guards.
#[test]
#[ignore = "known full-parser bug: a trailing `:` line promotes a preceding lazy list continuation to a definition term"]
fn full_parse_definition_list_from_trailing_colon_after_lazy_list_item() {
    let tree = parse("- a\nb\n\nc :\n", None);
    assert!(
        !format!("{tree:#?}").contains("DEFINITION_LIST"),
        "pandoc reads this as a bullet list plus a paragraph, with no definition list"
    );
}
