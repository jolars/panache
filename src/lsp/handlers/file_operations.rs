//! Workspace file-operation notifications (`didCreateFiles`/`didRenameFiles`/
//! `didDeleteFiles`).
//!
//! These are hygiene-only: they re-intern the affected paths into the salsa
//! `FileSet` (so `project_graph`, the set's only reader, re-runs and observes the
//! change), reload every open document's referenced files on the writer, and arm
//! the debounced settle so dependents re-lint over the fresh state. Delete also
//! evicts cached text and clears the deleted URI's own diagnostics.
//!
//! We deliberately do **not** register the `willCreateFiles`/`willDeleteFiles`
//! requests: creating a file inserts no scaffolding, and deleting one never
//! returns a destructive `WorkspaceEdit` — broken references simply surface as
//! diagnostics (`include-not-found`, etc.) on the next settle. The reference
//! rewriting on rename lives in the `willRenameFiles` request
//! ([`super::file_rename`]); this module only keeps server state coherent after
//! the operation has happened.
//!
//! These overlap with [`super::file_watcher::did_change_watched_files`] for
//! on-disk changes, but fire on editor-explorer operations even when a client
//! lacks `didChangeWatchedFiles` dynamic registration, and the delete path here
//! additionally clears the removed file's diagnostics.

use std::path::PathBuf;

use lsp_types::{CreateFilesParams, DeleteFilesParams, RenameFilesParams, Uri};

use crate::lsp::documents::reload_open_documents_referenced_files;
use crate::lsp::global_state::GlobalState;
use crate::lsp::uri_ext::UriExt;

/// Parse a file-operation URI string into a filesystem path.
fn op_uri_to_path(uri: &str) -> Option<PathBuf> {
    uri.parse::<Uri>()
        .ok()
        .and_then(|uri| uri.to_file_path().map(|p| p.into_owned()))
}

/// Handle `workspace/didCreateFiles`: pull each new file into the graph.
///
/// Interning flips a referenced-but-missing path's `None`->`Some` text input
/// (after the reload below), so a dependent's `include-not-found` clears.
pub(crate) fn did_create_files(gs: &mut GlobalState, params: CreateFilesParams) {
    for file in &params.files {
        if let Some(path) = op_uri_to_path(&file.uri) {
            gs.salsa.intern_file(Some(path));
        }
    }
    reload_open_documents_referenced_files(gs);
    gs.arm_settle();
}

/// Handle `workspace/didDeleteFiles`: drop each removed file from the graph.
///
/// Clears the deleted file's own diagnostics, evicts its cached text, and
/// re-interns it so `project_graph` re-runs and its filesystem probes observe
/// the absence — broken references in dependents surface on the next settle.
pub(crate) fn did_delete_files(gs: &mut GlobalState, params: DeleteFilesParams) {
    for file in &params.files {
        if let Ok(uri) = file.uri.parse::<Uri>() {
            gs.diagnostics
                .drop_uri(&uri, &gs.sender, gs.supports_pull_diagnostics);
        }
        if let Some(path) = op_uri_to_path(&file.uri) {
            gs.salsa.evict_file_text(&path);
            gs.salsa.intern_file(Some(path));
        }
    }
    reload_open_documents_referenced_files(gs);
    gs.arm_settle();
}

/// Handle `workspace/didRenameFiles`: treat each rename as delete-old +
/// create-new for graph purposes.
///
/// Open-document relocation is intentionally left to the editor's standard
/// `didClose(old)`/`didOpen(new)` pair; we only re-intern paths and re-lint so
/// references to the old name break and references to the new name resolve.
pub(crate) fn did_rename_files(gs: &mut GlobalState, params: RenameFilesParams) {
    for rename in &params.files {
        if let Ok(old_uri) = rename.old_uri.parse::<Uri>() {
            gs.diagnostics
                .drop_uri(&old_uri, &gs.sender, gs.supports_pull_diagnostics);
        }
        if let Some(old_path) = op_uri_to_path(&rename.old_uri) {
            gs.salsa.evict_file_text(&old_path);
            gs.salsa.intern_file(Some(old_path));
        }
        if let Some(new_path) = op_uri_to_path(&rename.new_uri) {
            gs.salsa.intern_file(Some(new_path));
        }
    }
    reload_open_documents_referenced_files(gs);
    gs.arm_settle();
}
