use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{Client, LanguageServer, LspService, Server};

pub struct PanacheLsp {
    client: Client,
    // Use String keys since Uri doesn't implement Send
    document_map: Arc<Mutex<HashMap<String, String>>>,
    workspace_root: Arc<Mutex<Option<PathBuf>>>,
}

impl PanacheLsp {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            document_map: Arc::new(Mutex::new(HashMap::new())),
            workspace_root: Arc::new(Mutex::new(None)),
        }
    }

    async fn load_config(&self) -> crate::Config {
        let workspace_root = self.workspace_root.lock().await;
        if let Some(root) = workspace_root.as_ref() {
            match crate::config::load(None, root, None) {
                Ok((config, path)) => {
                    if let Some(p) = path {
                        self.client
                            .log_message(
                                MessageType::INFO,
                                format!("Loaded config from {}", p.display()),
                            )
                            .await;
                    }
                    return config;
                }
                Err(e) => {
                    self.client
                        .log_message(
                            MessageType::WARNING,
                            format!("Failed to load config: {}", e),
                        )
                        .await;
                }
            }
        }
        crate::Config::default()
    }
}

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
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                document_formatting_provider: Some(OneOf::Left(true)),
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
        let uri = params.text_document.uri.to_string();
        let text = params.text_document.text;

        self.document_map.lock().await.insert(uri.clone(), text);

        self.client
            .log_message(MessageType::INFO, format!("Opened document: {}", uri))
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.to_string();

        // Since we use FULL sync, there's only one content change with the full document
        if let Some(change) = params.content_changes.into_iter().next() {
            self.document_map.lock().await.insert(uri, change.text);
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        self.document_map.lock().await.remove(&uri);
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        let uri_string = uri.to_string();

        self.client
            .log_message(
                MessageType::INFO,
                format!("Formatting request for {}", uri_string),
            )
            .await;

        // Get document content (clone to avoid holding lock across await)
        let text = {
            let document_map = self.document_map.lock().await;
            match document_map.get(&uri_string) {
                Some(t) => t.clone(),
                None => {
                    self.client
                        .log_message(
                            MessageType::ERROR,
                            format!("Document not found: {}", uri_string),
                        )
                        .await;
                    return Ok(None);
                }
            }
        };

        // Run formatting in a blocking task to avoid Send issues with rowan::SyntaxNode
        // Load config first
        let config = self.load_config().await;
        let text_clone = text.clone();
        let formatted = tokio::task::spawn_blocking(move || {
            // Use sync formatter with external formatters
            crate::format(&text_clone, Some(config))
        })
        .await
        .map_err(|_| tower_lsp_server::jsonrpc::Error::internal_error())?;

        // If the content didn't change, return None
        if formatted == text {
            return Ok(None);
        }

        // Calculate the range to replace (entire document)
        let lines: Vec<&str> = text.lines().collect();
        let end_line = lines.len().saturating_sub(1) as u32;
        let end_char = lines.last().map(|l| l.len()).unwrap_or(0) as u32;

        let range = Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: end_line,
                character: end_char,
            },
        };

        Ok(Some(vec![TextEdit {
            range,
            new_text: formatted,
        }]))
    }
}

pub async fn run() -> std::io::Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(PanacheLsp::new);
    Server::new(stdin, stdout, socket).serve(service).await;

    Ok(())
}
