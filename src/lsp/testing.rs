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
            .filter(|params| params.uri == target)
            .collect()
    }

    /// Force any pending lint deadlines to be due, dispatch them onto the
    /// pool, then drain completed tasks through `on_task` until idle or
    /// `timeout` elapses. Used by tests to deterministically exercise the
    /// debounced publish path.
    pub fn pump(&mut self, timeout: Duration) {
        let now = Instant::now();
        for deadline in self.gs.lint_deadlines.values_mut() {
            *deadline = now;
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
                    if self.gs.lint_deadlines.is_empty() {
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
