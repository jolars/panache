//! LSP Integration Tests
//!
//! These tests validate multi-step LSP protocol flows using an in-memory
//! test harness. They complement the unit tests in handler modules by
//! testing realistic workflows (open→edit→format→diagnostics) without
//! spawning external processes.

// The lsp feature is required for these tests
#![cfg(feature = "lsp")]
// `lsp_types::Uri` (fluent_uri-backed) trips `clippy::mutable_key_type` as a
// `HashMap` key, but `WorkspaceEdit.changes` mandates it and keys are never mutated.
#![allow(clippy::mutable_key_type)]

mod lsp {
    pub(super) mod helpers;
    pub(super) mod test_cancellation;
    pub(super) mod test_completion;
    pub(super) mod test_config_discovery;
    pub(super) mod test_config_errors;
    pub(super) mod test_config_reload;
    pub(super) mod test_diagnostics;
    pub(super) mod test_document_highlight;
    pub(super) mod test_document_lifecycle;
    pub(super) mod test_document_links;
    pub(super) mod test_file_operations;
    pub(super) mod test_file_rename;
    pub(super) mod test_file_watcher;
    pub(super) mod test_formatting;
    pub(super) mod test_goto_definition;
    pub(super) mod test_hover;
    pub(super) mod test_incremental_edits;
    pub(super) mod test_link_conversion;
    pub(super) mod test_linked_editing_range;
    pub(super) mod test_navigation;
    pub(super) mod test_on_type_formatting;
    pub(super) mod test_prepare_rename;
    pub(super) mod test_pull_diagnostics;
    pub(super) mod test_references;
    pub(super) mod test_rename;
    pub(super) mod test_semantic_tokens;
}
