use std::sync::Arc;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use super::{PanacheLsp, documents, handlers};

impl LanguageServer for PanacheLsp {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Store workspace root for config discovery
        // Try workspace_folders first, fall back to deprecated root_uri
        if let Some(folders) = params.workspace_folders
            && let Some(folder) = folders.first()
            && let Some(path) = folder.uri.to_file_path()
        {
            *self.workspace_root.lock().await = Some(path.into_owned());
        } else {
            #[allow(deprecated)]
            if let Some(root_uri) = params.root_uri
                && let Some(path) = root_uri.to_file_path()
            {
                *self.workspace_root.lock().await = Some(path.into_owned());
            }
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::INCREMENTAL),
                        ..Default::default()
                    },
                )),
                document_formatting_provider: Some(OneOf::Left(true)),
                document_range_formatting_provider: Some(OneOf::Left(true)),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "panache-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "panache LSP server initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        documents::did_open(
            &self.client,
            Arc::clone(&self.document_map),
            Arc::clone(&self.workspace_root),
            params,
        )
        .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        documents::did_change(
            Arc::clone(&self.document_map),
            Arc::clone(&self.workspace_root),
            &self.client,
            params,
        )
        .await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        documents::did_close(&self.client, Arc::clone(&self.document_map), params).await;
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        handlers::formatting::format_document(
            &self.client,
            Arc::clone(&self.document_map),
            Arc::clone(&self.workspace_root),
            params,
        )
        .await
    }

    async fn range_formatting(
        &self,
        params: DocumentRangeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        handlers::formatting::format_range(
            &self.client,
            Arc::clone(&self.document_map),
            Arc::clone(&self.workspace_root),
            params,
        )
        .await
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        handlers::code_actions::code_action(
            &self.client,
            Arc::clone(&self.document_map),
            Arc::clone(&self.workspace_root),
            params,
        )
        .await
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        handlers::document_symbols::document_symbol(
            &self.client,
            Arc::clone(&self.document_map),
            Arc::clone(&self.workspace_root),
            params,
        )
        .await
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        handlers::folding_ranges::folding_range(
            &self.client,
            Arc::clone(&self.document_map),
            params,
        )
        .await
    }
}
