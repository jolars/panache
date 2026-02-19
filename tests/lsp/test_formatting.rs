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
