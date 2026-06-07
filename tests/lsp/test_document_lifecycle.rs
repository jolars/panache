//! Tests for basic document lifecycle (open, edit, close).

use super::helpers::*;
use lsp_types::*;

#[test]
fn test_open_document() {
    let mut server = TestLspServer::new();

    // Open a simple document
    server.open_document("file:///test.qmd", "# Hello\n\nWorld", "quarto");

    // Verify document is in state
    let content = server.get_document_content("file:///test.qmd");
    assert_eq!(content, Some("# Hello\n\nWorld".to_string()));
}

#[test]
fn test_close_document() {
    let mut server = TestLspServer::new();

    // Open then close
    server.open_document("file:///test.qmd", "# Hello", "quarto");
    server.close_document("file:///test.qmd");

    // Verify document is removed from state
    let content = server.get_document_content("file:///test.qmd");
    assert_eq!(content, None);
}

#[test]
fn test_edit_document_full_replace() {
    let mut server = TestLspServer::new();

    // Open document
    server.open_document("file:///test.qmd", "# Original", "quarto");

    // Edit with full replacement
    server.edit_document(
        "file:///test.qmd",
        vec![full_document_change("# Modified\n\nNew content")],
    );

    // Verify content changed
    let content = server.get_document_content("file:///test.qmd");
    assert_eq!(content, Some("# Modified\n\nNew content".to_string()));
}

#[test]
fn test_server_advertises_save_capability() {
    // External linters run on save (not per keystroke), so the server must ask
    // clients to send `textDocument/didSave`.
    let mut server = TestLspServer::new();
    let result = server.initialize_result("file:///workspace");

    let Some(TextDocumentSyncCapability::Options(sync)) = result.capabilities.text_document_sync
    else {
        panic!("expected TextDocumentSync options");
    };
    assert!(
        matches!(
            sync.save,
            Some(TextDocumentSyncSaveOptions::SaveOptions(_))
                | Some(TextDocumentSyncSaveOptions::Supported(true))
        ),
        "server should advertise save support, got {:?}",
        sync.save
    );
}

#[test]
fn test_open_edit_save_keeps_document_state() {
    // The save path cancels any pending debounced lint and runs a full pass;
    // document state must remain consistent across open -> edit -> save.
    let mut server = TestLspServer::new();
    server.initialize("file:///workspace");
    server.open_document("file:///test.qmd", "# Original", "quarto");
    server.edit_document(
        "file:///test.qmd",
        vec![full_document_change("# Edited\n\nBody")],
    );

    server.save_document("file:///test.qmd");

    // Edited content survives the save pass and the document is still tracked.
    let content = server.get_document_content("file:///test.qmd");
    assert_eq!(content, Some("# Edited\n\nBody".to_string()));
}
