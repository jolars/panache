use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::Client;
use tower_lsp_server::ls_types::*;

use crate::linter;

use super::super::conversions::convert_diagnostic;
use super::super::helpers::get_config;

/// Parse document and run linter, then publish diagnostics
pub(crate) async fn lint_and_publish(
    client: &Client,
    workspace_root: &Arc<Mutex<Option<PathBuf>>>,
    uri: Uri,
    text: String,
) {
    // Use helper to load config
    let config = get_config(client, workspace_root, &uri).await;

    // Parse and lint (including external linters) in blocking task
    let text_clone = text.clone();
    let has_external_linters = !config.linters.is_empty();

    let diagnostics = if has_external_linters {
        // Use async runtime for external linters
        tokio::task::spawn_blocking(move || {
            let tree = crate::parse(&text_clone, Some(config.clone()));
            // Create a runtime for the async lint function
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(linter::lint_with_external(&tree, &text_clone, &config))
        })
        .await
    } else {
        // Regular sync lint for built-in rules only
        tokio::task::spawn_blocking(move || {
            let tree = crate::parse(&text_clone, Some(config.clone()));
            linter::lint(&tree, &text_clone, &config)
        })
        .await
    };

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
