//! Tests for the LSP pull diagnostics model (`textDocument/diagnostic` +
//! `workspace/diagnostic`), and the push/pull mode-switch.

use super::helpers::*;
use lsp_types::*;
use std::time::Duration;

/// Collect the `(uri, items)` pairs from full reports in a workspace result.
fn workspace_full_reports(
    result: &WorkspaceDiagnosticReportResult,
) -> Vec<(String, &[Diagnostic])> {
    let WorkspaceDiagnosticReportResult::Report(report) = result else {
        panic!("expected a full workspace report");
    };
    report
        .items
        .iter()
        .filter_map(|item| match item {
            WorkspaceDocumentDiagnosticReport::Full(full) => Some((
                full.uri.to_string(),
                full.full_document_diagnostic_report.items.as_slice(),
            )),
            _ => None,
        })
        .collect()
}

/// Pull the items out of a `textDocument/diagnostic` full report (panics on an
/// unchanged/partial result — the caller asserts the shape it expects).
fn full_items(result: &DocumentDiagnosticReportResult) -> &[Diagnostic] {
    match result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) => {
            &full.full_document_diagnostic_report.items
        }
        other => panic!("expected a full document report, got: {other:?}"),
    }
}

fn has_heading_hierarchy(diags: &[Diagnostic]) -> bool {
    diags.iter().any(|d| {
        matches!(
            d.code.as_ref(),
            Some(NumberOrString::String(s)) if s == "heading-hierarchy"
        )
    })
}

/// In pull mode the server must not push `publishDiagnostics`; the same
/// diagnostics are served on demand instead.
#[test]
fn pull_mode_suppresses_push_and_serves_on_demand() {
    let mut server = TestLspServer::new();
    server.initialize_pull("file:///workspace");
    assert!(server.pull_diagnostics_enabled());
    let uri = "file:///workspace/doc.qmd";

    server.open_document(uri, "# H1\n\n### H3 skip\n", "quarto");
    server.pump(Duration::from_secs(2));

    // No push notifications were emitted.
    assert!(
        server.drain_all_publish_diagnostics().is_empty(),
        "pull-capable client must not receive pushed diagnostics"
    );

    // The diagnostic is available via pull.
    let report = server.document_diagnostic(uri, None);
    assert!(
        has_heading_hierarchy(full_items(&report)),
        "expected heading-hierarchy via textDocument/diagnostic, got: {:?}",
        full_items(&report)
    );
}

/// A push-only client (default capabilities) still gets pushed diagnostics and
/// the pull store stays empty.
#[test]
fn push_mode_unchanged_for_non_pull_client() {
    let mut server = TestLspServer::new();
    server.initialize("file:///workspace");
    assert!(!server.pull_diagnostics_enabled());
    let uri = "file:///workspace/doc.qmd";

    server.open_document(uri, "# H1\n\n### H3 skip\n", "quarto");
    server.pump(Duration::from_secs(2));

    let publishes = server.drain_publish_diagnostics(uri);
    assert!(
        publishes
            .iter()
            .any(|p| has_heading_hierarchy(&p.diagnostics)),
        "push client should still receive heading-hierarchy via publishDiagnostics"
    );

    // The pull store is untouched in push mode: a pull yields an empty report.
    let report = server.document_diagnostic(uri, None);
    assert!(
        full_items(&report).is_empty(),
        "push mode must not populate the pull store"
    );
}

/// Re-pulling with the previously returned `result_id` yields an `unchanged`
/// report.
#[test]
fn unchanged_report_when_result_id_matches() {
    let mut server = TestLspServer::new();
    server.initialize_pull("file:///workspace");
    let uri = "file:///workspace/doc.qmd";

    server.open_document(uri, "# H1\n\n### H3 skip\n", "quarto");
    server.pump(Duration::from_secs(2));

    let first = server.document_diagnostic(uri, None);
    let result_id = match &first {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) => full
            .full_document_diagnostic_report
            .result_id
            .clone()
            .expect("full report should carry a result_id"),
        other => panic!("expected a full report, got: {other:?}"),
    };

    let again = server.document_diagnostic(uri, Some(&result_id));
    assert!(
        matches!(
            again,
            DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Unchanged(_))
        ),
        "re-pull with the same result_id should be unchanged, got: {again:?}"
    );
}

/// An async lint pass in pull mode nudges the client to re-pull via
/// `workspace/diagnostic/refresh`.
#[test]
fn lint_pass_sends_refresh_in_pull_mode() {
    let mut server = TestLspServer::new();
    server.initialize_pull("file:///workspace");
    let uri = "file:///workspace/doc.qmd";

    server.open_document(uri, "# H1\n\n### H3 skip\n", "quarto");
    server.pump(Duration::from_secs(2));

    assert!(
        server.drain_diagnostic_refresh() >= 1,
        "expected at least one workspace/diagnostic/refresh after a lint pass"
    );
}

/// `workspace/diagnostic` reports every stored document.
#[test]
fn workspace_pull_reports_all_open_documents() {
    let mut server = TestLspServer::new();
    server.initialize_pull("file:///workspace");
    let clean = "file:///workspace/clean.qmd";
    let dirty = "file:///workspace/dirty.qmd";

    // Pump after each open so the lint settles before the next document's salsa
    // write could cancel the in-flight pass.
    server.open_document(clean, "# H1\n\n## H2\n", "quarto");
    server.pump(Duration::from_secs(2));
    server.open_document(dirty, "# H1\n\n### H3 skip\n", "quarto");
    server.pump(Duration::from_secs(2));

    let WorkspaceDiagnosticReportResult::Report(report) = server.workspace_diagnostic(vec![])
    else {
        panic!("expected a full workspace report");
    };

    let mut saw_clean = false;
    let mut saw_dirty_violation = false;
    for item in &report.items {
        if let WorkspaceDocumentDiagnosticReport::Full(full) = item {
            let uri = full.uri.to_string();
            let items = &full.full_document_diagnostic_report.items;
            if uri == clean {
                saw_clean = true;
                assert!(items.is_empty(), "clean doc should have no diagnostics");
            } else if uri == dirty {
                saw_dirty_violation = has_heading_hierarchy(items);
            }
        }
    }
    assert!(saw_clean, "workspace report should include the clean doc");
    assert!(
        saw_dirty_violation,
        "workspace report should include the dirty doc's heading-hierarchy diagnostic"
    );
}

/// Closing a document drops it from the pull store so it stops appearing in
/// workspace pulls.
#[test]
fn closing_a_document_drops_it_from_the_store() {
    let mut server = TestLspServer::new();
    server.initialize_pull("file:///workspace");
    let uri = "file:///workspace/doc.qmd";

    server.open_document(uri, "# H1\n\n### H3 skip\n", "quarto");
    server.pump(Duration::from_secs(2));
    assert!(
        !full_items(&server.document_diagnostic(uri, None)).is_empty(),
        "doc should have diagnostics before close"
    );

    server.close_document(uri);

    let WorkspaceDiagnosticReportResult::Report(report) = server.workspace_diagnostic(vec![])
    else {
        panic!("expected a full workspace report");
    };
    assert!(
        !report.items.iter().any(|item| matches!(item,
                WorkspaceDocumentDiagnosticReport::Full(f) if f.uri.to_string() == uri)),
        "closed document must not appear in the workspace report"
    );
}

/// Cross-file diagnostics on an unopened project manifest (`_quarto.yml`) reach
/// the client through the workspace pull, keyed on the manifest's own URI — the
/// rust-analyzer `Cargo.toml` model carried over to pull.
#[test]
fn workspace_pull_includes_manifest_diagnostics() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root = temp_dir.path();
    let manifest_path = root.join("_quarto.yml");
    let doc_path = root.join("doc.qmd");

    // Malformed YAML (unterminated flow sequence) → a manifest parse error.
    std::fs::write(&manifest_path, "project:\n  type: [book\n").unwrap();
    std::fs::write(&doc_path, "# Title\n").unwrap();

    let mut server = TestLspServer::new();
    let root_uri = Uri::from_file_path(root).unwrap().to_string();
    server.initialize_pull(&root_uri);

    let doc_uri = Uri::from_file_path(&doc_path).unwrap().to_string();
    server.open_document(
        &doc_uri,
        &std::fs::read_to_string(&doc_path).unwrap(),
        "quarto",
    );
    server.save_document(&doc_uri);
    server.pump(Duration::from_secs(2));

    let manifest_uri = Uri::from_file_path(&manifest_path).unwrap().to_string();
    let reports = server.workspace_diagnostic(vec![]);
    let manifest = workspace_full_reports(&reports)
        .into_iter()
        .find(|(uri, _)| *uri == manifest_uri);
    let (_, items) = manifest.expect("workspace pull should include the unopened manifest");
    assert!(
        !items.is_empty(),
        "manifest parse error should surface via workspace pull"
    );
}

fn has_duplicate_label(diags: &[Diagnostic]) -> bool {
    diags.iter().any(|d| {
        matches!(
            d.code.as_ref(),
            Some(NumberOrString::String(s)) if s == "duplicate-reference-labels"
        )
    })
}

/// The full `related`-capable report (panics on an unchanged/partial result).
fn full_report(result: &DocumentDiagnosticReportResult) -> &RelatedFullDocumentDiagnosticReport {
    match result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) => full,
        other => panic!("expected a full document report, got: {other:?}"),
    }
}

/// A two-document Quarto project where the parent `a.qmd` includes `b.qmd` and
/// both define the same reference label. Because an included document's
/// definitions are collected before the includer's own, the cross-doc duplicate
/// is always attributed to the includer `a.qmd`, leaving `b.qmd`'s report clean.
/// The `_quarto.yml` root makes the project graph symmetric, so `b.qmd`'s
/// closure reaches the dirty `a.qmd`. Both documents are opened; returns
/// `(dirty_uri, clean_uri)` = `(a_uri, b_uri)`.
fn open_cross_file_duplicate_project(
    server: &mut TestLspServer,
    root: &std::path::Path,
) -> (String, String) {
    std::fs::write(root.join("_quarto.yml"), "project:\n  type: default\n").unwrap();
    let a_path = root.join("a.qmd");
    let b_path = root.join("b.qmd");
    std::fs::write(
        &a_path,
        "{{< include b.qmd >}}\n\n# A\n\n[ref]: https://example.com/a\n",
    )
    .unwrap();
    std::fs::write(&b_path, "# B\n\n[ref]: https://example.com/b\n").unwrap();

    let a_uri = Uri::from_file_path(&a_path).unwrap().to_string();
    let b_uri = Uri::from_file_path(&b_path).unwrap().to_string();
    server.open_document(&a_uri, &std::fs::read_to_string(&a_path).unwrap(), "quarto");
    server.pump(Duration::from_secs(2));
    server.open_document(&b_uri, &std::fs::read_to_string(&b_path).unwrap(), "quarto");
    server.save_document(&b_uri);
    server.pump(Duration::from_secs(2));
    (a_uri, b_uri)
}

/// A `related_document_support` client pulling a clean document receives the
/// cross-file diagnostics of its project-graph closure under `related_documents`.
#[test]
fn document_pull_carries_related_cross_file_diagnostics() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root = temp_dir.path();
    let mut server = TestLspServer::new();
    server.initialize_pull(&Uri::from_file_path(root).unwrap().to_string());

    let (dirty_uri, clean_uri) = open_cross_file_duplicate_project(&mut server, root);

    // Pull the clean document; its own report carries no duplicate, but the
    // related (dirty) document's cross-file diagnostic rides along.
    let report = server.document_diagnostic(&clean_uri, None);
    let full = full_report(&report);
    assert!(
        !has_duplicate_label(&full.full_document_diagnostic_report.items),
        "the pulled document's own report should be clean"
    );

    let related = full
        .related_documents
        .as_ref()
        .expect("related_documents should be populated for a related-support client");
    let dirty_uri_parsed: Uri = dirty_uri.parse().unwrap();
    let entry = related
        .get(&dirty_uri_parsed)
        .expect("the related document carrying the duplicate should be present");
    match entry {
        DocumentDiagnosticReportKind::Full(full) => {
            assert!(
                has_duplicate_label(&full.items),
                "the related document should carry the cross-file duplicate diagnostic"
            );
            assert!(
                full.result_id.is_some(),
                "the related report should carry a result_id"
            );
        }
        other => panic!("expected a full related report, got: {other:?}"),
    }
}

/// Without `related_document_support`, the same cross-file scenario leaves
/// `related_documents` empty (the diagnostics still reach the client via
/// `workspace/diagnostic`).
#[test]
fn document_pull_omits_related_documents_without_capability() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root = temp_dir.path();
    let mut server = TestLspServer::new();
    server.initialize_pull_no_related(&Uri::from_file_path(root).unwrap().to_string());

    let (_dirty_uri, clean_uri) = open_cross_file_duplicate_project(&mut server, root);

    let report = server.document_diagnostic(&clean_uri, None);
    assert!(
        full_report(&report).related_documents.is_none(),
        "related_documents must stay empty without related_document_support"
    );
}
