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
        // Newline trigger: continuation indentation inside list items. The
        // client must opt in to firing the request (Neovim core does not).
        document_on_type_formatting_provider: Some(DocumentOnTypeFormattingOptions {
            first_trigger_character: "\n".to_string(),
            more_trigger_character: None,
        }),
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
            resolve_provider: Some(true),
            ..Default::default()
        }),
        references_provider: Some(OneOf::Left(true)),
        // Highlight every occurrence of the symbol under the cursor within the
        // current document. See `handlers::document_highlight`.
        document_highlight_provider: Some(OneOf::Left(true)),
        // Additive, flavor-gated highlighting for pandoc/quarto-specific
        // constructs the editor's base grammar misses. Custom legend types
        // no-op until themed, so advertising on by default is harmless. See
        // `handlers::semantic_tokens`.
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                work_done_progress_options: WorkDoneProgressOptions {
                    work_done_progress: None,
                },
                legend: handlers::semantic_tokens::legend(),
                range: Some(false),
                full: Some(SemanticTokensFullOptions::Bool(true)),
            },
        )),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: WorkDoneProgressOptions {
                work_done_progress: None,
            },
        })),
        // Live, type-to-rename of a symbol and its linked occurrences within the
        // current document (LSP 3.17). See `handlers::linked_editing_range`.
        linked_editing_range_provider: Some(LinkedEditingRangeServerCapabilities::Simple(true)),
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
                // `willRename` returns a `WorkspaceEdit` rewriting cross-document
                // references; the `did*` notifications are hygiene-only (re-intern
                // + re-lint, see `handlers::file_operations`). `willCreate`/
                // `willDelete` stay unregistered: no scaffolding on create, no
                // destructive auto-edit on delete.
                will_rename: Some(FileOperationRegistrationOptions {
                    filters: file_operation_filters(),
                }),
                did_rename: Some(FileOperationRegistrationOptions {
                    filters: file_operation_filters(),
                }),
                did_create: Some(FileOperationRegistrationOptions {
                    filters: file_operation_filters(),
                }),
                did_delete: Some(FileOperationRegistrationOptions {
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

/// Read the experimental incremental-parsing flag from a settings/options JSON
/// value, tolerating the `settings.panache.*`, `panache.*`, and bare `*` nesting
/// that different clients use for both `initializationOptions` and
/// `workspace/didChangeConfiguration`. Returns `None` when the key is absent so
/// callers can distinguish "unset" from "explicitly false".
pub(crate) fn runtime_incremental_parsing_from_value(value: &Value) -> Option<bool> {
    fn get_bool(value: &Value, path: &[&str]) -> Option<bool> {
        let mut current = value;
        for key in path {
            current = current.get(key)?;
        }
        current.as_bool()
    }

    get_bool(
        value,
        &["settings", "panache", "experimental", "incrementalParsing"],
    )
    .or_else(|| get_bool(value, &["panache", "experimental", "incrementalParsing"]))
    .or_else(|| get_bool(value, &["experimental", "incrementalParsing"]))
}

fn experimental_incremental_parsing_from_initialize(params: &InitializeParams) -> bool {
    params
        .initialization_options
        .as_ref()
        .and_then(runtime_incremental_parsing_from_value)
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
        // Independent of pull support: lets `document_diagnostic` carry cross-file
        // diagnostics for the pulled document's project-graph closure inline.
        self.supports_related_documents = params
            .capabilities
            .text_document
            .as_ref()
            .and_then(|td| td.diagnostic.as_ref())
            .and_then(|d| d.related_document_support)
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
            // Config files so edits reload config for open documents (the
            // file-name-matched branch in `file_watcher`). Both spellings are
            // discovered by `config::load`.
            FileSystemWatcher {
                glob_pattern: GlobPattern::String("**/panache.toml".to_string()),
                kind: Some(WatchKind::all()),
            },
            FileSystemWatcher {
                glob_pattern: GlobPattern::String("**/.panache.toml".to_string()),
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

        // Answer a request inline on the main thread, the handler returning a
        // `Streamed { response, progress }`: the response is sent first, then
        // each pre-built `$/progress` notification in order. Used by the pull
        // diagnostics handlers, which read `GlobalState`'s store directly (a
        // cheap map lookup) rather than a pooled `StateSnapshot`, and stream the
        // remainder when the client supplies a `partialResultToken` (an absent
        // token leaves `progress` empty, so the whole report rides in the
        // response).
        macro_rules! inline_streaming {
            ($R:ty, $handler:expr) => {
                req = match req.extract::<<$R as r::Request>::Params>(<$R>::METHOD) {
                    Ok((id, params)) => {
                        let handlers::diagnostics::Streamed { response, progress } =
                            $handler(self, params);
                        self.respond(Response::new_ok(id, response));
                        for note in progress {
                            self.sender.send(lsp_server::Message::Notification(note));
                        }
                        return;
                    }
                    Err(ExtractError::MethodMismatch(req)) => req,
                    Err(ExtractError::JsonError { method, error }) => {
                        log::error!("invalid params for {method}: {error}");
                        return;
                    }
                };
            };
        }

        inline_streaming!(
            r::DocumentDiagnosticRequest,
            handlers::diagnostics::document_diagnostic
        );
        inline_streaming!(
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
        pool!(
            r::OnTypeFormatting,
            handlers::formatting::format_on_type,
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
            r::SemanticTokensFullRequest,
            handlers::semantic_tokens::semantic_tokens_full
        );
        pool!(
            r::GotoDefinition,
            handlers::goto_definition::goto_definition
        );
        pool!(r::HoverRequest, handlers::hover::hover);
        pool!(r::Completion, handlers::completion::completion);
        pool!(
            r::ResolveCompletionItem,
            handlers::completion::completion_item_resolve
        );
        pool!(r::Rename, handlers::rename::rename);
        pool!(
            r::PrepareRenameRequest,
            handlers::prepare_rename::prepare_rename
        );
        pool!(r::References, handlers::references::references);
        pool!(
            r::DocumentHighlightRequest,
            handlers::document_highlight::document_highlight
        );
        pool!(
            r::LinkedEditingRange,
            handlers::linked_editing_range::linked_editing_range
        );
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
            n::DidChangeConfiguration,
            handlers::configuration::did_change_configuration
        );
        handle!(
            n::DidChangeWatchedFiles,
            handlers::file_watcher::did_change_watched_files
        );
        handle!(
            n::DidCreateFiles,
            handlers::file_operations::did_create_files
        );
        handle!(
            n::DidRenameFiles,
            handlers::file_operations::did_rename_files
        );
        handle!(
            n::DidDeleteFiles,
            handlers::file_operations::did_delete_files
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
                publishes,
                external_ran,
            } => {
                // Drop a pass superseded by a newer settle.
                if generation != self.lint_generation {
                    return;
                }
                // Record that this generation's pass has landed, so a waiter
                // (the test harness's `pump`) can tell the settle is no longer in
                // flight.
                self.last_applied_lint_generation = generation;
                // Hand the complete merged set to the collection: it upserts the
                // URIs whose diagnostics changed, leaves unchanged ones untouched
                // (no redundant push, stable pull `result_id`), and clears every
                // URI the previous settle held but this one omits — clear-on-fix
                // for fixed manifests, closed docs, and resolved cross-file
                // diagnostics alike. A shared `_quarto.yml` stays flagged as long
                // as any open doc still reports it.
                self.diagnostics
                    .apply(publishes, &self.sender, self.supports_pull_diagnostics);
                // Retire exactly the externals this pass ran (a save queued after
                // dispatch stays pending for the next settle).
                self.external_pending
                    .retain(|uri| !external_ran.contains(uri));
                // One coalesced nudge after the whole batch (pull + refresh only; a
                // no-op for push clients).
                self.send_diagnostic_refresh();
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

    /// Spawn one settle pass that re-lints **every** open document over a single
    /// snapshot, tagged with `generation`. `external` is the set of URIs to also
    /// run external linters for (built-ins run for all docs regardless).
    ///
    /// One job, not a fan-out: a fan-out of N jobs would each clone a separate
    /// salsa handle that does not share one revision atomically, so a write
    /// landing mid-fan-out could cancel some jobs and not others — reintroducing
    /// the split-generation problem this model deletes. One job makes a concurrent
    /// write cancel the whole pass atomically; the partial work is discarded and
    /// the cancelling write has already armed the next settle, so nothing needs
    /// re-arming.
    pub(crate) fn spawn_settle_pass(
        &mut self,
        generation: u64,
        external: std::collections::HashSet<Uri>,
    ) {
        let snap = self.snapshot();
        let sender = self.pool.result_sender();
        let uris: Vec<Uri> = self
            .document_map
            .keys()
            .filter_map(|key| key.parse::<Uri>().ok())
            .collect();
        self.pool.spawn(move || {
            let result = catch_cancelled(|| {
                // Merge every document's publishes by URI. A project-graph
                // diagnostic for a shared path is accumulated once per project
                // document, so the same `Diagnostic` value arrives from several
                // passes; dedupe by value (a document's *own* built-in diagnostics
                // are distinct and so survive). A manifest error reachable from
                // several docs collapses the same way.
                let mut merged: std::collections::HashMap<Uri, Vec<lsp_types::Diagnostic>> =
                    std::collections::HashMap::new();
                for uri in &uris {
                    let run_external = external.contains(uri);
                    let mut publishes =
                        handlers::diagnostics::compute_publishes(&snap, uri, run_external);
                    let (manifest_pubs, _manifest_uris) =
                        handlers::diagnostics::manifest_publishes(&snap, uri);
                    publishes.extend(manifest_pubs);
                    // A broken discovered `panache.toml` surfaces as a diagnostic
                    // on its own file (clear-on-fix via the omitted-URI diff).
                    publishes.extend(handlers::diagnostics::config_publishes(&snap, uri));
                    for (target, _version, diags) in publishes {
                        let slot = merged.entry(target).or_default();
                        for diag in diags {
                            if !slot.contains(&diag) {
                                slot.push(diag);
                            }
                        }
                    }
                }
                merged
                    .into_iter()
                    .map(|(uri, mut diags)| {
                        diags.sort_by_key(|d| (d.range.start.line, d.range.start.character));
                        (uri, None, diags)
                    })
                    .collect::<Vec<_>>()
            });
            // A concurrent write cancels the pass (`result` is `None`); drop it.
            // That write already armed the next settle, which re-lints everything.
            if let Some(publishes) = result {
                let _ = sender.send(Task::Diagnostics {
                    generation,
                    publishes,
                    external_ran: external,
                });
            }
        });
    }

    /// Time until the workspace settle deadline, for the main-loop `select!`.
    pub(crate) fn next_lint_timeout(&self) -> Option<Duration> {
        self.settle_deadline
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
    }

    /// If the workspace settle deadline has elapsed, re-lint every open document.
    ///
    /// Two phases on purpose: **first** apply every open document's writes (load
    /// newly-referenced includes/bibliographies), **then** snapshot and spawn the
    /// single read pass. Loading after the snapshot would cancel the pass, so the
    /// write phase is batched ahead of it — the pass reads a settled database.
    //
    // Backstop hook: for pathological sessions with more open docs than the salsa
    // lru (512), a future `MAX_DOCS_PER_SETTLE` cap could lint a prioritized
    // subset (saved + recently-changed) per settle and re-arm for the remainder.
    // The raised lru removes the cliff for realistic sessions, so it is unused.
    pub(crate) fn dispatch_due_lints(&mut self) {
        let Some(deadline) = self.settle_deadline else {
            return;
        };
        if deadline > Instant::now() {
            return;
        }
        self.settle_deadline = None;

        // Write phase: load every open document's referenced files first. A
        // keystroke burst may have added an include/bibliography since the last
        // pass; `file_text` no longer lazy-loads (audit §3.2), so the writer loads
        // them here, coalesced onto the settle boundary.
        documents::reload_open_documents_referenced_files(self);

        // Read phase: snapshot and spawn the single all-docs pass over the
        // now-settled database under a fresh generation.
        self.lint_generation += 1;
        let generation = self.lint_generation;
        let external = self.external_pending.clone();
        self.spawn_settle_pass(generation, external);
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

    fn uri(s: &str) -> Uri {
        s.parse().expect("valid uri")
    }

    /// A current-generation settle result is applied: the published URI is stored,
    /// a URI the previous settle reported but this one omits is cleared, and
    /// exactly the externals this pass ran are retired — an external queued after
    /// dispatch (here `b`) survives for the next settle.
    #[test]
    fn settle_result_retires_external_and_clears_stale_uris() {
        let (tx, _client_rx) = crossbeam_channel::unbounded();
        let mut gs = GlobalState::new(ClientSender::new(tx));
        gs.supports_pull_diagnostics = true;
        let (a, b, x) = (
            uri("file:///a.qmd"),
            uri("file:///b.qmd"),
            uri("file:///x.qmd"),
        );
        // Seed the collection with `x` from a prior settle.
        gs.lint_generation = 5;
        gs.diagnostics
            .apply(vec![(x.clone(), None, Vec::new())], &gs.sender, true);
        gs.external_pending = [a.clone(), b.clone()].into_iter().collect();

        gs.on_task(Task::Diagnostics {
            generation: 5,
            publishes: vec![(a.clone(), None, Vec::new())],
            external_ran: [a.clone()].into_iter().collect(),
        });

        assert!(!gs.external_pending.contains(&a), "ran external retired");
        assert!(
            gs.external_pending.contains(&b),
            "external queued after dispatch must survive"
        );
        assert!(gs.diagnostics.get(&a).is_some(), "published `a` stored");
        assert!(
            gs.diagnostics.get(&x).is_none(),
            "omitted `x` cleared from the collection"
        );
    }

    /// A settle result tagged with a superseded generation is dropped wholesale:
    /// no delivery, no clear, no external retirement.
    #[test]
    fn stale_settle_result_is_dropped() {
        let (tx, _client_rx) = crossbeam_channel::unbounded();
        let mut gs = GlobalState::new(ClientSender::new(tx));
        gs.supports_pull_diagnostics = true;
        let (a, x) = (uri("file:///a.qmd"), uri("file:///x.qmd"));
        gs.lint_generation = 9;
        gs.diagnostics
            .apply(vec![(x.clone(), None, Vec::new())], &gs.sender, true);
        gs.external_pending = [a.clone()].into_iter().collect();

        gs.on_task(Task::Diagnostics {
            generation: 7,
            publishes: vec![(a.clone(), None, Vec::new())],
            external_ran: [a.clone()].into_iter().collect(),
        });

        assert_eq!(gs.external_pending, [a.clone()].into_iter().collect());
        assert!(gs.diagnostics.get(&a).is_none(), "stale pass not applied");
        assert!(gs.diagnostics.get(&x).is_some(), "prior `x` untouched");
    }
}
