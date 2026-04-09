//! CLI integration tests for panache.
//!
//! These tests execute the compiled binary and verify CLI behavior including:
//! - Subcommand behavior (format, parse, lint, lsp)
//! - Stdin/stdout handling
//! - Exit codes
//! - File I/O operations
//! - Error handling

mod common;
mod debug;
mod format;
mod lint;
mod parse;
mod translate;

#[cfg(feature = "lsp")]
mod lsp;
