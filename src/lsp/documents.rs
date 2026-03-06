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
use std::time::Instant;

/// Parse metadata from document text
fn parse_metadata(
    text: &str,
    uri: &Uri,
    bib_cache: &mut crate::lsp::BibliographyCache,
) -> Option<crate::metadata::DocumentMetadata> {
    // Convert URI to file path
    let file_path = uri.to_file_path()?.into_owned();

    // Parse the document
    let tree = crate::parse(text, None);

    // Extract metadata (but don't parse bibliography yet)
    let mut metadata = crate::metadata::extract_project_metadata(&tree, &file_path).ok()?;

    // Use cache to build bibliography index if needed
    if let Some(bib_info) = &metadata.bibliography {
        let index = bib_cache.build_index(&bib_info.paths);
        metadata.bibliography_parse = Some(crate::metadata::BibliographyParse {
            parse_errors: index.errors.iter().map(|e| e.message.clone()).collect(),
            index,
        });
    }

    Some(metadata)
}

/// Handle textDocument/didOpen notification
pub(crate) async fn did_open(
    client: &Client,
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    workspace_root: Arc<Mutex<Option<std::path::PathBuf>>>,
    bib_cache: Arc<Mutex<crate::lsp::BibliographyCache>>,
    params: DidOpenTextDocumentParams,
) {
    let uri = params.text_document.uri.to_string();
    let text = params.text_document.text.clone();
    log::debug!("did_open uri={}, bytes={}", uri, text.len());
    let start = Instant::now();
    let config = get_config(client, &workspace_root, &params.text_document.uri).await;
    let tree = GreenNode::from(crate::parse(&text, Some(config.clone())).green());
    let graph = if let Some(path) = params.text_document.uri.to_file_path() {
        let graph_start = Instant::now();
        let graph = crate::includes::ProjectGraph::build_project(&path, &text, &config);
        log::debug!(
            "did_open graph build in {:?}, docs={}, deps={}",
            graph_start.elapsed(),
            graph.documents().len(),
            graph.dependencies(&path, None).len()
        );
        graph
    } else {
        crate::includes::ProjectGraph::default()
    };

    // Parse metadata with bibliography cache
    let metadata = {
        let mut cache = bib_cache.lock().await;
        parse_metadata(&text, &params.text_document.uri, &mut cache)
    };

    // Store document state with metadata
    document_map.lock().await.insert(
        uri.clone(),
        DocumentState {
            text: text.clone(),
            metadata,
            graph,
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
    log::debug!("did_open complete in {:?}", start.elapsed());
}

/// Handle textDocument/didChange notification
pub(crate) async fn did_change(
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    workspace_root: Arc<Mutex<Option<std::path::PathBuf>>>,
    bib_cache: Arc<Mutex<crate::lsp::BibliographyCache>>,
    client: &Client,
    params: DidChangeTextDocumentParams,
) {
    let uri_string = params.text_document.uri.to_string();
    let change_count = params.content_changes.len();
    log::debug!("did_change uri={}, changes={}", uri_string, change_count);
    let start = Instant::now();
    let config = get_config(client, &workspace_root, &params.text_document.uri).await;

    // Apply incremental changes sequentially
    let mut dependent_uris: Option<Vec<Uri>> = None;
    {
        let mut document_map = document_map.lock().await;
        if let Some(doc_state) = document_map.get_mut(&uri_string) {
            // Store original state before applying changes
            let original_text = doc_state.text.clone();
            let original_tree = doc_state.tree.clone();
            let original_graph = doc_state.graph.clone();

            // Apply all changes to update the text
            for change in params.content_changes.iter() {
                doc_state.text = apply_content_change(&doc_state.text, change);
            }

            // Use incremental parsing for single changes, full reparse for multiple
            let new_tree = if params.content_changes.len() == 1 {
                let change = &params.content_changes[0];

                let (old_edit_start, old_edit_end, new_edit_start, new_edit_end) =
                    if let Some(range) = &change.range {
                        let old_start =
                            super::conversions::position_to_offset(&original_text, range.start)
                                .unwrap_or(0);
                        let old_end =
                            super::conversions::position_to_offset(&original_text, range.end)
                                .unwrap_or(original_text.len());
                        let new_end = old_start + change.text.len();
                        (old_start, old_end, old_start, new_end)
                    } else {
                        // Full document replacement
                        (0, original_text.len(), 0, doc_state.text.len())
                    };

                parse_incremental(
                    &doc_state.text,
                    Some(config.clone()),
                    &SyntaxNode::new_root(original_tree),
                    (old_edit_start, old_edit_end),
                    (new_edit_start, new_edit_end),
                )
                .tree
            } else {
                // Multiple changes - do full reparse for now
                crate::parse(&doc_state.text, Some(config.clone()))
            };

            doc_state.tree = GreenNode::from(new_tree.green());
            doc_state.graph = if let Some(path) = params.text_document.uri.to_file_path() {
                let graph_start = Instant::now();
                let graph =
                    crate::includes::ProjectGraph::build_project(&path, &doc_state.text, &config);
                log::debug!(
                    "did_change graph build in {:?}, docs={}, deps={}",
                    graph_start.elapsed(),
                    graph.documents().len(),
                    graph.dependencies(&path, None).len()
                );
                graph
            } else {
                crate::includes::ProjectGraph::default()
            };
            let dependents = if let Some(path) = params.text_document.uri.to_file_path() {
                original_graph.dependents(&path, None)
            } else {
                Vec::new()
            };
            if !dependents.is_empty() {
                dependent_uris = Some(
                    dependents
                        .into_iter()
                        .filter_map(Uri::from_file_path)
                        .collect(),
                );
            }
        } else {
            return;
        }
    };

    // Re-parse metadata after changes (using cache) - done outside the lock
    // First, get the text we need
    let current_text = {
        let document_map = document_map.lock().await;
        document_map
            .get(&uri_string)
            .map(|state| state.text.clone())
    };

    let metadata = if let Some(text) = current_text {
        let mut cache = bib_cache.lock().await;
        parse_metadata(&text, &params.text_document.uri, &mut cache)
    } else {
        None
    };

    // Update metadata in document state
    {
        let mut document_map = document_map.lock().await;
        if let Some(doc_state) = document_map.get_mut(&uri_string) {
            doc_state.metadata = metadata;
        }
    }

    if let Some(uris) = dependent_uris {
        for uri in uris {
            lint_and_publish(client, &document_map, &workspace_root, uri).await;
        }
    }

    // Run linter and publish diagnostics
    lint_and_publish(
        client,
        &document_map,
        &workspace_root,
        params.text_document.uri,
    )
    .await;
    log::debug!("did_change complete in {:?}", start.elapsed());
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
