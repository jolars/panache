//! Tests for diagnostic workflows (linting + code actions).

use super::helpers::*;
use lsp_types::*;
use std::time::Duration;

#[test]
fn test_diagnostics_on_heading_hierarchy_issue() {
    let mut server = TestLspServer::new();

    // Open a document with heading hierarchy issue (h1 → h3 skip)
    let content = "# Heading 1\n\n### Heading 3\n\nContent.";
    server.open_document("file:///test.qmd", content, "quarto");

    // Note: Diagnostics are published via client.publish_diagnostics()
    // which we can't easily capture in this test setup.
    // For now, we'll test code actions which are request/response.

    // Request code actions for the h3 line
    let code_actions = server.get_code_actions(
        "file:///test.qmd",
        0, // Whole document range to include full diagnostic span
        0,
        4,
        99,
    );

    // Should have a quick fix for heading hierarchy
    assert!(code_actions.is_some());
    let actions = code_actions.unwrap();

    // Find the heading hierarchy fix
    let fix = actions.iter().find(|action| {
        if let CodeActionOrCommand::CodeAction(ca) = action {
            ca.title.contains("heading")
        } else {
            false
        }
    });

    assert!(fix.is_some(), "Should have heading hierarchy fix");
}

#[test]
fn test_did_change_publishes_diagnostics_to_client() {
    let mut server = TestLspServer::new();
    server.initialize("file:///workspace");
    let uri = "file:///workspace/doc.qmd";

    // Open a clean document and drain the immediate did_open publish so it
    // doesn't pollute the assertion below.
    server.open_document(uri, "# H1\n", "quarto");
    server.pump(Duration::from_secs(2));
    server.drain_client_messages();

    // Edit to introduce a heading hierarchy violation; lint is debounced.
    server.edit_document(uri, vec![full_document_change("# H1\n\n### H3 skip\n")]);
    server.pump(Duration::from_secs(2));

    let publishes = server.drain_publish_diagnostics(uri);
    assert!(
        !publishes.is_empty(),
        "expected at least one publishDiagnostics from debounced did_change"
    );
    let diags = &publishes
        .last()
        .expect("publishes is non-empty per assertion above")
        .diagnostics;
    assert!(
        diags.iter().any(|d| matches!(
            d.code.as_ref(),
            Some(NumberOrString::String(s)) if s == "heading-hierarchy"
        )),
        "expected heading-hierarchy diagnostic in published list, got: {diags:?}"
    );
}

#[test]
fn test_code_actions_filter_quickfixes_to_requested_range() {
    let mut server = TestLspServer::new();
    let content = "# Heading 1\n\n### Heading 3\n\nContent.\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let code_actions = server
        .get_code_actions(
            "file:///test.qmd",
            0, // Request only first line range
            0,
            0,
            20,
        )
        .expect("code actions response");

    let has_heading_fix = code_actions.iter().any(|action| {
        if let CodeActionOrCommand::CodeAction(ca) = action {
            ca.title.contains("heading")
        } else {
            false
        }
    });

    assert!(
        !has_heading_fix,
        "Heading hierarchy quickfix should not appear outside requested range"
    );
}

#[test]
fn test_code_actions_require_diagnostic_to_be_fully_within_requested_range() {
    let mut server = TestLspServer::new();
    let content = "# Heading 1\n\n### Heading 3\n\nContent.\n";
    server.open_document("file:///test.qmd", content, "quarto");

    // This range intersects the heading diagnostic line but does not fully contain
    // the full heading range.
    let code_actions = server
        .get_code_actions(
            "file:///test.qmd",
            2, // "### Heading 3"
            0,
            2,
            1, // very narrow range
        )
        .expect("code actions response");

    let has_heading_fix = code_actions.iter().any(|action| {
        if let CodeActionOrCommand::CodeAction(ca) = action {
            ca.title.contains("heading")
        } else {
            false
        }
    });

    assert!(
        !has_heading_fix,
        "Quickfix should not be offered when request only partially overlaps diagnostic range"
    );
}

#[test]
fn test_code_actions_offer_quickfix_for_cursor_inside_diagnostic() {
    let mut server = TestLspServer::new();
    let content = "# Heading 1\n\n### Heading 3\n\nContent.\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let code_actions = server
        .get_code_actions(
            "file:///test.qmd",
            2, // "### Heading 3"
            2, // cursor inside heading text
            2,
            2, // zero-width LSP cursor range
        )
        .expect("code actions response");

    let has_heading_fix = code_actions.iter().any(|action| {
        if let CodeActionOrCommand::CodeAction(ca) = action {
            ca.title.contains("heading")
        } else {
            false
        }
    });

    assert!(
        has_heading_fix,
        "Quickfix should be offered when cursor is inside diagnostic range"
    );
}

#[test]
fn test_code_actions_offer_source_fix_all_for_fixable_diagnostics() {
    let mut server = TestLspServer::new();
    let content = "# Heading 1\n\n### Heading 3\n\nContent.\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let code_actions = server
        .get_code_actions("file:///test.qmd", 0, 0, 4, 99)
        .expect("code actions response");

    let fix_all = code_actions.iter().find_map(|action| {
        if let CodeActionOrCommand::CodeAction(ca) = action
            && ca.kind == Some(CodeActionKind::SOURCE_FIX_ALL)
        {
            return Some(ca);
        }
        None
    });
    let fix_all = fix_all.expect("expected source.fixAll code action");

    let edits = fix_all
        .edit
        .as_ref()
        .and_then(|edit| edit.changes.as_ref())
        .and_then(|changes| changes.get(&"file:///test.qmd".parse::<Uri>().expect("uri")))
        .expect("source.fixAll workspace edits");
    assert!(
        !edits.is_empty(),
        "source.fixAll should include at least one text edit"
    );
}

/// Saving a document in a multi-file include graph drives
/// `relint_with_dependents`, which reads `project_graph` (and the lint plan) on
/// *cloned* salsa handles with the global lock released, then performs
/// `ensure_file_text_cached` writes for the tracked include paths. This guards
/// that the cloned-handle read path stays live: the save completes (the timeout
/// catches any hang from a read handle outliving a write) without panicking, and
/// the parent's built-in lint plan still resolves afterwards.
#[test]
fn test_save_in_include_graph_completes_and_resolves_diagnostics() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root = temp_dir.path();
    let parent_path = root.join("parent.qmd");
    let child_path = root.join("child.qmd");

    std::fs::write(&child_path, "# Child Heading\n\n### Skipped Level\n").unwrap();
    std::fs::write(
        &parent_path,
        "{{< include child.qmd >}}\n\n# Parent Heading\n",
    )
    .unwrap();

    let mut server = TestLspServer::new();
    let root_uri = Uri::from_file_path(root).unwrap().to_string();
    server.initialize(&root_uri);

    let parent_uri = Uri::from_file_path(&parent_path).unwrap().to_string();
    server.open_document(
        &parent_uri,
        &std::fs::read_to_string(&parent_path).unwrap(),
        "quarto",
    );

    // `did_save` dispatches the relint synchronously on the main thread; with a
    // single-threaded sync model there is no read/write handle deadlock to guard.
    server.save_document(&parent_uri);

    assert!(
        server.get_built_in_diagnostics(&parent_uri).is_some(),
        "built-in diagnostics should still resolve after the save"
    );
}

#[test]
fn test_code_actions_do_not_offer_source_fix_all_without_fixes() {
    let mut server = TestLspServer::new();
    let content = "# Heading 1\n\n## Heading 2\n\nContent.\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let code_actions = server
        .get_code_actions("file:///test.qmd", 0, 0, 4, 99)
        .expect("code actions response");

    let has_fix_all = code_actions.iter().any(|action| {
        if let CodeActionOrCommand::CodeAction(ca) = action {
            ca.kind == Some(CodeActionKind::SOURCE_FIX_ALL)
        } else {
            false
        }
    });
    assert!(
        !has_fix_all,
        "source.fixAll should not be offered when there are no fixable diagnostics"
    );
}

#[test]
fn test_code_actions_no_refactors_inside_yaml_frontmatter() {
    let mut server = TestLspServer::new();
    let content = "---\ntitle: Report\nlist:\n  - a\n  - b\n---\n\nBody.\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let code_actions = server
        .get_code_actions(
            "file:///test.qmd",
            3, // inside frontmatter list item
            2,
            3,
            8,
        )
        .expect("code actions response");

    let has_refactor = code_actions.iter().any(|action| {
        if let CodeActionOrCommand::CodeAction(ca) = action {
            ca.kind
                .as_ref()
                .is_some_and(|kind| *kind == CodeActionKind::REFACTOR)
        } else {
            false
        }
    });
    assert!(
        !has_refactor,
        "Refactor code actions should not be offered inside YAML frontmatter"
    );
}

#[test]
fn test_hashpipe_yaml_parse_error_in_built_in_lint_plan() {
    let mut server = TestLspServer::new();
    let content = "```{r}\n#| fig-cap: [\na <- 1\n```\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let diagnostics = server
        .get_built_in_diagnostics("file:///test.qmd")
        .expect("diagnostics");

    let yaml_parse_error = diagnostics
        .iter()
        .find(|diag| diag.code == "yaml-parse-error")
        .expect("expected yaml-parse-error diagnostic");
    assert!(
        yaml_parse_error.message.contains("YAML parse error"),
        "expected YAML parse error message, got: {}",
        yaml_parse_error.message
    );
}

#[test]
fn test_math_parse_error_in_built_in_lint_plan() {
    let mut server = TestLspServer::new();
    // Unclosed `{` group inside inline math (Quarto enables tex_math by default).
    let content = "The relation $E = mc^{2$ holds.\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let diagnostics = server
        .get_built_in_diagnostics("file:///test.qmd")
        .expect("diagnostics");

    let math_error = diagnostics
        .iter()
        .find(|diag| diag.code == "math-unclosed-group")
        .expect("expected math-unclosed-group diagnostic");
    // Span points at the unclosed `{` (line 1, column 22).
    assert_eq!(math_error.location.line, 1);
    assert_eq!(math_error.location.column, 22);
}

#[test]
fn test_hashpipe_folded_scalar_parse_error_maps_to_host_position() {
    let mut server = TestLspServer::new();
    let content = "```{r}\n#| fig-cap: >-\n#|   A folded caption\n#| bad: [\na <- 1\n```\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let diagnostics = server
        .get_built_in_diagnostics("file:///test.qmd")
        .expect("diagnostics");

    let yaml_parse_error = diagnostics
        .iter()
        .find(|diag| diag.code == "yaml-parse-error")
        .expect("expected yaml-parse-error diagnostic");
    assert_eq!(yaml_parse_error.location.line, 4);
    assert_eq!(yaml_parse_error.location.column, 9);
}

#[test]
fn test_code_action_convert_implicit_heading_link_to_explicit() {
    let mut server = TestLspServer::new();
    let content = "# Unordered Lists\n\n[unordered lists]\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let code_actions = server
        .get_code_actions("file:///test.qmd", 2, 2, 2, 18)
        .expect("code actions response");

    let action = code_actions.iter().find_map(|action| {
        if let CodeActionOrCommand::CodeAction(ca) = action
            && ca.title == "Convert to explicit heading link"
        {
            return Some(ca);
        }
        None
    });
    let action = action.expect("expected heading link conversion action");

    let changes = action
        .edit
        .as_ref()
        .and_then(|edit| edit.changes.as_ref())
        .expect("workspace edit changes");
    let edits = changes
        .get(&"file:///test.qmd".parse::<Uri>().expect("uri"))
        .expect("edits for document");
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "[unordered lists](#unordered-lists)");
}

#[test]
fn test_code_action_convert_bullet_list_to_ordered() {
    let mut server = TestLspServer::new();
    let content = "- First\n- Second\n- Third\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let code_actions = server
        .get_code_actions("file:///test.qmd", 0, 0, 0, 7)
        .expect("code actions response");

    let action = code_actions.iter().find_map(|action| {
        if let CodeActionOrCommand::CodeAction(ca) = action
            && ca.title == "Convert to ordered list"
        {
            return Some(ca);
        }
        None
    });
    let action = action.expect("expected ordered list conversion action");

    let changes = action
        .edit
        .as_ref()
        .and_then(|edit| edit.changes.as_ref())
        .expect("workspace edit changes");
    let edits = changes
        .get(&"file:///test.qmd".parse::<Uri>().expect("uri"))
        .expect("edits for document");
    assert_eq!(edits.len(), 3);
    assert_eq!(edits[0].new_text, "1.");
    assert_eq!(edits[1].new_text, "2.");
    assert_eq!(edits[2].new_text, "3.");
}

#[test]
fn test_code_action_convert_ordered_list_to_bullet() {
    let mut server = TestLspServer::new();
    let content = "1. First\n2. Second\n3. Third\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let code_actions = server
        .get_code_actions("file:///test.qmd", 0, 0, 0, 8)
        .expect("code actions response");

    let action = code_actions.iter().find_map(|action| {
        if let CodeActionOrCommand::CodeAction(ca) = action
            && ca.title == "Convert to bullet list"
        {
            return Some(ca);
        }
        None
    });
    let action = action.expect("expected bullet list conversion action");

    let changes = action
        .edit
        .as_ref()
        .and_then(|edit| edit.changes.as_ref())
        .expect("workspace edit changes");
    let edits = changes
        .get(&"file:///test.qmd".parse::<Uri>().expect("uri"))
        .expect("edits for document");
    assert_eq!(edits.len(), 3);
    assert!(edits.iter().all(|edit| edit.new_text == "-"));
}

#[test]
fn test_code_action_convert_bullet_list_to_task() {
    let mut server = TestLspServer::new();
    let content = "- First\n- Second\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let code_actions = server
        .get_code_actions("file:///test.qmd", 0, 0, 0, 7)
        .expect("code actions response");

    let action = code_actions.iter().find_map(|action| {
        if let CodeActionOrCommand::CodeAction(ca) = action
            && ca.title == "Convert to task list"
        {
            return Some(ca);
        }
        None
    });
    let action = action.expect("expected task list conversion action");

    let changes = action
        .edit
        .as_ref()
        .and_then(|edit| edit.changes.as_ref())
        .expect("workspace edit changes");
    let edits = changes
        .get(&"file:///test.qmd".parse::<Uri>().expect("uri"))
        .expect("edits for document");
    assert_eq!(edits.len(), 2);
    assert!(edits.iter().all(|edit| edit.new_text == " [ ]"));
}

#[test]
fn test_code_action_convert_ordered_list_to_task() {
    let mut server = TestLspServer::new();
    let content = "1. First\n2. Second\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let code_actions = server
        .get_code_actions("file:///test.qmd", 0, 0, 0, 8)
        .expect("code actions response");

    let action = code_actions.iter().find_map(|action| {
        if let CodeActionOrCommand::CodeAction(ca) = action
            && ca.title == "Convert to task list"
        {
            return Some(ca);
        }
        None
    });
    let action = action.expect("expected task list conversion action");

    let changes = action
        .edit
        .as_ref()
        .and_then(|edit| edit.changes.as_ref())
        .expect("workspace edit changes");
    let edits = changes
        .get(&"file:///test.qmd".parse::<Uri>().expect("uri"))
        .expect("edits for document");
    assert_eq!(edits.len(), 4, "marker + checkbox edit per item");
    assert_eq!(edits[0].new_text, "-");
    assert_eq!(edits[1].new_text, " [ ]");
    assert_eq!(edits[2].new_text, "-");
    assert_eq!(edits[3].new_text, " [ ]");
}

#[test]
fn test_code_action_convert_task_list_to_ordered() {
    let mut server = TestLspServer::new();
    let content = "- [ ] First\n- [x] Second\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let code_actions = server
        .get_code_actions("file:///test.qmd", 0, 0, 0, 10)
        .expect("code actions response");

    let action = code_actions.iter().find_map(|action| {
        if let CodeActionOrCommand::CodeAction(ca) = action
            && ca.title == "Convert to ordered list"
        {
            return Some(ca);
        }
        None
    });
    let action = action.expect("expected ordered list conversion action");

    let changes = action
        .edit
        .as_ref()
        .and_then(|edit| edit.changes.as_ref())
        .expect("workspace edit changes");
    let edits = changes
        .get(&"file:///test.qmd".parse::<Uri>().expect("uri"))
        .expect("edits for document");
    assert_eq!(edits.len(), 4, "marker + checkbox edit per item");
    assert_eq!(edits[0].new_text, "1.");
    assert_eq!(edits[1].new_text, "");
    assert_eq!(edits[2].new_text, "2.");
    assert_eq!(edits[3].new_text, "");
}
