use std::collections::{HashMap, HashSet};
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
use salsa::{Durability, Setter};
use std::path::{Path, PathBuf};
use std::time::Instant;

fn tracked_paths_for_graph(
    root_path: &Path,
    graph: &crate::salsa::ProjectGraph,
) -> HashSet<PathBuf> {
    let mut tracked = HashSet::new();
    tracked.insert(root_path.to_path_buf());
    for document in graph.documents() {
        tracked.insert(document.clone());
        for dependency in graph.dependencies(document, None) {
            tracked.insert(dependency);
        }
    }
    tracked
}

/// Handle textDocument/didOpen notification
pub(crate) async fn did_open(
    client: &Client,
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    workspace_root: Arc<Mutex<Option<std::path::PathBuf>>>,
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
        let mut db = salsa_db.lock().await;
        let path = params
            .text_document
            .uri
            .to_file_path()
            .map(|p| p.into_owned())
            .unwrap_or_else(|| std::path::PathBuf::from("<memory>"));
        (
            db.update_file_text_with_durability(path, text.clone(), Durability::LOW),
            {
                let cfg = crate::salsa::FileConfig::new(&*db, config.clone());
                cfg.set_config(&mut *db)
                    .with_durability(Durability::MEDIUM)
                    .to(config.clone());
                cfg
            },
        )
    };
    let doc_path = params
        .text_document
        .uri
        .to_file_path()
        .map(|p| p.into_owned());

    // Store document state
    {
        let mut map = document_map.lock().await;
        map.insert(
            uri.clone(),
            DocumentState {
                path: doc_path.clone(),
                salsa_file,
                salsa_config,
                tree,
            },
        );
    }
    if let Some(path) = doc_path.as_ref() {
        let mut db = salsa_db.lock().await;
        let graph =
            crate::salsa::project_graph(&*db, salsa_file, salsa_config, path.clone()).clone();
        for tracked in tracked_paths_for_graph(path, &graph) {
            let _ = db.ensure_file_text_cached(tracked);
        }
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
    let (graph_text, graph_path, _rebuild_full_graph, salsa_file, salsa_config) = {
        let (salsa_file, salsa_config, original_tree, has_multiple_docs) = {
            let document_map = document_map.lock().await;
            let Some(doc_state) = document_map.get(&uri_string) else {
                return;
            };
            (
                doc_state.salsa_file,
                doc_state.salsa_config,
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
            salsa_config,
        )
    };
    {
        let mut db = salsa_db.lock().await;
        if let Some(text) = graph_text.as_ref() {
            if let Some(path) = graph_path.clone() {
                db.update_file_text(path, text.clone());
            } else {
                salsa_file
                    .set_text(&mut *db)
                    .with_durability(Durability::LOW)
                    .to(text.clone());
            }
        }
        salsa_config
            .set_config(&mut *db)
            .with_durability(Durability::MEDIUM)
            .to(config.clone());
    }
    if let Some(state) = document_map.lock().await.get_mut(&uri_string) {
        state.path = graph_path.clone();
    }
    if let Some(path) = graph_path.as_ref() {
        let (dependents, tracked_paths) = {
            let db = salsa_db.lock().await;
            let graph =
                crate::salsa::project_graph(&*db, salsa_file, salsa_config, path.to_path_buf())
                    .clone();
            let dependents = graph.dependents(path, None);
            let tracked_paths = tracked_paths_for_graph(path, &graph);
            (dependents, tracked_paths)
        };
        {
            let mut db = salsa_db.lock().await;
            for tracked in tracked_paths {
                let _ = db.ensure_file_text_cached(tracked);
            }
        }
        if !dependents.is_empty() {
            dependent_uris = Some(
                dependents
                    .into_iter()
                    .filter_map(Uri::from_file_path)
                    .collect(),
            );
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
    salsa_db: Arc<Mutex<crate::salsa::SalsaDb>>,
    params: DidCloseTextDocumentParams,
) {
    let uri = params.text_document.uri.to_string();
    document_map.lock().await.remove(&uri);

    let states: Vec<DocumentState> = {
        let map = document_map.lock().await;
        map.values().cloned().collect()
    };
    let mut retained = HashSet::new();
    let mut db = salsa_db.lock().await;
    for state in states {
        let Some(path) = state.path.clone() else {
            continue;
        };
        let graph =
            crate::salsa::project_graph(&*db, state.salsa_file, state.salsa_config, path.clone())
                .clone();
        for tracked in tracked_paths_for_graph(&path, &graph) {
            retained.insert(tracked.clone());
            let _ = db.ensure_file_text_cached(tracked);
        }
    }
    for cached in db.cached_file_paths() {
        if retained.contains(&cached) || cached.as_os_str() == "<memory>" {
            continue;
        }
        let _ = db.evict_file_text(&cached);
    }

    // Clear diagnostics
    client
        .publish_diagnostics(params.text_document.uri, vec![], None)
        .await;
}
