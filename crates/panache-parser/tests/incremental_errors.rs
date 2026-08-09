//! Error-splicing matrix for incremental reparsing.
//!
//! The governing invariant covers the syntax-error vector as much as the tree,
//! and the merge has exactly two buckets — errors in the retained prefix,
//! carried verbatim, and errors from the window parse, shifted to host
//! coordinates. Each bucket can be *unchanged*, *fixed*, or *introduced* by an
//! edit, and each reparse strategy reaches them differently, so this file is
//! that matrix: {unchanged, fixed, introduced} x {suffix window, section
//! window, full-parse bail}.
//!
//! Malformed YAML is the only source of syntax errors, so every case is built
//! from a frontmatter or mid-document metadata block. Mid-document metadata is
//! a Pandoc-dialect feature, which is why these run under default options.
//!
//! The strategy is pinned in every case: an error assertion that silently
//! started running against a full-parse fallback would prove nothing.

use panache_parser::parser::parse_with_errors;

mod common;
use common::reparse_or_full;

fn apply_edit(text: &str, old: (usize, usize), insert: &str) -> String {
    let mut out = String::with_capacity(text.len() - (old.1 - old.0) + insert.len());
    out.push_str(&text[..old.0]);
    out.push_str(insert);
    out.push_str(&text[old.1..]);
    out
}

/// Replace the first occurrence of `find` with `insert`, reparse
/// incrementally, and assert the strategy, the error count, and — the point of
/// the file — that the spliced errors equal a full parse's.
fn check(input: &str, find: &str, insert: &str, expected_strategy: &str, expected_errors: usize) {
    let (old_tree, old_errors) = parse_with_errors(input, None);
    let start = input
        .find(find)
        .unwrap_or_else(|| panic!("{find:?} not in {input:?}"));
    let old_edit = (start, start + find.len());
    let updated = apply_edit(input, old_edit, insert);
    let new_edit = (old_edit.0, old_edit.0 + insert.len());

    let inc = reparse_or_full(&updated, None, &old_tree, &old_errors, old_edit, new_edit);
    let (_, full_errors) = parse_with_errors(&updated, None);

    assert_eq!(
        inc.strategy, expected_strategy,
        "wrong strategy for {find:?} -> {insert:?}"
    );
    assert_eq!(
        inc.errors, full_errors,
        "spliced errors diverged from a full parse ({})",
        inc.strategy
    );
    assert_eq!(
        inc.errors.len(),
        expected_errors,
        "unexpected error count ({}): {:?}",
        inc.strategy,
        inc.errors
    );
}

// --- suffix window ------------------------------------------------------

/// Malformed frontmatter stays in the *retained prefix*: nothing in the
/// reparsed window can re-derive it, so it exists only in `old_errors`. This
/// is the case a splice that dropped the prefix bucket would fail.
#[test]
fn suffix_window_unchanged_error_in_the_retained_prefix() {
    check(
        "---\ntitle: [\n---\n\npara one\n\npara two\n",
        "para two",
        "para three",
        "suffix_window",
        1,
    );
}

#[test]
fn suffix_window_error_introduced_inside_the_window() {
    check(
        "para one\n\n---\ntitle: ok\n---\n\npara two\n",
        "ok",
        "[",
        "suffix_window",
        1,
    );
}

#[test]
fn suffix_window_error_fixed_inside_the_window() {
    check(
        "para one\n\n---\ntitle: [\n---\n\npara two\n",
        "[",
        "ok",
        "suffix_window",
        0,
    );
}

/// Both buckets at once: a prefix error that must survive and a window error
/// that must not be double-counted against it.
#[test]
fn suffix_window_carries_prefix_error_while_introducing_a_window_error() {
    check(
        "---\ntitle: [\n---\n\npara one\n\n---\nkey: ok\n---\n\npara two\n",
        "ok",
        "[",
        "suffix_window",
        2,
    );
}

// --- section window -----------------------------------------------------

const SECTIONS: &str =
    "# One\n\npara one\n\n## Two\n\n---\ntitle: ok\n---\n\npara two\n\n# Three\n\npara three\n";

const SECTIONS_BROKEN: &str =
    "# One\n\npara one\n\n## Two\n\n---\ntitle: [\n---\n\npara two\n\n# Three\n\npara three\n";

/// The edit is in the last section, so the window runs from `# Three` to EOF
/// and the malformed block two sections above it is prefix-bucket: carried,
/// never re-derived.
#[test]
fn section_window_unchanged_error_before_the_window() {
    check(
        SECTIONS_BROKEN,
        "para three",
        "para four",
        "section_window",
        1,
    );
}

#[test]
fn section_window_error_introduced_inside_the_window() {
    check(SECTIONS, "ok", "[", "section_window", 1);
}

#[test]
fn section_window_error_fixed_inside_the_window() {
    check(SECTIONS_BROKEN, "[", "ok", "section_window", 0);
}

// --- full-parse bail ----------------------------------------------------

/// The refdef guard bails on a `]:` near the edit. A bail is a plain full
/// parse, so its errors are correct by construction — but only if the bail
/// path actually reports them rather than returning an empty vector.
#[test]
fn full_reparse_bail_still_reports_errors() {
    check(
        "---\ntitle: [\n---\n\npara one\n\n[x]: /url\n\npara two\n",
        "/url",
        "/other",
        "full_reparse",
        1,
    );
}

#[test]
fn full_reparse_bail_reports_an_error_the_edit_introduces() {
    check(
        "---\ntitle: ok\n---\n\n[x]: /url\n\npara two\n",
        "ok",
        "[",
        "full_reparse",
        1,
    );
}

#[test]
fn full_reparse_bail_reports_an_error_the_edit_fixes() {
    check(
        "---\ntitle: [\n---\n\n[x]: /url\n\npara two\n",
        "[",
        "ok",
        "full_reparse",
        0,
    );
}
