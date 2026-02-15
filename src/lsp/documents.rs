use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::Client;
use tower_lsp_server::ls_types::*;

use super::conversions::apply_content_change;
use super::handlers::diagnostics::lint_and_publish;
use crate::lsp::DocumentState;

/// Parse metadata from document text
fn parse_metadata(text: &str, uri: &Uri) -> Option<crate::metadata::DocumentMetadata> {
    // Convert URI to file path - to_file_path() returns Option<Cow<Path>>
    let file_path = uri.to_file_path()?.into_owned();

    // Parse the document
    let tree = crate::parse(text, None);

    // Extract metadata
    crate::metadata::extract_metadata(&tree, &file_path).ok()
}

/// Handle textDocument/didOpen notification
pub(crate) async fn did_open(
    client: &Client,
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    workspace_root: Arc<Mutex<Option<std::path::PathBuf>>>,
    params: DidOpenTextDocumentParams,
) {
    let uri = params.text_document.uri.to_string();
    let text = params.text_document.text.clone();

    // Parse metadata
    let metadata = parse_metadata(&text, &params.text_document.uri);

    // Store document state with metadata
    document_map.lock().await.insert(
        uri.clone(),
        DocumentState {
            text: text.clone(),
            metadata,
        },
    );

    client
        .log_message(MessageType::INFO, format!("Opened document: {}", uri))
        .await;

    // Run linter and publish diagnostics
    lint_and_publish(
        client,
        &document_map,
        &workspace_root,
        params.text_document.uri,
    )
    .await;
}

/// Handle textDocument/didChange notification
pub(crate) async fn did_change(
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    workspace_root: Arc<Mutex<Option<std::path::PathBuf>>>,
    client: &Client,
    params: DidChangeTextDocumentParams,
) {
    let uri_string = params.text_document.uri.to_string();

    // Apply incremental changes sequentially
    {
        let mut document_map = document_map.lock().await;
        if let Some(doc_state) = document_map.get_mut(&uri_string) {
            for change in params.content_changes {
                doc_state.text = apply_content_change(&doc_state.text, &change);
            }

            // Re-parse metadata after changes
            doc_state.metadata = parse_metadata(&doc_state.text, &params.text_document.uri);
        } else {
            return;
        }
    };

    // Run linter and publish diagnostics
    lint_and_publish(
        client,
        &document_map,
        &workspace_root,
        params.text_document.uri,
    )
    .await;
}

/// Handle textDocument/didClose notification
pub(crate) async fn did_close(
    client: &Client,
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    params: DidCloseTextDocumentParams,
) {
    let uri = params.text_document.uri.to_string();
    document_map.lock().await.remove(&uri);

    // Clear diagnostics
    client
        .publish_diagnostics(params.text_document.uri, vec![], None)
        .await;
}
