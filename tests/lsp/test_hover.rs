//! Tests for hover (footnote and citation previews).

use super::helpers::*;
use tower_lsp_server::ls_types::*;

#[tokio::test]
async fn test_hover_on_included_footnote() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let child_path = temp_dir.path().join("_child.qmd");
    let parent_path = temp_dir.path().join("parent.qmd");

    std::fs::write(&child_path, "[^1]: Included footnote content.\n").unwrap();
    std::fs::write(&parent_path, "{{< include _child.qmd >}}\nRef[^1].\n").unwrap();

    let server = TestLspServer::new();
    let root_uri = Uri::from_file_path(temp_dir.path()).expect("root uri");
    let parent_uri = Uri::from_file_path(&parent_path).expect("parent uri");
    server.initialize(root_uri.as_str()).await;
    server
        .open_document(
            parent_uri.as_str(),
            &std::fs::read_to_string(&parent_path).unwrap(),
            "quarto",
        )
        .await;

    let hover = server.hover(parent_uri.as_str(), 1, 4).await;

    let Some(h) = hover else {
        panic!("Expected hover content");
    };
    if let HoverContents::Markup(markup) = h.contents {
        assert!(markup.value.contains("Included footnote content"));
    } else {
        panic!("Expected markup hover content");
    }
}

#[tokio::test]
async fn test_hover_on_footnote_reference() {
    let server = TestLspServer::new();

    // Open a document with footnote
    let content = r#"Text with footnote[^1] here.

[^1]: This is the footnote content with details.
"#;
    server
        .open_document("file:///test.md", content, "markdown")
        .await;

    // Request hover on the footnote reference [^1]
    let hover = server
        .hover(
            "file:///test.md",
            0,  // Line with footnote[^1]
            20, // Character position inside [^1]
        )
        .await;

    assert!(hover.is_some(), "Should have hover info for footnote");

    if let Some(h) = hover {
        // Check that it contains the footnote content
        if let HoverContents::Markup(markup) = h.contents {
            assert_eq!(markup.kind, MarkupKind::Markdown);
            assert!(
                markup.value.contains("footnote content"),
                "Should show footnote content"
            );
        } else {
            panic!("Expected markup hover content");
        }
    }
}

#[tokio::test]
async fn test_hover_on_plain_text() {
    let server = TestLspServer::new();

    // Open a document without any special elements
    let content = "Just plain text without footnotes.";
    server
        .open_document("file:///test.md", content, "markdown")
        .await;

    // Request hover in plain text
    let hover = server.hover("file:///test.md", 0, 10).await;

    assert!(hover.is_none(), "Should not have hover for plain text");
}

#[tokio::test]
async fn test_hover_on_footnote_with_formatting() {
    let server = TestLspServer::new();

    // Open a document with formatted footnote
    let content = r#"Reference[^note] in text.

[^note]: Footnote with *emphasis* and `code`.
"#;
    server
        .open_document("file:///test.md", content, "markdown")
        .await;

    // Request hover on footnote
    let hover = server
        .hover(
            "file:///test.md",
            0,  // Line with [^note]
            10, // Inside [^note]
        )
        .await;

    assert!(hover.is_some(), "Should have hover for formatted footnote");

    if let Some(h) = hover
        && let HoverContents::Markup(markup) = h.contents
    {
        let content = markup.value;
        assert!(content.contains("*emphasis*"));
        assert!(content.contains("`code`"));
    }
}

#[tokio::test]
async fn test_hover_on_undefined_footnote() {
    let server = TestLspServer::new();

    // Open a document with footnote reference but no definition
    let content = "Text with undefined[^missing] footnote.";
    server
        .open_document("file:///test.md", content, "markdown")
        .await;

    // Request hover on undefined footnote
    let hover = server
        .hover(
            "file:///test.md",
            0,
            25, // Inside [^missing]
        )
        .await;

    // Should return None when footnote definition doesn't exist
    assert!(
        hover.is_none(),
        "Should not have hover for undefined footnote"
    );
}
