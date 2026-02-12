use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::Client;
use tower_lsp_server::ls_types::*;

use crate::linter;

use super::super::config::load_config;
use super::super::conversions::convert_diagnostic;

/// Parse document and run linter, then publish diagnostics
pub(crate) async fn lint_and_publish(
    client: &Client,
    workspace_root: &Arc<Mutex<Option<PathBuf>>>,
    uri: Uri,
    text: String,
) {
    let workspace_root = workspace_root.lock().await.clone();
    let config = load_config(client, &workspace_root, Some(&uri)).await;

    // Parse and lint in blocking task
    let text_clone = text.clone();
    let diagnostics = tokio::task::spawn_blocking(move || {
        let tree = crate::parse(&text_clone, Some(config.clone()));
        linter::lint(&tree, &text_clone, &config)
    })
    .await;

    match diagnostics {
        Ok(panache_diagnostics) => {
            let lsp_diagnostics: Vec<Diagnostic> = panache_diagnostics
                .iter()
                .map(|d| convert_diagnostic(d, &text))
                .collect();

            client.publish_diagnostics(uri, lsp_diagnostics, None).await;
        }
        Err(e) => {
            client
                .log_message(MessageType::ERROR, format!("Linting task failed: {}", e))
                .await;
        }
    }
}
