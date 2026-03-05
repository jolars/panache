//! Integration tests for linting rules.
//!
//! Test files are stored in `tests/linting/*.md` and tested with direct assertions.

use panache::{Config, linter::lint};
use std::fs;
use std::path::Path;

fn lint_file(filename: &str) -> Vec<panache::linter::Diagnostic> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("linting")
        .join(filename);

    let input = fs::read_to_string(&path).unwrap_or_else(|_| panic!("Failed to read {}", filename));

    let config = Config::default();
    let tree = panache::parse(&input, Some(config.clone()));
    lint(&tree, &input, &config)
}

#[test]
fn test_ignore_directives() {
    let diagnostics = lint_file("ignore_directives.md");
    let hierarchy_issues: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "heading-hierarchy")
        .collect();

    // Should find 1 heading hierarchy issue:
    // Line 3: Skip from h1 to h4
    // The h5 on line 9 is in an ignore region and won't be reported
    // Note: The rule still sees headings in ignore regions when tracking context,
    // so h2 after h5 doesn't violate because prev_level is updated to h5
    assert_eq!(
        hierarchy_issues.len(),
        1,
        "Should find 1 heading hierarchy issue"
    );

    // Check that we found the right violation
    assert_eq!(
        hierarchy_issues[0].location.line, 3,
        "Should warn about h4 at line 3"
    );
}

#[test]
fn test_duplicate_references() {
    let diagnostics = lint_file("duplicate_references.md");
    let dup: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "duplicate-reference-labels")
        .collect();

    assert_eq!(dup.len(), 1, "Should find exactly 1 duplicate");
    assert!(dup[0].message.contains("[ref1]"));
    assert_eq!(dup[0].location.line, 10);
}

#[test]
fn test_duplicate_case_insensitive() {
    let diagnostics = lint_file("duplicate_case_insensitive.md");
    let dup: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "duplicate-reference-labels")
        .collect();

    assert_eq!(dup.len(), 2, "Should find 2 duplicates (case-insensitive)");
    assert!(dup[0].message.contains("[myref]"));
    assert!(dup[1].message.contains("[MYREF]"));
    assert_eq!(dup[0].location.line, 6);
    assert_eq!(dup[1].location.line, 7);
}

#[test]
fn test_duplicate_footnotes() {
    let diagnostics = lint_file("duplicate_footnotes.md");
    let dup: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "duplicate-reference-labels")
        .collect();

    assert_eq!(dup.len(), 2, "Should find 2 duplicate footnotes");
    assert!(dup.iter().any(|d| d.message.contains("[^1]")));
    assert!(dup.iter().any(|d| d.message.contains("[^note]")));
}

#[test]
fn test_no_duplicates() {
    let diagnostics = lint_file("no_duplicates.md");
    let dup: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "duplicate-reference-labels")
        .collect();

    assert_eq!(dup.len(), 0, "Clean file should have no duplicates");
}

#[test]
fn test_whitespace_normalization() {
    let diagnostics = lint_file("whitespace_normalization.md");
    let dup: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "duplicate-reference-labels")
        .collect();

    assert_eq!(
        dup.len(),
        2,
        "Whitespace should be normalized - all 3 labels match"
    );
    // All reference the first definition on line 5
    assert!(dup[0].message.contains("first defined at line 5"));
    assert!(dup[1].message.contains("first defined at line 5"));
}
