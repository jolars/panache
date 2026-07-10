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
    RenameFilesParams, Uri,
};

/// Main-loop side effects a write handler requests.
///
/// Write handlers run against writer-owned state (`&mut WriterHandle`) and must
/// not touch `GlobalState`, so anything that belongs to the main loop — the
/// settle timer, `external_pending`, the diagnostics store — is requested here
/// and applied by
/// [`GlobalState::apply_write_effects`](crate::lsp::global_state::GlobalState::apply_write_effects)
/// after the handler returns. When writes relocate onto the writer thread, this
/// struct is what travels back to the main loop.
#[derive(Default)]
pub(crate) struct WriteEffects {
    /// Arm the debounced workspace settle.
    pub(crate) settle: bool,
    /// Arm the settle AND run external linters for these URIs on the next pass.
    pub(crate) external: Vec<Uri>,
    /// Drop these URIs from the diagnostics store immediately (closed/deleted
    /// documents), ahead of the settle's clear-on-fix diff.
    pub(crate) dropped: Vec<Uri>,
}

impl WriteEffects {
    /// Request the debounced workspace settle.
    pub(crate) fn arm_settle(&mut self) {
        self.settle = true;
    }

    /// Request the settle and external linters for `uri` on the next pass.
    pub(crate) fn arm_settle_external(&mut self, uri: Uri) {
        self.external.push(uri);
    }

    /// Request `uri`'s immediate removal from the diagnostics store.
    pub(crate) fn drop_diagnostics(&mut self, uri: Uri) {
        self.dropped.push(uri);
    }
}

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
