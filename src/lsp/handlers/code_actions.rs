use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::Client;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use crate::linter;

use super::super::conversions::{convert_diagnostic, offset_to_position};
use super::super::helpers::get_document_and_config;

/// Handle textDocument/codeAction request
pub(crate) async fn code_action(
    client: &Client,
    document_map: Arc<Mutex<HashMap<String, String>>>,
    workspace_root: Arc<Mutex<Option<PathBuf>>>,
    params: CodeActionParams,
) -> Result<Option<CodeActionResponse>> {
    let uri = params.text_document.uri;

    // Use helper to get document and config
    let (text, config) =
        match get_document_and_config(client, &document_map, &workspace_root, &uri).await {
            Some(result) => result,
            None => return Ok(None),
        };

    // Run linter
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

            changes.insert(uri.clone(), text_edits);

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
