//! Tests for basic document lifecycle (open, edit, close).

use super::helpers::*;

#[tokio::test]
async fn test_open_document() {
    let server = TestLspServer::new();

    // Open a simple document
    server
        .open_document("file:///test.qmd", "# Hello\n\nWorld", "quarto")
        .await;

    // Verify document is in state
    let content = server.get_document_content("file:///test.qmd").await;
    assert_eq!(content, Some("# Hello\n\nWorld".to_string()));
}

#[tokio::test]
async fn test_close_document() {
    let server = TestLspServer::new();

    // Open then close
    server
        .open_document("file:///test.qmd", "# Hello", "quarto")
        .await;
    server.close_document("file:///test.qmd").await;

    // Verify document is removed from state
    let content = server.get_document_content("file:///test.qmd").await;
    assert_eq!(content, None);
}

#[tokio::test]
async fn test_edit_document_full_replace() {
    let server = TestLspServer::new();

    // Open document
    server
        .open_document("file:///test.qmd", "# Original", "quarto")
        .await;

    // Edit with full replacement
    server
        .edit_document(
            "file:///test.qmd",
            vec![full_document_change("# Modified\n\nNew content")],
        )
        .await;

    // Verify content changed
    let content = server.get_document_content("file:///test.qmd").await;
    assert_eq!(content, Some("# Modified\n\nNew content".to_string()));
}
