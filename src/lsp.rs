use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::{Client, LspService, Server};

use rowan::GreenNode;

mod bibliography_cache;
mod config;
mod conversions;
mod documents;
mod handlers;
mod helpers;
mod server;

pub use bibliography_cache::BibliographyCache;

/// State for a single document in the LSP.
#[derive(Clone)]
pub struct DocumentState {
    /// Parsed metadata from YAML frontmatter (if present).
    pub metadata: Option<crate::metadata::DocumentMetadata>,
    /// Salsa input for this document's text.
    pub salsa_file: crate::salsa::FileText,
    /// Salsa input for this document's config.
    pub salsa_config: crate::salsa::FileConfig,
    /// Cached definition index for cross-document lookups.
    pub definition_index: crate::salsa::DefinitionIndex,
    /// Cached ProjectGraph for cross-document lookups.
    pub graph: crate::salsa::ProjectGraph,
    /// Cached syntax tree for incremental parsing.
    pub tree: GreenNode,
}

pub struct PanacheLsp {
    client: Client,
    // Use String keys since Uri doesn't implement Send
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    workspace_root: Arc<Mutex<Option<PathBuf>>>,
    bibliography_cache: Arc<Mutex<BibliographyCache>>,
    salsa_db: Arc<Mutex<crate::salsa::SalsaDb>>,
}

impl PanacheLsp {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            document_map: Arc::new(Mutex::new(HashMap::new())),
            workspace_root: Arc::new(Mutex::new(None)),
            bibliography_cache: Arc::new(Mutex::new(BibliographyCache::new())),
            salsa_db: Arc::new(Mutex::new(crate::salsa::SalsaDb::default())),
        }
    }

    /// Get access to the bibliography cache for testing purposes.
    #[doc(hidden)]
    pub fn bibliography_cache(&self) -> Arc<Mutex<BibliographyCache>> {
        Arc::clone(&self.bibliography_cache)
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
}

pub async fn run() -> std::io::Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(PanacheLsp::new);
    Server::new(stdin, stdout, socket).serve(service).await;

    Ok(())
}
