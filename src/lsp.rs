use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::{Client, LspService, Server};

use rowan::GreenNode;

mod config;
mod conversions;
mod documents;
mod handlers;
mod helpers;
mod server;
mod symbols;

/// State for a single document in the LSP.
#[derive(Clone)]
pub struct DocumentState {
    /// Canonical file path for this document (if it exists on disk).
    pub path: Option<PathBuf>,
    /// Salsa input for this document's text.
    pub salsa_file: crate::salsa::FileText,
    /// Salsa input for this document's config.
    pub salsa_config: crate::salsa::FileConfig,
    /// Cached syntax tree for incremental parsing.
    pub tree: GreenNode,
    /// Cached parsed YAML regions for this document revision.
    pub parsed_yaml_regions: Vec<crate::syntax::ParsedYamlRegionSnapshot>,
}

pub struct PanacheLsp {
    client: Client,
    // Use String keys since Uri doesn't implement Send
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    workspace_root: Arc<Mutex<Option<PathBuf>>>,
    salsa_db: Arc<Mutex<crate::salsa::SalsaDb>>,
}

impl PanacheLsp {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            document_map: Arc::new(Mutex::new(HashMap::new())),
            workspace_root: Arc::new(Mutex::new(None)),
            salsa_db: Arc::new(Mutex::new(crate::salsa::SalsaDb::default())),
        }
    }

    /// Get access to the document map for testing purposes.
    ///
    /// This method is only available when the `lsp` feature is enabled
    /// and is intended for use in integration tests.
    #[doc(hidden)]
    pub fn document_map(&self) -> Arc<Mutex<HashMap<String, DocumentState>>> {
        Arc::clone(&self.document_map)
    }

    /// Get access to the workspace root for testing purposes.
    ///
    /// This method is only available when the `lsp` feature is enabled
    /// and is intended for use in integration tests.
    #[doc(hidden)]
    pub fn workspace_root(&self) -> Arc<Mutex<Option<PathBuf>>> {
        Arc::clone(&self.workspace_root)
    }

    /// Get access to the salsa database for testing purposes.
    #[doc(hidden)]
    pub fn salsa_db(&self) -> Arc<Mutex<crate::salsa::SalsaDb>> {
        Arc::clone(&self.salsa_db)
    }

    /// Trigger didChangeWatchedFiles for tests.
    #[doc(hidden)]
    pub async fn did_change_watched_files(
        &self,
        params: tower_lsp_server::ls_types::DidChangeWatchedFilesParams,
    ) {
        crate::lsp::handlers::file_watcher::did_change_watched_files(
            &self.client,
            Arc::clone(&self.document_map),
            Arc::clone(&self.salsa_db),
            Arc::clone(&self.workspace_root),
            params,
        )
        .await;
    }
}

pub async fn run() -> std::io::Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(PanacheLsp::new);
    Server::new(stdin, stdout, socket).serve(service).await;

    Ok(())
}
