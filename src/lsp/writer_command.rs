//! Typed write commands.
//!
//! Every notification that mutates the salsa database is modeled as a
//! [`WriteCommand`]. The main loop builds one and hands it to
//! [`GlobalState::apply_write`](crate::lsp::global_state::GlobalState::apply_write),
//! the single chokepoint through which all writes flow. Today `apply_write` runs
//! synchronously on the main loop; a later phase relocates the salsa side of it
//! onto a dedicated writer thread, at which point `WriteCommand` becomes the
//! message serialized across the channel. Centralizing writes behind one enum
//! now is what makes that relocation a transport change rather than a re-wiring
//! of every handler.

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
}
