use super::helpers::*;
use tower_lsp_server::ls_types::PrepareRenameResponse;
use tower_lsp_server::ls_types::Uri;

#[tokio::test]
async fn test_prepare_rename_bookdown_hyphenated_crossref_selects_full_key() {
    let server = TestLspServer::new();
    let content = "# Heading\n\n## Heading 2\n\nA ref to \\@ref(heading-2).\n";
    server
        .open_document("file:///test.Rmd", content, "rmarkdown")
        .await;

    let response = server
        .prepare_rename("file:///test.Rmd", 4, 20)
        .await
        .expect("prepare rename response");

    let PrepareRenameResponse::RangeWithPlaceholder { range, placeholder } = response else {
        panic!("expected prepare rename range");
    };

    assert_eq!(range.start.line, 4);
    assert_eq!(range.start.character, 15);
    assert_eq!(range.end.line, 4);
    assert_eq!(range.end.character, 24);
    assert_eq!(placeholder, "heading-2");
}

#[tokio::test]
async fn test_prepare_rename_heading_hash_link_selects_anchor_without_hash() {
    let server = TestLspServer::new();
    let content = "# Heading {#heading}\n\nSee [label](#heading).\n";
    server
        .open_document("file:///test.md", content, "markdown")
        .await;

    let response = server
        .prepare_rename("file:///test.md", 2, 15)
        .await
        .expect("prepare rename response");

    let PrepareRenameResponse::RangeWithPlaceholder { range, placeholder } = response else {
        panic!("expected prepare rename range");
    };

    assert_eq!(range.start.line, 2);
    assert_eq!(range.start.character, 13);
    assert_eq!(range.end.line, 2);
    assert_eq!(range.end.character, 20);
    assert_eq!(placeholder, "heading");
}

#[tokio::test]
async fn test_prepare_rename_inline_chunk_label_selects_full_hyphenated_label() {
    let server = TestLspServer::new();
    let content = "```{r}\n#| label: my-label\n1 + 1\n```\n";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let response = server
        .prepare_rename("file:///test.qmd", 1, 12)
        .await
        .expect("prepare rename response");

    let PrepareRenameResponse::RangeWithPlaceholder { range, placeholder } = response else {
        panic!("expected prepare rename range");
    };

    assert_eq!(range.start.line, 1);
    assert_eq!(range.start.character, 10);
    assert_eq!(range.end.line, 1);
    assert_eq!(range.end.character, 18);
    assert_eq!(placeholder, "my-label");
}

#[tokio::test]
async fn test_prepare_rename_executable_chunk_label_selects_full_hyphenated_label() {
    let server = TestLspServer::new();
    let content = "```{r my-label}\nplot(1, 1)\n```\n";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let response = server
        .prepare_rename("file:///test.qmd", 0, 7)
        .await
        .expect("prepare rename response");

    let PrepareRenameResponse::RangeWithPlaceholder { range, placeholder } = response else {
        panic!("expected prepare rename range");
    };

    assert_eq!(range.start.line, 0);
    assert_eq!(range.start.character, 6);
    assert_eq!(range.end.line, 0);
    assert_eq!(range.end.character, 14);
    assert_eq!(placeholder, "my-label");
}

#[tokio::test]
async fn test_prepare_rename_executable_chunk_option_label_selects_full_hyphenated_label() {
    let server = TestLspServer::new();
    let content = "```{r, label = \"my-label\"}\nplot(1, 1)\n```\n";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let response = server
        .prepare_rename("file:///test.qmd", 0, 17)
        .await
        .expect("prepare rename response");

    let PrepareRenameResponse::RangeWithPlaceholder { range, placeholder } = response else {
        panic!("expected prepare rename range");
    };

    assert_eq!(range.start.line, 0);
    assert_eq!(range.start.character, 16);
    assert_eq!(range.end.line, 0);
    assert_eq!(range.end.character, 24);
    assert_eq!(placeholder, "my-label");
}

#[tokio::test]
async fn test_prepare_rename_image_reference_selects_reference_label() {
    let server = TestLspServer::new();
    let content = "![Alt text][img]\n\n[img]: image.png\n";
    server
        .open_document("file:///test.md", content, "markdown")
        .await;

    let response = server
        .prepare_rename("file:///test.md", 0, 12)
        .await
        .expect("prepare rename response");

    let PrepareRenameResponse::RangeWithPlaceholder { range, placeholder } = response else {
        panic!("expected prepare rename range");
    };

    assert_eq!(range.start.line, 0);
    assert_eq!(range.start.character, 12);
    assert_eq!(range.end.line, 0);
    assert_eq!(range.end.character, 15);
    assert_eq!(placeholder, "img");
}

#[tokio::test]
async fn test_prepare_rename_numbered_example_label_selects_label_only() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root = temp_dir.path();
    let doc_path = root.join("doc.qmd");
    std::fs::write(root.join("panache.toml"), "flavor = \"pandoc\"\n").unwrap();
    let server = TestLspServer::new();
    let content = "(@good) This is a good example.\n\nAs (@good) illustrates.\n";
    std::fs::write(&doc_path, content).unwrap();
    let root_uri = Uri::from_file_path(root).expect("root uri");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    server.initialize(root_uri.as_str()).await;
    server
        .open_document(doc_uri.as_str(), content, "quarto")
        .await;

    let response = server
        .prepare_rename(doc_uri.as_str(), 2, 7)
        .await
        .expect("prepare rename response");
    let PrepareRenameResponse::RangeWithPlaceholder { range, placeholder } = response else {
        panic!("expected prepare rename range");
    };
    assert_eq!(range.start.line, 2);
    assert_eq!(range.start.character, 5);
    assert_eq!(range.end.line, 2);
    assert_eq!(range.end.character, 9);
    assert_eq!(placeholder, "good");
}
