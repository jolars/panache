//! Tests for inline ↔ reference link conversion code actions.

use super::helpers::*;
use lsp_types::*;

fn find_action_title<'a>(
    actions: &'a [CodeActionOrCommand],
    needle: &str,
) -> Option<&'a CodeAction> {
    actions.iter().find_map(|action| match action {
        CodeActionOrCommand::CodeAction(ca) if ca.title.contains(needle) => Some(ca),
        _ => None,
    })
}

#[test]
fn offers_convert_inline_link_to_reference() {
    let mut server = TestLspServer::new();
    let content = "See [the docs](https://example.com/) here.\n";
    server.open_document("file:///test.md", content, "markdown");

    // Cursor inside the inline link's text.
    let actions = server
        .get_code_actions("file:///test.md", 0, 6, 0, 6)
        .expect("code actions response");

    let convert = find_action_title(&actions, "Convert to reference link")
        .expect("Convert to reference link action");
    assert_eq!(convert.kind.as_ref().unwrap().as_str(), "refactor");

    let edits = convert
        .edit
        .as_ref()
        .and_then(|e| e.changes.as_ref())
        .and_then(|m| m.values().next())
        .expect("workspace edits");
    assert_eq!(edits.len(), 2, "in-place rewrite + new refdef");
    assert_eq!(edits[0].new_text, "[the docs][the-docs]");
    assert!(
        edits[1]
            .new_text
            .contains("[the-docs]: https://example.com/\n")
    );
}

#[test]
fn offers_convert_reference_link_to_inline_and_drops_orphan_def() {
    let mut server = TestLspServer::new();
    let content = "[docs][d]\n\n[d]: https://example.com/\n";
    server.open_document("file:///test.md", content, "markdown");

    let actions = server
        .get_code_actions("file:///test.md", 0, 2, 0, 2)
        .expect("code actions response");

    let convert = find_action_title(&actions, "Convert to inline link")
        .expect("Convert to inline link action");
    let edits = convert
        .edit
        .as_ref()
        .and_then(|e| e.changes.as_ref())
        .and_then(|m| m.values().next())
        .expect("workspace edits");
    assert_eq!(edits.len(), 2, "in-place rewrite + def deletion");
    assert_eq!(edits[0].new_text, "[docs](https://example.com/)");
    assert_eq!(edits[1].new_text, "", "deletes the orphaned def line");
}

#[test]
fn reference_to_inline_keeps_shared_def() {
    let mut server = TestLspServer::new();
    let content = "[a][d] and [b][d]\n\n[d]: https://example.com/\n";
    server.open_document("file:///test.md", content, "markdown");

    let actions = server
        .get_code_actions("file:///test.md", 0, 1, 0, 1)
        .expect("code actions response");

    let convert = find_action_title(&actions, "Convert to inline link")
        .expect("Convert to inline link action");
    let edits = convert
        .edit
        .as_ref()
        .and_then(|e| e.changes.as_ref())
        .and_then(|m| m.values().next())
        .expect("workspace edits");
    assert_eq!(edits.len(), 1, "shared def must stay in place");
}

#[test]
fn inline_to_reference_reuses_existing_def() {
    let mut server = TestLspServer::new();
    let content = "See [the docs](https://example.com/) here.\n\n[home]: https://example.com/\n";
    server.open_document("file:///test.md", content, "markdown");

    let actions = server
        .get_code_actions("file:///test.md", 0, 6, 0, 6)
        .expect("code actions response");

    let convert = find_action_title(&actions, "Convert to reference link")
        .expect("Convert to reference link action");
    let edits = convert
        .edit
        .as_ref()
        .and_then(|e| e.changes.as_ref())
        .and_then(|m| m.values().next())
        .expect("workspace edits");
    assert_eq!(edits.len(), 1, "matching url+title — reuse existing def");
    assert_eq!(edits[0].new_text, "[the docs][home]");
}
