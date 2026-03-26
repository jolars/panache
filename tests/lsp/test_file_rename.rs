use super::helpers::*;
use std::fs;
use tempfile::TempDir;
use tower_lsp_server::ls_types::Uri;

#[tokio::test]
async fn test_initialize_advertises_will_rename_file_operations() {
    let temp_dir = TempDir::new().unwrap();
    let root_uri = Uri::from_file_path(temp_dir.path()).unwrap();

    let server = TestLspServer::new();
    let init = server.initialize_result(root_uri.as_str()).await;

    let workspace_caps = init.capabilities.workspace.expect("workspace capabilities");
    let file_ops = workspace_caps
        .file_operations
        .expect("file operations capability");
    let will_rename = file_ops.will_rename.expect("will_rename filters");

    assert!(
        !will_rename.filters.is_empty(),
        "expected file-operation filters for document extensions"
    );
    assert!(
        will_rename
            .filters
            .iter()
            .any(|f| f.pattern.glob == "**/*.qmd"),
        "expected qmd filter in willRenameFiles registration"
    );
}

#[tokio::test]
async fn test_will_rename_files_returns_edit_for_markdown_links() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    fs::write(root.join("_quarto.yml"), "project: default\n").unwrap();
    fs::write(root.join("tables.qmd"), "# Tables\n").unwrap();
    let doc_path = root.join("doc.qmd");
    fs::write(
        &doc_path,
        "See [table](tables.qmd)\n\n![plot](tables.qmd#fig)\n",
    )
    .unwrap();

    let root_uri = Uri::from_file_path(root).unwrap();
    let doc_uri = Uri::from_file_path(&doc_path).unwrap();

    let server = TestLspServer::new();
    server.initialize(root_uri.as_str()).await;
    server
        .open_document(
            doc_uri.as_str(),
            &fs::read_to_string(&doc_path).unwrap(),
            "quarto",
        )
        .await;

    let old_uri = Uri::from_file_path(root.join("tables.qmd")).unwrap();
    let new_uri = Uri::from_file_path(root.join("tabular.qmd")).unwrap();
    let edit = server
        .will_rename_files(vec![(
            old_uri.as_str().to_string(),
            new_uri.as_str().to_string(),
        )])
        .await;

    let edit = edit.expect("expected workspace edit");
    let changes = edit.changes.expect("changes");
    let edits = changes.get(&doc_uri).expect("doc edits");
    assert!(
        edits.iter().any(|e| e.new_text == "tabular.qmd"),
        "expected inline link destination rewrite"
    );
    assert!(
        edits.iter().any(|e| e.new_text == "tabular.qmd#fig"),
        "expected image destination rewrite preserving fragment"
    );
}

#[tokio::test]
async fn test_will_rename_files_scans_standalone_workspace_documents() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    let docs_dir = root.join("docs");
    fs::create_dir_all(&docs_dir).unwrap();
    let image_path = docs_dir.join("tables.qmd");
    fs::write(&image_path, "# target\n").unwrap();

    let a_path = docs_dir.join("a.md");
    let b_path = docs_dir.join("b.md");
    fs::write(&a_path, "See [A](tables.qmd)\n").unwrap();
    fs::write(&b_path, "See ![B](tables.qmd)\n").unwrap();

    let root_uri = Uri::from_file_path(root).unwrap();
    let a_uri = Uri::from_file_path(&a_path).unwrap();
    let b_uri = Uri::from_file_path(&b_path).unwrap();

    let server = TestLspServer::new();
    server.initialize(root_uri.as_str()).await;

    // Open only one doc to verify fallback scanning also captures closed docs.
    server
        .open_document(
            a_uri.as_str(),
            &fs::read_to_string(&a_path).unwrap(),
            "markdown",
        )
        .await;

    let old_uri = Uri::from_file_path(&image_path).unwrap();
    let new_uri = Uri::from_file_path(docs_dir.join("tabular.qmd")).unwrap();
    let edit = server
        .will_rename_files(vec![(
            old_uri.as_str().to_string(),
            new_uri.as_str().to_string(),
        )])
        .await
        .expect("expected workspace edit");

    let changes = edit.changes.expect("changes");
    assert!(
        changes.contains_key(&a_uri),
        "open doc should be included in standalone scan"
    );
    assert!(
        changes.contains_key(&b_uri),
        "closed standalone doc should be discovered from workspace root"
    );
}
