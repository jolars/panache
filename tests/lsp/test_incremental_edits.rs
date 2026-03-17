//! Tests for incremental document synchronization.

use super::helpers::*;
use tower_lsp_server::ls_types::{DocumentSymbolResponse, GotoDefinitionResponse, Uri};

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

    let tree_after = server
        .get_document_tree("file:///test.qmd")
        .await
        .expect("tree after edit");
    let expected = panache::parse("# Title\n\nNew text.", None);
    assert_eq!(tree_after.text_range(), expected.text_range());
    assert_eq!(tree_after.to_string(), expected.to_string());
}

#[tokio::test]
async fn test_incremental_edit_updates_dependents() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root = temp_dir.path();
    std::fs::write(root.join("_quarto.yml"), "project: default\n").unwrap();

    let doc1_path = root.join("doc1.qmd");
    let doc2_path = root.join("doc2.qmd");
    std::fs::write(&doc1_path, "See [link][ref].\n").unwrap();
    std::fs::write(&doc2_path, "[ref]: https://example.com\n").unwrap();

    let doc1_uri = Uri::from_file_path(&doc1_path).unwrap();
    let doc2_uri = Uri::from_file_path(&doc2_path).unwrap();

    let server = TestLspServer::new();
    server
        .initialize(Uri::from_file_path(root).unwrap().as_str())
        .await;
    server
        .open_document(
            doc1_uri.as_str(),
            &std::fs::read_to_string(&doc1_path).unwrap(),
            "quarto",
        )
        .await;
    server
        .open_document(
            doc2_uri.as_str(),
            &std::fs::read_to_string(&doc2_path).unwrap(),
            "quarto",
        )
        .await;

    server
        .edit_document(
            doc2_uri.as_str(),
            vec![full_document_change("[ref]: https://example.org\n")],
        )
        .await;

    let result = server.goto_definition(doc1_uri.as_str(), 0, 12).await;
    let Some(GotoDefinitionResponse::Scalar(location)) = result else {
        panic!("Expected scalar location response");
    };
    assert_eq!(location.uri, doc2_uri);
}

#[tokio::test]
async fn test_incremental_edit_multiple_changes_single_notification() {
    let server = TestLspServer::new();

    // Open a document
    server
        .open_document("file:///test.qmd", "Line 1\nLine 2\nLine 3", "quarto")
        .await;
    // Apply multiple changes in a single notification
    server
        .edit_document(
            "file:///test.qmd",
            vec![
                incremental_change(0, 0, 0, 0, "Inserted line 1\nInserted line 2\n"),
                incremental_change(4, 0, 4, 6, "Line 3 updated"),
            ],
        )
        .await;

    let content = server.get_document_content("file:///test.qmd").await;
    assert_eq!(
        content,
        Some("Inserted line 1\nInserted line 2\nLine 1\nLine 2\nLine 3 updated".to_string())
    );

    let tree_after = server
        .get_document_tree("file:///test.qmd")
        .await
        .expect("tree after edit");
    let expected = panache::parse(
        "Inserted line 1\nInserted line 2\nLine 1\nLine 2\nLine 3 updated",
        None,
    );
    assert_eq!(tree_after.text_range(), expected.text_range());
    assert_eq!(tree_after.to_string(), expected.to_string());
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

#[tokio::test]
async fn test_incremental_edit_preserves_yaml_frontmatter_document_symbol() {
    let server = TestLspServer::new();
    let content = "---\ntitle: Old\n---\n\n# Heading\n";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let before = server
        .get_symbols("file:///test.qmd")
        .await
        .expect("symbols before edit");
    let DocumentSymbolResponse::Nested(before_symbols) = before else {
        panic!("Expected nested symbols");
    };
    assert!(
        before_symbols
            .iter()
            .any(|symbol| symbol.name == "YAML Frontmatter"),
        "Expected YAML frontmatter symbol before edit"
    );

    server
        .edit_document(
            "file:///test.qmd",
            vec![incremental_change(1, 7, 1, 10, "Updated Title")],
        )
        .await;

    let after = server
        .get_symbols("file:///test.qmd")
        .await
        .expect("symbols after edit");
    let DocumentSymbolResponse::Nested(after_symbols) = after else {
        panic!("Expected nested symbols");
    };
    let yaml_symbol = after_symbols
        .iter()
        .find(|symbol| symbol.name == "YAML Frontmatter")
        .expect("yaml symbol after edit");
    assert_eq!(yaml_symbol.range.start.line, 0);
    assert!(
        yaml_symbol.range.end.line >= 2,
        "frontmatter symbol should still span the YAML block"
    );
}
