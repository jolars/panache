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

#[tokio::test]
async fn test_will_rename_files_updates_include_shortcode_path() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    fs::write(root.join("_quarto.yml"), "project: default\n").unwrap();
    fs::write(root.join("tables.qmd"), "# Tables\n").unwrap();
    let doc_path = root.join("doc.qmd");
    fs::write(&doc_path, "{{< include \"tables.qmd\" >}}\n").unwrap();

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
        .await
        .expect("expected workspace edit");
    let changes = edit.changes.expect("changes");
    let edits = changes.get(&doc_uri).expect("doc edits");
    assert!(
        edits.iter().any(|e| e.new_text == "tabular.qmd"),
        "expected include shortcode path rewrite"
    );
}

#[tokio::test]
async fn test_will_rename_files_ignores_escaped_shortcode() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    fs::write(root.join("_quarto.yml"), "project: default\n").unwrap();
    fs::write(root.join("tables.qmd"), "# Tables\n").unwrap();
    let doc_path = root.join("doc.qmd");
    fs::write(&doc_path, "{{{< include tables.qmd >}}}\n").unwrap();

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

    assert!(
        edit.is_none(),
        "escaped shortcode should not produce file rename edits"
    );
}

#[tokio::test]
async fn test_will_rename_files_updates_quarto_navbar_href() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    let old_page = root.join("index.qmd");
    let new_page = root.join("home.qmd");
    fs::write(&old_page, "# Home\n").unwrap();
    fs::write(
        root.join("_quarto.yml"),
        "website:\n  navbar:\n    left:\n      - text: Home\n        href: index.qmd\n",
    )
    .unwrap();

    let root_uri = Uri::from_file_path(root).unwrap();
    let quarto_uri = Uri::from_file_path(root.join("_quarto.yml")).unwrap();
    let server = TestLspServer::new();
    server.initialize(root_uri.as_str()).await;

    let edit = server
        .will_rename_files(vec![(
            Uri::from_file_path(&old_page).unwrap().as_str().to_string(),
            Uri::from_file_path(&new_page).unwrap().as_str().to_string(),
        )])
        .await
        .expect("expected workspace edit");
    let changes = edit.changes.expect("changes");
    let edits = changes.get(&quarto_uri).expect("quarto edits");
    assert!(
        edits.iter().any(|e| e.new_text == "home.qmd"),
        "expected navbar href rewrite in _quarto.yml"
    );
}

#[tokio::test]
async fn test_will_rename_files_updates_quarto_navbar_bare_entry() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    let old_page = root.join("talks.qmd");
    let new_page = root.join("presentations.qmd");
    fs::write(&old_page, "# Talks\n").unwrap();
    fs::write(
        root.join("_quarto.yml"),
        "website:\n  navbar:\n    left:\n      - talks.qmd\n      - about.qmd\n",
    )
    .unwrap();

    let root_uri = Uri::from_file_path(root).unwrap();
    let quarto_uri = Uri::from_file_path(root.join("_quarto.yml")).unwrap();
    let server = TestLspServer::new();
    server.initialize(root_uri.as_str()).await;

    let edit = server
        .will_rename_files(vec![(
            Uri::from_file_path(&old_page).unwrap().as_str().to_string(),
            Uri::from_file_path(&new_page).unwrap().as_str().to_string(),
        )])
        .await
        .expect("expected workspace edit");
    let changes = edit.changes.expect("changes");
    let edits = changes.get(&quarto_uri).expect("quarto edits");
    assert!(
        edits.iter().any(|e| e.new_text == "presentations.qmd"),
        "expected bare navbar entry rewrite in _quarto.yml"
    );
}

#[tokio::test]
async fn test_will_rename_files_ignores_non_navbar_yaml_paths() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    let old_page = root.join("index.qmd");
    let new_page = root.join("home.qmd");
    fs::write(&old_page, "# Home\n").unwrap();
    fs::write(
        root.join("_quarto.yml"),
        "website:\n  sidebar:\n    contents:\n      - index.qmd\n",
    )
    .unwrap();

    let root_uri = Uri::from_file_path(root).unwrap();
    let server = TestLspServer::new();
    server.initialize(root_uri.as_str()).await;

    let edit = server
        .will_rename_files(vec![(
            Uri::from_file_path(&old_page).unwrap().as_str().to_string(),
            Uri::from_file_path(&new_page).unwrap().as_str().to_string(),
        )])
        .await;
    assert!(
        edit.is_none(),
        "non-navbar YAML paths should not be rewritten in this slice"
    );
}

#[tokio::test]
async fn test_will_rename_files_updates_quarto_book_chapter_entry() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    let old_page = root.join("intro.qmd");
    let new_page = root.join("getting-started.qmd");
    fs::write(&old_page, "# Intro\n").unwrap();
    fs::write(
        root.join("_quarto.yml"),
        "book:\n  chapters:\n    - index.qmd\n    - intro.qmd\n",
    )
    .unwrap();

    let root_uri = Uri::from_file_path(root).unwrap();
    let quarto_uri = Uri::from_file_path(root.join("_quarto.yml")).unwrap();
    let server = TestLspServer::new();
    server.initialize(root_uri.as_str()).await;

    let edit = server
        .will_rename_files(vec![(
            Uri::from_file_path(&old_page).unwrap().as_str().to_string(),
            Uri::from_file_path(&new_page).unwrap().as_str().to_string(),
        )])
        .await
        .expect("expected workspace edit");
    let changes = edit.changes.expect("changes");
    let edits = changes.get(&quarto_uri).expect("quarto edits");
    assert!(
        edits.iter().any(|e| e.new_text == "getting-started.qmd"),
        "expected book chapter entry rewrite"
    );
}

#[tokio::test]
async fn test_will_rename_files_updates_quarto_book_part_file() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    let old_page = root.join("dice.qmd");
    let new_page = root.join("dice-part.qmd");
    fs::write(&old_page, "# Part\n").unwrap();
    fs::write(
        root.join("_quarto.yml"),
        "book:\n  chapters:\n    - part: dice.qmd\n      chapters:\n        - basics.qmd\n",
    )
    .unwrap();

    let root_uri = Uri::from_file_path(root).unwrap();
    let quarto_uri = Uri::from_file_path(root.join("_quarto.yml")).unwrap();
    let server = TestLspServer::new();
    server.initialize(root_uri.as_str()).await;

    let edit = server
        .will_rename_files(vec![(
            Uri::from_file_path(&old_page).unwrap().as_str().to_string(),
            Uri::from_file_path(&new_page).unwrap().as_str().to_string(),
        )])
        .await
        .expect("expected workspace edit");
    let changes = edit.changes.expect("changes");
    let edits = changes.get(&quarto_uri).expect("quarto edits");
    assert!(
        edits.iter().any(|e| e.new_text == "dice-part.qmd"),
        "expected book part file rewrite"
    );
}

#[tokio::test]
async fn test_will_rename_files_ignores_quarto_book_part_title() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    let old_page = root.join("dice.qmd");
    let new_page = root.join("dice-part.qmd");
    fs::write(&old_page, "# Part\n").unwrap();
    fs::write(
        root.join("_quarto.yml"),
        "book:\n  chapters:\n    - part: \"Dice\"\n      chapters:\n        - basics.qmd\n",
    )
    .unwrap();

    let root_uri = Uri::from_file_path(root).unwrap();
    let server = TestLspServer::new();
    server.initialize(root_uri.as_str()).await;

    let edit = server
        .will_rename_files(vec![(
            Uri::from_file_path(&old_page).unwrap().as_str().to_string(),
            Uri::from_file_path(&new_page).unwrap().as_str().to_string(),
        )])
        .await;
    assert!(
        edit.is_none(),
        "book part title should not be treated as file path"
    );
}

#[tokio::test]
async fn test_will_rename_files_updates_quarto_book_appendix_entry() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    let old_page = root.join("tools.qmd");
    let new_page = root.join("tooling.qmd");
    fs::write(&old_page, "# Tools\n").unwrap();
    fs::write(
        root.join("_quarto.yml"),
        "book:\n  chapters:\n    - index.qmd\n  appendices:\n    - tools.qmd\n",
    )
    .unwrap();

    let root_uri = Uri::from_file_path(root).unwrap();
    let quarto_uri = Uri::from_file_path(root.join("_quarto.yml")).unwrap();
    let server = TestLspServer::new();
    server.initialize(root_uri.as_str()).await;

    let edit = server
        .will_rename_files(vec![(
            Uri::from_file_path(&old_page).unwrap().as_str().to_string(),
            Uri::from_file_path(&new_page).unwrap().as_str().to_string(),
        )])
        .await
        .expect("expected workspace edit");
    let changes = edit.changes.expect("changes");
    let edits = changes.get(&quarto_uri).expect("quarto edits");
    assert!(
        edits.iter().any(|e| e.new_text == "tooling.qmd"),
        "expected book appendices entry rewrite"
    );
}

#[tokio::test]
async fn test_will_rename_files_updates_quarto_bibliography_scalar() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    let old_file = root.join("references.bib");
    let new_file = root.join("refs.bib");
    fs::write(&old_file, "@book{x,}\n").unwrap();
    fs::write(root.join("_quarto.yml"), "bibliography: references.bib\n").unwrap();

    let root_uri = Uri::from_file_path(root).unwrap();
    let quarto_uri = Uri::from_file_path(root.join("_quarto.yml")).unwrap();
    let server = TestLspServer::new();
    server.initialize(root_uri.as_str()).await;

    let edit = server
        .will_rename_files(vec![(
            Uri::from_file_path(&old_file).unwrap().as_str().to_string(),
            Uri::from_file_path(&new_file).unwrap().as_str().to_string(),
        )])
        .await
        .expect("expected workspace edit");
    let changes = edit.changes.expect("changes");
    let edits = changes.get(&quarto_uri).expect("quarto edits");
    assert!(
        edits.iter().any(|e| e.new_text == "refs.bib"),
        "expected bibliography scalar rewrite"
    );
}

#[tokio::test]
async fn test_will_rename_files_updates_quarto_bibliography_list_entry() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    let old_file = root.join("references.bib");
    let new_file = root.join("refs.bib");
    fs::write(&old_file, "@book{x,}\n").unwrap();
    fs::write(
        root.join("_quarto.yml"),
        "bibliography:\n  - references.bib\n  - extra.bib\n",
    )
    .unwrap();

    let root_uri = Uri::from_file_path(root).unwrap();
    let quarto_uri = Uri::from_file_path(root.join("_quarto.yml")).unwrap();
    let server = TestLspServer::new();
    server.initialize(root_uri.as_str()).await;

    let edit = server
        .will_rename_files(vec![(
            Uri::from_file_path(&old_file).unwrap().as_str().to_string(),
            Uri::from_file_path(&new_file).unwrap().as_str().to_string(),
        )])
        .await
        .expect("expected workspace edit");
    let changes = edit.changes.expect("changes");
    let edits = changes.get(&quarto_uri).expect("quarto edits");
    assert!(
        edits.iter().any(|e| e.new_text == "refs.bib"),
        "expected bibliography list entry rewrite"
    );
}
