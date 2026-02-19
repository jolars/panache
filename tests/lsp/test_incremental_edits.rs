//! Tests for incremental document synchronization.

use super::helpers::*;

#[tokio::test]
async fn test_incremental_edit_simple() {
    let server = TestLspServer::new();

    // Open a document
    server
        .open_document("file:///test.qmd", "# Title\n\nOld text.", "quarto")
        .await;

    // Make an incremental edit (replace "Old" with "New")
    server
        .edit_document(
            "file:///test.qmd",
            vec![incremental_change(2, 0, 2, 3, "New")],
        )
        .await;

    // Verify the edit was applied
    let content = server.get_document_content("file:///test.qmd").await;
    assert_eq!(content, Some("# Title\n\nNew text.".to_string()));
}

#[tokio::test]
async fn test_incremental_edit_multiline() {
    let server = TestLspServer::new();

    // Open a document
    server
        .open_document(
            "file:///test.qmd",
            "Line 1\nLine 2\nLine 3\nLine 4",
            "quarto",
        )
        .await;

    // Delete lines 2-3
    server
        .edit_document("file:///test.qmd", vec![incremental_change(1, 0, 3, 0, "")])
        .await;

    // Verify the edit
    let content = server.get_document_content("file:///test.qmd").await;
    assert_eq!(content, Some("Line 1\nLine 4".to_string()));
}

#[tokio::test]
async fn test_multiple_documents() {
    let server = TestLspServer::new();

    // Open two documents
    server
        .open_document("file:///doc1.qmd", "# Doc 1", "quarto")
        .await;
    server
        .open_document("file:///doc2.qmd", "# Doc 2", "quarto")
        .await;

    // Edit both
    server
        .edit_document(
            "file:///doc1.qmd",
            vec![full_document_change("# Modified 1")],
        )
        .await;
    server
        .edit_document(
            "file:///doc2.qmd",
            vec![full_document_change("# Modified 2")],
        )
        .await;

    // Verify both were updated independently
    assert_eq!(
        server.get_document_content("file:///doc1.qmd").await,
        Some("# Modified 1".to_string())
    );
    assert_eq!(
        server.get_document_content("file:///doc2.qmd").await,
        Some("# Modified 2".to_string())
    );
}
