//! Test helpers for LSP integration testing.
//!
//! The synchronous in-process harness lives in the crate
//! (`panache::lsp::LspTester`); this module re-exports it as `TestLspServer`
//! and provides small change-event constructors.

use lsp_types::*;

pub use panache::lsp::{LspTester as TestLspServer, UriExt};

/// Whether `PANACHE_INCREMENTAL_PARSING` is forcing the incremental-parsing
/// flag, which makes assertions about the *client settings* plumbing moot.
///
/// Only tests that assert the flag's value, or that need it in a particular
/// state, use these guards; every test that asserts document *behavior* must
/// pass either way, which is the whole point of running the suite with the
/// override set.
pub fn incremental_parsing_forced_by_env() -> bool {
    std::env::var("PANACHE_INCREMENTAL_PARSING").is_ok()
}

/// Whether the environment override is forcing incremental parsing *off*, so a
/// test that needs it on cannot run.
pub fn incremental_parsing_forced_off() -> bool {
    matches!(
        std::env::var("PANACHE_INCREMENTAL_PARSING").as_deref(),
        Ok("0") | Ok("false")
    )
}

/// Helper to create a simple text change event (full document replacement).
pub fn full_document_change(text: &str) -> TextDocumentContentChangeEvent {
    TextDocumentContentChangeEvent {
        range: None,
        range_length: None,
        text: text.to_string(),
    }
}

/// Helper to create an incremental text change event.
pub fn incremental_change(
    start_line: u32,
    start_char: u32,
    end_line: u32,
    end_char: u32,
    text: &str,
) -> TextDocumentContentChangeEvent {
    TextDocumentContentChangeEvent {
        range: Some(Range {
            start: Position {
                line: start_line,
                character: start_char,
            },
            end: Position {
                line: end_line,
                character: end_char,
            },
        }),
        range_length: None,
        text: text.to_string(),
    }
}
