//! In-process test harness for LSP integration tests.
//!
//! Drives the synchronous handlers directly over a [`GlobalState`] — no JSON-RPC
//! loop, no threads, no async. Notifications mutate the state inline; requests
//! run against a [`StateSnapshot`]. This is `#[doc(hidden)]` and exists only so
//! the external `tests/lsp` suite can exercise realistic flows.

use std::time::{Duration, Instant};

use lsp_types::notification::Notification as _;
use lsp_types::request::Request as _;
use lsp_types::*;

use super::dispatch::server_capabilities;
use super::global_state::{ClientSender, GlobalState};
use super::{documents, handlers};
use crate::salsa::Db;

/// Decode the `value` of each streamed `$/progress` notification, asserting every
/// one carries the `$/progress` method and the expected partial-result `token`.
fn decode_progress<T: serde::de::DeserializeOwned>(
    notifications: Vec<lsp_server::Notification>,
    token: i32,
) -> Vec<T> {
    notifications
        .into_iter()
        .map(|note| {
            assert_eq!(note.method, "$/progress", "expected a $/progress chunk");
            let envelope: serde_json::Value = note.params;
            assert_eq!(
                envelope.get("token"),
                Some(&serde_json::json!(token)),
                "progress chunk carried the wrong token"
            );
            serde_json::from_value(envelope.get("value").expect("progress value").clone())
                .expect("partial-result value deserializes")
        })
        .collect()
}

/// A synchronous, in-memory driver for the LSP handlers.
pub struct LspTester {
    gs: GlobalState,
    client_rx: crossbeam_channel::Receiver<lsp_server::Message>,
}

impl Default for LspTester {
    fn default() -> Self {
        Self::new()
    }
}

impl LspTester {
    pub fn new() -> Self {
        let (tx, client_rx) = crossbeam_channel::unbounded();
        let gs = GlobalState::new(ClientSender::new(tx));
        Self { gs, client_rx }
    }

    fn snapshot(&self) -> super::global_state::StateSnapshot {
        self.gs.snapshot()
    }

    // --- lifecycle ---

    pub fn initialize(&mut self, root_uri: &str) {
        self.initialize_with_options(root_uri, None);
    }

    /// Initialize advertising pull-diagnostics client support (and refresh), so
    /// the server switches into pull mode (push suppressed).
    pub fn initialize_pull(&mut self, root_uri: &str) {
        let folder = WorkspaceFolder {
            uri: root_uri.parse().unwrap(),
            name: "workspace".to_string(),
        };
        let params = InitializeParams {
            workspace_folders: Some(vec![folder]),
            capabilities: ClientCapabilities {
                text_document: Some(TextDocumentClientCapabilities {
                    diagnostic: Some(DiagnosticClientCapabilities {
                        dynamic_registration: None,
                        related_document_support: Some(true),
                    }),
                    ..Default::default()
                }),
                workspace: Some(WorkspaceClientCapabilities {
                    diagnostic: Some(DiagnosticWorkspaceClientCapabilities {
                        refresh_support: Some(true),
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        self.gs.on_initialize(params);
    }

    /// Like [`Self::initialize_pull`] but without advertising
    /// `related_document_support`, so the server serves pull diagnostics while
    /// leaving `related_documents` empty.
    pub fn initialize_pull_no_related(&mut self, root_uri: &str) {
        let folder = WorkspaceFolder {
            uri: root_uri.parse().unwrap(),
            name: "workspace".to_string(),
        };
        let params = InitializeParams {
            workspace_folders: Some(vec![folder]),
            capabilities: ClientCapabilities {
                text_document: Some(TextDocumentClientCapabilities {
                    diagnostic: Some(DiagnosticClientCapabilities {
                        dynamic_registration: None,
                        related_document_support: None,
                    }),
                    ..Default::default()
                }),
                workspace: Some(WorkspaceClientCapabilities {
                    diagnostic: Some(DiagnosticWorkspaceClientCapabilities {
                        refresh_support: Some(true),
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        self.gs.on_initialize(params);
    }

    pub fn initialize_with_options(
        &mut self,
        root_uri: &str,
        initialization_options: Option<serde_json::Value>,
    ) {
        let folder = WorkspaceFolder {
            uri: root_uri.parse().unwrap(),
            name: "workspace".to_string(),
        };
        let params = InitializeParams {
            workspace_folders: Some(vec![folder]),
            initialization_options,
            ..Default::default()
        };
        self.gs.on_initialize(params);
    }

    pub fn initialize_result(&mut self, root_uri: &str) -> InitializeResult {
        self.initialize_result_with_options(root_uri, None)
    }

    pub fn initialize_result_with_options(
        &mut self,
        root_uri: &str,
        initialization_options: Option<serde_json::Value>,
    ) -> InitializeResult {
        self.initialize_with_options(root_uri, initialization_options);
        InitializeResult {
            capabilities: server_capabilities(),
            server_info: Some(ServerInfo {
                name: "panache-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        }
    }

    pub fn experimental_incremental_parsing_enabled(&self) -> bool {
        self.gs.runtime_settings.experimental_incremental_parsing
    }

    pub fn open_document(&mut self, uri: &str, content: &str, language_id: &str) {
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.parse().unwrap(),
                language_id: language_id.to_string(),
                version: 0,
                text: content.to_string(),
            },
        };
        documents::did_open(&mut self.gs, params);
    }

    pub fn close_document(&mut self, uri: &str) {
        let params = DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
        };
        documents::did_close(&mut self.gs, params);
    }

    pub fn edit_document(&mut self, uri: &str, changes: Vec<TextDocumentContentChangeEvent>) {
        let params = DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.parse().unwrap(),
                version: 1,
            },
            content_changes: changes,
        };
        documents::did_change(&mut self.gs, params);
    }

    pub fn save_document(&mut self, uri: &str) {
        let params = DidSaveTextDocumentParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            text: None,
        };
        documents::did_save(&mut self.gs, params);
    }

    pub fn did_change_watched_files(&mut self, files: Vec<FileEvent>) {
        let params = DidChangeWatchedFilesParams { changes: files };
        handlers::file_watcher::did_change_watched_files(&mut self.gs, params);
    }

    pub fn did_change_configuration(&mut self, settings: serde_json::Value) {
        let params = DidChangeConfigurationParams { settings };
        handlers::configuration::did_change_configuration(&mut self.gs, params);
    }

    pub fn did_create_files(&mut self, created: Vec<&str>) {
        let params = CreateFilesParams {
            files: created
                .into_iter()
                .map(|uri| FileCreate {
                    uri: uri.to_string(),
                })
                .collect(),
        };
        handlers::file_operations::did_create_files(&mut self.gs, params);
    }

    pub fn did_delete_files(&mut self, deleted: Vec<&str>) {
        let params = DeleteFilesParams {
            files: deleted
                .into_iter()
                .map(|uri| FileDelete {
                    uri: uri.to_string(),
                })
                .collect(),
        };
        handlers::file_operations::did_delete_files(&mut self.gs, params);
    }

    pub fn did_rename_files(&mut self, renames: Vec<(String, String)>) {
        let params = RenameFilesParams {
            files: renames
                .into_iter()
                .map(|(old_uri, new_uri)| FileRename { old_uri, new_uri })
                .collect(),
        };
        handlers::file_operations::did_rename_files(&mut self.gs, params);
    }

    // --- requests ---

    pub fn format_document(&self, uri: &str) -> Option<Vec<TextEdit>> {
        let params = DocumentFormattingParams {
            text_document: text_doc(uri),
            options: fmt_options(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        handlers::formatting::format_document(&self.snapshot(), params)
    }

    pub fn format_range(
        &self,
        uri: &str,
        start_line: u32,
        start_char: u32,
        end_line: u32,
        end_char: u32,
    ) -> Option<Vec<TextEdit>> {
        let params = DocumentRangeFormattingParams {
            text_document: text_doc(uri),
            range: range(start_line, start_char, end_line, end_char),
            options: fmt_options(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        handlers::formatting::format_range(&self.snapshot(), params)
    }

    pub fn on_type_formatting(
        &self,
        uri: &str,
        line: u32,
        character: u32,
        ch: &str,
    ) -> Option<Vec<TextEdit>> {
        let params = DocumentOnTypeFormattingParams {
            text_document_position: pos_params(uri, line, character),
            ch: ch.to_owned(),
            options: fmt_options(),
        };
        handlers::formatting::format_on_type(&self.snapshot(), params)
    }

    pub fn get_symbols(&self, uri: &str) -> Option<DocumentSymbolResponse> {
        let params = DocumentSymbolParams {
            text_document: text_doc(uri),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        handlers::document_symbols::document_symbol(&self.snapshot(), params)
    }

    pub fn document_links(&self, uri: &str) -> Option<Vec<DocumentLink>> {
        let params = DocumentLinkParams {
            text_document: text_doc(uri),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        handlers::document_links::document_links(&self.snapshot(), params)
    }

    pub fn resolve_document_link(&self, link: DocumentLink) -> DocumentLink {
        handlers::document_links::document_link_resolve(&self.snapshot(), link)
    }

    pub fn get_workspace_symbols(&self, query: &str) -> Option<Vec<WorkspaceSymbolSummary>> {
        let params = WorkspaceSymbolParams {
            query: query.to_string(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        match handlers::workspace_symbols::workspace_symbol(&self.snapshot(), params) {
            Some(WorkspaceSymbolResponse::Flat(symbols)) => Some(
                symbols
                    .into_iter()
                    .map(|symbol| WorkspaceSymbolSummary {
                        name: symbol.name,
                        location: symbol.location,
                    })
                    .collect(),
            ),
            Some(WorkspaceSymbolResponse::Nested(symbols)) => Some(
                symbols
                    .into_iter()
                    .filter_map(|symbol| {
                        let location = match symbol.location {
                            OneOf::Left(location) => location,
                            OneOf::Right(_) => return None,
                        };
                        Some(WorkspaceSymbolSummary {
                            name: symbol.name,
                            location,
                        })
                    })
                    .collect(),
            ),
            None => None,
        }
    }

    pub fn get_code_actions(
        &self,
        uri: &str,
        start_line: u32,
        start_char: u32,
        end_line: u32,
        end_char: u32,
    ) -> Option<CodeActionResponse> {
        let params = CodeActionParams {
            text_document: text_doc(uri),
            range: range(start_line, start_char, end_line, end_char),
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        handlers::code_actions::code_action(&self.snapshot(), params)
    }

    pub fn get_folding_ranges(&self, uri: &str) -> Option<Vec<FoldingRange>> {
        let params = FoldingRangeParams {
            text_document: text_doc(uri),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        handlers::folding_ranges::folding_range(&self.snapshot(), params)
    }

    pub fn semantic_tokens_full(&self, uri: &str) -> Option<SemanticTokensResult> {
        let params = SemanticTokensParams {
            text_document: text_doc(uri),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        handlers::semantic_tokens::semantic_tokens_full(&self.snapshot(), params)
    }

    pub fn goto_definition(
        &self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Option<GotoDefinitionResponse> {
        let params = GotoDefinitionParams {
            text_document_position_params: pos_params(uri, line, character),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        handlers::goto_definition::goto_definition(&self.snapshot(), params)
    }

    pub fn references(
        &self,
        uri: &str,
        line: u32,
        character: u32,
        include_declaration: bool,
    ) -> Option<Vec<Location>> {
        let params = ReferenceParams {
            text_document_position: pos_params(uri, line, character),
            context: ReferenceContext {
                include_declaration,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        handlers::references::references(&self.snapshot(), params)
    }

    pub fn linked_editing_range(
        &self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Option<LinkedEditingRanges> {
        let params = LinkedEditingRangeParams {
            text_document_position_params: pos_params(uri, line, character),
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        handlers::linked_editing_range::linked_editing_range(&self.snapshot(), params)
    }

    pub fn hover(&self, uri: &str, line: u32, character: u32) -> Option<Hover> {
        let params = HoverParams {
            text_document_position_params: pos_params(uri, line, character),
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        handlers::hover::hover(&self.snapshot(), params)
    }

    pub fn completion(&self, uri: &str, line: u32, character: u32) -> Option<CompletionResponse> {
        let params = CompletionParams {
            text_document_position: pos_params(uri, line, character),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        };
        handlers::completion::completion(&self.snapshot(), params)
    }

    pub fn rename(
        &self,
        uri: &str,
        line: u32,
        character: u32,
        new_name: &str,
    ) -> Option<WorkspaceEdit> {
        let params = RenameParams {
            text_document_position: pos_params(uri, line, character),
            new_name: new_name.to_string(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        handlers::rename::rename(&self.snapshot(), params)
    }

    pub fn prepare_rename(
        &self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Option<PrepareRenameResponse> {
        let params = pos_params(uri, line, character);
        handlers::prepare_rename::prepare_rename(&self.snapshot(), params)
    }

    pub fn will_rename_files(&self, renames: Vec<(String, String)>) -> Option<WorkspaceEdit> {
        let params = RenameFilesParams {
            files: renames
                .into_iter()
                .map(|(old_uri, new_uri)| FileRename { old_uri, new_uri })
                .collect(),
        };
        handlers::file_rename::will_rename_files(&self.snapshot(), params)
    }

    // --- state inspection (test-only) ---

    pub fn get_document_content(&self, uri: &str) -> Option<String> {
        let state = self.gs.document_map.get(uri)?;
        Some(
            state
                .salsa_file
                .content_or_empty(&self.gs.salsa)
                .to_string(),
        )
    }

    pub fn get_document_tree(&self, uri: &str) -> Option<crate::SyntaxNode> {
        self.gs
            .document_map
            .get(uri)
            .map(|state| crate::SyntaxNode::new_root(state.tree.clone()))
    }

    pub fn get_cached_file_text(&self, path: &std::path::Path) -> Option<String> {
        let file = self.gs.salsa.file_text(path.to_path_buf())?;
        Some(file.content_or_empty(&self.gs.salsa).to_string())
    }

    // --- main-loop pumping (publishes + cancel) ---

    /// Drain any messages the server has emitted to the client since the
    /// last call. Non-blocking.
    pub fn drain_client_messages(&self) -> Vec<lsp_server::Message> {
        let mut out = Vec::new();
        while let Ok(msg) = self.client_rx.try_recv() {
            out.push(msg);
        }
        out
    }

    /// Drain `textDocument/publishDiagnostics` notifications scoped to `uri`.
    pub fn drain_publish_diagnostics(&self, uri: &str) -> Vec<PublishDiagnosticsParams> {
        let target: Uri = uri.parse().expect("valid uri");
        self.drain_all_publish_diagnostics()
            .into_iter()
            .filter(|params| params.uri == target)
            .collect()
    }

    /// Every `publishDiagnostics` notification since the last drain, across all
    /// URIs. The per-URI [`Self::drain_publish_diagnostics`] consumes all client
    /// messages, so use this when a single test must inspect publishes for more
    /// than one URI (e.g. a diagnostic present on a manifest yet absent on the
    /// document).
    pub fn drain_all_publish_diagnostics(&self) -> Vec<PublishDiagnosticsParams> {
        self.drain_client_messages()
            .into_iter()
            .filter_map(|msg| match msg {
                lsp_server::Message::Notification(n)
                    if n.method == notification::PublishDiagnostics::METHOD =>
                {
                    serde_json::from_value::<PublishDiagnosticsParams>(n.params).ok()
                }
                _ => None,
            })
            .collect()
    }

    /// Force any pending lint deadlines to be due, dispatch them onto the
    /// pool, then drain completed tasks through `on_task` until idle or
    /// `timeout` elapses. Used by tests to deterministically exercise the
    /// debounced publish path.
    pub fn pump(&mut self, timeout: Duration) {
        // Force any armed settle to be due immediately.
        if self.gs.settle_deadline.is_some() {
            self.gs.settle_deadline = Some(Instant::now());
        }
        self.gs.dispatch_due_lints();

        let receiver = self.gs.task_receiver.clone();
        let end = Instant::now() + timeout;
        loop {
            let remaining = end.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            // Short ceiling so the first lull doesn't strand a slow worker
            // result; we'll retry until the overall `end` is reached.
            let step = remaining.min(Duration::from_millis(50));
            match receiver.recv_timeout(step) {
                Ok(task) => self.gs.on_task(task),
                Err(_) => {
                    // Quiescent only when nothing is due AND every dispatched
                    // settle has landed. A pass dispatched into the pool clears
                    // `settle_deadline` but its result is still in flight; exiting
                    // here would abandon it and lose the batch's diagnostics (a
                    // burst `did_open` over many docs outruns the poll step).
                    let settle_in_flight =
                        self.gs.last_applied_lint_generation != self.gs.lint_generation;
                    if self.gs.settle_deadline.is_none() && !settle_in_flight {
                        break;
                    }
                    self.gs.dispatch_due_lints();
                }
            }
        }
    }

    /// Push a `textDocument/formatting` request through the real dispatcher
    /// so it spawns onto the format pool. Returns the id for use with
    /// [`Self::send_cancel`].
    pub fn send_format_request_raw(&mut self, id: i32, uri: &str) -> lsp_server::RequestId {
        let req_id = lsp_server::RequestId::from(id);
        let params = DocumentFormattingParams {
            text_document: text_doc(uri),
            options: fmt_options(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        let req = lsp_server::Request::new(
            req_id.clone(),
            request::Formatting::METHOD.to_owned(),
            params,
        );
        self.gs.on_request(req);
        req_id
    }

    /// Send a `$/cancelRequest` for the given id through the real dispatcher.
    pub fn send_cancel(&mut self, id: lsp_server::RequestId) {
        // `lsp_server::RequestId` keeps its variants private; serde converts
        // both number and string forms to the LSP `NumberOrString` shape.
        let value = serde_json::to_value(&id).expect("serializable id");
        let cancel_id =
            serde_json::from_value::<NumberOrString>(value).expect("number-or-string id");
        let params = CancelParams { id: cancel_id };
        let not = lsp_server::Notification::new(notification::Cancel::METHOD.to_owned(), params);
        self.gs.on_notification(not);
    }

    pub fn get_built_in_diagnostics(
        &self,
        uri: &str,
    ) -> Option<Vec<crate::linter::diagnostics::Diagnostic>> {
        let state = self.gs.document_map.get(uri)?;
        let plan =
            crate::salsa::built_in_lint_plan(&self.gs.salsa, state.salsa_file, state.salsa_config)
                .clone();
        Some(plan.diagnostics)
    }

    /// Bench helper: re-lint **every open document** over a single snapshot
    /// (built-in only), returning the total publish count so callers can
    /// `black_box` it. This is the per-settle cost of the candidate "re-lint all
    /// open documents per quiescent settle" model (`TODO.md` rust-analyzer
    /// divergence). Unchanged docs should resolve to a `built_in_lint_plan` memo
    /// hit; the residual cost is the un-memoized text clone + `convert_diagnostic`
    /// in [`compute_publishes`](handlers::diagnostics::compute_publishes).
    pub fn relint_all_open_documents(&self) -> usize {
        let snap = self.snapshot();
        let mut total = 0;
        for key in self.gs.document_map.keys() {
            if let Ok(uri) = key.parse::<Uri>() {
                total += handlers::diagnostics::compute_publishes(&snap, &uri, false).len();
            }
        }
        total
    }

    /// Bench helper: the work a single `didChange` does in the current per-doc
    /// model — lint `uri` plus its project-graph dependents, built-in only.
    /// Baseline against which [`Self::relint_all_open_documents`] is compared.
    pub fn relint_with_dependents(&self, uri: &str) -> usize {
        let snap = self.snapshot();
        let Ok(uri) = uri.parse::<Uri>() else {
            return 0;
        };
        handlers::diagnostics::compute_publishes_with_dependents(&snap, &uri, false).len()
    }

    // --- pull diagnostics ---

    /// Whether the server is in pull-diagnostics mode (push suppressed).
    pub fn pull_diagnostics_enabled(&self) -> bool {
        self.gs.supports_pull_diagnostics
    }

    /// Pull `textDocument/diagnostic` for `uri`, optionally with a prior
    /// `result_id` (to exercise `unchanged` reports).
    pub fn document_diagnostic(
        &self,
        uri: &str,
        previous_result_id: Option<&str>,
    ) -> DocumentDiagnosticReportResult {
        let params = DocumentDiagnosticParams {
            text_document: text_doc(uri),
            identifier: None,
            previous_result_id: previous_result_id.map(str::to_string),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        handlers::diagnostics::document_diagnostic(&self.gs, params).response
    }

    /// Pull `textDocument/diagnostic` with a `partialResultToken`. Returns the
    /// response (first chunk) plus the deserialized `value` of each `$/progress`
    /// notification the handler streamed, asserting each carries `$/progress` and
    /// the matching token.
    pub fn document_diagnostic_streaming(
        &self,
        uri: &str,
        token: i32,
        previous_result_id: Option<&str>,
    ) -> (
        DocumentDiagnosticReportResult,
        Vec<DocumentDiagnosticReportPartialResult>,
    ) {
        let params = DocumentDiagnosticParams {
            text_document: text_doc(uri),
            identifier: None,
            previous_result_id: previous_result_id.map(str::to_string),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams {
                partial_result_token: Some(ProgressToken::Number(token)),
            },
        };
        let streamed = handlers::diagnostics::document_diagnostic(&self.gs, params);
        let progress = decode_progress(streamed.progress, token);
        (streamed.response, progress)
    }

    /// Pull `workspace/diagnostic`, optionally with known `(uri, result_id)`
    /// pairs (to exercise `unchanged` reports).
    pub fn workspace_diagnostic(
        &self,
        previous_result_ids: Vec<(&str, &str)>,
    ) -> WorkspaceDiagnosticReportResult {
        let params = WorkspaceDiagnosticParams {
            identifier: None,
            previous_result_ids: previous_result_ids
                .into_iter()
                .map(|(uri, value)| PreviousResultId {
                    uri: uri.parse().unwrap(),
                    value: value.to_string(),
                })
                .collect(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        handlers::diagnostics::workspace_diagnostic(&self.gs, params).response
    }

    /// Pull `workspace/diagnostic` with a `partialResultToken`. Returns the
    /// response (first chunk) plus the deserialized `value` of each `$/progress`
    /// notification the handler streamed.
    pub fn workspace_diagnostic_streaming(
        &self,
        token: i32,
        previous_result_ids: Vec<(&str, &str)>,
    ) -> (
        WorkspaceDiagnosticReportResult,
        Vec<WorkspaceDiagnosticReportPartialResult>,
    ) {
        let params = WorkspaceDiagnosticParams {
            identifier: None,
            previous_result_ids: previous_result_ids
                .into_iter()
                .map(|(uri, value)| PreviousResultId {
                    uri: uri.parse().unwrap(),
                    value: value.to_string(),
                })
                .collect(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams {
                partial_result_token: Some(ProgressToken::Number(token)),
            },
        };
        let streamed = handlers::diagnostics::workspace_diagnostic(&self.gs, params);
        let progress = decode_progress(streamed.progress, token);
        (streamed.response, progress)
    }

    /// Count `workspace/diagnostic/refresh` server→client requests since the last
    /// drain.
    pub fn drain_diagnostic_refresh(&self) -> usize {
        self.drain_client_messages()
            .into_iter()
            .filter(|msg| {
                matches!(
                    msg,
                    lsp_server::Message::Request(req)
                        if req.method == request::WorkspaceDiagnosticRefresh::METHOD
                )
            })
            .count()
    }
}

/// Flattened workspace symbol for assertions.
pub struct WorkspaceSymbolSummary {
    pub name: String,
    pub location: Location,
}

fn text_doc(uri: &str) -> TextDocumentIdentifier {
    TextDocumentIdentifier {
        uri: uri.parse().unwrap(),
    }
}

fn pos_params(uri: &str, line: u32, character: u32) -> TextDocumentPositionParams {
    TextDocumentPositionParams {
        text_document: text_doc(uri),
        position: Position { line, character },
    }
}

fn range(start_line: u32, start_char: u32, end_line: u32, end_char: u32) -> Range {
    Range {
        start: Position {
            line: start_line,
            character: start_char,
        },
        end: Position {
            line: end_line,
            character: end_char,
        },
    }
}

fn fmt_options() -> FormattingOptions {
    FormattingOptions {
        tab_size: 2,
        insert_spaces: true,
        ..Default::default()
    }
}
