//! Tests for formatting workflows.

use super::helpers::*;

#[tokio::test]
async fn test_format_simple_document() {
    let server = TestLspServer::new();

    // Open a document that needs formatting (long line)
    let content = "# Heading\n\nThis is a very long paragraph that should definitely be wrapped at around 80 characters because that is the default line width for panache.";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    // Request formatting
    let edits = server.format_document("file:///test.qmd").await;

    // Should return some edits
    assert!(edits.is_some());
    let edits = edits.unwrap();
    assert!(!edits.is_empty());

    // The edit should wrap the long line
    assert_eq!(edits.len(), 1);
    let edit = &edits[0];
    assert!(edit.new_text.contains("\n"));
}

#[tokio::test]
async fn test_format_already_formatted() {
    let server = TestLspServer::new();

    // Open an already well-formatted document
    let content = "# Heading\n\nShort paragraph.\n";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    // Request formatting
    let edits = server.format_document("file:///test.qmd").await;

    // Should return None (no changes needed)
    assert_eq!(edits, None);
}

#[tokio::test]
async fn test_format_after_edit() {
    let server = TestLspServer::new();

    // Open a formatted document
    server
        .open_document("file:///test.qmd", "# Heading\n\nShort.\n", "quarto")
        .await;

    // Edit to make it need formatting
    server
        .edit_document(
            "file:///test.qmd",
            vec![full_document_change(
                "# Heading\n\nThis is now a very long line that needs wrapping.",
            )],
        )
        .await;

    // Format should work
    let edits = server.format_document("file:///test.qmd").await;
    assert!(edits.is_some());
}

#[tokio::test]
async fn test_format_document_with_umlauts_frontmatter() {
    let server = TestLspServer::new();
    let content = "---\nauthor: Test \ntitle: smörgås \n--- \n# introduction \n\nåäö\n";

    server
        .open_document("file:///umlauts.qmd", content, "quarto")
        .await;

    let edits = server.format_document("file:///umlauts.qmd").await;
    if let Some(edits) = edits {
        assert_eq!(edits.len(), 1);
        let new_text = &edits[0].new_text;
        assert!(new_text.contains("smörgås"));
        assert!(new_text.contains("åäö"));
    }
}

#[tokio::test]
async fn test_range_formatting_fenced_code_case_file() {
    let server = TestLspServer::new();

    let content = include_str!("../cases/fenced_code/input.md");
    server
        .open_document("file:///fenced_code.md", content, "markdown")
        .await;

    // Lines 44-48 in the fixture (0-indexed 43..48)
    let edits = server
        .format_range("file:///fenced_code.md", 43, 0, 48, 0)
        .await;
    assert!(edits.is_some());
    let edit = &edits.unwrap()[0];
    assert!(edit.new_text.contains("```r"));
    assert!(edit.new_text.contains("a <- 1"));
    assert!(edit.new_text.contains("b <- 2"));
}

#[tokio::test]
async fn test_range_formatting_executable_chunk_case_file() {
    let server = TestLspServer::new();

    let content = include_str!("../cases/code_blocks_executable/input.qmd");
    server
        .open_document("file:///code_blocks_executable.qmd", content, "quarto")
        .await;

    // Line 14 in the fixture (0-indexed line 13). Use a cursor-style range at C2.
    let edits = server
        .format_range("file:///code_blocks_executable.qmd", 13, 1, 13, 1)
        .await;
    assert!(edits.is_some());
    let edit = &edits.unwrap()[0];
    assert_eq!(edit.new_text.matches("```{r}").count(), 1);
    assert!(edit.new_text.contains("#| echo: false"));
    assert!(edit.new_text.contains("#| fig-width: 8"));
    assert!(edit.new_text.contains("plot(1:10)"));
}

#[tokio::test]
async fn test_range_formatting_definition_list_case_file() {
    let server = TestLspServer::new();

    let content = include_str!("fixtures/definition_list.qmd");
    server
        .open_document("file:///definition_list.qmd", content, "quarto")
        .await;

    // Line 6 in the fixture (0-indexed line 5). Use full-line selection.
    let edits = server
        .format_range("file:///definition_list.qmd", 5, 0, 6, 0)
        .await;
    assert!(edits.is_some());
    let edit = &edits.unwrap()[0];
    assert_eq!(edit.new_text.matches("Headings").count(), 1);
    assert!(
        edit.new_text
            .contains(":   H1-H6 with proper nesting levels")
    );
}

#[tokio::test]
async fn test_range_formatting_definition_list_minimal_case() {
    let server = TestLspServer::new();

    let content = include_str!("fixtures/definition_list.qmd");
    server
        .open_document("file:///definition_list.qmd", content, "quarto")
        .await;

    // Line 6 in the fixture (0-indexed line 5). Use full-line selection.
    let edits = server
        .format_range("file:///definition_list.qmd", 5, 0, 6, 0)
        .await;
    assert!(edits.is_some());
    let edit = &edits.unwrap()[0];
    assert_eq!(edit.new_text.matches("Headings").count(), 1);
    assert!(
        edit.new_text
            .contains(":   H1-H6 with proper nesting levels")
    );
}

#[tokio::test]
async fn test_range_formatting_definition_list_minimal_case_no_panic() {
    let server = TestLspServer::new();

    let content = include_str!("fixtures/definition_list.qmd");
    server
        .open_document("file:///definition_list.qmd", content, "quarto")
        .await;

    // Match line 6 selection from editor, then request range formatting.
    let edits = server
        .format_range("file:///definition_list.qmd", 5, 0, 6, 0)
        .await;
    assert!(edits.is_some());
}

#[tokio::test]
async fn test_range_formatting_definition_list_nested_list_item() {
    let server = TestLspServer::new();

    let content = include_str!("fixtures/code_folding.qmd");
    server
        .open_document("file:///code_folding.qmd", content, "quarto")
        .await;

    // Line 10 in the fixture (0-indexed line 9). Use full-line selection.
    let edits = server
        .format_range("file:///code_folding.qmd", 9, 0, 10, 0)
        .await;
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

#[tokio::test]
async fn test_range_formatting_definition_list_term_line() {
    let server = TestLspServer::new();

    let content = include_str!("fixtures/code_folding.qmd");
    server
        .open_document("file:///code_folding.qmd", content, "quarto")
        .await;

    // Line 6 in the fixture (0-indexed line 5). Use term-only selection.
    let edits = server
        .format_range("file:///code_folding.qmd", 5, 0, 6, 0)
        .await;
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
