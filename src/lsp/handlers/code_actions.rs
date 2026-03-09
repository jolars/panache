use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};
use tower_lsp_server::Client;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use crate::linter;
use crate::lsp::DocumentState;
use crate::syntax::{AstNode, List};

use super::super::conversions::{convert_diagnostic, offset_to_position, position_to_offset};
use super::super::helpers::get_document_and_config;
use super::{footnote_conversion, list_conversion};

/// Handle textDocument/codeAction request
pub(crate) async fn code_action(
    client: &Client,
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    salsa_db: Arc<Mutex<crate::salsa::SalsaDb>>,
    workspace_root: Arc<Mutex<Option<PathBuf>>>,
    params: CodeActionParams,
) -> Result<Option<CodeActionResponse>> {
    let uri = params.text_document.uri;

    // Use helper to get document and config
    let (text, config) = match get_document_and_config(
        client,
        &document_map,
        &salsa_db,
        &workspace_root,
        &uri,
    )
    .await
    {
        Some(result) => result,
        None => return Ok(None),
    };

    #[derive(Debug)]
    struct ExternalLintJob {
        linter_name: String,
        content: String,
        mappings: Vec<crate::linter::code_block_collector::BlockMapping>,
    }

    // Phase A (blocking): parse + built-in lint + collect external jobs
    let text_clone = text.clone();
    let config_clone = config.clone();
    let doc_path = uri.to_file_path().map(|path| path.into_owned());
    let phase_a = tokio::task::spawn_blocking(move || {
        let tree = crate::parse(&text_clone, Some(config_clone.clone()));
        let metadata = doc_path
            .as_ref()
            .and_then(|path| crate::metadata::extract_project_metadata(&tree, path).ok());

        let mut diagnostics =
            linter::lint_with_metadata(&tree, &text_clone, &config_clone, metadata.as_ref());
        let mut jobs = Vec::new();

        if !config_clone.linters.is_empty() {
            let code_blocks = crate::utils::collect_code_blocks(&tree, &text_clone);
            for (language, linter_name) in &config_clone.linters {
                let Some(blocks) = code_blocks.get(language) else {
                    continue;
                };
                if blocks.is_empty() {
                    continue;
                }

                let concatenated =
                    crate::linter::code_block_collector::concatenate_with_blanks_and_mapping(
                        blocks,
                    );
                jobs.push(ExternalLintJob {
                    linter_name: linter_name.clone(),
                    content: concatenated.content,
                    mappings: concatenated.mappings,
                });
            }
        }

        diagnostics.sort_by_key(|d| (d.location.line, d.location.column));
        (diagnostics, jobs)
    })
    .await
    .map_err(|_| tower_lsp_server::jsonrpc::Error::internal_error())?;

    let (mut diagnostics, external_jobs) = phase_a;

    #[cfg(not(target_arch = "wasm32"))]
    if !external_jobs.is_empty() {
        let registry = Arc::new(crate::linter::external_linters::ExternalLinterRegistry::new());
        let max_parallel = config.external_max_parallel.max(1);
        let semaphore = Arc::new(Semaphore::new(max_parallel));
        let mut join_set = tokio::task::JoinSet::new();

        for job in external_jobs {
            let Ok(permit) = semaphore.clone().acquire_owned().await else {
                break;
            };
            let registry = registry.clone();
            let input = text.clone();
            join_set.spawn(async move {
                let _permit = permit;
                crate::linter::external_linters::run_linter(
                    &job.linter_name,
                    &job.content,
                    &input,
                    registry.as_ref(),
                    Some(&job.mappings),
                )
                .await
            });
        }

        while let Some(res) = join_set.join_next().await {
            match res {
                Ok(Ok(diags)) => diagnostics.extend(diags),
                Ok(Err(e)) => log::warn!("External linter failed: {}", e),
                Err(e) => log::warn!("External linter task join error: {}", e),
            }
        }

        diagnostics.sort_by_key(|d| (d.location.line, d.location.column));
    }

    let mut actions = Vec::new();

    // Add lint fix code actions
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

    let tree = crate::parse(&text, Some(config.clone()));

    // Add list conversion code actions (refactoring)
    // Parse tree synchronously (SyntaxNode is not Send, can't use spawn_blocking)
    if let Some(offset) = position_to_offset(&text, params.range.start)
        && let Some(list_node) = list_conversion::find_list_at_position(&tree, offset)
        && let Some(list) = List::cast(list_node.clone())
    {
        if list.is_loose() {
            // Offer to convert to compact
            let edits = list_conversion::convert_to_compact(&list_node, &text);
            if !edits.is_empty() {
                let mut changes = HashMap::new();
                changes.insert(uri.clone(), edits);

                let action = CodeAction {
                    title: "Convert to compact list".to_string(),
                    kind: Some(CodeActionKind::REFACTOR),
                    diagnostics: None,
                    edit: Some(WorkspaceEdit {
                        changes: Some(changes),
                        ..Default::default()
                    }),
                    ..Default::default()
                };

                actions.push(CodeActionOrCommand::CodeAction(action));
            }
        } else {
            // Offer to convert to loose
            let edits = list_conversion::convert_to_loose(&list_node, &text);
            if !edits.is_empty() {
                let mut changes = HashMap::new();
                changes.insert(uri.clone(), edits);

                let action = CodeAction {
                    title: "Convert to loose list".to_string(),
                    kind: Some(CodeActionKind::REFACTOR),
                    diagnostics: None,
                    edit: Some(WorkspaceEdit {
                        changes: Some(changes),
                        ..Default::default()
                    }),
                    ..Default::default()
                };

                actions.push(CodeActionOrCommand::CodeAction(action));
            }
        }
    }

    // Add footnote conversion code actions (refactoring)
    if let Some(offset) = position_to_offset(&text, params.range.start) {
        // Check for reference footnote at cursor
        if let Some(ref_node) =
            footnote_conversion::find_footnote_reference_at_position(&tree, offset)
        {
            // Only offer conversion if the definition is simple
            if footnote_conversion::can_convert_to_inline(&ref_node, &tree) {
                let edits = footnote_conversion::convert_to_inline(&ref_node, &tree, &text);
                if !edits.is_empty() {
                    let mut changes = HashMap::new();
                    changes.insert(uri.clone(), edits);

                    let action = CodeAction {
                        title: "Convert to inline footnote".to_string(),
                        kind: Some(CodeActionKind::REFACTOR),
                        diagnostics: None,
                        edit: Some(WorkspaceEdit {
                            changes: Some(changes),
                            ..Default::default()
                        }),
                        ..Default::default()
                    };

                    actions.push(CodeActionOrCommand::CodeAction(action));
                }
            }
        }

        // Check for inline footnote at cursor
        if let Some(inline_node) =
            footnote_conversion::find_inline_footnote_at_position(&tree, offset)
        {
            let edits = footnote_conversion::convert_to_reference(&inline_node, &tree, &text);
            if !edits.is_empty() {
                let mut changes = HashMap::new();
                changes.insert(uri.clone(), edits);

                let action = CodeAction {
                    title: "Convert to reference footnote".to_string(),
                    kind: Some(CodeActionKind::REFACTOR),
                    diagnostics: None,
                    edit: Some(WorkspaceEdit {
                        changes: Some(changes),
                        ..Default::default()
                    }),
                    ..Default::default()
                };

                actions.push(CodeActionOrCommand::CodeAction(action));
            }
        }
    }

    Ok(Some(actions))
}
