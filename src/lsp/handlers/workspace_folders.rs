//! Handler for `workspace/didChangeWorkspaceFolders`.

use lsp_types::DidChangeWorkspaceFoldersParams;

use crate::lsp::global_state::GlobalState;
use crate::lsp::uri_ext::UriExt;

/// Apply a workspace-folder change: drop removed folders, append added ones,
/// then re-resolve config for every open document.
///
/// Config is resolved per-document (longest-prefix match against the folder
/// list) and cached in each `DocumentState.salsa_config`, so a folder change can
/// change which `panache.toml` applies to an already-open document. Refresh them
/// all and re-lint over the fresh state --- the same path the config-file watcher
/// uses (see [`crate::lsp::handlers::file_watcher`]).
pub(crate) fn did_change_workspace_folders(
    gs: &mut GlobalState,
    params: DidChangeWorkspaceFoldersParams,
) {
    let removed: Vec<_> = params
        .event
        .removed
        .iter()
        .filter_map(|folder| folder.uri.to_file_path().map(|p| p.into_owned()))
        .collect();
    gs.workspace_folders
        .retain(|folder| !removed.contains(folder));

    for folder in &params.event.added {
        if let Some(path) = folder.uri.to_file_path().map(|p| p.into_owned())
            && !gs.workspace_folders.contains(&path)
        {
            gs.workspace_folders.push(path);
        }
    }

    crate::lsp::documents::reload_open_documents_config(gs);
    gs.arm_settle();
}
