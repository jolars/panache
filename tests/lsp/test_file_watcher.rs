use super::helpers::TestLspServer;
use std::fs;
use tempfile::TempDir;
use tower_lsp_server::ls_types::{CompletionResponse, FileChangeType, FileEvent, Uri};

#[tokio::test]
async fn test_watched_file_updates_cached_text() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    let child_path = root.join("child.qmd");
    let parent_path = root.join("parent.qmd");

    fs::write(&child_path, "Old\n").unwrap();
    fs::write(&parent_path, "{{< include child.qmd >}}\n").unwrap();

    let server = TestLspServer::new();
    let root_uri = Uri::from_file_path(root).unwrap().to_string();
    server.initialize(&root_uri).await;
    server
        .open_document(
            &Uri::from_file_path(&parent_path).unwrap().to_string(),
            "{{< include child.qmd >}}\n",
            "quarto",
        )
        .await;

    let cached = server.get_cached_file_text(&child_path).await;
    assert_eq!(cached, Some("Old\n".to_string()));

    fs::write(&child_path, "New\n").unwrap();
    server
        .did_change_watched_files(vec![FileEvent {
            uri: Uri::from_file_path(&child_path).unwrap(),
            typ: FileChangeType::CHANGED,
        }])
        .await;

    let cached = server.get_cached_file_text(&child_path).await;
    assert_eq!(cached, Some("New\n".to_string()));
}

#[tokio::test]
async fn test_bibliography_completion_updates_after_watcher_change() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    let bib_path = root.join("refs.bib");
    let doc_path = root.join("doc.qmd");

    fs::write(&bib_path, "@article{oldkey, title={Old}}\n").unwrap();
    fs::write(&doc_path, "---\nbibliography: refs.bib\n---\n\n@\n").unwrap();

    let server = TestLspServer::new();
    let root_uri = Uri::from_file_path(root).unwrap().to_string();
    let doc_uri = Uri::from_file_path(&doc_path).unwrap().to_string();
    server.initialize(&root_uri).await;
    server
        .open_document(&doc_uri, &fs::read_to_string(&doc_path).unwrap(), "quarto")
        .await;

    let completion = server.completion(&doc_uri, 4, 1).await;
    let Some(CompletionResponse::Array(items)) = completion else {
        panic!("Expected completion items");
    };
    assert!(items.iter().any(|i| i.label == "oldkey"));

    fs::write(&bib_path, "@article{newkey, title={New}}\n").unwrap();
    server
        .did_change_watched_files(vec![FileEvent {
            uri: Uri::from_file_path(&bib_path).unwrap(),
            typ: FileChangeType::CHANGED,
        }])
        .await;

    let completion = server.completion(&doc_uri, 4, 1).await;
    let Some(CompletionResponse::Array(items)) = completion else {
        panic!("Expected completion items");
    };
    assert!(items.iter().any(|i| i.label == "newkey"));
    assert!(!items.iter().any(|i| i.label == "oldkey"));
}
