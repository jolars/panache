//! Request/notification/task routing for the synchronous main loop.
//!
//! [`GlobalState::on_request`] ships read requests to the [`TaskPool`] over a
//! [`StateSnapshot`]; [`GlobalState::on_notification`] mutates state inline on
//! the main thread; [`GlobalState::on_task`] turns completed worker results into
//! client messages. The salsa-cancellation safety net lives here: a pooled read
//! that unwinds with [`salsa::Cancelled`] becomes a `ContentModified` response.

use std::time::{Duration, Instant};

use lsp_server::{ErrorCode, ExtractError, Notification, Request, RequestId, Response};
use lsp_types::notification::Notification as _;
use lsp_types::request::Request as _;
use lsp_types::{InitializeParams, ServerCapabilities, Uri};
use serde::Serialize;
use serde_json::Value;

use super::global_state::{GlobalState, LintRequest, StateSnapshot, Task};
use super::helpers::catch_cancelled;
use super::uri_ext::UriExt;
use super::{documents, handlers};

/// Which worker pool a request runs on: the shared `Main` pool for interactive
/// reads, or the single-thread `Fmt` pool that isolates slow external
/// formatters from hover/completion latency.
enum RequestPool {
    Main,
    Fmt,
}

/// Build the static server capabilities advertised at `initialize`.
pub(crate) fn server_capabilities() -> ServerCapabilities {
    use lsp_types::*;

    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::INCREMENTAL),
                // Expensive external linters run on save (not per keystroke); we
                // don't need the document text included.
                save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                    include_text: Some(false),
                })),
                ..Default::default()
            },
        )),
        document_formatting_provider: Some(OneOf::Left(true)),
        document_range_formatting_provider: Some(OneOf::Left(true)),
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        document_link_provider: Some(DocumentLinkOptions {
            resolve_provider: Some(true),
            work_done_progress_options: WorkDoneProgressOptions {
                work_done_progress: None,
            },
        }),
        folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec!["(".into(), "/".into(), "<".into()]),
            ..Default::default()
        }),
        references_provider: Some(OneOf::Left(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: WorkDoneProgressOptions {
                work_done_progress: None,
            },
        })),
        // Pull diagnostics (LSP 3.17). Advertising is harmless for push-only
        // clients (they ignore it); the server only switches off push for
        // clients that advertise pull support (see `on_initialize`).
        diagnostic_provider: Some(DiagnosticServerCapabilities::Options(DiagnosticOptions {
            identifier: Some("panache".to_string()),
            // Editing an include/bibliography changes diagnostics in other files.
            inter_file_dependencies: true,
            workspace_diagnostics: true,
            work_done_progress_options: WorkDoneProgressOptions {
                work_done_progress: None,
            },
        })),
        workspace: Some(WorkspaceServerCapabilities {
            workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                supported: Some(true),
                change_notifications: Some(OneOf::Left(true)),
            }),
            file_operations: Some(WorkspaceFileOperationsServerCapabilities {
                will_rename: Some(FileOperationRegistrationOptions {
                    filters: file_operation_filters(),
                }),
                ..Default::default()
            }),
        }),
        ..Default::default()
    }
}

fn watched_document_glob() -> Vec<lsp_types::FileSystemWatcher> {
    use lsp_types::*;
    crate::all_document_extensions()
        .iter()
        .map(|ext| FileSystemWatcher {
            glob_pattern: GlobPattern::String(format!("**/*.{ext}")),
            kind: Some(WatchKind::all()),
        })
        .collect()
}

fn file_operation_filters() -> Vec<lsp_types::FileOperationFilter> {
    use lsp_types::*;
    crate::all_document_extensions()
        .iter()
        .map(|ext| FileOperationFilter {
            scheme: Some("file".to_string()),
            pattern: FileOperationPattern {
                glob: format!("**/*.{ext}"),
                matches: Some(FileOperationPatternKind::File),
                options: None,
            },
        })
        .collect()
}

fn legacy_root_uri(params: &InitializeParams) -> Option<Uri> {
    let value = serde_json::to_value(params).ok()?;
    value
        .get("rootUri")
        .cloned()
        .and_then(|root_uri| serde_json::from_value(root_uri).ok())
}

fn experimental_incremental_parsing_from_initialize(params: &InitializeParams) -> bool {
    fn get_bool(value: &Value, path: &[&str]) -> Option<bool> {
        let mut current = value;
        for key in path {
            current = current.get(key)?;
        }
        current.as_bool()
    }

    let Some(options) = params.initialization_options.as_ref() else {
        return false;
    };

    get_bool(
        options,
        &["settings", "panache", "experimental", "incrementalParsing"],
    )
    .or_else(|| get_bool(options, &["panache", "experimental", "incrementalParsing"]))
    .or_else(|| get_bool(options, &["experimental", "incrementalParsing"]))
    .unwrap_or(false)
}

impl GlobalState {
    /// Apply `initialize` params: workspace root + runtime settings.
    pub(crate) fn on_initialize(&mut self, params: InitializeParams) {
        if let Some(folders) = params.workspace_folders.as_ref()
            && let Some(folder) = folders.first()
            && let Some(path) = folder.uri.to_file_path()
        {
            self.workspace_root = Some(path.into_owned());
        } else if let Some(root_uri) = legacy_root_uri(&params)
            && let Some(path) = root_uri.to_file_path()
        {
            self.workspace_root = Some(path.into_owned());
        }

        let experimental = experimental_incremental_parsing_from_initialize(&params);
        self.runtime_settings.experimental_incremental_parsing = experimental;
        log::debug!(
            "lsp runtime setting experimental.incrementalParsing={experimental} (initialize options)"
        );

        // Pull diagnostics mode-switch: a client that advertises
        // `textDocument.diagnostic` is served via pull only (push is suppressed).
        // `workspace.diagnostic.refresh_support` lets us nudge it to re-pull when
        // an async pass updates the store.
        self.supports_pull_diagnostics = params
            .capabilities
            .text_document
            .as_ref()
            .is_some_and(|td| td.diagnostic.is_some());
        self.supports_diagnostic_refresh = params
            .capabilities
            .workspace
            .as_ref()
            .and_then(|ws| ws.diagnostic.as_ref())
            .and_then(|d| d.refresh_support)
            .unwrap_or(false);
        log::debug!(
            "lsp pull diagnostics: supported={} refresh={}",
            self.supports_pull_diagnostics,
            self.supports_diagnostic_refresh
        );
        if self.supports_pull_diagnostics && !self.supports_diagnostic_refresh {
            log::debug!(
                "client supports pull diagnostics but not refresh; async results \
                 (save-time external linters, cross-file) reach it only on its next pull"
            );
        }
    }

    /// Post-`initialized` side effects: log + register file watchers.
    pub(crate) fn on_initialized(&mut self) {
        use lsp_types::*;
        self.sender
            .log_message(MessageType::INFO, "panache LSP server initialized");
        log::debug!("initialized LSP server");

        let mut watchers = vec![
            FileSystemWatcher {
                glob_pattern: GlobPattern::String("**/*.bib".to_string()),
                kind: Some(WatchKind::all()),
            },
            FileSystemWatcher {
                glob_pattern: GlobPattern::String("**/*.json".to_string()),
                kind: Some(WatchKind::all()),
            },
            FileSystemWatcher {
                glob_pattern: GlobPattern::String("**/*.yaml".to_string()),
                kind: Some(WatchKind::all()),
            },
            FileSystemWatcher {
                glob_pattern: GlobPattern::String("**/*.yml".to_string()),
                kind: Some(WatchKind::all()),
            },
            FileSystemWatcher {
                glob_pattern: GlobPattern::String("**/*.ris".to_string()),
                kind: Some(WatchKind::all()),
            },
        ];
        watchers.extend(watched_document_glob());

        let registration = Registration {
            id: "watch-bibliography-files".to_string(),
            method: "workspace/didChangeWatchedFiles".to_string(),
            register_options: serde_json::to_value(DidChangeWatchedFilesRegistrationOptions {
                watchers,
            })
            .ok(),
        };
        self.send_request::<lsp_types::request::RegisterCapability>(RegistrationParams {
            registrations: vec![registration],
        });
    }

    /// Route an incoming request: pool it over a read snapshot, or reject.
    pub(crate) fn on_request(&mut self, mut req: Request) {
        use lsp_types::request as r;

        macro_rules! pool {
            ($R:ty, $handler:expr) => {
                pool!($R, $handler, spawn_request);
            };
            ($R:ty, $handler:expr, $spawn:ident) => {
                req = match req.extract::<<$R as r::Request>::Params>(<$R>::METHOD) {
                    Ok((id, params)) => {
                        return self.$spawn::<_, <$R as r::Request>::Result>(id, params, $handler);
                    }
                    Err(ExtractError::MethodMismatch(req)) => req,
                    Err(ExtractError::JsonError { method, error }) => {
                        log::error!("invalid params for {method}: {error}");
                        return;
                    }
                };
            };
        }

        // Answer a request inline on the main thread. Used by the pull
        // diagnostics handlers, which read `GlobalState`'s store directly (a
        // cheap map lookup) rather than a pooled `StateSnapshot`.
        macro_rules! inline {
            ($R:ty, $handler:expr) => {
                req = match req.extract::<<$R as r::Request>::Params>(<$R>::METHOD) {
                    Ok((id, params)) => {
                        let result = $handler(self, params);
                        return self.respond(Response::new_ok(id, result));
                    }
                    Err(ExtractError::MethodMismatch(req)) => req,
                    Err(ExtractError::JsonError { method, error }) => {
                        log::error!("invalid params for {method}: {error}");
                        return;
                    }
                };
            };
        }

        inline!(
            r::DocumentDiagnosticRequest,
            handlers::diagnostics::document_diagnostic
        );
        inline!(
            r::WorkspaceDiagnosticRequest,
            handlers::diagnostics::workspace_diagnostic
        );

        pool!(
            r::Formatting,
            handlers::formatting::format_document,
            spawn_format_request
        );
        pool!(
            r::RangeFormatting,
            handlers::formatting::format_range,
            spawn_format_request
        );
        pool!(r::CodeActionRequest, handlers::code_actions::code_action);
        pool!(
            r::DocumentSymbolRequest,
            handlers::document_symbols::document_symbol
        );
        pool!(
            r::DocumentLinkRequest,
            handlers::document_links::document_links
        );
        pool!(
            r::DocumentLinkResolve,
            handlers::document_links::document_link_resolve
        );
        pool!(
            r::FoldingRangeRequest,
            handlers::folding_ranges::folding_range
        );
        pool!(
            r::GotoDefinition,
            handlers::goto_definition::goto_definition
        );
        pool!(r::HoverRequest, handlers::hover::hover);
        pool!(r::Completion, handlers::completion::completion);
        pool!(r::Rename, handlers::rename::rename);
        pool!(
            r::PrepareRenameRequest,
            handlers::prepare_rename::prepare_rename
        );
        pool!(r::References, handlers::references::references);
        pool!(
            r::WorkspaceSymbolRequest,
            handlers::workspace_symbols::workspace_symbol
        );
        pool!(r::WillRenameFiles, handlers::file_rename::will_rename_files);

        self.respond(Response::new_err(
            req.id,
            ErrorCode::MethodNotFound as i32,
            format!("unhandled request: {}", req.method),
        ));
    }

    /// Route an incoming notification (mutates state on the main thread).
    pub(crate) fn on_notification(&mut self, mut not: Notification) {
        use lsp_types::notification as n;

        macro_rules! handle {
            ($N:ty, $handler:expr) => {
                not = match not.extract::<<$N as n::Notification>::Params>(<$N>::METHOD) {
                    Ok(params) => return $handler(self, params),
                    Err(ExtractError::MethodMismatch(not)) => not,
                    Err(ExtractError::JsonError { method, error }) => {
                        log::error!("invalid params for {method}: {error}");
                        return;
                    }
                };
            };
        }

        handle!(n::DidOpenTextDocument, documents::did_open);
        handle!(n::DidChangeTextDocument, documents::did_change);
        handle!(n::DidSaveTextDocument, documents::did_save);
        handle!(n::DidCloseTextDocument, documents::did_close);
        handle!(
            n::DidChangeWatchedFiles,
            handlers::file_watcher::did_change_watched_files
        );
        handle!(n::Cancel, GlobalState::on_cancel);

        log::debug!("ignoring notification: {}", not.method);
    }

    /// `$/cancelRequest`: mark the id so its eventual result is relabeled.
    fn on_cancel(&mut self, params: lsp_types::CancelParams) {
        let id: RequestId = match params.id {
            lsp_types::NumberOrString::Number(n) => RequestId::from(n),
            lsp_types::NumberOrString::String(s) => RequestId::from(s),
        };
        if self.in_flight.contains(&id) {
            self.cancelled.insert(id);
        }
    }

    /// Turn a completed worker result into client messages.
    pub(crate) fn on_task(&mut self, task: Task) {
        match task {
            Task::Response(resp) => {
                if self.cancelled.contains(&resp.id) {
                    let id = resp.id.clone();
                    self.respond(Response::new_err(
                        id,
                        ErrorCode::RequestCanceled as i32,
                        "request cancelled".to_owned(),
                    ));
                } else {
                    self.respond(resp);
                }
            }
            Task::Diagnostics {
                generation,
                key,
                publishes,
                manifest_uris,
            } => {
                // Drop results superseded by a newer edit.
                if self.lint_generations.get(&key).copied() == Some(generation) {
                    for (uri, version, diags) in publishes {
                        self.deliver_diagnostics(uri, version, diags);
                    }
                    // Clear-on-fix: a manifest this pass no longer reports gets an
                    // empty delivery — but only if no OTHER open document still
                    // reports it (a shared `_quarto.yml` stays flagged until every
                    // dependent is fixed/closed). The manifest URI differs from
                    // `key`, so the generation guard alone never clears it.
                    let previous = self
                        .published_manifest_uris
                        .get(&key)
                        .cloned()
                        .unwrap_or_default();
                    for stale in previous.difference(&manifest_uris) {
                        let still_referenced = self
                            .published_manifest_uris
                            .iter()
                            .any(|(other_key, set)| other_key != &key && set.contains(stale));
                        if !still_referenced {
                            self.deliver_diagnostics(stale.clone(), None, Vec::new());
                        }
                    }
                    if manifest_uris.is_empty() {
                        self.published_manifest_uris.remove(&key);
                    } else {
                        self.published_manifest_uris.insert(key, manifest_uris);
                    }
                    // One coalesced nudge after the whole batch (pull + refresh
                    // only; a no-op for push clients).
                    self.send_diagnostic_refresh();
                }
            }
            Task::DiagnosticsCancelled { generation, key } => {
                // Batched dispatch keeps sibling lints from cancelling each other,
                // but a write that doesn't itself re-arm a lint — `didClose`, a
                // file-watcher event, a config reload — can still cancel an
                // in-flight pass. This is the recovery net: if no newer edit has
                // superseded it (stale generation ⇒ a fresh lint is already
                // queued), re-arm the debounce so the diagnostics recompute
                // instead of being lost until the next edit. Built-in only; the
                // next save refreshes external-linter diagnostics.
                if self.lint_generations.get(&key).copied() == Some(generation)
                    && let Ok(uri) = key.parse::<Uri>()
                {
                    log::debug!("lint for {key} cancelled by concurrent write; re-arming");
                    self.schedule_lint(
                        &uri,
                        LintRequest {
                            with_dependents: true,
                            run_external: false,
                        },
                    );
                }
            }
        }
    }

    /// Spawn a pooled read request; its result returns as a [`Task::Response`].
    pub(crate) fn spawn_request<P, R>(
        &mut self,
        id: RequestId,
        params: P,
        f: fn(&StateSnapshot, P) -> R,
    ) where
        P: Send + 'static,
        R: Serialize + Send + 'static,
    {
        self.spawn_request_on(RequestPool::Main, id, params, f);
    }

    /// Spawn a formatting request on the dedicated `fmt_pool` so a slow
    /// external formatter can't stall hover/completion latency on the main
    /// pool.
    pub(crate) fn spawn_format_request<P, R>(
        &mut self,
        id: RequestId,
        params: P,
        f: fn(&StateSnapshot, P) -> R,
    ) where
        P: Send + 'static,
        R: Serialize + Send + 'static,
    {
        self.spawn_request_on(RequestPool::Fmt, id, params, f);
    }

    fn spawn_request_on<P, R>(
        &mut self,
        pool: RequestPool,
        id: RequestId,
        params: P,
        f: fn(&StateSnapshot, P) -> R,
    ) where
        P: Send + 'static,
        R: Serialize + Send + 'static,
    {
        self.in_flight.insert(id.clone());
        let snap = self.snapshot();
        let pool = match pool {
            RequestPool::Main => &self.pool,
            RequestPool::Fmt => &self.fmt_pool,
        };
        let sender = pool.result_sender();
        pool.spawn(move || {
            // `catch_cancelled` maps a salsa cancellation to `None` and
            // re-raises every other panic. Catch those here so a handler bug
            // becomes an `InternalError` response rather than unwinding past
            // the send and leaving the request id forever unanswered (the
            // client would hang). The worker's own `catch_unwind` only ever
            // sees non-request jobs after this.
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                catch_cancelled(|| f(&snap, params))
            }));
            let task = match outcome {
                Ok(Some(value)) => Task::Response(Response::new_ok(id, value)),
                Ok(None) => Task::Response(Response::new_err(
                    id,
                    ErrorCode::ContentModified as i32,
                    "content modified".to_owned(),
                )),
                Err(panic) => {
                    let msg = panic
                        .downcast_ref::<&'static str>()
                        .copied()
                        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
                        .unwrap_or("<non-string panic payload>");
                    log::error!("LSP request handler panicked: {msg}");
                    Task::Response(Response::new_err(
                        id,
                        ErrorCode::InternalError as i32,
                        "internal error: request handler panicked".to_owned(),
                    ))
                }
            };
            let _ = sender.send(task);
        });
    }

    /// Dispatch a lint pass to the pool. Bumps the lint generation and tags the
    /// result with it, so a later edit (which bumps again) drops a stale result.
    pub(crate) fn spawn_lint(&mut self, uri: Uri, with_dependents: bool, run_external: bool) {
        let key = uri.to_string();
        let generation = {
            let g = self.lint_generations.entry(key.clone()).or_insert(0);
            *g += 1;
            *g
        };
        let snap = self.snapshot();
        let sender = self.pool.result_sender();
        self.pool.spawn(move || {
            let result = catch_cancelled(|| {
                let mut publishes = if with_dependents {
                    handlers::diagnostics::compute_publishes_with_dependents(
                        &snap,
                        &uri,
                        run_external,
                    )
                } else {
                    handlers::diagnostics::compute_publishes(&snap, &uri, run_external)
                };
                // Project-manifest (`_quarto.yml` etc.) diagnostics, published on
                // the manifest's own URI. Computed once for the primary document
                // (the query already spans the whole project graph), so dependents
                // don't recompute it.
                let (manifest_pubs, manifest_uris) =
                    handlers::diagnostics::manifest_publishes(&snap, &uri);
                publishes.extend(manifest_pubs);
                (publishes, manifest_uris)
            });
            match result {
                Some((publishes, manifest_uris)) => {
                    let _ = sender.send(Task::Diagnostics {
                        generation,
                        key,
                        publishes,
                        manifest_uris,
                    });
                }
                // A concurrent write cancelled the read. Signal the main loop so
                // it can retry rather than silently losing these diagnostics.
                None => {
                    let _ = sender.send(Task::DiagnosticsCancelled { generation, key });
                }
            }
        });
    }

    /// Time until the nearest due lint deadline, for the main-loop `select!`.
    pub(crate) fn next_lint_timeout(&self) -> Option<Duration> {
        self.lint_deadlines
            .values()
            .min()
            .map(|&deadline| deadline.saturating_duration_since(Instant::now()))
    }

    /// Dispatch any debounced lints whose deadline has elapsed.
    ///
    /// Two phases on purpose: **first** apply every due document's writes (load
    /// newly-referenced includes/bibliographies), **then** snapshot and spawn the
    /// reads. Interleaving them — `load → spawn → load → spawn` — let one
    /// document's write cancel the previous document's just-spawned lint (a
    /// pooled salsa read on a cloned handle), so a burst of opens/edits would
    /// drop diagnostics for all but the last. Batching the write phase ahead of
    /// the read phase means every spawned lint reads a settled database.
    pub(crate) fn dispatch_due_lints(&mut self) {
        let now = Instant::now();
        let due: Vec<String> = self
            .lint_deadlines
            .iter()
            .filter(|(_, deadline)| **deadline <= now)
            .map(|(key, _)| key.clone())
            .collect();
        if due.is_empty() {
            return;
        }

        // Write phase: load every due document's referenced files first. A
        // keystroke burst may have added an include/bibliography since the last
        // pass; `file_text` no longer lazy-loads (audit §3.2), so the writer loads
        // them here, coalesced onto the debounce boundary.
        for key in &due {
            self.lint_deadlines.remove(key);
            if let Some((salsa_file, salsa_config, Some(path))) = self
                .document_map
                .get(key)
                .map(|doc| (doc.salsa_file, doc.salsa_config, doc.path.clone()))
            {
                documents::load_project_files(self, salsa_file, salsa_config, path);
            }
        }

        // Read phase: snapshot and spawn over the now-settled database. No salsa
        // writes happen between these spawns, so no lint cancels a sibling.
        for key in &due {
            let Ok(uri) = key.parse::<Uri>() else {
                continue;
            };
            let request = self.pending_lints.remove(key).unwrap_or(LintRequest {
                with_dependents: true,
                run_external: false,
            });
            self.spawn_lint(uri, request.with_dependents, request.run_external);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::global_state::ClientSender;
    use std::time::Duration;

    fn panicking_handler(_: &StateSnapshot, _: ()) {
        panic!("boom");
    }

    /// A handler panic must come back as an `InternalError` response for the
    /// request id, not vanish — otherwise the client waits forever.
    #[test]
    fn panicking_handler_yields_internal_error_response() {
        let (tx, _client_rx) = crossbeam_channel::unbounded();
        let mut gs = GlobalState::new(ClientSender::new(tx));

        let id = RequestId::from(1);
        gs.spawn_request::<(), ()>(id.clone(), (), panicking_handler);

        let task = gs
            .task_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("pooled handler should post a result even on panic");

        match task {
            Task::Response(resp) => {
                assert_eq!(resp.id, id);
                let err = resp.error.expect("panic should produce an error response");
                assert_eq!(err.code, ErrorCode::InternalError as i32);
            }
            _ => panic!("expected a Task::Response"),
        }
    }

    fn new_state() -> GlobalState {
        let (tx, _client_rx) = crossbeam_channel::unbounded();
        GlobalState::new(ClientSender::new(tx))
    }

    /// A lint cancelled by a concurrent write (e.g. `didClose`) must be re-armed
    /// so its diagnostics aren't lost — provided no newer edit superseded it.
    #[test]
    fn cancelled_lint_with_current_generation_is_rearmed() {
        let mut gs = new_state();
        let key = "file:///doc.qmd".to_string();
        gs.lint_generations.insert(key.clone(), 7);

        gs.on_task(Task::DiagnosticsCancelled {
            generation: 7,
            key: key.clone(),
        });

        assert!(
            gs.lint_deadlines.contains_key(&key),
            "a current-generation cancellation should re-arm the debounced lint"
        );
        assert!(gs.pending_lints.contains_key(&key));
    }

    /// A cancellation whose generation is stale means a newer lint is already
    /// queued; re-arming again would be redundant, so it must be dropped.
    #[test]
    fn cancelled_lint_with_stale_generation_is_dropped() {
        let mut gs = new_state();
        let key = "file:///doc.qmd".to_string();
        gs.lint_generations.insert(key.clone(), 9);

        gs.on_task(Task::DiagnosticsCancelled {
            generation: 7,
            key: key.clone(),
        });

        assert!(
            !gs.lint_deadlines.contains_key(&key),
            "a stale-generation cancellation should not re-arm"
        );
    }
}
