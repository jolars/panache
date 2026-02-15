use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::{Client, LspService, Server};

mod config;
mod conversions;
mod documents;
mod handlers;
mod helpers;
mod server;

/// State for a single document in the LSP.
#[derive(Clone)]
pub struct DocumentState {
    /// The document text content.
    pub text: String,
    /// Parsed metadata from YAML frontmatter (if present).
    pub metadata: Option<crate::metadata::DocumentMetadata>,
}

pub struct PanacheLsp {
    client: Client,
    // Use String keys since Uri doesn't implement Send
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
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
}

pub async fn run() -> std::io::Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(PanacheLsp::new);
    Server::new(stdin, stdout, socket).serve(service).await;

    Ok(())
}
