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
            2, // Line with "### Heading 3"
            0,
            2,
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
