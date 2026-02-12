use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::Client;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use crate::linter;

use super::super::config::load_config;
use super::super::conversions::{convert_diagnostic, offset_to_position};

/// Handle textDocument/codeAction request
pub(crate) async fn code_action(
    client: &Client,
    document_map: Arc<Mutex<HashMap<String, String>>>,
    workspace_root: Arc<Mutex<Option<PathBuf>>>,
    params: CodeActionParams,
) -> Result<Option<CodeActionResponse>> {
    let uri_string = params.text_document.uri.to_string();

    // Get document content
    let text = {
        let document_map = document_map.lock().await;
        match document_map.get(&uri_string) {
            Some(t) => t.clone(),
            None => return Ok(None),
        }
    };

    // Load config and run linter
    let workspace_root = workspace_root.lock().await.clone();
    let config = load_config(client, &workspace_root, Some(&params.text_document.uri)).await;
    let text_clone = text.clone();
    let diagnostics = tokio::task::spawn_blocking(move || {
        let tree = crate::parse(&text_clone, Some(config.clone()));
        linter::lint(&tree, &text_clone, &config)
    })
    .await
    .map_err(|_| tower_lsp_server::jsonrpc::Error::internal_error())?;

    // Convert fixes to code actions
    let mut actions = Vec::new();
    for diag in diagnostics {
        if let Some(ref fix) = diag.fix {
            let mut changes = HashMap::new();
            let text_edits: Vec<TextEdit> = fix
                .edits
                .iter()
                .map(|edit| {
                    let start = offset_to_position(&text, edit.range.start().into());
                    let end = offset_to_position(&text, edit.range.end().into());
                    TextEdit {
                        range: Range { start, end },
                        new_text: edit.replacement.clone(),
                    }
                })
                .collect();

            changes.insert(params.text_document.uri.clone(), text_edits);

            let action = CodeAction {
                title: fix.message.clone(),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![convert_diagnostic(&diag, &text)]),
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    ..Default::default()
                }),
                ..Default::default()
            };

            actions.push(CodeActionOrCommand::CodeAction(action));
        }
    }

    Ok(Some(actions))
}
