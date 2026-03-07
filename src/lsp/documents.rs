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
use salsa::Setter;
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
    salsa_db: Arc<Mutex<crate::salsa::SalsaDb>>,
    params: DidOpenTextDocumentParams,
) {
    let uri = params.text_document.uri.to_string();
    let text = params.text_document.text.clone();
    log::debug!("did_open uri={}, bytes={}", uri, text.len());
    let start = Instant::now();
    let config = get_config(client, &workspace_root, &params.text_document.uri).await;
    let tree = GreenNode::from(crate::parse(&text, Some(config.clone())).green());
    let (salsa_file, salsa_config) = {
        let db = salsa_db.lock().await;
        (
            crate::salsa::FileText::new(&*db, text.clone()),
            crate::salsa::FileConfig::new(&*db, config.clone()),
        )
    };
    let doc_path = params.text_document.uri.to_file_path();
    let has_other_docs = { !document_map.lock().await.is_empty() };
    let graph = if let Some(path) = doc_path.as_ref() {
        if has_other_docs {
            crate::includes::ProjectGraph::build_project(path, &text, &config)
        } else {
            crate::includes::ProjectGraph::build(path, &text, &config)
        }
    } else {
        crate::includes::ProjectGraph::default()
    };

    // Parse metadata with bibliography cache
    let metadata = {
        let mut cache = bib_cache.lock().await;
        parse_metadata(&text, &params.text_document.uri, &mut cache)
    };

    // Store document state with metadata
    {
        let mut map = document_map.lock().await;
        if has_other_docs {
            for state in map.values_mut() {
                state.graph = graph.clone();
            }
        }
        map.insert(
            uri.clone(),
            DocumentState {
                metadata,
                salsa_file,
                salsa_config,
                graph,
                tree,
            },
        );
    }

    client
        .log_message(MessageType::INFO, format!("Opened document: {}", uri))
        .await;

    // Run linter and publish diagnostics
    lint_and_publish(
        client,
        &document_map,
        &salsa_db,
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
    salsa_db: Arc<Mutex<crate::salsa::SalsaDb>>,
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
    let (graph_text, graph_path, rebuild_full_graph, salsa_file) = {
        let (salsa_file, original_tree, has_multiple_docs) = {
            let document_map = document_map.lock().await;
            let Some(doc_state) = document_map.get(&uri_string) else {
                return;
            };
            (
                doc_state.salsa_file,
                doc_state.tree.clone(),
                document_map.len() > 1,
            )
        };
        let original_text = {
            let db = salsa_db.lock().await;
            salsa_file.text(&*db).clone()
        };

        // Apply all changes to update the text
        let mut updated_text = original_text.clone();
        for change in params.content_changes.iter() {
            updated_text = apply_content_change(&updated_text, change);
        }

        // Use incremental parsing for single changes, full reparse for multiple
        let green = {
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
                        (0, original_text.len(), 0, updated_text.len())
                    };

                parse_incremental(
                    &updated_text,
                    Some(config.clone()),
                    &SyntaxNode::new_root(original_tree),
                    (old_edit_start, old_edit_end),
                    (new_edit_start, new_edit_end),
                )
                .tree
            } else {
                // Multiple changes - do full reparse for now
                crate::parse(&updated_text, Some(config.clone()))
            };

            GreenNode::from(new_tree.green())
        };
        {
            let mut document_map = document_map.lock().await;
            let Some(doc_state) = document_map.get_mut(&uri_string) else {
                return;
            };
            doc_state.tree = green;
        }

        (
            Some(updated_text),
            params
                .text_document
                .uri
                .to_file_path()
                .map(|p| p.into_owned()),
            has_multiple_docs,
            salsa_file,
        )
    };
    let _config_input = {
        let mut db = salsa_db.lock().await;
        if let Some(text) = graph_text.as_ref() {
            salsa_file.set_text(&mut *db).to(text.clone());
        }
        let config_input = crate::salsa::FileConfig::new(&*db, config.clone());
        drop(db);
        if let Some(state) = document_map.lock().await.get_mut(&uri_string) {
            state.salsa_config = config_input;
        }
        config_input
    };
    let new_graph = if let (Some(path), Some(text)) = (graph_path.as_ref(), graph_text.as_ref()) {
        if rebuild_full_graph {
            crate::includes::ProjectGraph::build_project(path, text, &config)
        } else {
            crate::includes::ProjectGraph::build(path, text, &config)
        }
    } else {
        crate::includes::ProjectGraph::default()
    };
    if let Some(path) = graph_path.as_ref()
        && rebuild_full_graph
    {
        let dependents = new_graph.dependents(path, None);
        if !dependents.is_empty() {
            dependent_uris = Some(
                dependents
                    .into_iter()
                    .filter_map(Uri::from_file_path)
                    .collect(),
            );
        }
    }

    // Re-parse metadata after changes (using cache) - done outside the lock
    // First, get the text we need
    let current_text = {
        let salsa_file = {
            let document_map = document_map.lock().await;
            document_map.get(&uri_string).map(|state| state.salsa_file)
        };
        if let Some(salsa_file) = salsa_file {
            let db = salsa_db.lock().await;
            Some(salsa_file.text(&*db).clone())
        } else {
            None
        }
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
            if rebuild_full_graph {
                for state in document_map.values_mut() {
                    state.graph = new_graph.clone();
                }
            } else {
                doc_state.graph = new_graph.clone();
            }
        }
    }

    if let Some(uris) = dependent_uris {
        for uri in uris {
            lint_and_publish(client, &document_map, &salsa_db, &workspace_root, uri).await;
        }
    }

    // Run linter and publish diagnostics
    lint_and_publish(
        client,
        &document_map,
        &salsa_db,
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
