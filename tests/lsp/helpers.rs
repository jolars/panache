//! Test helpers for LSP integration testing
//!
//! This module provides utilities to test LSP functionality in-memory
//! without spawning the binary or dealing with stdio protocol.

use std::path::PathBuf;
use std::sync::Arc;
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{LanguageServer, LspService};

use panache::lsp::PanacheLsp;
use panache::salsa::Db;

/// Test harness for LSP integration tests.
///
/// Wraps a `PanacheLsp` instance created via `LspService::new`.
/// Provides helper methods for common LSP operations.
pub struct TestLspServer {
    lsp: Arc<PanacheLsp>,
}

impl TestLspServer {
    /// Create a new test LSP server.
    ///
    /// This creates a real `PanacheLsp` instance with a real `Client`,
    /// using the same `LspService::new` pattern as production code.
    pub fn new() -> Self {
        // Use Arc to share ownership between the closure and our return value
        let lsp_arc: Arc<std::sync::Mutex<Option<Arc<PanacheLsp>>>> =
            Arc::new(std::sync::Mutex::new(None));
        let lsp_arc_clone = Arc::clone(&lsp_arc);

        let (_service, _socket) = LspService::new(move |client| {
            let lsp = Arc::new(PanacheLsp::new(client));
            *lsp_arc_clone.lock().unwrap() = Some(Arc::clone(&lsp));

            // Return the Arc wrapped in a struct that implements LanguageServer
            LspWrapper { inner: lsp }
        });

        // Extract the PanacheLsp Arc
        let lsp = lsp_arc
            .lock()
            .unwrap()
            .take()
            .expect("PanacheLsp should have been initialized");

        Self { lsp }
    }

    /// Initialize the server with a workspace root.
    pub async fn initialize(&self, root_uri: &str) {
        let folder = WorkspaceFolder {
            uri: root_uri.parse().unwrap(),
            name: "workspace".to_string(),
        };
        let params = InitializeParams {
            workspace_folders: Some(vec![folder]),
            ..Default::default()
        };
        let _ = self.lsp.initialize(params).await;
    }

    /// Open a document with the given URI and content.
    ///
    /// Simulates the `textDocument/didOpen` notification.
    pub async fn open_document(&self, uri: &str, content: &str, language_id: &str) {
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.parse().unwrap(),
                language_id: language_id.to_string(),
                version: 0,
                text: content.to_string(),
            },
        };

        self.lsp.did_open(params).await;
    }

    /// Close a document.
    ///
    /// Simulates the `textDocument/didClose` notification.
    pub async fn close_document(&self, uri: &str) {
        let params = DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
        };

        self.lsp.did_close(params).await;
    }

    /// Edit a document with incremental changes.
    ///
    /// Simulates the `textDocument/didChange` notification with INCREMENTAL sync.
    pub async fn edit_document(&self, uri: &str, changes: Vec<TextDocumentContentChangeEvent>) {
        let params = DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.parse().unwrap(),
                version: 1,
            },
            content_changes: changes,
        };

        self.lsp.did_change(params).await;
    }

    /// Format a document.
    ///
    /// Simulates the `textDocument/formatting` request.
    /// Returns the list of text edits (or None if no formatting needed).
    pub async fn format_document(&self, uri: &str) -> Option<Vec<TextEdit>> {
        let params = DocumentFormattingParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            options: FormattingOptions {
                tab_size: 2,
                insert_spaces: true,
                ..Default::default()
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        self.lsp.formatting(params).await.unwrap()
    }

    /// Format a document range.
    ///
    /// Simulates the `textDocument/rangeFormatting` request.
    pub async fn format_range(
        &self,
        uri: &str,
        start_line: u32,
        start_char: u32,
        end_line: u32,
        end_char: u32,
    ) -> Option<Vec<TextEdit>> {
        let params = DocumentRangeFormattingParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range {
                start: Position {
                    line: start_line,
                    character: start_char,
                },
                end: Position {
                    line: end_line,
                    character: end_char,
                },
            },
            options: FormattingOptions {
                tab_size: 2,
                insert_spaces: true,
                ..Default::default()
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        self.lsp.range_formatting(params).await.unwrap()
    }

    /// Get document symbols.
    ///
    /// Simulates the `textDocument/documentSymbol` request.
    pub async fn get_symbols(&self, uri: &str) -> Option<DocumentSymbolResponse> {
        let params = DocumentSymbolParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        self.lsp.document_symbol(params).await.unwrap()
    }

    /// Get workspace symbols for a query.
    ///
    /// Simulates the `workspace/symbol` request.
    pub async fn get_workspace_symbols(&self, query: &str) -> Option<Vec<SymbolInformation>> {
        let params = WorkspaceSymbolParams {
            query: query.to_string(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        match self.lsp.symbol(params).await.unwrap() {
            Some(WorkspaceSymbolResponse::Flat(symbols)) => Some(symbols),
            Some(WorkspaceSymbolResponse::Nested(_)) => {
                panic!("Expected flat workspace symbols response")
            }
            None => None,
        }
    }

    /// Get code actions for a range.
    ///
    /// Simulates the `textDocument/codeAction` request.
    pub async fn get_code_actions(
        &self,
        uri: &str,
        start_line: u32,
        start_char: u32,
        end_line: u32,
        end_char: u32,
    ) -> Option<CodeActionResponse> {
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range {
                start: Position {
                    line: start_line,
                    character: start_char,
                },
                end: Position {
                    line: end_line,
                    character: end_char,
                },
            },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        self.lsp.code_action(params).await.unwrap()
    }

    /// Get folding ranges.
    ///
    /// Simulates the `textDocument/foldingRange` request.
    pub async fn get_folding_ranges(&self, uri: &str) -> Option<Vec<FoldingRange>> {
        let params = FoldingRangeParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        self.lsp.folding_range(params).await.unwrap()
    }

    /// Get the current content of a document from the server's state.
    ///
    /// This is a test-only method to inspect internal state.
    pub async fn get_document_content(&self, uri: &str) -> Option<String> {
        let doc_map = self.lsp.document_map();
        let state = {
            let docs = doc_map.lock().await;
            docs.get(uri).cloned()
        };
        let state = state?;
        let db = self.lsp.salsa_db();
        let db = db.lock().await;
        Some(state.salsa_file.text(&*db).clone())
    }

    /// Get a cached file's text from the salsa db (test-only).
    pub async fn get_cached_file_text(&self, path: &std::path::Path) -> Option<String> {
        let db = self.lsp.salsa_db();
        let db = db.lock().await;
        let file = db.file_text(path.to_path_buf())?;
        Some(file.text(&*db).clone())
    }

    /// Get built-in lint diagnostics for a document from Salsa (test-only).
    pub async fn get_built_in_diagnostics(
        &self,
        uri: &str,
    ) -> Option<Vec<panache::linter::diagnostics::Diagnostic>> {
        let parsed_uri: Uri = uri.parse().ok()?;
        let state = {
            let doc_map = self.lsp.document_map();
            let docs = doc_map.lock().await;
            docs.get(uri).cloned()
        }?;

        let path = state
            .path
            .clone()
            .or_else(|| parsed_uri.to_file_path().map(|p| p.into_owned()))
            .unwrap_or_else(|| PathBuf::from("<memory>"));

        let db = self.lsp.salsa_db();
        let db = db.lock().await;
        let plan =
            panache::salsa::built_in_lint_plan(&*db, state.salsa_file, state.salsa_config, path)
                .clone();
        Some(plan.diagnostics)
    }

    /// Trigger the file watcher handler (test-only).
    pub async fn did_change_watched_files(&self, files: Vec<FileEvent>) {
        let params = DidChangeWatchedFilesParams { changes: files };
        self.lsp.did_change_watched_files(params).await;
    }

    /// Get the current syntax tree for a document (test-only).
    pub async fn get_document_tree(&self, uri: &str) -> Option<panache::SyntaxNode> {
        let doc_map = self.lsp.document_map();
        let docs = doc_map.lock().await;
        docs.get(uri)
            .map(|state| panache::SyntaxNode::new_root(state.tree.clone()))
    }

    /// Go to definition at a specific position.
    ///
    /// Simulates the `textDocument/definition` request.
    pub async fn goto_definition(
        &self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Option<GotoDefinitionResponse> {
        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: uri.parse().unwrap(),
                },
                position: Position { line, character },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        self.lsp.goto_definition(params).await.unwrap()
    }

    /// Find references at a specific position.
    pub async fn references(
        &self,
        uri: &str,
        line: u32,
        character: u32,
        include_declaration: bool,
    ) -> Option<Vec<Location>> {
        let params = ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: uri.parse().unwrap(),
                },
                position: Position { line, character },
            },
            context: ReferenceContext {
                include_declaration,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        self.lsp.references(params).await.unwrap()
    }

    /// Get hover information at a specific position.
    ///
    /// Simulates the `textDocument/hover` request.
    pub async fn hover(&self, uri: &str, line: u32, character: u32) -> Option<Hover> {
        let params = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: uri.parse().unwrap(),
                },
                position: Position { line, character },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        self.lsp.hover(params).await.unwrap()
    }

    /// Get completion items at a specific position.
    ///
    /// Simulates the `textDocument/completion` request.
    pub async fn completion(
        &self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Option<CompletionResponse> {
        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: uri.parse().unwrap(),
                },
                position: Position { line, character },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        };

        self.lsp.completion(params).await.unwrap()
    }

    /// Rename symbol at a specific position.
    pub async fn rename(
        &self,
        uri: &str,
        line: u32,
        character: u32,
        new_name: &str,
    ) -> Option<WorkspaceEdit> {
        let params = RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: uri.parse().unwrap(),
                },
                position: Position { line, character },
            },
            new_name: new_name.to_string(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        self.lsp.rename(params).await.unwrap()
    }

    /// Prepare rename at a specific position.
    pub async fn prepare_rename(
        &self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Option<PrepareRenameResponse> {
        let params = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            position: Position { line, character },
        };

        self.lsp.prepare_rename(params).await.unwrap()
    }
}

/// Wrapper that delegates all LanguageServer methods to the inner Arc<PanacheLsp>.
///
/// This is needed because LspService requires ownership of the LanguageServer impl,
/// but we also need to retain a reference for testing.
struct LspWrapper {
    inner: Arc<PanacheLsp>,
}

// Delegate all LanguageServer methods to the inner Arc<PanacheLsp>
impl LanguageServer for LspWrapper {
    async fn initialize(
        &self,
        params: InitializeParams,
    ) -> tower_lsp_server::jsonrpc::Result<InitializeResult> {
        self.inner.initialize(params).await
    }

    async fn initialized(&self, params: InitializedParams) {
        self.inner.initialized(params).await
    }

    async fn shutdown(&self) -> tower_lsp_server::jsonrpc::Result<()> {
        self.inner.shutdown().await
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.inner.did_open(params).await
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        self.inner.did_change(params).await
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.inner.did_close(params).await
    }

    async fn formatting(
        &self,
        params: DocumentFormattingParams,
    ) -> tower_lsp_server::jsonrpc::Result<Option<Vec<TextEdit>>> {
        self.inner.formatting(params).await
    }

    async fn range_formatting(
        &self,
        params: DocumentRangeFormattingParams,
    ) -> tower_lsp_server::jsonrpc::Result<Option<Vec<TextEdit>>> {
        self.inner.range_formatting(params).await
    }

    async fn code_action(
        &self,
        params: CodeActionParams,
    ) -> tower_lsp_server::jsonrpc::Result<Option<CodeActionResponse>> {
        self.inner.code_action(params).await
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> tower_lsp_server::jsonrpc::Result<Option<DocumentSymbolResponse>> {
        self.inner.document_symbol(params).await
    }

    async fn folding_range(
        &self,
        params: FoldingRangeParams,
    ) -> tower_lsp_server::jsonrpc::Result<Option<Vec<FoldingRange>>> {
        self.inner.folding_range(params).await
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> tower_lsp_server::jsonrpc::Result<Option<GotoDefinitionResponse>> {
        self.inner.goto_definition(params).await
    }

    async fn references(
        &self,
        params: ReferenceParams,
    ) -> tower_lsp_server::jsonrpc::Result<Option<Vec<Location>>> {
        self.inner.references(params).await
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> tower_lsp_server::jsonrpc::Result<Option<WorkspaceSymbolResponse>> {
        self.inner.symbol(params).await
    }

    async fn hover(&self, params: HoverParams) -> tower_lsp_server::jsonrpc::Result<Option<Hover>> {
        self.inner.hover(params).await
    }

    async fn completion(
        &self,
        params: CompletionParams,
    ) -> tower_lsp_server::jsonrpc::Result<Option<CompletionResponse>> {
        self.inner.completion(params).await
    }

    async fn rename(
        &self,
        params: RenameParams,
    ) -> tower_lsp_server::jsonrpc::Result<Option<WorkspaceEdit>> {
        self.inner.rename(params).await
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> tower_lsp_server::jsonrpc::Result<Option<PrepareRenameResponse>> {
        self.inner.prepare_rename(params).await
    }
}

/// Helper to create a simple text change event (full document replacement).
pub fn full_document_change(text: &str) -> TextDocumentContentChangeEvent {
    TextDocumentContentChangeEvent {
        range: None,
        range_length: None,
        text: text.to_string(),
    }
}

/// Helper to create an incremental text change event.
pub fn incremental_change(
    start_line: u32,
    start_char: u32,
    end_line: u32,
    end_char: u32,
    text: &str,
) -> TextDocumentContentChangeEvent {
    TextDocumentContentChangeEvent {
        range: Some(Range {
            start: Position {
                line: start_line,
                character: start_char,
            },
            end: Position {
                line: end_line,
                character: end_char,
            },
        }),
        range_length: None,
        text: text.to_string(),
    }
}
