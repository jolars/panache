use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::Client;
use tower_lsp_server::ls_types::*;

use super::conversions::apply_content_change;
use super::handlers::diagnostics::lint_and_publish;

/// Handle textDocument/didOpen notification
pub(crate) async fn did_open(
    client: &Client,
    document_map: Arc<Mutex<HashMap<String, String>>>,
    workspace_root: Arc<Mutex<Option<std::path::PathBuf>>>,
    params: DidOpenTextDocumentParams,
) {
    let uri = params.text_document.uri.to_string();
    let text = params.text_document.text;

    document_map.lock().await.insert(uri.clone(), text.clone());

    client
        .log_message(MessageType::INFO, format!("Opened document: {}", uri))
        .await;

    // Run linting and publish diagnostics
    lint_and_publish(client, &workspace_root, params.text_document.uri, text).await;
}

/// Handle textDocument/didChange notification
pub(crate) async fn did_change(
    document_map: Arc<Mutex<HashMap<String, String>>>,
    workspace_root: Arc<Mutex<Option<std::path::PathBuf>>>,
    client: &Client,
    params: DidChangeTextDocumentParams,
) {
    let uri = params.text_document.uri.to_string();

    // Apply incremental changes sequentially
    let text = {
        let mut document_map = document_map.lock().await;
        if let Some(text) = document_map.get_mut(&uri) {
            for change in params.content_changes {
                *text = apply_content_change(text, &change);
            }
            text.clone()
        } else {
            return;
        }
    };

    // Run linting and publish diagnostics
    lint_and_publish(client, &workspace_root, params.text_document.uri, text).await;
}

/// Handle textDocument/didClose notification
pub(crate) async fn did_close(
    client: &Client,
    document_map: Arc<Mutex<HashMap<String, String>>>,
    params: DidCloseTextDocumentParams,
) {
    let uri = params.text_document.uri.to_string();
    document_map.lock().await.remove(&uri);

    // Clear diagnostics
    client
        .publish_diagnostics(params.text_document.uri, vec![], None)
        .await;
}
