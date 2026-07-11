//! Document lifecycle notifications (`didOpen`/`didChange`/`didSave`/`didClose`).
//!
//! These run against writer-owned state (`&mut WriterState`) — the salsa
//! database, the document map, the settle timer, and the diagnostics store.
//! Parsing and state updates happen inline so interactive requests always see
//! the latest tree; the expensive lint (project-graph recompute + diagnostics)
//! is deferred to the debounced workspace settle, which re-lints every open
//! document over one snapshot. Every salsa-input write here arms that settle
//! (via [`WriterState::arm_settle`] or [`WriterState::arm_settle_external`]) so
//! a write that cancels an in-flight pass also schedules its recomputation.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, MessageType, TextDocumentContentChangeEvent,
};
use rowan::GreenNode;
use salsa::{Durability, Setter};

use super::conversions::{apply_content_change, apply_content_change_with_edit_ranges};
use super::uri_ext::UriExt;
use super::writer::WriterState;
use crate::lsp::DocumentState;
use crate::parser::{parse_incremental_suffix_with_refdefs, parse_with_refdefs};
use crate::syntax::SyntaxNode;

type CombinedEditRanges = (String, (usize, usize), (usize, usize));

/// Test hook: makes the next `did_change` panic right after its salsa text
/// write, exercising the writer loop's post-panic document heal.
#[cfg(test)]
pub(crate) static PANIC_AFTER_DID_CHANGE_TEXT_WRITE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

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
    w: &mut WriterState,
    salsa_file: crate::salsa::FileText,
    salsa_config: crate::salsa::FileConfig,
    root_path: PathBuf,
) -> HashSet<PathBuf> {
    let load = w
        .db_mut()
        .load_referenced_files(salsa_file, salsa_config, root_path);
    // Only the freshly-read inputs now reflect the disk; a harvest batch read
    // before this load must not regress them (no-op outside a harvest cycle).
    // An already-cached path was NOT re-read (`load_file_from_disk` populates
    // absent inputs only) and may be stale — it must stay harvestable, or the
    // in-flight batch carrying its out-of-band edit would be discarded.
    for path in &load.loaded {
        w.shield_from_harvest(path);
    }
    load.tracked
}

/// Reload every open document's project-referenced files on the writer.
///
/// A filesystem change (watcher event, file operation) may have flipped a
/// referenced include/bibliography's `None`->`Some` text input (or vice versa);
/// loading here before the next snapshot lets the re-lint observe fresh content.
pub(crate) fn reload_open_documents_referenced_files(w: &mut WriterState) {
    let open_docs = w.open_documents();
    // A path open as a document has buffer-authoritative content; it must never
    // be re-read from disk below or an unsaved edit would be clobbered.
    let open_paths = w.open_document_paths();
    let mut referenced: HashSet<PathBuf> = HashSet::new();
    for (salsa_file, salsa_config, path) in open_docs {
        referenced.extend(load_project_files(w, salsa_file, salsa_config, path));
    }
    // Self-heal: refresh referenced files whose on-disk content changed since
    // they were cached. Not every client delivers `didChangeWatchedFiles` for
    // every referenced-file edit --- nvim emits no watch event for a
    // bibliography open in a buffer --- so without this an out-of-band change
    // stays frozen in salsa until the document is reloaded. Runs on the writer
    // over the deduplicated referenced set (open documents excluded); the
    // compare-then-skip inside `resync_cached_file_from_disk` means an unchanged
    // file triggers no revision bump or downstream re-lint.
    //
    // TODO: this compensates for clients whose file-watching is incomplete. If
    // editor watch delivery becomes reliable (or we drive referenced-file
    // updates entirely through the watcher), revisit whether this disk re-read
    // can be dropped in favor of the pure `didChangeWatchedFiles` path.
    for path in referenced {
        if open_paths.contains(&path) {
            continue;
        }
        w.db_mut()
            .resync_cached_file_from_disk(&path, Durability::MEDIUM);
        // Synced (or confirmed equal to disk): an in-flight harvest batch
        // read this path earlier and must not regress it.
        w.shield_from_harvest(&path);
    }
}

/// Re-read on-disk config for every open document and refresh its `FileConfig`
/// salsa input.
///
/// Config is normally re-read on each `did_open`/`did_change`, so an idle open
/// document keeps stale config when `panache.toml` changes underneath it. This
/// refreshes those documents on demand (config-file watcher event or a
/// `workspace/didChangeConfiguration` notification). The set mirrors the
/// unconditional `did_change` write (salsa only bumps the revision when the
/// value actually differs); the caller arms the settle so the all-docs re-lint
/// re-publishes diagnostics.
pub(crate) fn reload_open_documents_config(w: &mut WriterState) {
    let entries: Vec<(lsp_types::Uri, crate::salsa::FileConfig)> = w
        .document_map()
        .iter()
        .filter_map(|(uri_str, state)| Some((uri_str.parse().ok()?, state.salsa_config)))
        .collect();
    for (uri, salsa_config) in entries {
        let new_config = w.load_config_notifying(&uri);
        salsa_config
            .set_config(w.db_mut())
            .with_durability(Durability::MEDIUM)
            .to(new_config);
    }
}

/// Handle `textDocument/didOpen`.
pub(crate) fn did_open(w: &mut WriterState, params: DidOpenTextDocumentParams) {
    let uri = params.text_document.uri.clone();
    let uri_string = uri.to_string();
    let text = params.text_document.text.clone();
    log::debug!("did_open uri={uri_string}, bytes={}", text.len());
    let start = Instant::now();

    let config = w.load_config_notifying(&uri);
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
            w.db_mut()
                .update_file_text_with_durability(path, text.clone(), Durability::LOW)
        }
        None => w
            .db_mut()
            .create_in_memory_file(text.clone(), Durability::LOW),
    };
    let salsa_config = {
        let cfg = crate::salsa::FileConfig::new(w.db(), config.clone());
        cfg.set_config(w.db_mut())
            .with_durability(Durability::MEDIUM)
            .to(config.clone());
        cfg
    };

    w.document_map_mut().insert(
        uri_string.clone(),
        DocumentState {
            path: doc_path.clone(),
            salsa_file,
            salsa_config,
            tree,
            version: params.text_document.version,
        },
    );

    if let Some(path) = doc_path.as_ref() {
        load_project_files(w, salsa_file, salsa_config, path.clone());
    }

    w.sender()
        .log_message(MessageType::INFO, format!("Opened document: {uri_string}"));

    // Arm the workspace settle instead of spawning a lint inline: a
    // workspace-restore burst of opens each writes salsa, and an inline lint
    // would be cancelled by the next open's write. Open runs external linters
    // (like save) so their diagnostics surface without waiting for the first
    // manual save.
    w.arm_settle_external(uri);
    log::debug!("did_open complete in {:?}", start.elapsed());
}

/// Handle `textDocument/didChange`.
pub(crate) fn did_change(w: &mut WriterState, params: DidChangeTextDocumentParams) {
    let uri = params.text_document.uri.clone();
    let uri_string = uri.to_string();
    let change_count = params.content_changes.len();
    log::debug!("did_change uri={uri_string}, changes={change_count}");
    let start = Instant::now();

    let config = w.load_config_notifying(&uri);
    let incremental_enabled = w.runtime_settings().experimental_incremental_parsing;

    let Some((salsa_file, salsa_config, original_tree_green)) = w
        .document_map()
        .get(&uri_string)
        .map(|doc| (doc.salsa_file, doc.salsa_config, doc.tree.clone()))
    else {
        return;
    };

    let original_text = salsa_file.content_or_empty(w.db()).to_string();

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
        w.db_mut()
            .update_file_text(path.clone(), updated_text.clone());
    } else {
        salsa_file
            .set_text(w.db_mut())
            .with_durability(Durability::LOW)
            .to(Some(std::sync::Arc::from(updated_text.clone())));
    }
    // Test hook for the writer loop's post-panic document heal: this is the
    // worst spot to die — salsa already holds the new text while the cached
    // tree and version still describe the old buffer.
    #[cfg(test)]
    if PANIC_AFTER_DID_CHANGE_TEXT_WRITE.swap(false, std::sync::atomic::Ordering::SeqCst) {
        panic!("test-injected did_change panic");
    }
    let refdefs = crate::salsa::refdef_set(w.db(), salsa_file, salsa_config).clone();

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

    if let Some(doc_state) = w.document_map_mut().get_mut(&uri_string) {
        doc_state.tree = green;
        doc_state.path = doc_path_for_salsa.clone();
        doc_state.version = params.text_document.version;
    } else {
        return;
    }

    salsa_config
        .set_config(w.db_mut())
        .with_durability(Durability::MEDIUM)
        .to(config.clone());

    // Defer the expensive re-lint to the debounced settle so a burst of
    // keystrokes collapses into one pass and a save's formatting request never
    // queues behind per-keystroke work. No external linters — they wait for save.
    w.arm_settle();

    log::debug!(
        "did_change complete (parse+state) in {:?}; settle armed",
        start.elapsed()
    );
}

/// Rebuild `uri`'s cached document state from the current salsa text, after a
/// panicking write handler may have left it half-updated (see the writer
/// loop's `apply_write` guard). `did_change` pushes the new text into salsa
/// *before* parsing, so a mid-parse panic strands the old tree and version
/// against the new text — and a deterministic parser bug would re-strand it
/// on every keystroke while reads keep serving the stale tree. The salsa
/// input is the server's best knowledge of the buffer; reparse it fully
/// (incremental state is exactly what cannot be trusted here) and re-arm the
/// settle the dead handler never reached.
pub(crate) fn resync_document_after_panic(
    w: &mut WriterState,
    uri: &lsp_types::Uri,
    version: Option<i32>,
) {
    let uri_string = uri.to_string();
    let Some((salsa_file, salsa_config)) = w
        .document_map()
        .get(&uri_string)
        .map(|doc| (doc.salsa_file, doc.salsa_config))
    else {
        // The panic struck before the document was ever inserted (a `did_open`
        // that died between its salsa text write and the map insert). There is
        // no cached tree to rebuild, but the salsa write did land, so still arm
        // the settle the dead handler never reached — the all-docs pass then
        // reconciles diagnostics over current state, upholding the invariant
        // that every write (and every heal) arms a settle.
        log::warn!(
            "post-panic resync: no document state for {}; arming settle only",
            uri.as_str()
        );
        w.arm_settle();
        return;
    };
    let config = w.load_config_notifying(uri);
    let text = salsa_file.content_or_empty(w.db()).to_string();
    let refdefs = crate::salsa::refdef_set(w.db(), salsa_file, salsa_config).clone();
    let green = GreenNode::from(parse_with_refdefs(&text, Some(config), refdefs).green());
    if let Some(doc_state) = w.document_map_mut().get_mut(&uri_string) {
        doc_state.tree = green;
        if let Some(version) = version {
            doc_state.version = version;
        }
    }
    w.arm_settle();
}

/// Handle `textDocument/didSave`.
///
/// Save is the point at which heavier external linters run (skipped on every
/// keystroke). The fresh settle re-lints every open document; only the saved
/// document runs external linters.
pub(crate) fn did_save(w: &mut WriterState, params: DidSaveTextDocumentParams) {
    let uri = params.text_document.uri;
    // A save may have introduced new includes/bibliography since the document
    // was opened; load them on the writer so the debounced pass's snapshot sees
    // them. (The dispatch write phase reloads too, but doing it here keeps
    // interactive reads in the debounce window consistent.)
    if let Some((salsa_file, salsa_config, Some(path))) = w
        .document_map()
        .get(&uri.to_string())
        .map(|doc| (doc.salsa_file, doc.salsa_config, doc.path.clone()))
    {
        load_project_files(w, salsa_file, salsa_config, path);
    }
    // Save is the heavy pass: external linters for the saved document. Debounced
    // like every other settle so a save-all burst coalesces into one pass.
    w.arm_settle_external(uri);
}

/// Handle `textDocument/didClose`.
pub(crate) fn did_close(w: &mut WriterState, params: DidCloseTextDocumentParams) {
    let uri = params.text_document.uri.clone();
    let uri_string = uri.to_string();
    w.document_map_mut().remove(&uri_string);

    // Drop the closed document's own diagnostics immediately so a pull issued
    // before the next settle no longer reports it (push: empty publish). Any
    // manifests it contributed are reconciled by the settle armed below: the
    // all-docs pass re-lints the remaining documents and the clear-on-fix diff
    // clears a manifest once no open document still reports it.
    w.drop_diagnostics(&uri);

    // Reuse the shared projection instead of cloning every remaining
    // `DocumentState` (each carries a `GreenNode` CST) just to read three
    // fields. `open_documents()` already yields exactly the path-backed
    // `(salsa_file, salsa_config, path)` tuples this loop needs.
    let mut retained = HashSet::new();
    for (salsa_file, salsa_config, path) in w.open_documents() {
        let tracked = load_project_files(w, salsa_file, salsa_config, path);
        retained.extend(tracked);
    }
    for cached in w.db().cached_file_paths() {
        if retained.contains(&cached) {
            continue;
        }
        let _ = w.db_mut().evict_file_text(&cached);
    }

    // Closing a document changes the database for the remaining open docs (a
    // closed include affects its parent), and the eviction above may cancel an
    // in-flight pass. Arm the settle so the remaining docs are re-linted over the
    // post-close snapshot.
    w.arm_settle();
}

#[cfg(test)]
mod tests {
    use super::apply_changes_descending_with_combined_ranges;
    use lsp_types::{Position, Range, TextDocumentContentChangeEvent};

    /// A post-panic resync for a URI with no cached document state (a `did_open`
    /// that died between its salsa text write and the map insert) must still arm
    /// the settle the dead handler never reached — otherwise the orphaned salsa
    /// input is never reconciled and no re-lint runs.
    #[test]
    fn resync_absent_document_arms_settle() {
        use crate::lsp::global_state::ClientSender;
        use crate::lsp::writer::WriterState;

        let (tx, _rx) = crossbeam_channel::unbounded();
        let mut w = WriterState::new(ClientSender::new(tx));
        let uri: lsp_types::Uri = "file:///ghost.qmd".parse().unwrap();
        assert!(w.settle_deadline().is_none(), "no settle armed initially");

        super::resync_document_after_panic(&mut w, &uri, Some(0));

        assert!(
            w.settle_deadline().is_some(),
            "absent-document resync must still arm the settle"
        );
    }

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
