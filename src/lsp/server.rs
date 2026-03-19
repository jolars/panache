use std::sync::Arc;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use super::{PanacheLsp, documents, handlers};

fn watched_document_glob() -> Vec<FileSystemWatcher> {
    crate::all_document_extensions()
        .iter()
        .map(|ext| FileSystemWatcher {
            glob_pattern: GlobPattern::String(format!("**/*.{ext}")),
            kind: Some(WatchKind::all()),
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

impl LanguageServer for PanacheLsp {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Store workspace root for config discovery
        // Try workspace_folders first, then legacy rootUri fallback.
        if let Some(folders) = params.workspace_folders.as_ref()
            && let Some(folder) = folders.first()
            && let Some(path) = folder.uri.to_file_path()
        {
            *self.workspace_root.lock().await = Some(path.into_owned());
        } else if let Some(root_uri) = legacy_root_uri(&params)
            && let Some(path) = root_uri.to_file_path()
        {
            *self.workspace_root.lock().await = Some(path.into_owned());
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
                definition_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions::default()),
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
                    file_operations: None,
                }),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "panache-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "panache LSP server initialized")
            .await;
        log::debug!("initialized LSP server");

        // Register file watchers for bibliography files
        if let Ok(options) = serde_json::to_value(DidChangeWatchedFilesRegistrationOptions {
            watchers: {
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
                watchers
            },
        }) {
            let registrations = vec![Registration {
                id: "watch-bibliography-files".to_string(),
                method: "workspace/didChangeWatchedFiles".to_string(),
                register_options: Some(options),
            }];

            if let Err(e) = self.client.register_capability(registrations).await {
                self.client
                    .log_message(
                        MessageType::WARNING,
                        format!("Failed to register file watchers: {:?}", e),
                    )
                    .await;
            }
        }
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        documents::did_open(
            &self.client,
            Arc::clone(&self.document_map),
            Arc::clone(&self.workspace_root),
            Arc::clone(&self.salsa_db),
            params,
        )
        .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        documents::did_change(
            Arc::clone(&self.document_map),
            Arc::clone(&self.workspace_root),
            Arc::clone(&self.salsa_db),
            &self.client,
            params,
        )
        .await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        documents::did_close(
            &self.client,
            Arc::clone(&self.document_map),
            Arc::clone(&self.salsa_db),
            params,
        )
        .await;
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        handlers::formatting::format_document(
            &self.client,
            Arc::clone(&self.document_map),
            Arc::clone(&self.salsa_db),
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
            Arc::clone(&self.salsa_db),
            Arc::clone(&self.workspace_root),
            params,
        )
        .await
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        handlers::code_actions::code_action(
            &self.client,
            Arc::clone(&self.document_map),
            Arc::clone(&self.salsa_db),
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
            Arc::clone(&self.salsa_db),
            Arc::clone(&self.workspace_root),
            params,
        )
        .await
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        handlers::folding_ranges::folding_range(
            &self.client,
            Arc::clone(&self.document_map),
            Arc::clone(&self.salsa_db),
            Arc::clone(&self.workspace_root),
            params,
        )
        .await
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        handlers::goto_definition::goto_definition(
            &self.client,
            Arc::clone(&self.document_map),
            Arc::clone(&self.salsa_db),
            Arc::clone(&self.workspace_root),
            params,
        )
        .await
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        handlers::hover::hover(
            &self.client,
            Arc::clone(&self.document_map),
            Arc::clone(&self.salsa_db),
            Arc::clone(&self.workspace_root),
            params,
        )
        .await
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        handlers::completion::completion(
            &self.client,
            Arc::clone(&self.document_map),
            Arc::clone(&self.salsa_db),
            Arc::clone(&self.workspace_root),
            params,
        )
        .await
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        handlers::rename::rename(
            &self.client,
            Arc::clone(&self.document_map),
            Arc::clone(&self.salsa_db),
            Arc::clone(&self.workspace_root),
            params,
        )
        .await
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        handlers::prepare_rename::prepare_rename(
            &self.client,
            Arc::clone(&self.document_map),
            Arc::clone(&self.salsa_db),
            Arc::clone(&self.workspace_root),
            params,
        )
        .await
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        handlers::references::references(
            &self.client,
            Arc::clone(&self.document_map),
            Arc::clone(&self.salsa_db),
            Arc::clone(&self.workspace_root),
            params,
        )
        .await
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<WorkspaceSymbolResponse>> {
        handlers::workspace_symbols::workspace_symbol(
            &self.client,
            Arc::clone(&self.document_map),
            Arc::clone(&self.salsa_db),
            Arc::clone(&self.workspace_root),
            params,
        )
        .await
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        handlers::file_watcher::did_change_watched_files(
            &self.client,
            Arc::clone(&self.document_map),
            Arc::clone(&self.salsa_db),
            Arc::clone(&self.workspace_root),
            params,
        )
        .await;
    }
}
