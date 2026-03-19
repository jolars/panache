use super::helpers::*;
use tower_lsp_server::ls_types::PrepareRenameResponse;

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

    let PrepareRenameResponse::Range(range) = response else {
        panic!("expected prepare rename range");
    };

    assert_eq!(range.start.line, 4);
    assert_eq!(range.start.character, 15);
    assert_eq!(range.end.line, 4);
    assert_eq!(range.end.character, 24);
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

    let PrepareRenameResponse::Range(range) = response else {
        panic!("expected prepare rename range");
    };

    assert_eq!(range.start.line, 2);
    assert_eq!(range.start.character, 13);
    assert_eq!(range.end.line, 2);
    assert_eq!(range.end.character, 20);
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

    let PrepareRenameResponse::Range(range) = response else {
        panic!("expected prepare rename range");
    };

    assert_eq!(range.start.line, 1);
    assert_eq!(range.start.character, 10);
    assert_eq!(range.end.line, 1);
    assert_eq!(range.end.character, 18);
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

    let PrepareRenameResponse::Range(range) = response else {
        panic!("expected prepare rename range");
    };

    assert_eq!(range.start.line, 0);
    assert_eq!(range.start.character, 6);
    assert_eq!(range.end.line, 0);
    assert_eq!(range.end.character, 14);
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

    let PrepareRenameResponse::Range(range) = response else {
        panic!("expected prepare rename range");
    };

    assert_eq!(range.start.line, 0);
    assert_eq!(range.start.character, 16);
    assert_eq!(range.end.line, 0);
    assert_eq!(range.end.character, 24);
}
