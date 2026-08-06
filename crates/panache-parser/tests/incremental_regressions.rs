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

use panache_parser::parser::{fingerprint, parse, parse_incremental_suffix};

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

/// Incremental invariant: the spliced tree must equal a full parse of the
/// edited text, structurally (fingerprint) and textually.
fn check_incremental(before: &str, old_edit: (usize, usize), insert: &str) {
    let old_tree = parse(before, None);
    let updated = apply_edit(before, old_edit, insert);
    let new_edit = (old_edit.0, old_edit.0 + insert.len());
    let inc = parse_incremental_suffix(&updated, None, &old_tree, old_edit, new_edit);
    let full = parse(&updated, None);
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
}

// Fuzz find: snippet unterminated_fence, seed 2654436537, single edit #153.
// Inserting at the blank line between an unterminated fence and the next
// paragraph produced a restart offset *after* the insertion point, so the
// splice retained stale bytes and dropped the inserted `\`. Fixed by the
// restart <= edit-start guard in `parse_incremental_suffix_inner`.
#[test]
fn insertion_at_blank_line_after_unterminated_fence() {
    check_incremental("```\ncode\n\npara after\n", (9, 9), "\\");
}

// Fuzz find: medium_quarto.qmd, seed 12648174, single edit #15.
// A 22-byte deletion inside a `:::` callout body (swallowing the
// `{download="hello.qmd"}` attribute and the line break before the `:::`
// closer) made the section-window reparse diverge from a full parse: the
// window was parsed as a standalone document, and the mangled div context
// leaked across the window boundary. Fixed by parsing the section tail to
// EOF and re-adopting the old suffix only on structural equality.
#[test]
fn section_window_divergence_on_mangled_div_in_callout() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../benches/documents/medium_quarto.qmd");
    let before = std::fs::read_to_string(&path).expect("corpus document present");
    check_incremental(&before, (2415, 2437), "_");
}
