use super::helpers::{TestLspServer, UriExt};
use lsp_types::{CompletionResponse, FileChangeType, FileEvent, NumberOrString, Uri};
use std::fs;
use std::time::Duration;
use tempfile::TempDir;

/// Whether any publish for `uri` in `publishes` carries a `yaml-parse-error`.
fn has_yaml_parse_error(publishes: &[lsp_types::PublishDiagnosticsParams], uri: &Uri) -> bool {
    has_code(publishes, uri, "yaml-parse-error")
}

/// Whether any publish for `uri` in `publishes` carries a diagnostic with `code`.
fn has_code(publishes: &[lsp_types::PublishDiagnosticsParams], uri: &Uri, code: &str) -> bool {
    publishes
        .iter()
        .filter(|p| &p.uri == uri)
        .flat_map(|p| &p.diagnostics)
        .any(|d| matches!(d.code.as_ref(), Some(NumberOrString::String(s)) if s == code))
}

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

#[test]
fn test_broken_quarto_yml_publishes_on_manifest_uri_not_document() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    let doc_path = root.join("doc.qmd");
    let quarto_path = root.join("_quarto.yml");
    // Malformed project manifest; the document's own frontmatter is valid.
    fs::write(&quarto_path, "title: [\n").unwrap();
    fs::write(&doc_path, "---\ntitle: Doc\n---\n\n# Heading\n").unwrap();

    let mut server = TestLspServer::new();
    let root_uri = Uri::from_file_path(root).unwrap().to_string();
    let doc_uri = Uri::from_file_path(&doc_path).unwrap();
    let quarto_uri = Uri::from_file_path(&quarto_path).unwrap();
    server.initialize(&root_uri);
    server.open_document(
        &doc_uri.to_string(),
        &fs::read_to_string(&doc_path).unwrap(),
        "quarto",
    );
    server.pump(Duration::from_secs(2));

    let publishes = server.drain_all_publish_diagnostics();
    assert!(
        has_yaml_parse_error(&publishes, &quarto_uri),
        "expected yaml-parse-error on _quarto.yml; publishes: {publishes:?}"
    );
    assert!(
        !has_yaml_parse_error(&publishes, &doc_uri),
        "broken _quarto.yml must NOT surface a yaml-parse-error on the document"
    );
}

#[test]
fn test_manifest_diagnostic_clears_when_quarto_yml_fixed() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    let doc_path = root.join("doc.qmd");
    let quarto_path = root.join("_quarto.yml");
    fs::write(&quarto_path, "title: [\n").unwrap();
    fs::write(&doc_path, "# Heading\n").unwrap();

    let mut server = TestLspServer::new();
    let root_uri = Uri::from_file_path(root).unwrap().to_string();
    let doc_uri = Uri::from_file_path(&doc_path).unwrap().to_string();
    let quarto_uri = Uri::from_file_path(&quarto_path).unwrap();
    server.initialize(&root_uri);
    server.open_document(&doc_uri, "# Heading\n", "quarto");
    server.pump(Duration::from_secs(2));

    let initial = server.drain_all_publish_diagnostics();
    assert!(
        has_yaml_parse_error(&initial, &quarto_uri),
        "expected initial manifest error on _quarto.yml; got {initial:?}"
    );

    // Fix the manifest on disk and notify via the watcher.
    fs::write(&quarto_path, "title: ok\n").unwrap();
    server.did_change_watched_files(vec![FileEvent {
        uri: quarto_uri.clone(),
        typ: FileChangeType::CHANGED,
    }]);
    server.pump(Duration::from_secs(2));

    let after = server.drain_publish_diagnostics(&quarto_uri.to_string());
    let last = after
        .last()
        .expect("expected a clearing publish on _quarto.yml after the fix");
    assert!(
        last.diagnostics.is_empty(),
        "manifest diagnostic should be cleared after the fix, got {:?}",
        last.diagnostics
    );
}

#[test]
fn test_quarto_yml_schema_diagnostic_published_and_clears() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    let doc_path = root.join("doc.qmd");
    let quarto_path = root.join("_quarto.yml");
    // Valid YAML, but a type mismatch the Quarto schema can decide (`render` is
    // an array). Type/enum checks are on by default; unknown-key is opt-in.
    fs::write(&quarto_path, "project:\n  render: true\n").unwrap();
    fs::write(&doc_path, "# Heading\n").unwrap();

    let mut server = TestLspServer::new();
    let root_uri = Uri::from_file_path(root).unwrap().to_string();
    let doc_uri = Uri::from_file_path(&doc_path).unwrap();
    let quarto_uri = Uri::from_file_path(&quarto_path).unwrap();
    server.initialize(&root_uri);
    server.open_document(&doc_uri.to_string(), "# Heading\n", "quarto");
    server.pump(Duration::from_secs(2));

    let initial = server.drain_all_publish_diagnostics();
    assert!(
        has_code(&initial, &quarto_uri, "quarto-schema-type-mismatch"),
        "expected a quarto-schema diagnostic on _quarto.yml; got {initial:?}"
    );
    assert!(
        !has_code(&initial, &doc_uri, "quarto-schema-type-mismatch"),
        "manifest schema diagnostic must NOT land on the document"
    );

    // Fix the manifest on disk and notify via the watcher.
    fs::write(&quarto_path, "project:\n  render: [index.qmd]\n").unwrap();
    server.did_change_watched_files(vec![FileEvent {
        uri: quarto_uri.clone(),
        typ: FileChangeType::CHANGED,
    }]);
    server.pump(Duration::from_secs(2));

    let after = server.drain_publish_diagnostics(&quarto_uri.to_string());
    let last = after
        .last()
        .expect("expected a clearing publish on _quarto.yml after the fix");
    assert!(
        last.diagnostics.is_empty(),
        "schema diagnostic should clear after the fix, got {:?}",
        last.diagnostics
    );
}

#[test]
fn test_shared_manifest_diagnostic_clears_only_when_all_docs_closed() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    let doc_a = root.join("a.qmd");
    let doc_b = root.join("b.qmd");
    let quarto_path = root.join("_quarto.yml");
    fs::write(&quarto_path, "title: [\n").unwrap();
    fs::write(&doc_a, "# A\n").unwrap();
    fs::write(&doc_b, "# B\n").unwrap();

    let mut server = TestLspServer::new();
    let root_uri = Uri::from_file_path(root).unwrap().to_string();
    let a_uri = Uri::from_file_path(&doc_a).unwrap().to_string();
    let b_uri = Uri::from_file_path(&doc_b).unwrap().to_string();
    let quarto_uri = Uri::from_file_path(&quarto_path).unwrap();
    server.initialize(&root_uri);
    server.open_document(&a_uri, "# A\n", "quarto");
    server.open_document(&b_uri, "# B\n", "quarto");
    server.pump(Duration::from_secs(2));
    server.drain_client_messages();

    // Close A: the manifest is still referenced by B, so it must NOT be cleared.
    server.close_document(&a_uri);
    server.pump(Duration::from_secs(2));
    let after_a = server.drain_publish_diagnostics(&quarto_uri.to_string());
    assert!(
        !after_a.iter().any(|p| p.diagnostics.is_empty()),
        "manifest diagnostic must not be cleared while another open doc still references it; got {after_a:?}"
    );

    // Close B: now no open document references the manifest, so it clears.
    server.close_document(&b_uri);
    server.pump(Duration::from_secs(2));
    let after_b = server.drain_publish_diagnostics(&quarto_uri.to_string());
    assert!(
        after_b
            .last()
            .map(|p| p.diagnostics.is_empty())
            .unwrap_or(false),
        "manifest diagnostic should clear once the last referencing doc closes; got {after_b:?}"
    );
}
