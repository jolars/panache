//! `workspace/didChangeConfiguration` handler.
//!
//! Live-applies a client configuration change without a server restart: pushed
//! runtime settings (currently `experimental.incrementalParsing`) update in
//! place, on-disk `panache.toml` config is re-read for every open document, and
//! the debounced settle re-publishes diagnostics over the fresh state.

use lsp_types::DidChangeConfigurationParams;

use crate::lsp::dispatch::runtime_incremental_parsing_from_value;
use crate::lsp::documents;
use crate::lsp::global_state::GlobalState;

pub(crate) fn did_change_configuration(gs: &mut GlobalState, params: DidChangeConfigurationParams) {
    // The push payload is optional: clients using the pull model send `null`.
    // Either way we still reload on-disk config below, so a bare notification is
    // a useful "reload config" signal.
    if !params.settings.is_null()
        && let Some(incremental) = runtime_incremental_parsing_from_value(&params.settings)
        && gs.runtime_settings.experimental_incremental_parsing != incremental
    {
        log::debug!(
            "lsp runtime setting experimental.incrementalParsing={incremental} \
             (didChangeConfiguration)"
        );
        gs.runtime_settings.experimental_incremental_parsing = incremental;
    }

    documents::reload_open_documents_config(gs);
    gs.arm_settle();
}
