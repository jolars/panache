use super::helpers::{TestLspServer, UriExt};
use lsp_types::{CompletionResponse, FileChangeType, FileEvent, Uri};
use std::fs;
use tempfile::TempDir;

#[test]
fn test_watched_file_updates_cached_text() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    let child_path = root.join("child.qmd");
    let parent_path = root.join("parent.qmd");

    fs::write(&child_path, "Old\n").unwrap();
    fs::write(&parent_path, "{{< include child.qmd >}}\n").unwrap();

    let mut server = TestLspServer::new();
    let root_uri = Uri::from_file_path(root).unwrap().to_string();
    server.initialize(&root_uri);
    server.open_document(
        &Uri::from_file_path(&parent_path).unwrap().to_string(),
        "{{< include child.qmd >}}\n",
        "quarto",
    );

    let cached = server.get_cached_file_text(&child_path);
    assert_eq!(cached, Some("Old\n".to_string()));

    fs::write(&child_path, "New\n").unwrap();
    server.did_change_watched_files(vec![FileEvent {
        uri: Uri::from_file_path(&child_path).unwrap(),
        typ: FileChangeType::CHANGED,
    }]);

    let cached = server.get_cached_file_text(&child_path);
    assert_eq!(cached, Some("New\n".to_string()));
}

#[test]
fn test_watcher_loads_newly_created_referenced_file() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    let child_path = root.join("child.qmd");
    let parent_path = root.join("parent.qmd");

    // Parent includes child, but child does not exist on disk yet.
    fs::write(&parent_path, "{{< include child.qmd >}}\nRef[^1].\n").unwrap();

    let mut server = TestLspServer::new();
    let root_uri = Uri::from_file_path(root).unwrap().to_string();
    let parent_uri = Uri::from_file_path(&parent_path).unwrap().to_string();
    server.initialize(&root_uri);
    server.open_document(
        &parent_uri,
        &fs::read_to_string(&parent_path).unwrap(),
        "quarto",
    );

    // Child missing -> not cached, footnote hover unresolved.
    assert_eq!(server.get_cached_file_text(&child_path), None);
    assert!(server.hover(&parent_uri, 1, 4).is_none());

    // Create the child and notify via the watcher. The writer must pull it in
    // (file_text no longer lazy-loads).
    fs::write(&child_path, "[^1]: Created footnote.\n").unwrap();
    server.did_change_watched_files(vec![FileEvent {
        uri: Uri::from_file_path(&child_path).unwrap(),
        typ: FileChangeType::CREATED,
    }]);

    assert!(
        server.get_cached_file_text(&child_path).is_some(),
        "watcher should load a newly-created referenced file"
    );
    assert!(
        server.hover(&parent_uri, 1, 4).is_some(),
        "hover should resolve once the referenced file is created"
    );
}

#[test]
fn test_bibliography_completion_updates_after_watcher_change() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    let bib_path = root.join("refs.bib");
    let doc_path = root.join("doc.qmd");

    fs::write(&bib_path, "@article{oldkey, title={Old}}\n").unwrap();
    fs::write(&doc_path, "---\nbibliography: refs.bib\n---\n\n@\n").unwrap();

    let mut server = TestLspServer::new();
    let root_uri = Uri::from_file_path(root).unwrap().to_string();
    let doc_uri = Uri::from_file_path(&doc_path).unwrap().to_string();
    server.initialize(&root_uri);
    server.open_document(&doc_uri, &fs::read_to_string(&doc_path).unwrap(), "quarto");

    let completion = server.completion(&doc_uri, 4, 1);
    let Some(CompletionResponse::Array(items)) = completion else {
        panic!("Expected completion items");
    };
    assert!(items.iter().any(|i| i.label == "oldkey"));

    fs::write(&bib_path, "@article{newkey, title={New}}\n").unwrap();
    server.did_change_watched_files(vec![FileEvent {
        uri: Uri::from_file_path(&bib_path).unwrap(),
        typ: FileChangeType::CHANGED,
    }]);

    let completion = server.completion(&doc_uri, 4, 1);
    let Some(CompletionResponse::Array(items)) = completion else {
        panic!("Expected completion items");
    };
    assert!(items.iter().any(|i| i.label == "newkey"));
    assert!(!items.iter().any(|i| i.label == "oldkey"));
}
