//! Lint pipeline: pure, synchronous computation of the diagnostics to publish.
//!
//! These functions run on a [`TaskPool`](crate::lsp::task_pool) worker over a
//! [`StateSnapshot`]. They never touch the client directly — they *return* the
//! publishes, and the main loop turns them into `textDocument/publishDiagnostics`
//! notifications (dropping stale ones via the lint generation counter).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use lsp_types::{
    Diagnostic, DiagnosticSeverity, DocumentDiagnosticParams, DocumentDiagnosticReport,
    DocumentDiagnosticReportKind, DocumentDiagnosticReportPartialResult,
    DocumentDiagnosticReportResult, FullDocumentDiagnosticReport, ProgressToken, Range,
    RelatedFullDocumentDiagnosticReport, RelatedUnchangedDocumentDiagnosticReport,
    UnchangedDocumentDiagnosticReport, Uri, WorkspaceDiagnosticParams, WorkspaceDiagnosticReport,
    WorkspaceDiagnosticReportPartialResult, WorkspaceDiagnosticReportResult,
    WorkspaceDocumentDiagnosticReport, WorkspaceFullDocumentDiagnosticReport,
    WorkspaceUnchangedDocumentDiagnosticReport,
};
use serde::Serialize;

use super::super::conversions::{convert_diagnostic, offset_to_position};
use crate::lsp::global_state::{GlobalState, StateSnapshot};
use crate::lsp::uri_ext::UriExt;

/// A single `publishDiagnostics` payload: target URI, optional version, diags.
pub(crate) type Publish = (Uri, Option<i32>, Vec<Diagnostic>);

/// How many per-document reports ride in a single `workspace/diagnostic` chunk
/// when the client requests partial results.
const WORKSPACE_REPORT_CHUNK_SIZE: usize = 64;

/// How many `relatedDocuments` entries ride in a single `textDocument/diagnostic`
/// chunk when the client requests partial results.
const RELATED_REPORT_CHUNK_SIZE: usize = 64;

/// A pull-diagnostics handler result split for the dispatcher: the `response`
/// carries the request's first chunk, and `progress` carries the remaining
/// chunks as pre-built `$/progress` notifications (sent after the response, in
/// order). When the client didn't supply a `partialResultToken`, `progress` is
/// empty and `response` is the whole report — behavior identical to before
/// streaming existed.
pub(crate) struct Streamed<R> {
    pub(crate) response: R,
    pub(crate) progress: Vec<lsp_server::Notification>,
}

impl<R> Streamed<R> {
    /// A whole, un-streamed report (no `$/progress` chunks).
    fn whole(response: R) -> Self {
        Self {
            response,
            progress: Vec::new(),
        }
    }
}

/// Build one `$/progress` notification carrying a partial-result `value` keyed by
/// the client's `token`. `lsp_types::ProgressParamsValue` only models work-done
/// progress, so the envelope is assembled directly.
fn progress_notification(token: &ProgressToken, value: impl Serialize) -> lsp_server::Notification {
    #[derive(Serialize)]
    struct Envelope<'a, T> {
        token: &'a ProgressToken,
        value: T,
    }
    lsp_server::Notification::new("$/progress".to_owned(), Envelope { token, value })
}

/// YAML parse-error diagnostics for the project-manifest files (`_quarto.yml`,
/// `_metadata.yml`, `_bookdown.yml`/`_output.yml`, and `metadata-files:`
/// includes) reachable from `uri`'s project, each published against the
/// manifest's OWN URI — even when that file isn't open in the editor (the
/// rust-analyzer `Cargo.toml` model). Manifest text is read from salsa (a
/// tracked input), not from an open document.
///
/// Returns the publishes plus the set of manifest URIs that received a
/// diagnostic. The all-docs settle pass merges these across documents and the
/// `DiagnosticCollection` reconciles clears from the complete set, so the
/// returned URI set is informational for callers that want it.
pub(crate) fn manifest_publishes(snap: &StateSnapshot, uri: &Uri) -> (Vec<Publish>, HashSet<Uri>) {
    let Some(doc_state) = snap.document_state(uri) else {
        return (Vec::new(), HashSet::new());
    };
    // Collect linter diagnostics per manifest path so parse errors and
    // `quarto-schema` diagnostics for the same file land in ONE publish — the
    // LSP replaces the full diagnostic set per URI, so two publishes for the
    // same URI would clobber each other.
    let mut by_path: BTreeMap<PathBuf, Vec<crate::linter::diagnostics::Diagnostic>> =
        BTreeMap::new();

    let parse_diags = crate::salsa::project_manifest_diagnostics(
        snap.db(),
        doc_state.salsa_file,
        doc_state.salsa_config,
    );
    for (path, yaml_error) in parse_diags {
        let Some(file_text) = snap.db().file_text(path.clone()) else {
            continue;
        };
        let Some(manifest_text) = file_text.text(snap.db()).as_deref() else {
            continue;
        };
        if let Some(diag) =
            crate::linter::metadata_diagnostics::yaml_error_diagnostic(yaml_error, manifest_text)
        {
            by_path.entry(path.clone()).or_default().push(diag);
        }
    }

    let schema_diags = crate::salsa::project_manifest_schema_diagnostics(
        snap.db(),
        doc_state.salsa_file,
        doc_state.salsa_config,
    );
    for (path, diags) in schema_diags {
        by_path
            .entry(path.clone())
            .or_default()
            .extend(diags.iter().cloned());
    }

    let mut publishes = Vec::new();
    let mut manifest_uris = HashSet::new();
    for (path, diags) in by_path {
        let Some(target_uri) = Uri::from_file_path(&path) else {
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
        let converted = diags
            .iter()
            .map(|d| convert_diagnostic(d, manifest_text))
            .collect();
        publishes.push((target_uri.clone(), None, converted));
        manifest_uris.insert(target_uri);
    }
    (publishes, manifest_uris)
}

/// A `panache.toml` parse-error diagnostic published against the config file's
/// OWN URI — the rust-analyzer `Cargo.toml` model, mirroring
/// [`manifest_publishes`] — so a broken *discovered* config surfaces even when
/// the file isn't open in the editor.
///
/// The document itself still parses and lints under the default config (see
/// [`crate::lsp::config::load_config_with_source`]); this diagnostic is the
/// *why your settings aren't being applied* signal. Returns an empty vec when
/// the config parses, so the settle pass omits the URI and
/// [`DiagnosticCollection`](crate::lsp::global_state::DiagnosticCollection)
/// clears any prior error (clear-on-fix).
pub(crate) fn config_publishes(snap: &StateSnapshot, uri: &Uri) -> Vec<Publish> {
    let Err(err) = crate::lsp::config::try_load_config(&snap.workspace_folders, Some(uri)) else {
        return Vec::new();
    };
    let Some(target_uri) = Uri::from_file_path(&err.path) else {
        return Vec::new();
    };
    // The config file is typically not an open document, so read its text from
    // disk to map the byte span to a range.
    let Ok(text) = std::fs::read_to_string(&err.path) else {
        return Vec::new();
    };
    let range = match err.span {
        Some(span) => Range {
            start: offset_to_position(&text, span.start),
            end: offset_to_position(&text, span.end.min(text.len())),
        },
        // No span: anchor at the file's start so the diagnostic still lands.
        None => Range::default(),
    };
    let diagnostic = Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("panache".to_owned()),
        message: err.message,
        ..Diagnostic::default()
    };
    vec![(target_uri, None, vec![diagnostic])]
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

/// A stable, content-addressed `result_id` for a pulled document's diagnostics.
///
/// On-demand pulls (see [`document_diagnostic`]) don't go through the store's
/// sequential `result_id` allocator, so the id must be derivable from the items
/// alone: re-pulling unchanged diagnostics yields the same id (→ an `unchanged`
/// report) and any change yields a different one. `Diagnostic` isn't `Hash`, so
/// hash its stable JSON encoding.
fn result_id_for(items: &[Diagnostic]) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_vec(items)
        .unwrap_or_default()
        .hash(&mut hasher);
    hasher.finish().to_string()
}

/// Pull handler for `textDocument/diagnostic`, run on the worker pool.
///
/// Recomputes the pulled document's diagnostics over the snapshot so the report
/// reflects the **current** buffer. Serving the debounced settle store here
/// instead would trail the buffer by up to one debounce, which surfaced as neovim
/// showing diagnostics "one edit behind" (it pulls immediately after `didChange`,
/// before the settle re-lints). Running on the pool rather than inline on the
/// event loop keeps the recompute off the main thread: it no longer blocks a
/// synchronous `textDocument/formatting`, and a concurrent edit's salsa write
/// cancels an in-flight pull (unwinding to `ContentModified`, on which the client
/// re-pulls) so a keystroke burst can't stack full lints and freeze the editor.
///
/// This gives freshness *and* responsiveness without redundant work: the heavy
/// part — rebuilding the cross-document reference index that resolves `@ref`s
/// against every chapter of a book — is memoized by salsa, so an *unchanged*
/// re-pull is cheap and only an edited buffer pays the rebuild (and only
/// off-thread). External linters are still skipped on a pull (they run on the
/// settle).
///
/// Returns an `unchanged` report when the client's `previous_result_id` still
/// matches, else a full report. For `related_document_support` clients,
/// `related_documents` carries the cross-file diagnostics of the pulled document's
/// project-graph closure, read from the snapshot's store view (see
/// [`related_documents`]); otherwise those reach the client only via
/// `workspace/diagnostic`.
pub(crate) fn document_diagnostic(
    snap: &StateSnapshot,
    params: DocumentDiagnosticParams,
) -> Streamed<DocumentDiagnosticReportResult> {
    // A push-only client is served by push: never surface diagnostics through a
    // pull (keeps push and pull mutually exclusive — no double reporting).
    if !snap.supports_pull_diagnostics {
        return Streamed::whole(DocumentDiagnosticReportResult::Report(
            DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                related_documents: None,
                full_document_diagnostic_report: FullDocumentDiagnosticReport::default(),
            }),
        ));
    }
    let token = params.partial_result_params.partial_result_token;
    let uri = params.text_document.uri;

    // Recompute this document's own diagnostics over the snapshot. `compute_publishes`
    // returns one entry per affected URI; the requested URI's entry carries its
    // built-in + own cross-file diagnostics. The salsa reads here can be cancelled
    // by a concurrent write, unwinding this pooled job into a `ContentModified`.
    let items = compute_publishes(snap, &uri, false)
        .into_iter()
        .find(|(target, _, _)| *target == uri)
        .map(|(_, _, items)| items)
        .unwrap_or_default();
    let result_id = result_id_for(&items);

    // Computed for both arms: an unchanged main document can still have related
    // documents whose diagnostics changed. When the client asked for partial
    // results, the first chunk rides in the response and the rest stream as
    // `$/progress` notifications (see `split_related`).
    let related = related_documents(snap, &uri);
    let (related, progress) = split_related(token.as_ref(), related);
    let report = if params.previous_result_id.as_deref() == Some(result_id.as_str()) {
        DocumentDiagnosticReport::Unchanged(RelatedUnchangedDocumentDiagnosticReport {
            related_documents: related,
            unchanged_document_diagnostic_report: UnchangedDocumentDiagnosticReport { result_id },
        })
    } else {
        DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
            related_documents: related,
            full_document_diagnostic_report: FullDocumentDiagnosticReport {
                result_id: Some(result_id),
                items,
            },
        })
    };
    Streamed {
        response: DocumentDiagnosticReportResult::Report(report),
        progress,
    }
}

/// Split a `relatedDocuments` map into the entries kept in the response and the
/// `$/progress` chunks that stream the remainder.
///
/// With no `token` (or a map that fits in one chunk) the whole map stays in the
/// response and no progress is emitted, so a non-streaming client sees today's
/// behavior unchanged. The map's iteration order is irrelevant: every entry is
/// keyed by its own URI and the client merges across response + chunks.
fn split_related(
    token: Option<&ProgressToken>,
    related: Option<HashMap<Uri, DocumentDiagnosticReportKind>>,
) -> (
    Option<HashMap<Uri, DocumentDiagnosticReportKind>>,
    Vec<lsp_server::Notification>,
) {
    let Some(token) = token else {
        return (related, Vec::new());
    };
    let Some(map) = related else {
        return (None, Vec::new());
    };
    if map.len() <= RELATED_REPORT_CHUNK_SIZE {
        return ((!map.is_empty()).then_some(map), Vec::new());
    }
    let mut entries: Vec<(Uri, DocumentDiagnosticReportKind)> = map.into_iter().collect();
    let rest = entries.split_off(RELATED_REPORT_CHUNK_SIZE);
    let response_map: HashMap<Uri, DocumentDiagnosticReportKind> = entries.into_iter().collect();
    let progress = rest
        .chunks(RELATED_REPORT_CHUNK_SIZE)
        .map(|chunk| {
            let chunk_map: HashMap<Uri, DocumentDiagnosticReportKind> =
                chunk.iter().cloned().collect();
            progress_notification(
                token,
                DocumentDiagnosticReportPartialResult {
                    related_documents: Some(chunk_map),
                },
            )
        })
        .collect();
    (Some(response_map), progress)
}

/// The cross-file diagnostics to attach under `related_documents` for a pull of
/// `uri`, or `None` when the client lacks `related_document_support`, the
/// document isn't an on-disk open file, or no related document currently carries
/// diagnostics.
///
/// Relatedness is the document's project-graph closure (every file transitively
/// reachable in either direction, plus the project manifests it links to), which
/// is symmetric by construction — unlike per-document diagnostic *attribution*,
/// where a cross-doc duplicate lands on whichever file salsa visits second. The
/// closure decides *which* documents are related; their diagnostic *content* is
/// read from the store, the single source of truth. A related document whose
/// diagnostics were cleared has been dropped from the store and so simply falls
/// out of the map (the authoritative clear path stays `workspace/diagnostic`).
///
/// Reads the store view carried on the snapshot, so it runs on the pool with the
/// rest of the pull handler; `project_structure` is a memoized, range-free query
/// the settle already warms.
fn related_documents(
    snap: &StateSnapshot,
    uri: &Uri,
) -> Option<HashMap<Uri, DocumentDiagnosticReportKind>> {
    if !snap.supports_related_documents {
        return None;
    }
    let doc_state = snap.document_state(uri)?;
    // In-memory buffers have no path and no project graph; nothing to relate.
    let root = uri.to_file_path()?.into_owned();
    let graph =
        crate::salsa::project_structure(snap.db(), doc_state.salsa_file, doc_state.salsa_config);

    let mut map = HashMap::new();
    for path in project_closure(graph, &root) {
        let Some(target) = Uri::from_file_path(&path) else {
            continue;
        };
        if target == *uri {
            continue;
        }
        let Some(stored) = snap.diagnostics.get(&target) else {
            continue;
        };
        if stored.items.is_empty() {
            continue;
        }
        map.insert(
            target,
            DocumentDiagnosticReportKind::Full(FullDocumentDiagnosticReport {
                result_id: Some(stored.result_id.clone()),
                items: stored.items.clone(),
            }),
        );
    }
    (!map.is_empty()).then_some(map)
}

/// Every path transitively connected to `root` in `graph`, in either direction
/// and across every edge kind (so project manifests, reached via `ProjectConfig`
/// /`MetadataFile` edges, are included), excluding `root` itself.
fn project_closure(graph: &crate::salsa::ProjectGraph, root: &PathBuf) -> HashSet<PathBuf> {
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut stack = vec![root.clone()];
    while let Some(path) = stack.pop() {
        for next in graph
            .dependencies(&path, None)
            .into_iter()
            .chain(graph.dependents(&path, None))
        {
            if visited.insert(next.clone()) {
                stack.push(next);
            }
        }
    }
    visited.remove(root);
    visited
}

/// Pull handler for `workspace/diagnostic`.
///
/// Returns one report per URI in the pull store, emitting `unchanged` where the
/// client already holds the current `result_id` (matched against
/// `previous_result_ids`).
pub(crate) fn workspace_diagnostic(
    gs: &GlobalState,
    params: WorkspaceDiagnosticParams,
) -> Streamed<WorkspaceDiagnosticReportResult> {
    // Push-only clients are served by push; never surface the unified store via a
    // pull (see `document_diagnostic`).
    if !gs.supports_pull_diagnostics {
        return Streamed::whole(WorkspaceDiagnosticReportResult::Report(
            WorkspaceDiagnosticReport { items: Vec::new() },
        ));
    }
    let known: HashMap<&Uri, &str> = params
        .previous_result_ids
        .iter()
        .map(|prev| (&prev.uri, prev.value.as_str()))
        .collect();

    let items: Vec<WorkspaceDocumentDiagnosticReport> = gs
        .diagnostics
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

    // With a `partialResultToken` the first batch rides in the response and the
    // rest stream as `$/progress` notifications; otherwise the whole report is
    // returned as before.
    let token = params.partial_result_params.partial_result_token;
    let (items, progress) = split_workspace_items(token.as_ref(), items);
    Streamed {
        response: WorkspaceDiagnosticReportResult::Report(WorkspaceDiagnosticReport { items }),
        progress,
    }
}

/// Split the per-document workspace reports into the batch kept in the response
/// and the `$/progress` chunks streaming the remainder. With no `token` the whole
/// list stays in the response (today's behavior).
fn split_workspace_items(
    token: Option<&ProgressToken>,
    items: Vec<WorkspaceDocumentDiagnosticReport>,
) -> (
    Vec<WorkspaceDocumentDiagnosticReport>,
    Vec<lsp_server::Notification>,
) {
    let Some(token) = token else {
        return (items, Vec::new());
    };
    let mut chunks = items.chunks(WORKSPACE_REPORT_CHUNK_SIZE);
    let first = chunks.next().map(<[_]>::to_vec).unwrap_or_default();
    let progress = chunks
        .map(|chunk| {
            progress_notification(
                token,
                WorkspaceDiagnosticReportPartialResult {
                    items: chunk.to_vec(),
                },
            )
        })
        .collect();
    (first, progress)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token() -> ProgressToken {
        ProgressToken::Number(1)
    }

    fn workspace_item(i: usize) -> WorkspaceDocumentDiagnosticReport {
        let uri: Uri = format!("file:///doc{i}.qmd").parse().unwrap();
        WorkspaceDocumentDiagnosticReport::Full(WorkspaceFullDocumentDiagnosticReport {
            uri,
            version: None,
            full_document_diagnostic_report: FullDocumentDiagnosticReport::default(),
        })
    }

    fn workspace_uris(items: &[WorkspaceDocumentDiagnosticReport]) -> Vec<String> {
        items
            .iter()
            .map(|item| match item {
                WorkspaceDocumentDiagnosticReport::Full(full) => full.uri.as_str().to_owned(),
                WorkspaceDocumentDiagnosticReport::Unchanged(unchanged) => {
                    unchanged.uri.as_str().to_owned()
                }
            })
            .collect()
    }

    fn related_map(n: usize) -> HashMap<Uri, DocumentDiagnosticReportKind> {
        (0..n)
            .map(|i| {
                let uri: Uri = format!("file:///rel{i}.qmd").parse().unwrap();
                (
                    uri,
                    DocumentDiagnosticReportKind::Full(FullDocumentDiagnosticReport::default()),
                )
            })
            .collect()
    }

    /// The workspace items a progress notification carries.
    fn workspace_chunk_items(
        note: &lsp_server::Notification,
    ) -> Vec<WorkspaceDocumentDiagnosticReport> {
        assert_eq!(note.method, "$/progress");
        let value = note.params.get("value").unwrap().clone();
        serde_json::from_value::<WorkspaceDiagnosticReportPartialResult>(value)
            .unwrap()
            .items
    }

    /// The related-document keys a progress notification carries.
    fn related_chunk_keys(note: &lsp_server::Notification) -> Vec<String> {
        assert_eq!(note.method, "$/progress");
        let value = note.params.get("value").unwrap().clone();
        serde_json::from_value::<DocumentDiagnosticReportPartialResult>(value)
            .unwrap()
            .related_documents
            .unwrap()
            .keys()
            .map(|u| u.as_str().to_owned())
            .collect()
    }

    #[test]
    fn workspace_no_token_keeps_everything_in_response() {
        let items: Vec<_> = (0..WORKSPACE_REPORT_CHUNK_SIZE + 5)
            .map(workspace_item)
            .collect();
        let expected = workspace_uris(&items);
        let (first, progress) = split_workspace_items(None, items);
        assert!(progress.is_empty(), "no token => no streaming");
        assert_eq!(workspace_uris(&first), expected);
    }

    #[test]
    fn workspace_single_chunk_emits_no_progress() {
        let items: Vec<_> = (0..WORKSPACE_REPORT_CHUNK_SIZE)
            .map(workspace_item)
            .collect();
        let expected = workspace_uris(&items);
        let (first, progress) = split_workspace_items(Some(&token()), items);
        assert!(
            progress.is_empty(),
            "exactly one chunk fits in the response"
        );
        assert_eq!(workspace_uris(&first), expected);
    }

    #[test]
    fn workspace_multi_chunk_preserves_every_report() {
        let total = WORKSPACE_REPORT_CHUNK_SIZE * 2 + 3;
        let items: Vec<_> = (0..total).map(workspace_item).collect();
        let expected = workspace_uris(&items);
        let (first, progress) = split_workspace_items(Some(&token()), items);

        assert_eq!(
            first.len(),
            WORKSPACE_REPORT_CHUNK_SIZE,
            "first chunk is full"
        );
        assert_eq!(progress.len(), 2, "two streamed chunks for 2*N+3 items");

        let mut seen = workspace_uris(&first);
        for note in &progress {
            let chunk = workspace_chunk_items(note);
            assert!(
                chunk.len() <= WORKSPACE_REPORT_CHUNK_SIZE,
                "no chunk exceeds the chunk size"
            );
            seen.extend(workspace_uris(&chunk));
        }
        assert_eq!(seen, expected, "union of response + chunks == whole report");
    }

    #[test]
    fn related_no_token_keeps_whole_map() {
        let map = related_map(RELATED_REPORT_CHUNK_SIZE + 5);
        let expected = map.len();
        let (response, progress) = split_related(None, Some(map));
        assert!(progress.is_empty());
        assert_eq!(response.unwrap().len(), expected);
    }

    #[test]
    fn related_none_stays_none() {
        let (response, progress) = split_related(Some(&token()), None);
        assert!(response.is_none());
        assert!(progress.is_empty());
    }

    #[test]
    fn related_single_chunk_emits_no_progress() {
        let map = related_map(RELATED_REPORT_CHUNK_SIZE);
        let (response, progress) = split_related(Some(&token()), Some(map));
        assert_eq!(response.unwrap().len(), RELATED_REPORT_CHUNK_SIZE);
        assert!(progress.is_empty());
    }

    #[test]
    fn related_multi_chunk_preserves_every_entry() {
        let total = RELATED_REPORT_CHUNK_SIZE * 2 + 3;
        let map = related_map(total);
        let expected: HashSet<String> = map.keys().map(|u| u.as_str().to_owned()).collect();
        let (response, progress) = split_related(Some(&token()), Some(map));

        let response = response.expect("first chunk rides in the response");
        assert_eq!(
            response.len(),
            RELATED_REPORT_CHUNK_SIZE,
            "first chunk is full"
        );
        assert_eq!(progress.len(), 2, "two streamed chunks for 2*N+3 entries");

        let mut seen: HashSet<String> = response.keys().map(|u| u.as_str().to_owned()).collect();
        for note in &progress {
            let keys = related_chunk_keys(note);
            assert!(
                keys.len() <= RELATED_REPORT_CHUNK_SIZE,
                "no chunk exceeds the size"
            );
            seen.extend(keys);
        }
        assert_eq!(seen, expected, "union of response + chunks == whole map");
    }
}
