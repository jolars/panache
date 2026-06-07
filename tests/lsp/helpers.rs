//! Test helpers for LSP integration testing.
//!
//! The synchronous in-process harness lives in the crate
//! (`panache::lsp::LspTester`); this module re-exports it as `TestLspServer`
//! and provides small change-event constructors.

use lsp_types::*;

pub use panache::lsp::{LspTester as TestLspServer, UriExt};

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
