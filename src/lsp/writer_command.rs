//! Typed write commands.
//!
//! Every notification that mutates the salsa database is modeled as a
//! [`WriteCommand`]. The main loop builds one and hands it to
//! [`GlobalState::apply_write`](crate::lsp::global_state::GlobalState::apply_write),
//! the single chokepoint through which all writes flow — applied synchronously
//! in inline mode, serialized across the channel to the dedicated writer thread
//! in threaded mode.

use lsp_types::{
    CreateFilesParams, DeleteFilesParams, DidChangeConfigurationParams,
    DidChangeTextDocumentParams, DidChangeWatchedFilesParams, DidChangeWorkspaceFoldersParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    RenameFilesParams,
};

/// A database-mutating notification, ready to apply. Variants mirror the LSP
/// notification names one-to-one, so the shared `Did` prefix is intentional.
#[allow(clippy::enum_variant_names)]
pub(crate) enum WriteCommand {
    DidOpen(DidOpenTextDocumentParams),
    DidChange(DidChangeTextDocumentParams),
    DidSave(DidSaveTextDocumentParams),
    DidClose(DidCloseTextDocumentParams),
    DidChangeConfiguration(DidChangeConfigurationParams),
    DidChangeWatchedFiles(DidChangeWatchedFilesParams),
    DidChangeWorkspaceFolders(DidChangeWorkspaceFoldersParams),
    DidCreateFiles(CreateFilesParams),
    DidRenameFiles(RenameFilesParams),
    DidDeleteFiles(DeleteFilesParams),
    /// Panics inside `apply_write`, standing in for a buggy handler: the
    /// writer thread must survive it (tests only).
    #[cfg(test)]
    PanicForTest,
}

impl WriteCommand {
    /// The single document a panicked handler may have left half-updated,
    /// plus the client version the command carried (when it does): the writer
    /// loop's post-panic heal target. `None` for commands without one
    /// mutation target (watcher events, config pushes, file operations) —
    /// their handlers mutate via the same smaller steps the settle re-runs,
    /// so there is no single document to rebuild.
    ///
    /// `DidClose` is deliberately `None` too: rebuilding a document the
    /// client just closed would resurrect it server-side, and a partial
    /// close leaves nothing divergent (the tree is dropped with the entry).
    pub(crate) fn heal_target(&self) -> Option<(lsp_types::Uri, Option<i32>)> {
        match self {
            WriteCommand::DidOpen(params) => Some((
                params.text_document.uri.clone(),
                Some(params.text_document.version),
            )),
            WriteCommand::DidChange(params) => Some((
                params.text_document.uri.clone(),
                Some(params.text_document.version),
            )),
            WriteCommand::DidSave(params) => Some((params.text_document.uri.clone(), None)),
            _ => None,
        }
    }
}
