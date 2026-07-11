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
