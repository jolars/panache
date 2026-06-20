//! Tests for `textDocument/onTypeFormatting`.
//!
//! Scoped to continuation indentation after Enter inside a list item: the
//! handler aligns the new line to the item's content column. Whitespace only —
//! it never inserts a marker and never guesses new-item-vs-exit intent.

use super::helpers::*;

/// Pressing Enter at the end of a bullet item indents the new line to the
/// item's content column (2 for `- `).
#[test]
fn on_type_bullet_continuation_indents_two() {
    let mut server = TestLspServer::new();
    // Post-Enter buffer: trailing empty line, cursor at line 1, column 0.
    server.open_document("file:///t.md", "- first item\n", "markdown");

    let edits = server
        .on_type_formatting("file:///t.md", 1, 0, "\n")
        .expect("edits for list continuation");

    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "  ");
    assert_eq!(edits[0].range.start.line, 1);
    assert_eq!(edits[0].range.start.character, 0);
}

/// Ordered markers are wider, so the content column is 3 for `1. `.
#[test]
fn on_type_ordered_continuation_indents_three() {
    let mut server = TestLspServer::new();
    server.open_document("file:///t.md", "1. first item\n", "markdown");

    let edits = server
        .on_type_formatting("file:///t.md", 1, 0, "\n")
        .expect("edits for ordered continuation");

    assert_eq!(edits[0].new_text, "   ");
}

/// A nested item's marker column already encodes its depth, so continuation of
/// `  - inner` lands at column 4.
#[test]
fn on_type_nested_continuation_uses_marker_column() {
    let mut server = TestLspServer::new();
    server.open_document("file:///t.md", "- outer\n  - inner\n", "markdown");

    let edits = server
        .on_type_formatting("file:///t.md", 2, 0, "\n")
        .expect("edits for nested continuation");

    assert_eq!(edits[0].new_text, "    ");
}

/// When the new line already carries the right indentation (e.g. the editor
/// auto-indented), no edit is produced.
#[test]
fn on_type_already_indented_returns_none() {
    let mut server = TestLspServer::new();
    server.open_document("file:///t.md", "- first item\n  ", "markdown");

    let edits = server.on_type_formatting("file:///t.md", 1, 2, "\n");
    assert_eq!(edits, None);
}

/// Only the newline trigger acts; any other character is a no-op.
#[test]
fn on_type_non_newline_trigger_is_noop() {
    let mut server = TestLspServer::new();
    server.open_document("file:///t.md", "- first item\n", "markdown");

    let edits = server.on_type_formatting("file:///t.md", 1, 0, ";");
    assert_eq!(edits, None);
}

/// Outside any list there is nothing to continue.
#[test]
fn on_type_outside_list_returns_none() {
    let mut server = TestLspServer::new();
    server.open_document("file:///t.md", "a plain paragraph\n", "markdown");

    let edits = server.on_type_formatting("file:///t.md", 1, 0, "\n");
    assert_eq!(edits, None);
}

/// The server advertises the capability with `\n` as the trigger character so
/// clients know to fire the request.
#[test]
fn on_type_capability_advertised() {
    let mut server = TestLspServer::new();
    let result = server.initialize_result("file:///");

    let provider = result
        .capabilities
        .document_on_type_formatting_provider
        .expect("on-type formatting capability advertised");
    assert_eq!(provider.first_trigger_character, "\n");
}
