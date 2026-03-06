//! File watcher handler for bibliography files.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::Client;
use tower_lsp_server::ls_types::*;

use crate::lsp::{BibliographyCache, DocumentState};

use super::diagnostics::lint_and_publish;

pub(crate) async fn did_change_watched_files(
    client: &Client,
    bib_cache: Arc<Mutex<BibliographyCache>>,
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    workspace_root: Arc<Mutex<Option<PathBuf>>>,
    params: DidChangeWatchedFilesParams,
) {
    // Process each file change
    for change in params.changes {
        let Some(path) = change.uri.to_file_path() else {
            continue;
        };

        // Check if this is a bibliography file
        let extension = path.extension().and_then(|e| e.to_str());
        if !matches!(
            extension,
            Some("bib") | Some("json") | Some("yaml") | Some("yml") | Some("ris")
        ) {
            continue;
        }

        client
            .log_message(
                MessageType::INFO,
                format!("Bibliography file changed: {}", path.display()),
            )
            .await;

        // Invalidate the cache for this file
        {
            let mut cache = bib_cache.lock().await;
            cache.invalidate(&path);
        }

        // Find all documents that reference this bibliography file and re-lint them
        let affected_documents: Vec<Uri> = {
            let doc_map = document_map.lock().await;
            doc_map
                .iter()
                .filter_map(|(uri_str, state)| {
                    // Check if this document's bibliography includes the changed file
                    state.metadata.as_ref().and_then(|meta| {
                        meta.bibliography.as_ref().and_then(|bib_info| {
                            if bib_info.paths.iter().any(|p| p == &path) {
                                // Parse the URI string back to Uri
                                uri_str.parse::<Uri>().ok()
                            } else {
                                None
                            }
                        })
                    })
                })
                .collect()
        };

        // Re-lint each affected document
        for uri in &affected_documents {
            lint_and_publish(client, &document_map, &workspace_root, uri.clone()).await;
        }
    }
}
