//! Document lifecycle notifications (`didOpen`/`didChange`/`didSave`/`didClose`).
//!
//! These run synchronously on the main-loop thread with `&mut GlobalState`: they
//! are the sole writers of the salsa database and the document map. They write
//! *inputs* only -- no parsing happens here. The tree is derived on demand from
//! `crate::salsa::parsed_document`, which splices incrementally off its own
//! side-channel base for documents these handlers have admitted, so the main
//! loop pays no parse time per keystroke. The expensive lint (project-graph
//! recompute + diagnostics) is deferred to the debounced workspace settle,
//! which re-lints every open document over one snapshot. Every salsa-input
//! write here arms that settle (directly or via
//! [`GlobalState::arm_settle_external`]) so a write that cancels an in-flight
//! pass also schedules its recomputation.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, MessageType,
};
use salsa::{Durability, Setter};

use super::conversions::apply_content_change;
use super::global_state::GlobalState;
use super::uri_ext::UriExt;
use crate::lsp::DocumentState;

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

/// Reload every open document's project-referenced files on the writer.
///
/// A filesystem change (watcher event, file operation) may have flipped a
/// referenced include/bibliography's `None`->`Some` text input (or vice versa);
/// loading here before the next snapshot lets the re-lint observe fresh content.
pub(crate) fn reload_open_documents_referenced_files(gs: &mut GlobalState) {
    let open_docs: Vec<(crate::salsa::FileText, crate::salsa::FileConfig, PathBuf)> = gs
        .document_map
        .values()
        .filter_map(|state| {
            let path = crate::salsa::Db::path_of_id(&gs.salsa, state.file_id)?;
            Some((state.salsa_file, state.salsa_config, path))
        })
        .collect();
    // A path open as a document has buffer-authoritative content; it must never
    // be re-read from disk below or an unsaved edit would be clobbered.
    let open_paths: HashSet<PathBuf> = gs
        .document_map
        .values()
        .filter_map(|state| crate::salsa::Db::path_of_id(&gs.salsa, state.file_id))
        .collect();
    let mut referenced: HashSet<PathBuf> = HashSet::new();
    for (salsa_file, salsa_config, path) in open_docs {
        referenced.extend(load_project_files(gs, salsa_file, salsa_config, path));
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
        gs.salsa
            .resync_cached_file_from_disk(&path, Durability::MEDIUM);
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
pub(crate) fn reload_open_documents_config(gs: &mut GlobalState) {
    let entries: Vec<(String, lsp_types::Uri)> = gs
        .document_map
        .keys()
        .filter_map(|uri_str| Some((uri_str.clone(), uri_str.parse().ok()?)))
        .collect();
    for (uri_str, uri) in entries {
        let new_config = gs.load_config_notifying(&uri);
        // Re-point at the shared handle for the reloaded value: a no-op when the
        // config is unchanged, and a switch to the value's shared handle when it
        // changed (rather than mutating a handle other documents may share).
        let interned = gs.intern_config(new_config);
        if let Some(state) = gs.document_map_mut().get_mut(&uri_str) {
            state.salsa_config = interned;
        }
    }
}

/// Handle `textDocument/didOpen`.
pub(crate) fn did_open(gs: &mut GlobalState, params: DidOpenTextDocumentParams) {
    let uri = params.text_document.uri.clone();
    let uri_string = uri.to_string();
    let text = params.text_document.text.clone();
    log::debug!("did_open uri={uri_string}, bytes={}", text.len());
    let start = Instant::now();

    let config = gs.load_config_notifying(&uri);
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
    // Share one `FileConfig` input across every document that resolves to this
    // config value, so config-keyed salsa queries memoize once across the
    // project rather than per document (see `GlobalState::intern_config`).
    let salsa_config = gs.intern_config(config.clone());

    // The freshly-registered input always has a `FileId`; key the document on
    // that stable identity instead of duplicating its path.
    let file_id = gs
        .salsa
        .file_id_for_input(salsa_file)
        .expect("just-registered document input has a FileId");

    // Admit the document to the incremental side channel, so the first parse
    // (demanded by the settle armed below, on a pool thread) records a base for
    // the next keystroke to splice against. With the flag off nothing is
    // admitted and every parse stays a full parse.
    if gs.runtime_settings.experimental_incremental_parsing {
        gs.salsa.reparse_admit(salsa_file, salsa_config);
    }

    gs.document_map_mut().insert(
        uri_string.clone(),
        DocumentState {
            file_id,
            salsa_file,
            salsa_config,
        },
    );

    if let Some(path) = doc_path.as_ref() {
        load_project_files(gs, salsa_file, salsa_config, path.clone());
    }

    gs.sender
        .log_message(MessageType::INFO, format!("Opened document: {uri_string}"));

    // Arm the workspace settle instead of spawning a lint inline: a
    // workspace-restore burst of opens each writes salsa, and an inline lint
    // would be cancelled by the next open's write. Open runs external linters
    // (like save) so their diagnostics surface without waiting for the first
    // manual save.
    gs.arm_settle_external(uri);
    log::debug!("did_open complete in {:?}", start.elapsed());
}

/// Handle `textDocument/didChange`.
pub(crate) fn did_change(gs: &mut GlobalState, params: DidChangeTextDocumentParams) {
    let uri = params.text_document.uri.clone();
    let uri_string = uri.to_string();
    let change_count = params.content_changes.len();
    log::debug!("did_change uri={uri_string}, changes={change_count}");
    let start = Instant::now();

    let config = gs.load_config_notifying(&uri);

    let Some((salsa_file, previous_config)) = gs
        .document_map
        .get(&uri_string)
        .map(|doc| (doc.salsa_file, doc.salsa_config))
    else {
        return;
    };

    // Any shape, any order: the changes are applied in the order the client
    // sent them, each against the text its predecessors produced, which is what
    // the protocol specifies. Nothing here needs to derive an edit range --
    // `parsed_document` recovers the one it needs by diffing the whole texts.
    let mut updated_text = salsa_file.content_or_empty(&gs.salsa).to_string();
    for change in params.content_changes.iter() {
        updated_text = apply_content_change(&updated_text, change);
    }

    let doc_path_for_salsa = uri.to_file_path().map(|p| p.into_owned());
    if let Some(path) = doc_path_for_salsa.as_ref() {
        gs.salsa.update_file_text(path.clone(), updated_text);
    } else {
        salsa_file
            .set_text(&mut gs.salsa)
            .with_durability(Durability::LOW)
            .to(Some(std::sync::Arc::from(updated_text)));
    }

    // Re-point the document at the shared handle for its (possibly reloaded)
    // config value. When the config is unchanged this is the same interned
    // handle the document already held, so it is a no-op; when it changed, the
    // document moves to the value's shared handle rather than mutating a handle
    // other documents may share (see `GlobalState::intern_config`).
    let interned_config = gs.intern_config(config);
    if let Some(doc_state) = gs.document_map_mut().get_mut(&uri_string) {
        // `file_id`/`salsa_file` are invariant across a content edit (the same
        // path resolves to the same interned input), so only the (possibly
        // re-interned) config handle needs refreshing.
        doc_state.salsa_config = interned_config;
    } else {
        return;
    }

    // Re-admit under whatever config handle the document now holds: a config
    // reload mints a new one, and the base recorded under the old handle can
    // never be hit again. Only when it actually moved -- `did_open` (or the
    // runtime toggle) already admitted the handle the document arrived with,
    // and re-admitting is a scan of the channel on a path that runs per
    // keystroke.
    if gs.runtime_settings.experimental_incremental_parsing && interned_config != previous_config {
        gs.salsa.reparse_admit(salsa_file, interned_config);
    }

    // No parse here at all. The settle demands it within the debounce window,
    // on a pool thread, and salsa's per-key claim dedupes concurrent demands --
    // so the main loop stops paying parse time per keystroke.
    gs.arm_settle();

    log::debug!(
        "did_change complete (state) in {:?}; settle armed",
        start.elapsed()
    );
}

/// Handle `textDocument/didSave`.
///
/// Save is the point at which heavier external linters run (skipped on every
/// keystroke). The fresh settle re-lints every open document; only the saved
/// document runs external linters.
pub(crate) fn did_save(gs: &mut GlobalState, params: DidSaveTextDocumentParams) {
    let uri = params.text_document.uri;
    // A save may have introduced new includes/bibliography since the document
    // was opened; load them on the writer so the debounced pass's snapshot sees
    // them. (The dispatch write phase reloads too, but doing it here keeps
    // interactive reads in the debounce window consistent.)
    if let Some((salsa_file, salsa_config, Some(path))) =
        gs.document_map.get(&uri.to_string()).map(|doc| {
            (
                doc.salsa_file,
                doc.salsa_config,
                crate::salsa::Db::path_of_id(&gs.salsa, doc.file_id),
            )
        })
    {
        load_project_files(gs, salsa_file, salsa_config, path);
    }
    // Save is the heavy pass: external linters for the saved document. Debounced
    // like every other settle so a save-all burst coalesces into one pass.
    gs.arm_settle_external(uri);
}

/// Handle `textDocument/didClose`.
pub(crate) fn did_close(gs: &mut GlobalState, params: DidCloseTextDocumentParams) {
    let uri = params.text_document.uri.clone();
    let uri_string = uri.to_string();
    // Retire the reparse base before the map entry goes: after the removal
    // there is no way left to recover the document's salsa input.
    if let Some(state) = gs.document_map.get(&uri_string) {
        let salsa_file = state.salsa_file;
        gs.salsa.reparse_retire_file(salsa_file);
    }
    gs.document_map_mut().remove(&uri_string);

    // Drop the closed document's own diagnostics immediately so a pull issued
    // before the next settle no longer reports it (push: empty publish). Any
    // manifests it contributed are reconciled by the settle armed below: the
    // all-docs pass re-lints the remaining documents and the clear-on-fix diff
    // clears a manifest once no open document still reports it.
    gs.diagnostics
        .drop_uri(&uri, &gs.sender, gs.supports_pull_diagnostics);

    let states: Vec<DocumentState> = gs.document_map.values().cloned().collect();
    let mut retained = HashSet::new();
    for state in states {
        let Some(path) = crate::salsa::Db::path_of_id(&gs.salsa, state.file_id) else {
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

    // Closing a document changes the database for the remaining open docs (a
    // closed include affects its parent), and the eviction above may cancel an
    // in-flight pass. Arm the settle so the remaining docs are re-linted over the
    // post-close snapshot.
    gs.arm_settle();
}
