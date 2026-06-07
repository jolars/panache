//! Tests for formatting workflows.

use super::helpers::*;
use lsp_types::Uri;
use std::fs;
use tempfile::TempDir;

/// Files matching `extend-exclude` (or `exclude`) in the discovered
/// `panache.toml` must be skipped by `textDocument/formatting`. Without this,
/// editors with format-on-save will keep rewriting files the project owner
/// explicitly opted out of.
#[test]
fn lsp_format_document_respects_extend_exclude() {
    let mut server = TestLspServer::new();
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Make this a project root so config discovery stops here.
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::write(
        root.join("panache.toml"),
        "extend-exclude = [\"vendor/**\"]\n",
    )
    .unwrap();

    let vendor_dir = root.join("vendor");
    fs::create_dir_all(&vendor_dir).unwrap();
    let doc_path = vendor_dir.join("third_party.qmd");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    let root_uri = Uri::from_file_path(root).expect("root uri");
    server.initialize(root_uri.as_str());

    // Contents that would otherwise produce edits (long line wraps to 80).
    let long = "This is a very long paragraph that should definitely be wrapped at around 80 characters because that is the default line width for panache.";
    server.open_document(doc_uri.as_str(), long, "quarto");

    let edits = server.format_document(doc_uri.as_str());
    assert_eq!(
        edits, None,
        "file under an excluded path must not return formatting edits"
    );
}

/// Range formatting is treated as an explicit user action (the user selected
/// text and asked to format it) and intentionally bypasses excludes, mirroring
/// the CLI's "explicit target bypasses excludes" rule.
#[test]
fn lsp_format_range_bypasses_extend_exclude() {
    let mut server = TestLspServer::new();
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    fs::create_dir_all(root.join(".git")).unwrap();
    fs::write(
        root.join("panache.toml"),
        "extend-exclude = [\"vendor/**\"]\n",
    )
    .unwrap();

    let vendor_dir = root.join("vendor");
    fs::create_dir_all(&vendor_dir).unwrap();
    let doc_path = vendor_dir.join("third_party.qmd");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    let root_uri = Uri::from_file_path(root).expect("root uri");
    server.initialize(root_uri.as_str());

    let long = "This is a very long paragraph that should definitely be wrapped at around 80 characters because that is the default line width for panache.";
    server.open_document(doc_uri.as_str(), long, "quarto");

    // Single-line selection covering the long paragraph. Range formatting
    // must still fire because the user explicitly asked for it.
    let edits = server.format_range(doc_uri.as_str(), 0, 0, 0, 0);
    assert!(
        edits.is_some(),
        "range formatting must run even when the file is under an excluded path"
    );
}

/// A file just outside the excluded path should still be formatted normally,
/// proving the exclude is doing real matching rather than blanket-skipping.
#[test]
fn lsp_format_document_formats_non_excluded_siblings() {
    let mut server = TestLspServer::new();
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    fs::create_dir_all(root.join(".git")).unwrap();
    fs::write(
        root.join("panache.toml"),
        "extend-exclude = [\"vendor/**\"]\n",
    )
    .unwrap();

    let doc_path = root.join("intro.qmd");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    let root_uri = Uri::from_file_path(root).expect("root uri");
    server.initialize(root_uri.as_str());

    let long = "This is a very long paragraph that should definitely be wrapped at around 80 characters because that is the default line width for panache.";
    server.open_document(doc_uri.as_str(), long, "quarto");

    let edits = server.format_document(doc_uri.as_str());
    assert!(
        edits.is_some(),
        "non-excluded sibling must still receive formatting edits"
    );
}

#[test]
fn test_format_simple_document() {
    let mut server = TestLspServer::new();

    // Open a document that needs formatting (long line)
    let content = "# Heading\n\nThis is a very long paragraph that should definitely be wrapped at around 80 characters because that is the default line width for panache.";
    server.open_document("file:///test.qmd", content, "quarto");

    // Request formatting
    let edits = server.format_document("file:///test.qmd");

    // Should return some edits
    assert!(edits.is_some());
    let edits = edits.unwrap();
    assert!(!edits.is_empty());

    // The edit should wrap the long line
    assert_eq!(edits.len(), 1);
    let edit = &edits[0];
    assert!(edit.new_text.contains("\n"));
}

#[test]
fn test_format_already_formatted() {
    let mut server = TestLspServer::new();

    // Open an already well-formatted document
    let content = "# Heading\n\nShort paragraph.\n";
    server.open_document("file:///test.qmd", content, "quarto");

    // Request formatting
    let edits = server.format_document("file:///test.qmd");

    // Should return None (no changes needed)
    assert_eq!(edits, None);
}

#[test]
fn test_format_after_edit() {
    let mut server = TestLspServer::new();

    // Open a formatted document
    server.open_document("file:///test.qmd", "# Heading\n\nShort.\n", "quarto");

    // Edit to make it need formatting
    server.edit_document(
        "file:///test.qmd",
        vec![full_document_change(
            "# Heading\n\nThis is now a very long line that needs wrapping.",
        )],
    );

    // Format should work
    let edits = server.format_document("file:///test.qmd");
    assert!(edits.is_some());
}

#[test]
fn test_format_document_with_umlauts_frontmatter() {
    let mut server = TestLspServer::new();
    let content = "---\nauthor: Test \ntitle: smörgås \n--- \n# introduction \n\nåäö\n";

    server.open_document("file:///umlauts.qmd", content, "quarto");

    let edits = server.format_document("file:///umlauts.qmd");
    if let Some(edits) = edits {
        assert_eq!(edits.len(), 1);
        let new_text = &edits[0].new_text;
        assert!(new_text.contains("smörgås"));
        assert!(new_text.contains("åäö"));
    }
}

#[test]
fn test_format_document_normalizes_yaml_frontmatter_with_builtin_engine() {
    let mut server = TestLspServer::new();
    let content = "---\necho:    false\nlist:\n  -  a\n  -     b\n---\n\n# intro\n";

    server.open_document("file:///frontmatter.qmd", content, "quarto");

    let edits = server.format_document("file:///frontmatter.qmd");
    assert!(edits.is_some());
    let edit = &edits.unwrap()[0];
    assert!(edit.new_text.contains("\necho: false\n"));
    assert!(edit.new_text.contains("\nlist:\n  - a\n  - b\n"));
}

#[test]
fn test_range_formatting_fenced_code_case_file() {
    let mut server = TestLspServer::new();

    let content = include_str!("../fixtures/cases/fenced_code/input.md");
    server.open_document("file:///fenced_code.md", content, "markdown");

    // Lines 44-48 in the fixture (0-indexed 43..48)
    let edits = server.format_range("file:///fenced_code.md", 43, 0, 48, 0);
    assert!(edits.is_some());
    let edit = &edits.unwrap()[0];
    assert!(edit.new_text.contains("```r"));
    assert!(edit.new_text.contains("a <- 1"));
    assert!(edit.new_text.contains("b <- 2"));
}

#[test]
fn test_range_formatting_executable_chunk_case_file() {
    let mut server = TestLspServer::new();

    let content = include_str!("../fixtures/cases/code_blocks_executable/input.qmd");
    server.open_document("file:///code_blocks_executable.qmd", content, "quarto");

    // Line 14 in the fixture (0-indexed line 13). Use a cursor-style range at C2.
    let edits = server.format_range("file:///code_blocks_executable.qmd", 13, 1, 13, 1);
    assert!(edits.is_some());
    let edit = &edits.unwrap()[0];
    assert_eq!(edit.new_text.matches("```{r}").count(), 1);
    assert!(edit.new_text.contains("#| echo: false"));
    assert!(edit.new_text.contains("#| fig-width: 8"));
    assert!(edit.new_text.contains("plot(1:10)"));
}

#[test]
fn test_range_formatting_definition_list_case_file() {
    let mut server = TestLspServer::new();

    let content = include_str!("fixtures/definition_list.qmd");
    server.open_document("file:///definition_list.qmd", content, "quarto");

    // Line 6 in the fixture (0-indexed line 5). Use full-line selection.
    let edits = server.format_range("file:///definition_list.qmd", 5, 0, 6, 0);
    assert!(edits.is_some());
    let edit = &edits.unwrap()[0];
    assert_eq!(edit.new_text.matches("Headings").count(), 1);
    assert!(
        edit.new_text
            .contains(":   H1-H6 with proper nesting levels")
    );
}

#[test]
fn test_range_formatting_definition_list_minimal_case() {
    let mut server = TestLspServer::new();

    let content = include_str!("fixtures/definition_list.qmd");
    server.open_document("file:///definition_list.qmd", content, "quarto");

    // Line 6 in the fixture (0-indexed line 5). Use full-line selection.
    let edits = server.format_range("file:///definition_list.qmd", 5, 0, 6, 0);
    assert!(edits.is_some());
    let edit = &edits.unwrap()[0];
    assert_eq!(edit.new_text.matches("Headings").count(), 1);
    assert!(
        edit.new_text
            .contains(":   H1-H6 with proper nesting levels")
    );
}

#[test]
fn test_range_formatting_definition_list_minimal_case_no_panic() {
    let mut server = TestLspServer::new();

    let content = include_str!("fixtures/definition_list.qmd");
    server.open_document("file:///definition_list.qmd", content, "quarto");

    // Match line 6 selection from editor, then request range formatting.
    let edits = server.format_range("file:///definition_list.qmd", 5, 0, 6, 0);
    assert!(edits.is_some());
}

#[test]
fn test_range_formatting_definition_list_nested_list_item() {
    let mut server = TestLspServer::new();

    let content = include_str!("fixtures/code_folding.qmd");
    server.open_document("file:///code_folding.qmd", content, "quarto");

    // Line 10 in the fixture (0-indexed line 9). Use full-line selection.
    let edits = server.format_range("file:///code_folding.qmd", 9, 0, 10, 0);
    assert!(edits.is_some());
    let edit = &edits.unwrap()[0];
    assert_eq!(edit.new_text.matches("Code folding").count(), 1);
    assert!(
        edit.new_text
            .contains(":   Fold sections of your document (`textDocument/foldingRange`)")
    );
    assert!(edit.new_text.contains("Headings"));
    assert!(edit.new_text.contains("Code"));
    assert!(edit.new_text.contains("blocks"));
    assert!(edit.new_text.contains("Fenced divs"));
    assert!(edit.new_text.contains("YAML frontmatter"));
}

#[test]
fn test_range_formatting_definition_list_term_line() {
    let mut server = TestLspServer::new();

    let content = include_str!("fixtures/code_folding.qmd");
    server.open_document("file:///code_folding.qmd", content, "quarto");

    // Line 6 in the fixture (0-indexed line 5). Use term-only selection.
    let edits = server.format_range("file:///code_folding.qmd", 5, 0, 6, 0);
    assert!(edits.is_some());
    let edit = &edits.unwrap()[0];
    assert_eq!(edit.new_text.matches("Code folding").count(), 1);
    assert!(
        edit.new_text
            .contains(":   Fold sections of your document (`textDocument/foldingRange`)")
    );
    assert!(edit.new_text.contains("Headings"));
    assert!(edit.new_text.contains("Code"));
    assert!(edit.new_text.contains("blocks"));
    assert!(edit.new_text.contains("Fenced divs"));
    assert!(edit.new_text.contains("YAML frontmatter"));
}
