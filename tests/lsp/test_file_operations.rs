//! Tests for the `did*` workspace file-operation notifications
//! (`didCreateFiles`/`didRenameFiles`/`didDeleteFiles`).
//!
//! These are hygiene-only: they re-intern affected paths so `project_graph`
//! re-runs and re-lint surfaces/clears cross-document diagnostics. The matching
//! `will*` requests are intentionally unregistered (no scaffolding on create, no
//! destructive auto-edit on delete).

use super::helpers::*;
use lsp_types::*;
use std::fs;
use std::time::Duration;
use tempfile::TempDir;

/// Pull the items out of a full `textDocument/diagnostic` report.
fn full_items(result: &DocumentDiagnosticReportResult) -> &[Diagnostic] {
    match result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) => {
            &full.full_document_diagnostic_report.items
        }
        other => panic!("expected a full document report, got: {other:?}"),
    }
}

fn has_include_not_found(diags: &[Diagnostic]) -> bool {
    diags.iter().any(|d| {
        matches!(
            d.code.as_ref(),
            Some(NumberOrString::String(s)) if s == "include-not-found"
        )
    })
}

#[test]
fn test_initialize_advertises_did_file_operations() {
    let temp_dir = TempDir::new().unwrap();
    let root_uri = Uri::from_file_path(temp_dir.path()).unwrap();

    let mut server = TestLspServer::new();
    let init = server.initialize_result(root_uri.as_str());

    let file_ops = init
        .capabilities
        .workspace
        .expect("workspace capabilities")
        .file_operations
        .expect("file operations capability");

    for (name, filters) in [
        ("did_rename", &file_ops.did_rename),
        ("did_create", &file_ops.did_create),
        ("did_delete", &file_ops.did_delete),
    ] {
        let registration = filters
            .as_ref()
            .unwrap_or_else(|| panic!("expected {name} registration"));
        assert!(
            registration
                .filters
                .iter()
                .any(|f| f.pattern.glob == "**/*.qmd"),
            "expected qmd filter in {name} registration"
        );
    }

    // The `will*` create/delete requests stay unregistered (hygiene-only scope).
    assert!(
        file_ops.will_create.is_none(),
        "willCreate must not be registered"
    );
    assert!(
        file_ops.will_delete.is_none(),
        "willDelete must not be registered"
    );
}

#[test]
fn test_did_create_files_clears_include_not_found() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    let child_path = root.join("child.qmd");
    let parent_path = root.join("parent.qmd");
    // Parent includes a child that does not yet exist on disk.
    fs::write(&parent_path, "{{< include child.qmd >}}\n").unwrap();

    let mut server = TestLspServer::new();
    let root_uri = Uri::from_file_path(root).unwrap().to_string();
    let parent_uri = Uri::from_file_path(&parent_path).unwrap().to_string();
    let child_uri = Uri::from_file_path(&child_path).unwrap().to_string();

    server.initialize_pull(&root_uri);
    server.open_document(
        &parent_uri,
        &fs::read_to_string(&parent_path).unwrap(),
        "quarto",
    );
    server.pump(Duration::from_secs(2));

    assert!(
        has_include_not_found(full_items(&server.document_diagnostic(&parent_uri, None))),
        "missing include target should report include-not-found"
    );

    // Create the child and notify via didCreateFiles; interning re-runs the
    // project graph so the include now resolves.
    fs::write(&child_path, "# Child\n").unwrap();
    server.did_create_files(vec![&child_uri]);
    server.pump(Duration::from_secs(2));

    assert!(
        !has_include_not_found(full_items(&server.document_diagnostic(&parent_uri, None))),
        "include-not-found should clear once the target is created"
    );
}

#[test]
fn test_did_delete_files_surfaces_broken_include() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    let child_path = root.join("child.qmd");
    let parent_path = root.join("parent.qmd");
    fs::write(&child_path, "# Child\n").unwrap();
    fs::write(&parent_path, "{{< include child.qmd >}}\n").unwrap();

    let mut server = TestLspServer::new();
    let root_uri = Uri::from_file_path(root).unwrap().to_string();
    let parent_uri = Uri::from_file_path(&parent_path).unwrap().to_string();
    let child_uri = Uri::from_file_path(&child_path).unwrap().to_string();

    server.initialize_pull(&root_uri);
    server.open_document(
        &parent_uri,
        &fs::read_to_string(&parent_path).unwrap(),
        "quarto",
    );
    server.pump(Duration::from_secs(2));

    assert!(
        !has_include_not_found(full_items(&server.document_diagnostic(&parent_uri, None))),
        "an existing include target should not report include-not-found"
    );

    fs::remove_file(&child_path).unwrap();
    server.did_delete_files(vec![&child_uri]);
    server.pump(Duration::from_secs(2));

    assert!(
        has_include_not_found(full_items(&server.document_diagnostic(&parent_uri, None))),
        "deleting the include target should surface include-not-found"
    );
}

#[test]
fn test_did_delete_files_clears_deleted_documents_diagnostics() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    let doc_path = root.join("doc.qmd");
    // A heading-hierarchy violation gives the document its own diagnostic.
    fs::write(&doc_path, "# H1\n\n### H3 skip\n").unwrap();

    let mut server = TestLspServer::new();
    let root_uri = Uri::from_file_path(root).unwrap().to_string();
    let doc_uri = Uri::from_file_path(&doc_path).unwrap().to_string();

    // Push mode: the deleted file's diagnostics are cleared with an empty publish.
    server.initialize(&root_uri);
    server.open_document(&doc_uri, &fs::read_to_string(&doc_path).unwrap(), "quarto");
    server.pump(Duration::from_secs(2));

    let published = server.drain_publish_diagnostics(&doc_uri);
    assert!(
        published.iter().any(|p| !p.diagnostics.is_empty()),
        "document should have published its own diagnostics"
    );

    fs::remove_file(&doc_path).unwrap();
    server.did_delete_files(vec![&doc_uri]);

    let after = server.drain_publish_diagnostics(&doc_uri);
    let last = after
        .last()
        .expect("deleting the file should publish a clearing notification");
    assert!(
        last.diagnostics.is_empty(),
        "the deleted file's diagnostics should be cleared"
    );
}

#[test]
fn test_did_rename_files_breaks_stale_include_reference() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    let child_path = root.join("child.qmd");
    let renamed_path = root.join("kid.qmd");
    let parent_path = root.join("parent.qmd");
    fs::write(&child_path, "# Child\n").unwrap();
    // Parent still references the old name after the rename.
    fs::write(&parent_path, "{{< include child.qmd >}}\n").unwrap();

    let mut server = TestLspServer::new();
    let root_uri = Uri::from_file_path(root).unwrap().to_string();
    let parent_uri = Uri::from_file_path(&parent_path).unwrap().to_string();
    let old_uri = Uri::from_file_path(&child_path).unwrap().to_string();
    let new_uri = Uri::from_file_path(&renamed_path).unwrap().to_string();

    server.initialize_pull(&root_uri);
    server.open_document(
        &parent_uri,
        &fs::read_to_string(&parent_path).unwrap(),
        "quarto",
    );
    server.pump(Duration::from_secs(2));

    assert!(
        !has_include_not_found(full_items(&server.document_diagnostic(&parent_uri, None))),
        "include resolves before the rename"
    );

    fs::rename(&child_path, &renamed_path).unwrap();
    server.did_rename_files(vec![(old_uri, new_uri)]);
    server.pump(Duration::from_secs(2));

    assert!(
        has_include_not_found(full_items(&server.document_diagnostic(&parent_uri, None))),
        "renaming the target should break the stale include reference"
    );
}
