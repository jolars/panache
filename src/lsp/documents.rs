//! Document lifecycle notifications (`didOpen`/`didChange`/`didSave`/`didClose`).
//!
//! These run synchronously on the main-loop thread with `&mut GlobalState`: they
//! are the sole writers of the salsa database and the document map. Parsing and
//! state updates happen inline so interactive requests always see the latest
//! tree; the expensive lint (project-graph recompute + diagnostics) is dispatched
//! to the [`TaskPool`](crate::lsp::task_pool) — debounced for `didChange`,
//! immediate for `didOpen`/`didSave`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, MessageType, TextDocumentContentChangeEvent,
};
use rowan::GreenNode;
use salsa::{Durability, Setter};

use super::config::load_config;
use super::conversions::{apply_content_change, apply_content_change_with_edit_ranges};
use super::global_state::GlobalState;
use super::uri_ext::UriExt;
use crate::lsp::DocumentState;
use crate::parser::{parse_incremental_suffix_with_refdefs, parse_with_refdefs};
use crate::syntax::SyntaxNode;

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

/// Handle `textDocument/didOpen`.
pub(crate) fn did_open(gs: &mut GlobalState, params: DidOpenTextDocumentParams) {
    let uri = params.text_document.uri.clone();
    let uri_string = uri.to_string();
    let text = params.text_document.text.clone();
    log::debug!("did_open uri={uri_string}, bytes={}", text.len());
    let start = Instant::now();

    let config = load_config(&gs.workspace_root, Some(&uri));
    let (tree, parsed_yaml_regions) = {
        let syntax_tree = crate::parse(&text, Some(config.clone()));
        let parsed_yaml_regions = crate::syntax::collect_parsed_yaml_region_snapshots(&syntax_tree);
        (GreenNode::from(syntax_tree.green()), parsed_yaml_regions)
    };

    let doc_path = uri.to_file_path().map(|p| p.into_owned());
    let path_for_salsa = doc_path
        .clone()
        .unwrap_or_else(|| PathBuf::from("<memory>"));
    let salsa_file =
        gs.salsa
            .update_file_text_with_durability(path_for_salsa, text.clone(), Durability::LOW);
    let salsa_config = {
        let cfg = crate::salsa::FileConfig::new(&gs.salsa, config.clone());
        cfg.set_config(&mut gs.salsa)
            .with_durability(Durability::MEDIUM)
            .to(config.clone());
        cfg
    };

    gs.document_map_mut().insert(
        uri_string.clone(),
        DocumentState {
            path: doc_path.clone(),
            salsa_file,
            salsa_config,
            tree,
            parsed_yaml_regions,
        },
    );

    if let Some(path) = doc_path.as_ref() {
        let graph =
            crate::salsa::project_graph(&gs.salsa, salsa_file, salsa_config, path.clone()).clone();
        for tracked in tracked_paths_for_graph(path, &graph) {
            let _ = gs.salsa.ensure_file_text_cached(tracked);
        }
    }

    gs.sender
        .log_message(MessageType::INFO, format!("Opened document: {uri_string}"));

    // Open is a one-time event: run external linters now so their diagnostics
    // surface immediately rather than waiting for the first save.
    gs.spawn_lint(uri, false, true);
    log::debug!("did_open complete in {:?}", start.elapsed());
}

/// Handle `textDocument/didChange`.
pub(crate) fn did_change(gs: &mut GlobalState, params: DidChangeTextDocumentParams) {
    let uri = params.text_document.uri.clone();
    let uri_string = uri.to_string();
    let change_count = params.content_changes.len();
    log::debug!("did_change uri={uri_string}, changes={change_count}");
    let start = Instant::now();

    let config = load_config(&gs.workspace_root, Some(&uri));
    let incremental_enabled = gs.runtime_settings.experimental_incremental_parsing;

    let Some((salsa_file, salsa_config, original_tree_green)) = gs
        .document_map
        .get(&uri_string)
        .map(|doc| (doc.salsa_file, doc.salsa_config, doc.tree.clone()))
    else {
        return;
    };

    let original_text = salsa_file.text(&gs.salsa).clone();

    // Compute the post-edit text and (when incremental parsing is enabled and
    // edit ranges can be derived) the old/new edit ranges.
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
        match apply_changes_descending_with_combined_ranges(&original_text, &params.content_changes)
        {
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

    // Push the new text into salsa first so `refdef_set` reflects it; the parser
    // then reuses the cached refdef map and downstream queries hit the same cache.
    let doc_path_for_salsa = uri.to_file_path().map(|p| p.into_owned());
    if let Some(path) = doc_path_for_salsa.as_ref() {
        gs.salsa
            .update_file_text(path.clone(), updated_text.clone());
    } else {
        salsa_file
            .set_text(&mut gs.salsa)
            .with_durability(Durability::LOW)
            .to(updated_text.clone());
    }
    let refdefs = crate::salsa::refdef_set(&gs.salsa, salsa_file, salsa_config).clone();

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

    log::debug!("did_change parse strategy={strategy} changes={change_count}");

    let parsed_yaml_regions =
        crate::syntax::collect_parsed_yaml_region_snapshots(&SyntaxNode::new_root(green.clone()));

    if let Some(doc_state) = gs.document_map_mut().get_mut(&uri_string) {
        doc_state.tree = green;
        doc_state.parsed_yaml_regions = parsed_yaml_regions;
        doc_state.path = doc_path_for_salsa.clone();
    } else {
        return;
    }

    salsa_config
        .set_config(&mut gs.salsa)
        .with_durability(Durability::MEDIUM)
        .to(config.clone());

    // Defer the expensive lint to a debounced pass so a burst of keystrokes
    // collapses into one lint and a save's formatting request never queues
    // behind per-keystroke work.
    gs.schedule_lint(&uri);

    log::debug!(
        "did_change complete (parse+state) in {:?}; lint debounced",
        start.elapsed()
    );
}

/// Handle `textDocument/didSave`.
///
/// Save is the point at which heavier external linters run (skipped on every
/// keystroke). Any pending debounced pass is superseded by the fresh generation
/// that [`GlobalState::spawn_lint`] bumps.
pub(crate) fn did_save(gs: &mut GlobalState, params: DidSaveTextDocumentParams) {
    let uri = params.text_document.uri;
    gs.lint_deadlines.remove(&uri.to_string());
    // A `did_change` arriving while this save's external-linter pass is in
    // flight will bump the generation and discard its result; the next
    // debounced pass then runs built-in-only, so external diagnostics stay
    // stale until the next save. Accepted trade-off — keystroke debouncing
    // matters more than freshness of an inherently slow signal.
    gs.spawn_lint(uri, true, true);
}

/// Handle `textDocument/didClose`.
pub(crate) fn did_close(gs: &mut GlobalState, params: DidCloseTextDocumentParams) {
    let uri = params.text_document.uri.clone();
    let uri_string = uri.to_string();
    gs.document_map_mut().remove(&uri_string);
    gs.forget_lint(&uri);

    let states: Vec<DocumentState> = gs.document_map.values().cloned().collect();
    let mut retained = HashSet::new();
    for state in states {
        let Some(path) = state.path.clone() else {
            continue;
        };
        let graph = crate::salsa::project_graph(
            &gs.salsa,
            state.salsa_file,
            state.salsa_config,
            path.clone(),
        )
        .clone();
        for tracked in tracked_paths_for_graph(&path, &graph) {
            retained.insert(tracked.clone());
            let _ = gs.salsa.ensure_file_text_cached(tracked);
        }
    }
    for cached in gs.salsa.cached_file_paths() {
        if retained.contains(&cached) || cached.as_os_str() == "<memory>" {
            continue;
        }
        let _ = gs.salsa.evict_file_text(&cached);
    }

    gs.sender.publish_diagnostics(uri, vec![], None);
}

#[cfg(test)]
mod tests {
    use super::apply_changes_descending_with_combined_ranges;
    use lsp_types::{Position, Range, TextDocumentContentChangeEvent};

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
