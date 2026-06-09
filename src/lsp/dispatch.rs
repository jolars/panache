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

use super::global_state::{GlobalState, StateSnapshot, Task};
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
            } => {
                // Drop results superseded by a newer edit.
                if self.lint_generations.get(&key).copied() == Some(generation) {
                    for (uri, version, diags) in publishes {
                        self.sender.publish_diagnostics(uri, diags, version);
                    }
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
            let publishes = catch_cancelled(|| {
                if with_dependents {
                    handlers::diagnostics::compute_publishes_with_dependents(
                        &snap,
                        &uri,
                        run_external,
                    )
                } else {
                    handlers::diagnostics::compute_publishes(&snap, &uri, run_external)
                }
            });
            if let Some(publishes) = publishes {
                let _ = sender.send(Task::Diagnostics {
                    generation,
                    key,
                    publishes,
                });
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
    pub(crate) fn dispatch_due_lints(&mut self) {
        let now = Instant::now();
        let due: Vec<String> = self
            .lint_deadlines
            .iter()
            .filter(|(_, deadline)| **deadline <= now)
            .map(|(key, _)| key.clone())
            .collect();
        for key in due {
            self.lint_deadlines.remove(&key);
            let Ok(uri) = key.parse::<Uri>() else {
                continue;
            };
            // A keystroke burst may have added an include/bibliography since the
            // last pass. `file_text` no longer lazy-loads (audit §3.2), so load
            // any newly-referenced file on the writer here --- coalesced onto the
            // debounce boundary --- before `spawn_lint` takes its snapshot.
            if let Some((salsa_file, salsa_config, Some(path))) = self
                .document_map
                .get(&key)
                .map(|doc| (doc.salsa_file, doc.salsa_config, doc.path.clone()))
            {
                documents::load_project_files(self, salsa_file, salsa_config, path);
            }
            // Debounced (per-keystroke) pass: dependents, built-in only.
            self.spawn_lint(uri, true, false);
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
}
