//! LSP Integration Tests
//!
//! These tests validate multi-step LSP protocol flows using an in-memory
//! test harness. They complement the unit tests in handler modules by
//! testing realistic workflows (open→edit→format→diagnostics) without
//! spawning external processes.

// The lsp feature is required for these tests
#![cfg(feature = "lsp")]

mod lsp {
    pub(super) mod helpers;
    pub(super) mod test_diagnostics;
    pub(super) mod test_document_lifecycle;
    pub(super) mod test_formatting;
    pub(super) mod test_incremental_edits;
    pub(super) mod test_navigation;
}
