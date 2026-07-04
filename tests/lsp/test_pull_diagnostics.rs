//! Tests for the LSP pull diagnostics model (`textDocument/diagnostic` +
//! `workspace/diagnostic`), and the push/pull mode-switch.

use super::helpers::*;
use lsp_server::{ErrorCode, Message};
use lsp_types::*;
use std::time::Duration;

/// The `id`'d response among drained client messages, if one has been sent.
fn response_for<'a>(
    messages: &'a [Message],
    id: &lsp_server::RequestId,
) -> Option<&'a lsp_server::Response> {
    messages.iter().find_map(|msg| match msg {
        Message::Response(resp) if resp.id == *id => Some(resp),
        _ => None,
    })
}

/// A `textDocument/diagnostic` pull is dispatched to the worker pool, not answered
/// inline on the event loop: the response arrives only after a `pump` drives the
/// pooled result. Inline dispatch (the old behavior) recomputed the whole linter
/// on the main thread per pull, so a neovim keystroke-burst of pulls pegged the
/// loop and froze a synchronous format-on-save behind them. Regression guard for
/// that freeze.
#[test]
fn document_pull_is_answered_off_the_event_loop() {
    let mut server = TestLspServer::new();
    server.initialize_pull("file:///workspace");
    let uri = "file:///workspace/doc.qmd";
    server.open_document(uri, "# H1\n\n### H3 skip\n", "quarto");
    server.pump(Duration::from_secs(2));
    server.drain_client_messages();

    let id = server.send_document_diagnostic_request_raw(7, uri);
    // Inline handling would already have sent the response; the pooled path posts
    // its result to the task channel, which only `pump` turns into a response.
    let immediate = server.drain_client_messages();
    assert!(
        response_for(&immediate, &id).is_none(),
        "pull must not be answered inline on the event loop"
    );

    server.pump(Duration::from_secs(2));
    let drained = server.drain_client_messages();
    let response = response_for(&drained, &id).expect("expected an async response for the pull");
    assert!(
        response.error.is_none(),
        "the pull should succeed, got error: {:?}",
        response.error
    );
}

/// An in-flight `textDocument/diagnostic` pull is cancellable via `$/cancelRequest`
/// because it runs on the pool. This is the mechanism that keeps a burst of pulls
/// from stacking full recomputes: a superseding edit (or explicit cancel) drops
/// the earlier pull instead of running every one to completion.
#[test]
fn document_pull_is_cancellable() {
    let mut server = TestLspServer::new();
    server.initialize_pull("file:///workspace");
    let uri = "file:///workspace/doc.qmd";
    server.open_document(uri, "# H1\n\n### H3 skip\n", "quarto");
    server.pump(Duration::from_secs(2));
    server.drain_client_messages();

    let id = server.send_document_diagnostic_request_raw(11, uri);
    server.send_cancel(id.clone());
    server.pump(Duration::from_secs(2));

    let drained = server.drain_client_messages();
    let response = response_for(&drained, &id).expect("expected a response for the cancelled pull");
    let err = response
        .error
        .as_ref()
        .expect("a cancelled pull should produce an error response");
    assert_eq!(
        err.code,
        ErrorCode::RequestCanceled as i32,
        "expected RequestCanceled, got code {}: {}",
        err.code,
        err.message
    );
}

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

/// A burst of `did_open`s (a client opening a whole workspace) arms a single
/// settle over every document. `pump` must wait for that in-flight pass to be
/// applied before reporting quiescence — otherwise the whole batch's diagnostics
/// never reach the store and `workspace/diagnostic` comes back empty.
///
/// Regression: `pump` used to break on the first lull whenever `settle_deadline`
/// was clear, abandoning a settle pass still running on the pool once the pass
/// outran the poll step. The fix tracks the last-applied lint generation so
/// `pump` blocks until the dispatched pass lands.
#[test]
fn workspace_pull_reports_a_burst_open_after_one_pump() {
    let mut server = TestLspServer::new();
    server.initialize_pull("file:///workspace");

    // Open many heavyweight documents WITHOUT pumping between them, so a single
    // settle covers the whole batch and the pass is large enough to outrun the
    // poll step. Each doc carries a heading-hierarchy violation plus filler so
    // the pass does real parse+lint work.
    let mut filler = String::new();
    for i in 0..1_000 {
        filler.push_str(&format!(
            "Paragraph {i:04} alpha beta gamma delta epsilon.\n"
        ));
    }
    let body = format!("# H1\n\n### H3 skip\n\n{filler}");

    let count = 40;
    let uris: Vec<String> = (0..count)
        .map(|i| format!("file:///workspace/doc{i:03}.qmd"))
        .collect();
    for uri in &uris {
        server.open_document(uri, &body, "quarto");
    }

    // One pump for the whole burst.
    server.pump(Duration::from_secs(10));

    let report = server.workspace_diagnostic(vec![]);
    let reported = workspace_full_reports(&report);
    let reported_uris: std::collections::HashSet<&str> =
        reported.iter().map(|(uri, _)| uri.as_str()).collect();

    for uri in &uris {
        assert!(
            reported_uris.contains(uri.as_str()),
            "workspace report dropped {uri}: only {}/{count} of the burst reported",
            reported_uris.len()
        );
        let items = reported
            .iter()
            .find(|(u, _)| u == uri)
            .map(|(_, items)| *items)
            .unwrap_or(&[]);
        assert!(
            has_heading_hierarchy(items),
            "expected heading-hierarchy diagnostic for {uri}"
        );
    }
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

// --- partial result streaming (`partialResultToken`) ---

/// Every per-document report URI in a workspace result (full or unchanged).
fn workspace_result_uris(
    result: &WorkspaceDiagnosticReportResult,
) -> std::collections::HashSet<String> {
    let WorkspaceDiagnosticReportResult::Report(report) = result else {
        panic!("expected a full workspace report");
    };
    report.items.iter().map(workspace_item_uri).collect()
}

fn workspace_item_uri(item: &WorkspaceDocumentDiagnosticReport) -> String {
    match item {
        WorkspaceDocumentDiagnosticReport::Full(full) => full.uri.as_str().to_owned(),
        WorkspaceDocumentDiagnosticReport::Unchanged(unchanged) => {
            unchanged.uri.as_str().to_owned()
        }
    }
}

/// A small workspace pulled with a `partialResultToken` fits in one chunk: the
/// whole report rides in the response and no `$/progress` is streamed.
#[test]
fn workspace_streaming_single_chunk_emits_no_progress() {
    let mut server = TestLspServer::new();
    server.initialize_pull("file:///workspace");
    let a = "file:///workspace/a.qmd";
    let b = "file:///workspace/b.qmd";

    server.open_document(a, "# H1\n\n### H3 skip\n", "quarto");
    server.pump(Duration::from_secs(2));
    server.open_document(b, "# H1\n\n## H2\n", "quarto");
    server.pump(Duration::from_secs(2));

    let (response, progress) = server.workspace_diagnostic_streaming(7, vec![]);
    assert!(
        progress.is_empty(),
        "a workspace smaller than one chunk should stream no progress"
    );
    let uris = workspace_result_uris(&response);
    assert!(uris.contains(a) && uris.contains(b));
}

/// A workspace larger than one chunk streams the remainder as `$/progress`: the
/// union of the response and every chunk equals the whole (non-streaming) report.
#[test]
fn workspace_streaming_splits_large_report() {
    let mut server = TestLspServer::new();
    server.initialize_pull("file:///workspace");

    // Just over `WORKSPACE_REPORT_CHUNK_SIZE` (64) so the report spans two chunks.
    // Pump after each open so the settle lints the document before the next open's
    // salsa write could cancel the in-flight pass (same pattern as the other
    // workspace-pull tests).
    let count = 66;
    for i in 0..count {
        let uri = format!("file:///workspace/doc{i}.qmd");
        server.open_document(&uri, "# H1\n\n### H3 skip\n", "quarto");
        server.pump(Duration::from_secs(2));
    }

    let whole = workspace_result_uris(&server.workspace_diagnostic(vec![]));
    assert_eq!(
        whole.len(),
        count,
        "every opened document should be reported"
    );

    let (response, progress) = server.workspace_diagnostic_streaming(11, vec![]);
    assert!(
        !progress.is_empty(),
        "a {count}-document workspace should stream at least one progress chunk"
    );

    let mut streamed = workspace_result_uris(&response);
    for chunk in &progress {
        for item in &chunk.items {
            assert!(
                streamed.insert(workspace_item_uri(item)),
                "no document should appear twice across response + chunks"
            );
        }
    }
    assert_eq!(
        streamed, whole,
        "union of response + progress chunks must equal the whole report"
    );
}

/// A document whose related closure fits in one chunk streams no `$/progress`:
/// `related_documents` rides whole in the response even with a token.
#[test]
fn document_streaming_single_related_chunk_emits_no_progress() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root = temp_dir.path();
    let mut server = TestLspServer::new();
    server.initialize_pull(&Uri::from_file_path(root).unwrap().to_string());

    let (dirty_uri, clean_uri) = open_cross_file_duplicate_project(&mut server, root);

    let (response, progress) = server.document_diagnostic_streaming(&clean_uri, 3, None);
    assert!(
        progress.is_empty(),
        "a single related document fits in one chunk; nothing to stream"
    );
    let dirty: Uri = dirty_uri.parse().unwrap();
    assert!(
        full_report(&response)
            .related_documents
            .as_ref()
            .expect("related_documents present")
            .contains_key(&dirty),
        "the related document should ride in the response"
    );
}

/// In PULL mode a `textDocument/diagnostic` pull is computed on demand, so it is
/// current *immediately* after an edit — without waiting for the debounced settle
/// and without depending on the client re-pulling on
/// `workspace/diagnostic/refresh`. This is the regression guard for the
/// one-edit-behind bug: previously the pull served the debounced store, so a pull
/// issued right after `didChange` returned the previous edit's diagnostics.
#[test]
fn pull_bibliography_load_error_is_current_without_waiting_for_settle() {
    let mut server = TestLspServer::new();
    server.initialize_pull("file:///workspace");
    assert!(server.pull_diagnostics_enabled());
    let uri = "file:///workspace/doc.qmd";

    let with_r = "---\nbibliography: references.bib\n---\n\nSee [@known].\n";

    let load_error_path = |result: &DocumentDiagnosticReportResult| -> Option<String> {
        full_items(result)
            .iter()
            .find(|d| matches!(&d.code, Some(NumberOrString::String(s)) if s == "bibliography-load-error"))
            // Normalize the path separator so the slash-prefixed assertions below
            // (which disambiguate `eferences.bib` from `references.bib`) hold on
            // Windows, where the path uses `\`.
            .map(|d| d.message.replace('\\', "/"))
    };

    let delete_r = TextDocumentContentChangeEvent {
        range: Some(Range {
            start: Position {
                line: 1,
                character: 14,
            },
            end: Position {
                line: 1,
                character: 15,
            },
        }),
        range_length: None,
        text: String::new(),
    };
    let insert_r = TextDocumentContentChangeEvent {
        range: Some(Range {
            start: Position {
                line: 1,
                character: 14,
            },
            end: Position {
                line: 1,
                character: 14,
            },
        }),
        range_length: None,
        text: "r".to_string(),
    };

    server.open_document(uri, with_r, "quarto");
    server.pump(Duration::from_secs(2));
    let first = server.document_diagnostic(uri, None);
    assert!(
        load_error_path(&first).is_some_and(|m| m.contains("references.bib")),
        "initial pull must report references.bib, got: {:?}",
        load_error_path(&first)
    );

    // Edit to eferences and pull WITHOUT pumping the settle: on-demand compute
    // makes it current immediately (this is the fix — previously one-behind).
    server.edit_document(uri, vec![delete_r]);
    server.drain_client_messages();
    let fresh = server.document_diagnostic(uri, None);
    assert!(
        load_error_path(&fresh).is_some_and(|m| m.contains("/eferences.bib")),
        "pull right after the edit must already report eferences.bib, got: {:?}",
        load_error_path(&fresh)
    );

    // Restore references.bib and pull again without pumping: still current.
    server.edit_document(uri, vec![insert_r]);
    server.drain_client_messages();
    let restored = server.document_diagnostic(uri, None);
    let msg = load_error_path(&restored);
    assert!(
        msg.as_ref().is_some_and(|m| m.contains("/references.bib")),
        "pull right after restoring must report references.bib, got: {msg:?}"
    );
}
