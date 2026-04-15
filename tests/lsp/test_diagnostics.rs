//! Tests for diagnostic workflows (linting + code actions).

use super::helpers::*;
use tower_lsp_server::ls_types::*;

#[tokio::test]
async fn test_diagnostics_on_heading_hierarchy_issue() {
    let server = TestLspServer::new();

    // Open a document with heading hierarchy issue (h1 → h3 skip)
    let content = "# Heading 1\n\n### Heading 3\n\nContent.";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    // Note: Diagnostics are published via client.publish_diagnostics()
    // which we can't easily capture in this test setup.
    // For now, we'll test code actions which are request/response.

    // Request code actions for the h3 line
    let code_actions = server
        .get_code_actions(
            "file:///test.qmd",
            0, // Whole document range to include full diagnostic span
            0,
            4,
            99,
        )
        .await;

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

// Document diagnostics are not currently supported in the test harness.

#[tokio::test]
async fn test_code_actions_filter_quickfixes_to_requested_range() {
    let server = TestLspServer::new();
    let content = "# Heading 1\n\n### Heading 3\n\nContent.\n";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let code_actions = server
        .get_code_actions(
            "file:///test.qmd",
            0, // Request only first line range
            0,
            0,
            20,
        )
        .await
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

#[tokio::test]
async fn test_code_actions_require_diagnostic_to_be_fully_within_requested_range() {
    let server = TestLspServer::new();
    let content = "# Heading 1\n\n### Heading 3\n\nContent.\n";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

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
        .await
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

#[tokio::test]
async fn test_code_actions_offer_quickfix_for_cursor_inside_diagnostic() {
    let server = TestLspServer::new();
    let content = "# Heading 1\n\n### Heading 3\n\nContent.\n";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let code_actions = server
        .get_code_actions(
            "file:///test.qmd",
            2, // "### Heading 3"
            2, // cursor inside heading text
            2,
            2, // zero-width LSP cursor range
        )
        .await
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

#[tokio::test]
async fn test_code_actions_offer_source_fix_all_for_fixable_diagnostics() {
    let server = TestLspServer::new();
    let content = "# Heading 1\n\n### Heading 3\n\nContent.\n";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let code_actions = server
        .get_code_actions("file:///test.qmd", 0, 0, 4, 99)
        .await
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

#[tokio::test]
async fn test_code_actions_do_not_offer_source_fix_all_without_fixes() {
    let server = TestLspServer::new();
    let content = "# Heading 1\n\n## Heading 2\n\nContent.\n";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let code_actions = server
        .get_code_actions("file:///test.qmd", 0, 0, 4, 99)
        .await
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

#[tokio::test]
async fn test_code_actions_no_refactors_inside_yaml_frontmatter() {
    let server = TestLspServer::new();
    let content = "---\ntitle: Report\nlist:\n  - a\n  - b\n---\n\nBody.\n";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let code_actions = server
        .get_code_actions(
            "file:///test.qmd",
            3, // inside frontmatter list item
            2,
            3,
            8,
        )
        .await
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

#[tokio::test]
async fn test_hashpipe_yaml_parse_error_in_built_in_lint_plan() {
    let server = TestLspServer::new();
    let content = "```{r}\n#| fig-cap: [\na <- 1\n```\n";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let diagnostics = server
        .get_built_in_diagnostics("file:///test.qmd")
        .await
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

#[tokio::test]
async fn test_hashpipe_folded_scalar_parse_error_maps_to_host_position() {
    let server = TestLspServer::new();
    let content = "```{r}\n#| fig-cap: >-\n#|   A folded caption\n#| bad: [\na <- 1\n```\n";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let diagnostics = server
        .get_built_in_diagnostics("file:///test.qmd")
        .await
        .expect("diagnostics");

    let yaml_parse_error = diagnostics
        .iter()
        .find(|diag| diag.code == "yaml-parse-error")
        .expect("expected yaml-parse-error diagnostic");
    assert_eq!(yaml_parse_error.location.line, 4);
    assert_eq!(yaml_parse_error.location.column, 9);
}

#[tokio::test]
async fn test_code_action_convert_implicit_heading_link_to_explicit() {
    let server = TestLspServer::new();
    let content = "# Unordered Lists\n\n[unordered lists]\n";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let code_actions = server
        .get_code_actions("file:///test.qmd", 2, 2, 2, 18)
        .await
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

#[tokio::test]
async fn test_code_action_convert_bullet_list_to_ordered() {
    let server = TestLspServer::new();
    let content = "- First\n- Second\n- Third\n";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let code_actions = server
        .get_code_actions("file:///test.qmd", 0, 0, 0, 7)
        .await
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

#[tokio::test]
async fn test_code_action_convert_ordered_list_to_bullet() {
    let server = TestLspServer::new();
    let content = "1. First\n2. Second\n3. Third\n";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let code_actions = server
        .get_code_actions("file:///test.qmd", 0, 0, 0, 8)
        .await
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

#[tokio::test]
async fn test_code_action_convert_bullet_list_to_task() {
    let server = TestLspServer::new();
    let content = "- First\n- Second\n";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let code_actions = server
        .get_code_actions("file:///test.qmd", 0, 0, 0, 7)
        .await
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

#[tokio::test]
async fn test_code_action_convert_ordered_list_to_task() {
    let server = TestLspServer::new();
    let content = "1. First\n2. Second\n";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let code_actions = server
        .get_code_actions("file:///test.qmd", 0, 0, 0, 8)
        .await
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

#[tokio::test]
async fn test_code_action_convert_task_list_to_ordered() {
    let server = TestLspServer::new();
    let content = "- [ ] First\n- [x] Second\n";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let code_actions = server
        .get_code_actions("file:///test.qmd", 0, 0, 0, 10)
        .await
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
