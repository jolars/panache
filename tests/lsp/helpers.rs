//! Test helpers for LSP integration testing
//!
//! This module provides utilities to test LSP functionality in-memory
//! without spawning the binary or dealing with stdio protocol.

use std::sync::Arc;
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{LanguageServer, LspService};

use panache::lsp::PanacheLsp;

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
        let docs = doc_map.lock().await;
        docs.get(uri).map(|state| state.text.clone())
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

    async fn hover(&self, params: HoverParams) -> tower_lsp_server::jsonrpc::Result<Option<Hover>> {
        self.inner.hover(params).await
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
