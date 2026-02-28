use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::Client;
use tower_lsp_server::ls_types::*;

use super::conversions::apply_content_change;
use super::handlers::diagnostics::lint_and_publish;
use super::helpers::get_config;
use crate::lsp::DocumentState;
use crate::parser::parse_incremental;
use crate::syntax::SyntaxNode;
use rowan::GreenNode;

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
    let config = get_config(client, &workspace_root, &params.text_document.uri).await;
    let tree = GreenNode::from(crate::parse(&text, Some(config)).green());

    // Parse metadata
    let metadata = parse_metadata(&text, &params.text_document.uri);

    // Store document state with metadata
    document_map.lock().await.insert(
        uri.clone(),
        DocumentState {
            text: text.clone(),
            metadata,
            tree,
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
    let config = get_config(client, &workspace_root, &params.text_document.uri).await;

    // Apply incremental changes sequentially
    {
        let mut document_map = document_map.lock().await;
        if let Some(doc_state) = document_map.get_mut(&uri_string) {
            let mut old_edit_start: Option<usize> = None;
            let mut old_edit_end: Option<usize> = None;
            let mut new_edit_start: Option<usize> = None;
            let mut new_edit_end: Option<usize> = None;

            for change in params.content_changes {
                let old_text = doc_state.text.clone();
                if let Some(range) = &change.range {
                    let start_offset =
                        super::conversions::position_to_offset(&old_text, range.start).unwrap_or(0);
                    let end_offset = super::conversions::position_to_offset(&old_text, range.end)
                        .unwrap_or(old_text.len());
                    old_edit_start =
                        Some(old_edit_start.map_or(start_offset, |s| s.min(start_offset)));
                    old_edit_end = Some(old_edit_end.map_or(end_offset, |e| e.max(end_offset)));

                    let new_end = start_offset.saturating_add(change.text.len());
                    new_edit_start =
                        Some(new_edit_start.map_or(start_offset, |s| s.min(start_offset)));
                    new_edit_end = Some(new_edit_end.map_or(new_end, |e| e.max(new_end)));
                } else {
                    old_edit_start = Some(0);
                    old_edit_end = Some(old_text.len());
                    new_edit_start = Some(0);
                    new_edit_end = Some(change.text.len());
                }

                doc_state.text = apply_content_change(&doc_state.text, &change);
            }

            let old_edit_range = if let (Some(start), Some(end)) = (old_edit_start, old_edit_end) {
                (start, end)
            } else {
                (0, doc_state.text.len())
            };

            let new_edit_range = if let (Some(start), Some(end)) = (new_edit_start, new_edit_end) {
                (start, end)
            } else {
                (0, doc_state.text.len())
            };

            let new_tree = parse_incremental(
                &doc_state.text,
                Some(config.clone()),
                &SyntaxNode::new_root(doc_state.tree.clone()),
                old_edit_range,
                new_edit_range,
            )
            .tree;

            doc_state.tree = GreenNode::from(new_tree.green());

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
