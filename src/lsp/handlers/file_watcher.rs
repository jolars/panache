//! File watcher handler for bibliography files.

use salsa::Durability;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::Client;
use tower_lsp_server::ls_types::*;

use crate::lsp::DocumentState;
use crate::syntax::SyntaxNode;

use super::super::helpers;
use super::diagnostics::lint_and_publish;

pub(crate) async fn did_change_watched_files(
    client: &Client,
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    salsa_db: Arc<Mutex<crate::salsa::SalsaDb>>,
    workspace_root: Arc<Mutex<Option<PathBuf>>>,
    params: DidChangeWatchedFilesParams,
) {
    // Process each file change
    for change in params.changes {
        let Some(path) = change.uri.to_file_path() else {
            continue;
        };

        let extension = path.extension().and_then(|e| e.to_str());
        let is_bibliography = matches!(
            extension,
            Some("bib") | Some("json") | Some("yaml") | Some("yml") | Some("ris")
        );

        // Always keep salsa's cached file text in sync when possible.
        if let Ok(contents) = std::fs::read_to_string(&path) {
            let mut db = salsa_db.lock().await;
            if db.update_file_text_if_cached_with_durability(&path, contents, Durability::MEDIUM) {
                client
                    .log_message(
                        MessageType::INFO,
                        format!("Updated cached file: {}", path.display()),
                    )
                    .await;
            }
        }

        if !is_bibliography {
            continue;
        }

        client
            .log_message(
                MessageType::INFO,
                format!("Bibliography file changed: {}", path.display()),
            )
            .await;

        // Find all documents that reference this bibliography file and re-lint them.
        // Must consult salsa metadata so bib watcher updates take effect immediately.
        let affected_documents: Vec<Uri> = {
            let states: Vec<(String, DocumentState)> = {
                let doc_map = document_map.lock().await;
                doc_map
                    .iter()
                    .map(|(uri_str, state)| (uri_str.clone(), state.clone()))
                    .collect()
            };

            let db = salsa_db.lock().await;
            states
                .into_iter()
                .filter_map(|(uri_str, state)| {
                    let doc_path = state.path?;
                    if !helpers::is_yaml_frontmatter_valid(&SyntaxNode::new_root(
                        state.tree.clone(),
                    )) {
                        return None;
                    }
                    let metadata = crate::salsa::metadata(
                        &*db,
                        state.salsa_file,
                        state.salsa_config,
                        doc_path,
                    )
                    .clone();
                    let bib_info = metadata.bibliography.as_ref()?;
                    if bib_info.paths.iter().any(|p| p == &path) {
                        uri_str.parse::<Uri>().ok()
                    } else {
                        None
                    }
                })
                .collect()
        };

        // Re-lint each affected document
        for uri in &affected_documents {
            lint_and_publish(
                client,
                &document_map,
                &salsa_db,
                &workspace_root,
                uri.clone(),
            )
            .await;
        }
    }
}
