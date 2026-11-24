//! Minimized reproducers for divergences found by the incremental fuzz
//! harness (`incremental_fuzz.rs`). Each comment names the trap and the
//! fuzz case that found it; a red test here is `#[ignore]`d only until its
//! fix lands (see the "Incremental Parsing" roadmap in `TODO.md`).
//!
//! Five of the finds turned out to be **full-parser bugs**, not incremental
//! ones: the spliced tree faithfully matched the full parse (the debug
//! oracle passed) and the full parse itself reordered bytes, panicked, or
//! diverged from pandoc. They are pinned here because this harness found
//! them, but they were fixed in the block parser, not in the incremental
//! machinery.

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

#[test]
fn full_parse_lossless_refdef_after_list_item_line() {
    assert_full_parse_lossless("- a\n[x]: /url\n");
}

#[test]
fn full_parse_must_not_panic_on_line_block_in_list_item() {
    let input = "- x\n\n  | a\n b |\n";
    let tree = parse(input, None);
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn full_parse_lossless_thematic_break_after_blockquote() {
    assert_full_parse_lossless("> a\n---\nb\n");
}

#[test]
fn full_parse_lossless_setext_underline_after_setext_heading() {
    assert_full_parse_lossless("a\nb\n---\nc\n---\n");
}

#[test]
fn full_parse_lossless_setext_pair_after_unterminated_fence() {
    assert_full_parse_lossless("```\nx\n---\ny\n---\n");
}

/// Incremental invariant: the spliced tree must equal a full parse of the
/// edited text, structurally (fingerprint), textually, and in its syntax
/// errors.
fn check_incremental(before: &str, old_edit: (usize, usize), insert: &str) {
    check_incremental_via(before, old_edit, insert, None);
}

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

#[test]
fn insertion_at_blank_line_after_unterminated_fence() {
    check_incremental("```\ncode\n\npara after\n", (9, 9), "\\");
}

#[test]
fn definition_marker_suffix_after_a_retained_paragraph() {
    check_incremental("see [x] and [foo] here\n\nmore prose\n", (24, 35), ":");
}

#[test]
fn tilde_definition_marker_suffix_after_a_retained_paragraph() {
    check_incremental("term line\n\nmore prose\n", (11, 22), "~ definition\n");
}

#[test]
fn definition_marker_suffix_after_a_retained_definition_list() {
    check_incremental("term\n\n: definition\n\nmore prose\n", (20, 24), "~");
}

#[test]
fn definition_list_grown_below_a_retained_definition_list() {
    check_incremental("term\n\n: definition\n\nmore prose\n", (31, 31), ":");
}

#[test]
fn caption_marker_suffix_after_a_retained_table() {
    check_incremental("| a | b |\n|---|---|\n| 1 | 2 |\n\npara\n", (31, 35), ":");
}

#[test]
fn suffix_content_can_reinterpret_a_retained_thematic_break() {
    check_incremental("- a\n\n---\n\n- b\n", (12, 13), "---\nk: v\n---\n");
}

fn callout_document() -> String {
    let mut doc = String::from("---\ntitle: Callouts\n---\n\n# Overview\n\n");
    for i in 0..40 {
        doc.push_str(&format!(
            "Paragraph {i} of the overview, here to keep the section below a\nsmall enough share of the document that the window is admitted.\n\n"
        ));
    }
    doc.push_str("## Downloads\n\n::: {.callout-note}\nGrab the [example](hello.qmd){download=\"hello.qmd\"}\n:::\n\nTrailing prose inside the edited section.\n\n");
    doc.push_str("## Afterword\n\nProse the mangled div must not swallow.\n");
    doc
}

#[test]
fn section_window_divergence_on_mangled_div_in_callout() {
    let before = callout_document();
    let attr = before.find("{download=").expect("attribute present");
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
// against the full parse, so a guard declined the shape outright until the
// parser bug below was fixed. Now that it is, the splice is admitted and
// agrees with the full parse on pandoc's answer. The find is not CRLF-specific:
// it reproduces byte for byte on LF, and predates the line-ending fix that
// reshuffled the corpus onto it.
//
// All three name their strategy: under the retired guard these fell back to a
// full parse, at which point the oracle compared a full parse against itself
// and the cases passed without exercising a splice at all.
#[test]
fn trailing_definition_marker_window_after_a_retained_lazy_list() {
    check_incremental_strategy(
        "- item one\ncontinuat\n\nem two\n",
        (25, 28),
        ":",
        "suffix_window",
    );
}

// The `~` spelling, and the CRLF twin of the case as the fuzzer found it.
#[test]
fn trailing_tilde_marker_window_after_a_retained_lazy_list() {
    check_incremental_strategy(
        "- item one\ncontinuat\n\nem two\n",
        (25, 28),
        "~",
        "suffix_window",
    );
}

#[test]
fn trailing_definition_marker_window_under_crlf() {
    check_incremental_strategy(
        "- item one\ncontinuat\r\n\r\nem two\n",
        (27, 30),
        ":",
        "suffix_window",
    );
}

// The full-parser bug the three cases above used to work around. Not an
// incremental bug and not a losslessness failure (the tree round-trips), which
// is why the fuzz harness's lossy-or-panic skip did not catch it: it was a
// *divergence from pandoc*, so the harness saw a well-formed oracle and
// demanded the splice match it. Pandoc:
//
//   [ BulletList [[Plain [Str "a", SoftBreak, Str "b"]]]
//   , Para [Str "c", Space, Str ":"] ]
//
// Fixed by `fix(parser): gate definition term lookahead on real indent`: the
// container-indent strip was slicing content bytes off an under-indented lazy
// continuation, manufacturing a bare `:` marker out of `c :`, and the term
// lookahead read that as one. It now declines a `FrameVerdict::FakedIndent`
// line. That retired `first_block_has_trailing_definition_marker` in the
// reparse cascade, whose only job was to keep the splice matching this parse.
#[test]
fn full_parse_definition_list_from_trailing_colon_after_lazy_list_item() {
    let tree = parse("- a\nb\n\nc :\n", None);
    assert!(
        !format!("{tree:#?}").contains("DEFINITION_LIST"),
        "pandoc reads this as a bullet list plus a paragraph, with no definition list"
    );
}

#[test]
fn vitepress_edit_changes_the_closing_fence() {
    use panache_parser::{Flavor, ParserOptions};

    let before = "Earlier paragraph.\n\n:::: info Note\n::: tip Inner\nBody.\n::::\nOutside.\n";
    let options = Some(ParserOptions::for_flavor(Flavor::VitePress));
    let (old_tree, old_errors) = parse_with_errors(before, options.clone());
    let closing = before.find("\n::::\n").unwrap() + 1;
    let updated = apply_edit(before, (closing, closing + 1), "");
    let spliced = reparse_or_full(
        &updated,
        options.clone(),
        &old_tree,
        &old_errors,
        (closing, closing + 1),
        (closing, closing),
    );
    let (full, errors) = parse_with_errors(&updated, options);
    assert_eq!(spliced.tree.green(), full.green());
    assert_eq!(spliced.errors, errors);
}
