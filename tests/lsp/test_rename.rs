use super::helpers::*;
use std::fs;
use tower_lsp_server::ls_types::Uri;

#[tokio::test]
async fn test_rename_citation_updates_bib_and_dependents() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root = temp_dir.path();
    fs::write(root.join("_quarto.yml"), "project: default\n").unwrap();

    let bib_path = root.join("refs.bib");
    fs::write(&bib_path, "@article{oldkey,\n  title = {Old}\n}\n").unwrap();

    let doc1_path = root.join("doc1.qmd");
    let doc2_path = root.join("doc2.qmd");
    fs::write(
        &doc1_path,
        "---\nbibliography: refs.bib\n---\nSee [@oldkey].\n",
    )
    .unwrap();
    fs::write(
        &doc2_path,
        "---\nbibliography: refs.bib\n---\nAlso [@oldkey].\n",
    )
    .unwrap();

    let doc1_uri = Uri::from_file_path(&doc1_path).unwrap();
    let doc2_uri = Uri::from_file_path(&doc2_path).unwrap();
    let bib_uri = Uri::from_file_path(&bib_path).unwrap();
    let root_uri = Uri::from_file_path(root).unwrap();

    let server = TestLspServer::new();
    server.initialize(root_uri.as_str()).await;
    server
        .open_document(
            doc1_uri.as_str(),
            &fs::read_to_string(&doc1_path).unwrap(),
            "quarto",
        )
        .await;
    server
        .open_document(
            doc2_uri.as_str(),
            &fs::read_to_string(&doc2_path).unwrap(),
            "quarto",
        )
        .await;

    let edit = server
        .rename(doc1_uri.as_str(), 3, 7, "newkey")
        .await
        .expect("rename edit");
    let changes = edit.changes.expect("changes");

    assert!(changes.contains_key(&doc1_uri));
    assert!(changes.contains_key(&doc2_uri));
    assert!(changes.contains_key(&bib_uri));
}

#[tokio::test]
async fn test_rename_citation_updates_inline_references() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root = temp_dir.path();
    std::fs::write(root.join("_quarto.yml"), "project: default\n").unwrap();

    let doc_path = root.join("doc.qmd");
    std::fs::write(
        &doc_path,
        "---\nreferences:\n  - id: oldkey\n    title: Inline\n---\n\nSee [@oldkey].\n",
    )
    .unwrap();

    let doc_uri = Uri::from_file_path(&doc_path).unwrap();
    let root_uri = Uri::from_file_path(root).unwrap();

    let server = TestLspServer::new();
    server.initialize(root_uri.as_str()).await;
    server
        .open_document(
            doc_uri.as_str(),
            &std::fs::read_to_string(&doc_path).unwrap(),
            "quarto",
        )
        .await;

    let edit = server
        .rename(doc_uri.as_str(), 6, 7, "newkey")
        .await
        .expect("rename edit");
    let changes = edit.changes.expect("changes");

    assert!(changes.contains_key(&doc_uri));
}

#[tokio::test]
async fn test_rename_citation_updates_csl_yaml() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root = temp_dir.path();
    std::fs::write(root.join("_quarto.yml"), "project: default\n").unwrap();

    let bib_path = root.join("refs.yaml");
    std::fs::write(&bib_path, "- id: oldkey\n  title: Sample\n").unwrap();

    let doc_path = root.join("doc.qmd");
    std::fs::write(
        &doc_path,
        "---\nbibliography: refs.yaml\n---\n\nSee [@oldkey].\n",
    )
    .unwrap();

    let doc_uri = Uri::from_file_path(&doc_path).unwrap();
    let bib_uri = Uri::from_file_path(&bib_path).unwrap();
    let root_uri = Uri::from_file_path(root).unwrap();

    let server = TestLspServer::new();
    server.initialize(root_uri.as_str()).await;
    server
        .open_document(
            doc_uri.as_str(),
            &std::fs::read_to_string(&doc_path).unwrap(),
            "quarto",
        )
        .await;

    let edit = server
        .rename(doc_uri.as_str(), 4, 7, "newkey")
        .await
        .expect("rename edit");
    let changes = edit.changes.expect("changes");

    assert!(changes.contains_key(&doc_uri));
    assert!(changes.contains_key(&bib_uri));
}

#[tokio::test]
async fn test_rename_citation_updates_csl_json() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root = temp_dir.path();
    std::fs::write(root.join("_quarto.yml"), "project: default\n").unwrap();

    let bib_path = root.join("refs.json");
    std::fs::write(&bib_path, "[{\"id\":\"oldkey\",\"title\":\"Sample\"}]").unwrap();

    let doc_path = root.join("doc.qmd");
    std::fs::write(
        &doc_path,
        "---\nbibliography: refs.json\n---\n\nSee [@oldkey].\n",
    )
    .unwrap();

    let doc_uri = Uri::from_file_path(&doc_path).unwrap();
    let bib_uri = Uri::from_file_path(&bib_path).unwrap();
    let root_uri = Uri::from_file_path(root).unwrap();

    let server = TestLspServer::new();
    server.initialize(root_uri.as_str()).await;
    server
        .open_document(
            doc_uri.as_str(),
            &std::fs::read_to_string(&doc_path).unwrap(),
            "quarto",
        )
        .await;

    let edit = server
        .rename(doc_uri.as_str(), 4, 7, "newkey")
        .await
        .expect("rename edit");
    let changes = edit.changes.expect("changes");

    assert!(changes.contains_key(&doc_uri));
    assert!(changes.contains_key(&bib_uri));
}
