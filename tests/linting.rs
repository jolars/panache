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

fn lint_file_with_config(filename: &str, config_toml: &str) -> Vec<panache::linter::Diagnostic> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("linting")
        .join(filename);
    let input = fs::read_to_string(&path).unwrap_or_else(|_| panic!("Failed to read {}", filename));
    let config = toml::from_str::<Config>(config_toml).expect("valid config");
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
fn test_chunk_label_and_heading_id_can_share_label() {
    let diagnostics = lint_file_with_config(
        "chunk_label_and_heading_id_same_label.Rmd",
        r#"
flavor = "rmarkdown"
"#,
    );
    let dup: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "duplicate-reference-labels")
        .collect();

    assert!(
        dup.is_empty(),
        "Heading IDs and chunk labels should not be treated as duplicate cross-reference labels"
    );
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

#[test]
fn test_undefined_anchor() {
    let diagnostics = lint_file("undefined_anchor.md");
    let anchors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "undefined-anchor")
        .collect();

    assert_eq!(anchors.len(), 1, "Should flag 1 unresolvable anchor");
    assert!(anchors[0].message.contains("#reel"));
    assert_eq!(anchors[0].location.line, 3);
}

#[test]
fn test_citation_ref_anchor() {
    // Pandoc renders bibliography entries with id="ref-<citekey>"; links of
    // the shape [text](#ref-citekey) override the citation's display text.
    // See https://github.com/jolars/panache/discussions/289 and
    // https://github.com/jgm/pandoc/issues/11657.
    let diagnostics = lint_file("citation_ref_anchor.md");
    let anchors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "undefined-anchor")
        .collect();

    assert_eq!(
        anchors.len(),
        1,
        "ref-<citekey> anchors should resolve for cited keys, got {:?}",
        anchors
    );
    assert!(anchors[0].message.contains("#ref-missing"));
}

#[test]
fn test_missing_reference_targets() {
    let diagnostics = lint_file("missing_references.md");
    let missing_ref: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "undefined-reference-label")
        .collect();
    let missing_footnote: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "undefined-footnote-id")
        .collect();

    assert_eq!(
        missing_ref.len(),
        1,
        "Should flag 1 missing reference label"
    );
    assert_eq!(missing_footnote.len(), 1, "Should flag 1 missing footnote");
    assert!(missing_ref[0].message.contains("[missing]"));
    assert!(missing_footnote[0].message.contains("[^missing-note]"));
}

#[test]
fn test_missing_reference_targets_can_be_disabled() {
    let diagnostics = lint_file_with_config(
        "missing_references.md",
        r#"
[lint.rules]
undefined-references = false
"#,
    );

    let missing_ref: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "undefined-reference-label")
        .collect();
    let missing_footnote: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "undefined-footnote-id")
        .collect();

    assert!(missing_ref.is_empty());
    assert!(missing_footnote.is_empty());
}

#[test]
fn test_unused_definitions() {
    let diagnostics = lint_file("unused_definitions.md");
    let unused_labels: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "unused-definition-label")
        .collect();
    let unused_footnotes: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "unused-footnote-id")
        .collect();

    assert_eq!(
        unused_labels.len(),
        1,
        "Should flag one unused reference label"
    );
    assert_eq!(unused_footnotes.len(), 1, "Should flag one unused footnote");
    assert!(unused_labels[0].message.contains("[unusedlabel]"));
    assert!(unused_footnotes[0].message.contains("[^2]"));
}

#[test]
fn test_unused_definitions_can_be_disabled() {
    let diagnostics = lint_file_with_config(
        "unused_definitions.md",
        r#"
[lint.rules]
unused-definitions = false
"#,
    );

    let unused_labels: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "unused-definition-label")
        .collect();
    let unused_footnotes: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "unused-footnote-id")
        .collect();

    assert!(unused_labels.is_empty());
    assert!(unused_footnotes.is_empty());
}

#[test]
fn test_bookdown_chunk_crossref_is_resolved() {
    let diagnostics = lint_file_with_config(
        "bookdown_chunk_crossref.Rmd",
        r#"
flavor = "rmarkdown"
"#,
    );

    let missing_ref: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "undefined-reference-label")
        .collect();

    assert!(
        missing_ref.is_empty(),
        "Bookdown chunk cross-reference should resolve against chunk labels"
    );
}

#[test]
fn test_bookdown_theorem_crossref_is_resolved() {
    let diagnostics = lint_file_with_config(
        "bookdown_theorem_crossref.Rmd",
        r#"
flavor = "rmarkdown"
"#,
    );

    let missing_ref: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "undefined-reference-label")
        .collect();

    assert!(
        missing_ref.is_empty(),
        "Bookdown theorem cross-reference should resolve against fenced div id"
    );
}

#[test]
fn test_bookdown_equation_crossref_is_resolved() {
    let diagnostics = lint_file_with_config(
        "bookdown_equation_crossref.Rmd",
        r#"
flavor = "rmarkdown"
"#,
    );

    let missing_ref: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "undefined-reference-label")
        .collect();

    assert!(
        missing_ref.is_empty(),
        "Bookdown equation cross-reference should resolve against equation labels"
    );
}

#[test]
fn test_bookdown_equation_crossref_can_be_disabled() {
    let diagnostics = lint_file_with_config(
        "bookdown_equation_crossref.Rmd",
        r#"
flavor = "rmarkdown"

[extensions]
bookdown-equation-references = false
"#,
    );

    let missing_ref: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "undefined-reference-label")
        .collect();

    assert_eq!(
        missing_ref.len(),
        2,
        "Disabling bookdown equation references should restore unresolved eq diagnostics"
    );
}

#[test]
fn test_chunk_label_spaces() {
    let diagnostics = lint_file_with_config(
        "chunk_label_spaces.md",
        r#"
flavor = "quarto"
"#,
    );
    let label_issues: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "chunk-label-spaces")
        .collect();

    assert_eq!(
        label_issues.len(),
        2,
        "Should flag labels containing spaces"
    );
    assert!(label_issues[0].message.contains("several words"));
    assert!(label_issues[1].message.contains("another label"));
}

#[test]
fn test_chunk_label_spaces_can_be_disabled() {
    let diagnostics = lint_file_with_config(
        "chunk_label_spaces.md",
        r#"
flavor = "quarto"

[lint.rules]
chunk-label-spaces = false
"#,
    );

    let label_issues: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "chunk-label-spaces")
        .collect();
    assert!(label_issues.is_empty());
}

#[test]
fn test_missing_chunk_labels() {
    let diagnostics = lint_file_with_config(
        "missing_chunk_labels.md",
        r#"
flavor = "quarto"
"#,
    );
    let missing: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "missing-chunk-labels")
        .collect();
    assert_eq!(
        missing.len(),
        1,
        "Should flag only unlabeled executable chunks"
    );
}

#[test]
fn test_missing_chunk_labels_can_be_disabled() {
    let diagnostics = lint_file_with_config(
        "missing_chunk_labels.md",
        r#"
flavor = "quarto"

[lint.rules]
missing-chunk-labels = false
"#,
    );
    let missing: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "missing-chunk-labels")
        .collect();
    assert!(missing.is_empty());
}

#[test]
fn test_missing_figure_crossref_captions_quarto_is_not_flagged() {
    let diagnostics = lint_file_with_config(
        "missing_figure_crossref_captions.qmd",
        r#"
flavor = "quarto"
"#,
    );
    let caption_issues: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "figure-crossref-captions")
        .collect();

    assert!(
        caption_issues.is_empty(),
        "Quarto figure crossrefs should not require fig-cap"
    );
}

#[test]
fn test_missing_figure_crossref_captions_bookdown() {
    let diagnostics = lint_file_with_config(
        "missing_figure_crossref_captions.Rmd",
        r#"
flavor = "rmarkdown"
"#,
    );
    let caption_issues: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "figure-crossref-captions")
        .collect();

    assert_eq!(
        caption_issues.len(),
        1,
        "Should flag one bookdown figure crossref with missing caption"
    );
    assert!(caption_issues[0].message.contains("@fig:a-label"));
}

#[test]
fn test_missing_figure_crossref_captions_can_be_disabled() {
    let diagnostics = lint_file_with_config(
        "missing_figure_crossref_captions.Rmd",
        r#"
flavor = "rmarkdown"

[lint.rules]
figure-crossref-captions = false
"#,
    );
    let caption_issues: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "figure-crossref-captions")
        .collect();
    assert!(caption_issues.is_empty());
}

#[test]
fn test_unknown_emoji_alias() {
    let diagnostics = lint_file_with_config(
        "emoji_aliases.md",
        r#"
[extensions]
emoji = true
"#,
    );

    let emoji_issues: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "unknown-emoji-alias")
        .collect();
    assert_eq!(emoji_issues.len(), 1, "Should flag one unknown emoji alias");
    assert!(emoji_issues[0].message.contains(":not-a-real-emoji:"));
}

#[test]
fn test_unknown_emoji_alias_can_be_disabled() {
    let diagnostics = lint_file_with_config(
        "emoji_aliases.md",
        r#"
[extensions]
emoji = true

[lint.rules]
unknown-emoji-alias = false
"#,
    );

    let emoji_issues: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "unknown-emoji-alias")
        .collect();
    assert!(emoji_issues.is_empty());
}

#[test]
fn test_unused_definitions_resolved_across_project_files() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root = temp_dir.path();
    let doc1 = root.join("1-one.Rmd");
    let doc2 = root.join("2-two.Rmd");

    fs::write(root.join("_bookdown.yml"), "").unwrap();
    fs::write(&doc1, "[shared]: https://example.com\n").unwrap();
    fs::write(&doc2, "See [x][shared].\n").unwrap();

    let input = fs::read_to_string(&doc1).unwrap();
    let config = toml::from_str::<Config>("flavor = \"rmarkdown\"").expect("valid config");
    let tree = panache::parse(&input, Some(config.clone()));
    let metadata = panache::metadata::extract_project_metadata(&tree, &doc1).ok();
    let diagnostics =
        panache::linter::lint_with_metadata(&tree, &input, &config, metadata.as_ref());

    let unused_labels: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "unused-definition-label")
        .collect();
    assert!(
        unused_labels.is_empty(),
        "Definition used in sibling project document should not be flagged unused"
    );
}

#[test]
fn test_html_entities_default_on() {
    let diagnostics = lint_file("html_entities.md");
    let issues: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "html-entities")
        .collect();

    assert_eq!(
        issues.len(),
        3,
        "expected exactly 3 html-entities diagnostics (typo + missing-semi + near-miss), got {:?}",
        issues.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let typo = issues
        .iter()
        .find(|d| d.message.contains("&ellips;"))
        .expect("typo diagnostic for &ellips;");
    assert_eq!(typo.location.line, 1);

    let missing_semi = issues
        .iter()
        .find(|d| d.message.contains("&numero"))
        .expect("missing-semicolon diagnostic for &numero");
    assert_eq!(missing_semi.location.line, 3);
    assert!(missing_semi.message.contains("missing"));

    let near_miss = issues
        .iter()
        .find(|d| d.message.contains("&hellp"))
        .expect("near-miss diagnostic for &hellp");
    assert_eq!(near_miss.location.line, 5);
    assert!(
        near_miss
            .notes
            .iter()
            .any(|n| n.message.contains("&hellip;"))
    );
}

#[test]
fn test_html_entities_can_be_disabled() {
    let diagnostics = lint_file_with_config(
        "html_entities.md",
        r#"
[lint.rules]
html-entities = false
"#,
    );

    let issues: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "html-entities")
        .collect();
    assert!(issues.is_empty());
}

#[test]
fn test_adjacent_footnote_refs() {
    let diagnostics = lint_file("adjacent_footnote_refs.md");
    let issues: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "adjacent-footnote-refs")
        .collect();

    // One gap between [^a][^b] and two gaps in [^e][^f][^g] = 3 total.
    assert_eq!(issues.len(), 3, "expected 3 diagnostics, got {:?}", issues);
    for diag in &issues {
        let fix = diag.fix.as_ref().expect("rule provides an auto-fix");
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(fix.edits[0].replacement, " ");
    }
}

#[test]
fn test_footnote_ref_in_footnote_def() {
    let diagnostics = lint_file("footnote_ref_in_footnote_def.md");
    let issues: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "footnote-ref-in-footnote-def")
        .collect();

    // Expected hits inside def bodies:
    //   1. `[^b]` in [^a]:'s body
    //   2. `[^c]` inside **bold** in [^b]:'s body
    //   3. `[^d]` inside ~~strike~~ in [^b]:'s body
    //   4. `[^e]` inside [link]() in [^b]:'s body
    //   5. `[^g]` inside the nested blockquote in [^d]:'s body
    //   6. `[^h]` inside the nested list in [^d]:'s body
    // Non-hits: `[^f]` in a code span, `[@key]` citations, outer refs.
    assert_eq!(issues.len(), 6, "expected 6 diagnostics, got {:?}", issues);

    for diag in &issues {
        assert!(diag.fix.is_none(), "rule must not auto-fix");
        assert!(
            diag.notes.iter().any(|n| n.message.contains("nest")),
            "expected help note explaining footnotes don't nest"
        );
        assert!(diag.message.contains("pandoc"));
    }
}

#[test]
fn test_footnote_ref_in_footnote_def_can_be_disabled() {
    let diagnostics = lint_file_with_config(
        "footnote_ref_in_footnote_def.md",
        r#"
[lint.rules]
footnote-ref-in-footnote-def = false
"#,
    );

    let issues: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "footnote-ref-in-footnote-def")
        .collect();
    assert!(issues.is_empty());
}

#[test]
fn test_crossref_as_link_target() {
    let diagnostics = lint_file("crossref_as_link_target.md");
    let issues: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "crossref-as-link-target")
        .collect();

    // Four offenders: inline link, citation-key link, image link, wrapped link.
    assert_eq!(issues.len(), 4, "expected 4 diagnostics, got {:?}", issues);

    for diag in &issues {
        let fix = diag.fix.as_ref().expect("rule provides an auto-fix");
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(fix.edits[0].replacement, "#");
        let r = fix.edits[0].range;
        let span: usize = (r.end() - r.start()).into();
        assert_eq!(span, 1, "fix span must target exactly the '@' byte");
    }
}

#[test]
fn test_crossref_as_link_target_can_be_disabled() {
    let diagnostics = lint_file_with_config(
        "crossref_as_link_target.md",
        r#"
[lint.rules]
crossref-as-link-target = false
"#,
    );

    let issues: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "crossref-as-link-target")
        .collect();
    assert!(issues.is_empty());
}

#[test]
fn test_heading_eaten_attrs() {
    let diagnostics = lint_file("heading_eaten_attrs.md");
    let issues: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "heading-eaten-attrs")
        .collect();

    // Line 1: one comment after the (eaten) attr block.
    // Line 9: two comments around the (eaten) attr block.
    assert_eq!(issues.len(), 3, "expected 3 diagnostics, got {:#?}", issues);
    let lines: Vec<usize> = issues.iter().map(|d| d.location.line).collect();
    assert_eq!(lines, vec![1, 9, 9]);
    assert!(issues.iter().all(|d| d.message.contains("literal")));
    assert!(issues.iter().all(|d| d.fix.is_none()));
}

#[test]
fn test_heading_eaten_attrs_can_be_disabled() {
    let diagnostics = lint_file_with_config(
        "heading_eaten_attrs.md",
        r#"
[lint.rules]
heading-eaten-attrs = false
"#,
    );

    let issues: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "heading-eaten-attrs")
        .collect();
    assert!(issues.is_empty());
}

#[test]
fn test_heading_strip_comments_residue_off_by_default() {
    let diagnostics = lint_file("heading_strip_comments_residue.md");
    let issues: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "heading-strip-comments-residue")
        .collect();
    assert!(
        issues.is_empty(),
        "expected no diagnostics by default, got {:#?}",
        issues
    );
}

#[test]
fn test_heading_strip_comments_residue_when_enabled() {
    let diagnostics = lint_file_with_config(
        "heading_strip_comments_residue.md",
        r#"
[lint.rules]
heading-strip-comments-residue = true
"#,
    );
    let issues: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "heading-strip-comments-residue")
        .collect();

    // Only line 1 should fire: line 7's attrs are eaten (sibling rule), so
    // there are no real attrs adjacent to the comment.
    assert_eq!(issues.len(), 1, "expected 1 diagnostic, got {:#?}", issues);
    assert_eq!(issues[0].location.line, 1);
    assert!(issues[0].message.contains("--strip-comments"));
    assert!(issues[0].fix.is_none());
}

#[test]
fn test_stray_fenced_div_markers() {
    let diagnostics = lint_file("stray_fenced_div_markers.md");
    let issues: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "stray-fenced-div-markers")
        .collect();

    assert_eq!(issues.len(), 2, "expected 2 diagnostics, got {:#?}", issues);
    let lines: Vec<usize> = issues.iter().map(|d| d.location.line).collect();
    assert_eq!(lines, vec![9, 23]);
    assert!(issues.iter().all(|d| d.fix.is_none()));
    assert!(
        issues[0].message.contains(":::"),
        "message should mention the marker"
    );
}
