//! Lint pipeline: pure, synchronous computation of the diagnostics to publish.
//!
//! These functions run on a [`TaskPool`](crate::lsp::task_pool) worker over a
//! [`StateSnapshot`]. They never touch the client directly — they *return* the
//! publishes, and the main loop turns them into `textDocument/publishDiagnostics`
//! notifications (dropping stale ones via the lint generation counter).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use lsp_types::{
    Diagnostic, DocumentDiagnosticParams, DocumentDiagnosticReport, DocumentDiagnosticReportResult,
    FullDocumentDiagnosticReport, RelatedFullDocumentDiagnosticReport,
    RelatedUnchangedDocumentDiagnosticReport, UnchangedDocumentDiagnosticReport, Uri,
    WorkspaceDiagnosticParams, WorkspaceDiagnosticReport, WorkspaceDiagnosticReportResult,
    WorkspaceDocumentDiagnosticReport, WorkspaceFullDocumentDiagnosticReport,
    WorkspaceUnchangedDocumentDiagnosticReport,
};

use super::super::conversions::convert_diagnostic;
use crate::lsp::global_state::{GlobalState, StateSnapshot};
use crate::lsp::uri_ext::UriExt;

/// A single `publishDiagnostics` payload: target URI, optional version, diags.
pub(crate) type Publish = (Uri, Option<i32>, Vec<Diagnostic>);

/// YAML parse-error diagnostics for the project-manifest files (`_quarto.yml`,
/// `_metadata.yml`, `_bookdown.yml`/`_output.yml`, and `metadata-files:`
/// includes) reachable from `uri`'s project, each published against the
/// manifest's OWN URI — even when that file isn't open in the editor (the
/// rust-analyzer `Cargo.toml` model). Manifest text is read from salsa (a
/// tracked input), not from an open document.
///
/// Returns the publishes plus the set of manifest URIs that received a
/// diagnostic. The all-docs settle pass merges these across documents and
/// reconciles clears via `GlobalState::last_published_uris`, so the returned set
/// is informational for callers that want it.
pub(crate) fn manifest_publishes(snap: &StateSnapshot, uri: &Uri) -> (Vec<Publish>, HashSet<Uri>) {
    let Some(doc_state) = snap.document_state(uri) else {
        return (Vec::new(), HashSet::new());
    };
    let mut publishes = Vec::new();
    let mut manifest_uris = HashSet::new();
    let manifest_diags = crate::salsa::project_manifest_diagnostics(
        snap.db(),
        doc_state.salsa_file,
        doc_state.salsa_config,
    );
    for (path, yaml_error) in manifest_diags {
        let Some(target_uri) = Uri::from_file_path(path) else {
            continue;
        };
        // The manifest is typically NOT an open document, so read its text from
        // the tracked salsa input rather than the document map.
        let Some(file_text) = snap.db().file_text(path.clone()) else {
            continue;
        };
        let Some(manifest_text) = file_text.text(snap.db()).as_deref() else {
            continue;
        };
        if let Some(diag) =
            crate::linter::metadata_diagnostics::yaml_error_diagnostic(yaml_error, manifest_text)
        {
            publishes.push((
                target_uri.clone(),
                None,
                vec![convert_diagnostic(&diag, manifest_text)],
            ));
            manifest_uris.insert(target_uri);
        }
    }
    (publishes, manifest_uris)
}

/// Compute diagnostics for `uri` and any documents that depend on it.
///
/// Bench-only baseline (`lsp_relint`): the live model re-lints every open
/// document per settle, so it no longer walks dependents. Retained to measure the
/// old "changed doc + dependents" per-edit cost against the all-docs cost. Lints
/// the project graph's dependents built-in-only, then the target document itself
/// (with external linters iff `run_external`).
pub(crate) fn compute_publishes_with_dependents(
    snap: &StateSnapshot,
    uri: &Uri,
    run_external: bool,
) -> Vec<Publish> {
    let mut publishes = Vec::new();

    // Find documents that include `uri`; lint each built-in-only first.
    if let Some(state) = snap.document_state(uri)
        && let Some(path) = state.path.as_ref()
    {
        let graph =
            crate::salsa::project_structure(snap.db(), state.salsa_file, state.salsa_config)
                .clone();
        for dependent in graph.dependents(path, None) {
            if let Some(dep_uri) = Uri::from_file_path(&dependent) {
                publishes.extend(compute_publishes(snap, &dep_uri, false));
            }
        }
    }

    publishes.extend(compute_publishes(snap, uri, run_external));
    publishes
}

/// Compute the diagnostics publishes for a single document.
///
/// Combines the built-in lint plan (+ external linters when `run_external`) with
/// the project-graph accumulated diagnostics, keyed by file path. Returns one
/// [`Publish`] per affected document.
pub(crate) fn compute_publishes(
    snap: &StateSnapshot,
    uri: &Uri,
    run_external: bool,
) -> Vec<Publish> {
    log::debug!(
        "compute_publishes uri={} run_external={}",
        uri.as_str(),
        run_external
    );

    let Some(doc_state) = snap.document_state(uri) else {
        log::warn!("Document not found for lint: {}", uri.as_str());
        return Vec::new();
    };

    let text = doc_state.salsa_file.content_or_empty(snap.db()).to_string();

    let lint_plan =
        crate::salsa::built_in_lint_plan(snap.db(), doc_state.salsa_file, doc_state.salsa_config)
            .clone();

    let mut panache_diagnostics = lint_plan.diagnostics;
    let external_jobs = lint_plan.external_jobs;

    #[cfg(not(target_arch = "wasm32"))]
    if run_external && !external_jobs.is_empty() {
        let registry = crate::linter::external_linters::ExternalLinterRegistry::new();
        for job in &external_jobs {
            match crate::linter::external_linters_sync::run_linter_sync(
                &job.linter_name,
                &job.language,
                &job.content,
                &text,
                &registry,
                Some(job.mappings.as_slice()),
            ) {
                Ok(diags) => panache_diagnostics.extend(diags),
                Err(e) => log::warn!("External linter failed: {e}"),
            }
        }
        panache_diagnostics.sort_by_key(|d| (d.location.line, d.location.column));
    }

    let own_diagnostics: Vec<Diagnostic> = panache_diagnostics
        .iter()
        .map(|d| convert_diagnostic(d, &text))
        .collect();

    // The document's own path, if it has one (an in-memory buffer does not, so
    // it contributes no project-graph entry and is published only under its URI).
    let root_path = uri.to_file_path().map(|p| p.into_owned());

    let mut by_path: HashMap<PathBuf, Vec<crate::linter::diagnostics::Diagnostic>> = HashMap::new();
    for entry in crate::salsa::project_graph::accumulated::<crate::salsa::GraphDiagnostic>(
        snap.db(),
        doc_state.salsa_file,
        doc_state.salsa_config,
    ) {
        by_path
            .entry(entry.0.path.clone())
            .or_default()
            .push(entry.0.diagnostic.clone());
    }
    if let Some(root_path) = root_path {
        by_path.entry(root_path).or_default();
    }

    let mut publishes = Vec::new();
    let mut published_root = false;
    for (path, diags) in by_path {
        let target_uri = Uri::from_file_path(&path).unwrap_or_else(|| uri.clone());

        let target_text = if target_uri == *uri {
            text.clone()
        } else {
            let Some(target_state) = snap.document_state(&target_uri) else {
                continue;
            };
            target_state
                .salsa_file
                .content_or_empty(snap.db())
                .to_string()
        };

        let mapped: Vec<Diagnostic> = diags
            .iter()
            .map(|d| convert_diagnostic(d, &target_text))
            .collect();

        if target_uri == *uri {
            let mut merged = own_diagnostics.clone();
            merged.extend(mapped);
            publishes.push((uri.clone(), None, merged));
            published_root = true;
        } else {
            publishes.push((target_uri, None, mapped));
        }
    }

    if !published_root {
        publishes.push((uri.clone(), None, own_diagnostics));
    }

    publishes
}

/// Pull handler for `textDocument/diagnostic`.
///
/// Reads the document's latest diagnostics from the pull store (filled by the
/// same async lint pipeline that drives push). Returns an `unchanged` report when
/// the client's `previous_result_id` still matches, else a full report. A missing
/// entry yields an empty full report (clears any stale client-side state).
///
/// `related_documents` is left empty: cross-file diagnostics reach the client via
/// `workspace/diagnostic` (the server advertises `inter_file_dependencies` +
/// `workspace_diagnostics`), so per-document related plumbing is unnecessary.
pub(crate) fn document_diagnostic(
    gs: &GlobalState,
    params: DocumentDiagnosticParams,
) -> DocumentDiagnosticReportResult {
    let uri = params.text_document.uri;
    let report = match gs.diagnostics_store.get(&uri) {
        Some(stored) if params.previous_result_id.as_deref() == Some(stored.result_id.as_str()) => {
            DocumentDiagnosticReport::Unchanged(RelatedUnchangedDocumentDiagnosticReport {
                related_documents: None,
                unchanged_document_diagnostic_report: UnchangedDocumentDiagnosticReport {
                    result_id: stored.result_id.clone(),
                },
            })
        }
        Some(stored) => DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
            related_documents: None,
            full_document_diagnostic_report: FullDocumentDiagnosticReport {
                result_id: Some(stored.result_id.clone()),
                items: stored.items.clone(),
            },
        }),
        None => DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
            related_documents: None,
            full_document_diagnostic_report: FullDocumentDiagnosticReport::default(),
        }),
    };
    DocumentDiagnosticReportResult::Report(report)
}

/// Pull handler for `workspace/diagnostic`.
///
/// Returns one report per URI in the pull store, emitting `unchanged` where the
/// client already holds the current `result_id` (matched against
/// `previous_result_ids`).
pub(crate) fn workspace_diagnostic(
    gs: &GlobalState,
    params: WorkspaceDiagnosticParams,
) -> WorkspaceDiagnosticReportResult {
    let known: HashMap<&Uri, &str> = params
        .previous_result_ids
        .iter()
        .map(|prev| (&prev.uri, prev.value.as_str()))
        .collect();

    let items = gs
        .diagnostics_store
        .iter()
        .map(|(uri, stored)| {
            if known.get(uri).copied() == Some(stored.result_id.as_str()) {
                WorkspaceDocumentDiagnosticReport::Unchanged(
                    WorkspaceUnchangedDocumentDiagnosticReport {
                        uri: uri.clone(),
                        version: stored.version.map(i64::from),
                        unchanged_document_diagnostic_report: UnchangedDocumentDiagnosticReport {
                            result_id: stored.result_id.clone(),
                        },
                    },
                )
            } else {
                WorkspaceDocumentDiagnosticReport::Full(WorkspaceFullDocumentDiagnosticReport {
                    uri: uri.clone(),
                    version: stored.version.map(i64::from),
                    full_document_diagnostic_report: FullDocumentDiagnosticReport {
                        result_id: Some(stored.result_id.clone()),
                        items: stored.items.clone(),
                    },
                })
            }
        })
        .collect();

    WorkspaceDiagnosticReportResult::Report(WorkspaceDiagnosticReport { items })
}
