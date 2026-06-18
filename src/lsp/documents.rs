//! Document lifecycle notifications (`didOpen`/`didChange`/`didSave`/`didClose`).
//!
//! These run synchronously on the main-loop thread with `&mut GlobalState`: they
//! are the sole writers of the salsa database and the document map. Parsing and
//! state updates happen inline so interactive requests always see the latest
//! tree; the expensive lint (project-graph recompute + diagnostics) is dispatched
//! to the [`TaskPool`](crate::lsp::task_pool) — debounced for `didChange`,
//! immediate for `didOpen`/`didSave`.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, MessageType, TextDocumentContentChangeEvent,
};
use rowan::GreenNode;
use salsa::{Durability, Setter};

use super::config::load_config;
use super::conversions::{apply_content_change, apply_content_change_with_edit_ranges};
use super::global_state::{GlobalState, LintRequest};
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

/// Discover and load every file the project graph references for `root_path`,
/// on the writer. Thin wrapper over [`crate::salsa::SalsaDb::load_referenced_files`]
/// (shared with the CLI lint path); returns the final tracked set for
/// `did_close` retention.
pub(crate) fn load_project_files(
    gs: &mut GlobalState,
    salsa_file: crate::salsa::FileText,
    salsa_config: crate::salsa::FileConfig,
    root_path: PathBuf,
) -> HashSet<PathBuf> {
    gs.salsa
        .load_referenced_files(salsa_file, salsa_config, root_path)
}

/// Handle `textDocument/didOpen`.
pub(crate) fn did_open(gs: &mut GlobalState, params: DidOpenTextDocumentParams) {
    let uri = params.text_document.uri.clone();
    let uri_string = uri.to_string();
    let text = params.text_document.text.clone();
    log::debug!("did_open uri={uri_string}, bytes={}", text.len());
    let start = Instant::now();

    let config = load_config(&gs.workspace_root, Some(&uri));
    let tree = {
        let syntax_tree = crate::parse(&text, Some(config.clone()));
        GreenNode::from(syntax_tree.green())
    };

    let doc_path = uri.to_file_path().map(|p| p.into_owned());
    // On-disk documents register under their path; an in-memory buffer gets a
    // distinct `FileId` with no backing path (retires the `<memory>` sentinel,
    // and avoids two untitled buffers colliding on one key) (audit §3.3 / G3).
    let salsa_file = match doc_path.clone() {
        Some(path) => {
            gs.salsa
                .update_file_text_with_durability(path, text.clone(), Durability::LOW)
        }
        None => gs
            .salsa
            .create_in_memory_file(text.clone(), Durability::LOW),
    };
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
        },
    );

    if let Some(path) = doc_path.as_ref() {
        load_project_files(gs, salsa_file, salsa_config, path.clone());
    }

    gs.sender
        .log_message(MessageType::INFO, format!("Opened document: {uri_string}"));

    // Debounce the lint instead of spawning it inline: a workspace-restore burst
    // of opens each writes salsa, and an inline lint would be cancelled by the
    // next open's write. Open runs external linters (like save) so their
    // diagnostics surface without waiting for the first manual save.
    gs.schedule_lint(
        &uri,
        LintRequest {
            with_dependents: false,
            run_external: true,
        },
    );
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

    let original_text = salsa_file.content_or_empty(&gs.salsa).to_string();

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
            .to(Some(std::sync::Arc::from(updated_text.clone())));
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

    if let Some(doc_state) = gs.document_map_mut().get_mut(&uri_string) {
        doc_state.tree = green;
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
    // behind per-keystroke work. Built-in only — external linters wait for save.
    gs.schedule_lint(
        &uri,
        LintRequest {
            with_dependents: true,
            run_external: false,
        },
    );

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
    // A save may have introduced new includes/bibliography since the document
    // was opened; load them on the writer so the debounced pass's snapshot sees
    // them. (The dispatch write phase reloads too, but doing it here keeps
    // interactive reads in the debounce window consistent.)
    if let Some((salsa_file, salsa_config, Some(path))) = gs
        .document_map
        .get(&uri.to_string())
        .map(|doc| (doc.salsa_file, doc.salsa_config, doc.path.clone()))
    {
        load_project_files(gs, salsa_file, salsa_config, path);
    }
    // Save is the heavy pass: dependents + external linters. Debounced like every
    // other lint so a save-all burst coalesces and the read isn't cancelled by a
    // sibling save's write.
    gs.schedule_lint(
        &uri,
        LintRequest {
            with_dependents: true,
            run_external: true,
        },
    );
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
        let tracked = load_project_files(gs, state.salsa_file, state.salsa_config, path);
        retained.extend(tracked);
    }
    for cached in gs.salsa.cached_file_paths() {
        if retained.contains(&cached) {
            continue;
        }
        let _ = gs.salsa.evict_file_text(&cached);
    }

    gs.drop_document_diagnostics(&uri);
    // `forget_lint` may also have cleared manifest entries; one coalesced nudge
    // (pull + refresh only; a no-op for push clients).
    gs.send_diagnostic_refresh();
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
