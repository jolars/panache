//! Tests for completion (citation completion).

use super::helpers::*;

#[tokio::test]
async fn test_completion_without_citation_context() {
    let server = TestLspServer::new();

    // Open a document without citation context
    let content = "Just plain text.";
    server
        .open_document("file:///test.md", content, "markdown")
        .await;

    // Request completion in plain text
    let result = server.completion("file:///test.md", 0, 5).await;

    // Should return None when not in citation context
    assert!(
        result.is_none(),
        "Should not provide completions outside citation context"
    );
}

#[tokio::test]
async fn test_completion_in_citation_without_bibliography() {
    let server = TestLspServer::new();

    // Open a document with citation syntax but no bibliography configured
    let content = "Text with [@] citation.";
    server
        .open_document("file:///test.md", content, "markdown")
        .await;

    // Request completion at @ position
    let result = server
        .completion(
            "file:///test.md",
            0,
            12, // Position after [@
        )
        .await;

    // Should return None when no bibliography is configured
    assert!(
        result.is_none(),
        "Should not provide completions without bibliography"
    );
}

// Note: Testing actual citation completion would require:
// 1. Creating a temporary bibliography file
// 2. Configuring the workspace root
// 3. Setting up document metadata
// This is better suited for integration tests with a full workspace setup.
