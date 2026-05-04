use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::Client;
use tower_lsp_server::ls_types::*;

use super::conversions::{apply_content_change, apply_content_change_with_edit_ranges};
use super::handlers::diagnostics::lint_and_publish;
use super::helpers::get_config;
use crate::lsp::{DocumentState, LspRuntimeSettings};
use crate::parser::{parse_incremental_suffix_with_refdefs, parse_with_refdefs};
use crate::syntax::SyntaxNode;
use rowan::GreenNode;
use salsa::{Durability, Setter};
use std::path::{Path, PathBuf};
use std::time::Instant;

type CombinedEditRanges = (String, (usize, usize), (usize, usize));

fn apply_changes_descending_with_combined_ranges(
    original_text: &str,
    changes: &[TextDocumentContentChangeEvent],
) -> Option<CombinedEditRanges> {
    if changes.is_empty() {
        return None;
    }

    let mut updated_text = original_text.to_owned();
    let mut combined_old_start = usize::MAX;
    let mut combined_old_end = 0usize;
    let mut previous_start: Option<usize> = None;

    for change in changes {
        let (next_text, old_edit, _) =
            apply_content_change_with_edit_ranges(&updated_text, change)?;

        if let Some(prev_start) = previous_start
            && (old_edit.0 >= prev_start || old_edit.1 > prev_start)
        {
            return None;
        }
        previous_start = Some(old_edit.0);

        combined_old_start = combined_old_start.min(old_edit.0);
        combined_old_end = combined_old_end.max(old_edit.1);
        updated_text = next_text;
    }

    if combined_old_start == usize::MAX {
        return None;
    }

    let net_delta = updated_text.len() as isize - original_text.len() as isize;
    let combined_new_start = combined_old_start;
    let combined_new_end = combined_old_end.saturating_add_signed(net_delta);
    if combined_new_end < combined_new_start || combined_new_end > updated_text.len() {
        return None;
    }

    Some((
        updated_text,
        (combined_old_start, combined_old_end),
        (combined_new_start, combined_new_end),
    ))
}

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
    let (tree, parsed_yaml_regions) = {
        let syntax_tree = crate::parse(&text, Some(config.clone()));
        let parsed_yaml_regions = crate::syntax::collect_parsed_yaml_region_snapshots(&syntax_tree);
        (GreenNode::from(syntax_tree.green()), parsed_yaml_regions)
    };
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
                parsed_yaml_regions,
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
    runtime_settings: Arc<Mutex<LspRuntimeSettings>>,
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
    let (graph_text, graph_path, salsa_file, salsa_config) = {
        let (salsa_file, salsa_config, original_tree_green) = {
            let document_map = document_map.lock().await;
            let Some(doc_state) = document_map.get(&uri_string) else {
                return;
            };
            (
                doc_state.salsa_file,
                doc_state.salsa_config,
                doc_state.tree.clone(),
            )
        };
        let original_text = {
            let db = salsa_db.lock().await;
            salsa_file.text(&*db).clone()
        };

        let incremental_enabled = {
            runtime_settings
                .lock()
                .await
                .experimental_incremental_parsing
        };

        // Compute the post-edit text and (when incremental parsing is
        // enabled and edit ranges can be derived) the old/new edit
        // ranges. Decoupling this from the parse call lets us update
        // salsa's `FileText` early so the parser and downstream salsa
        // queries share one cached `refdef_set` per text change.
        let change_count = params.content_changes.len();
        let (updated_text, edit_ranges) = if !incremental_enabled {
            let mut text = original_text.clone();
            for change in params.content_changes.iter() {
                text = apply_content_change(&text, change);
            }
            (text, None)
        } else if change_count == 1 {
            let change = &params.content_changes[0];
            match apply_content_change_with_edit_ranges(&original_text, change) {
                Some((text, old_edit, new_edit)) => (text, Some((old_edit, new_edit))),
                None => (apply_content_change(&original_text, change), None),
            }
        } else {
            match apply_changes_descending_with_combined_ranges(
                &original_text,
                &params.content_changes,
            ) {
                Some((text, old_edit, new_edit)) => (text, Some((old_edit, new_edit))),
                None => {
                    let mut text = original_text.clone();
                    for change in params.content_changes.iter() {
                        text = apply_content_change(&text, change);
                    }
                    (text, None)
                }
            }
        };

        // Push the new text into salsa first so `refdef_set` reflects
        // it; then the parser reuses the cached refdef map and
        // downstream salsa queries (linter, project graph, ...) hit
        // the same cache instead of re-scanning the document.
        let doc_path_for_salsa = params
            .text_document
            .uri
            .to_file_path()
            .map(|p| p.into_owned());
        let refdefs = {
            let mut db = salsa_db.lock().await;
            if let Some(path) = doc_path_for_salsa.as_ref() {
                db.update_file_text(path.clone(), updated_text.clone());
            } else {
                salsa_file
                    .set_text(&mut *db)
                    .with_durability(Durability::LOW)
                    .to(updated_text.clone());
            }
            crate::salsa::refdef_set(&*db, salsa_file, salsa_config).clone()
        };

        let (green, strategy) = if let Some((old_edit, new_edit)) = edit_ranges {
            let old_tree = SyntaxNode::new_root(original_tree_green);
            let updated = parse_incremental_suffix_with_refdefs(
                &updated_text,
                Some(config.clone()),
                refdefs.clone(),
                &old_tree,
                old_edit,
                new_edit,
            );
            let label = match (change_count, updated.strategy) {
                (1, "section_window") => "section_window_single_change_experimental",
                (1, "suffix_window") => "suffix_incremental_single_change_experimental",
                (1, _) => "full_reparse_single_change_incremental_fallback",
                (_, "section_window") => "section_window_multi_change_coalesced_experimental",
                (_, "suffix_window") => "suffix_incremental_multi_change_coalesced_experimental",
                (_, _) => "full_reparse_multi_change_incremental_fallback",
            };
            (GreenNode::from(updated.tree.green()), label)
        } else {
            let parsed = parse_with_refdefs(&updated_text, Some(config.clone()), refdefs);
            let label = if !incremental_enabled {
                if change_count == 1 {
                    "full_reparse_single_change_incremental_disabled"
                } else {
                    "full_reparse_multi_change"
                }
            } else if change_count == 1 {
                "full_reparse_single_change_fallback"
            } else {
                "full_reparse_multi_change_incremental_fallback"
            };
            (GreenNode::from(parsed.green()), label)
        };

        log::debug!(
            "did_change parse strategy={} changes={}",
            strategy,
            params.content_changes.len()
        );

        let parsed_yaml_regions = crate::syntax::collect_parsed_yaml_region_snapshots(
            &SyntaxNode::new_root(green.clone()),
        );
        {
            let mut document_map = document_map.lock().await;
            let Some(doc_state) = document_map.get_mut(&uri_string) else {
                return;
            };
            doc_state.tree = green;
            doc_state.parsed_yaml_regions = parsed_yaml_regions;
        }

        (
            Some(updated_text),
            params
                .text_document
                .uri
                .to_file_path()
                .map(|p| p.into_owned()),
            salsa_file,
            salsa_config,
        )
    };
    // File text was already pushed into salsa before the parse to
    // populate the `refdef_set` cache; only the config and any
    // downstream-tracked side state remain to be updated here.
    let _ = graph_text;
    {
        let mut db = salsa_db.lock().await;
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

#[cfg(test)]
mod tests {
    use super::apply_changes_descending_with_combined_ranges;
    use tower_lsp_server::ls_types::{Position, Range, TextDocumentContentChangeEvent};

    fn change(
        start_line: u32,
        start_char: u32,
        end_line: u32,
        end_char: u32,
        text: &str,
    ) -> TextDocumentContentChangeEvent {
        TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: start_line,
                    character: start_char,
                },
                end: Position {
                    line: end_line,
                    character: end_char,
                },
            }),
            range_length: None,
            text: text.to_owned(),
        }
    }

    #[test]
    fn coalesces_multiple_descending_changes() {
        let original = "abcdef\n";
        let changes = vec![change(0, 3, 0, 4, "X"), change(0, 1, 0, 2, "Y")];

        let (updated, old_range, new_range) =
            apply_changes_descending_with_combined_ranges(original, &changes)
                .expect("descending changes should coalesce");

        assert_eq!(updated, "aYcXef\n");
        assert_eq!(old_range, (1, 4));
        assert_eq!(new_range, (1, 4));
    }

    #[test]
    fn rejects_non_descending_overlapping_changes() {
        let original = "abcdef\n";
        let changes = vec![change(0, 1, 0, 3, "XX"), change(0, 2, 0, 4, "YY")];

        assert!(apply_changes_descending_with_combined_ranges(original, &changes).is_none());
    }

    #[test]
    fn computes_net_delta_for_insert_and_delete_mix() {
        let original = "abcdef\n";
        let changes = vec![change(0, 5, 0, 5, "ZZ"), change(0, 1, 0, 3, "Q")];

        let (updated, old_range, new_range) =
            apply_changes_descending_with_combined_ranges(original, &changes)
                .expect("descending mixed changes should coalesce");

        assert_eq!(updated, "aQdeZZf\n");
        assert_eq!(old_range, (1, 5));
        assert_eq!(new_range, (1, 6));
    }
}
