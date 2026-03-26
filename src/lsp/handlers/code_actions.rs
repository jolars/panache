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
use super::{footnote_conversion, heading_link_conversion, list_conversion};

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
    let request_range = params.range;
    let parsed_yaml_regions = {
        let map = document_map.lock().await;
        map.get(&uri.to_string())
            .map(|state| state.parsed_yaml_regions.clone())
            .unwrap_or_default()
    };
    let request_start_offset = position_to_offset(&text, request_range.start);
    let request_end_offset = position_to_offset(&text, request_range.end)
        .or_else(|| request_start_offset.map(|start| start.saturating_add(1)));
    let in_frontmatter_region =
        if let (Some(start), Some(end)) = (request_start_offset, request_end_offset) {
            let end = end.max(start.saturating_add(1));
            parsed_yaml_regions
                .iter()
                .find(|region| region.is_frontmatter())
                .is_some_and(|frontmatter| {
                    let host_range = frontmatter.host_range();
                    host_range.start < end && start < host_range.end
                })
        } else {
            false
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
            let lsp_diag = convert_diagnostic(&diag, &text);
            if !ranges_overlap(lsp_diag.range, request_range) {
                continue;
            }
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
                diagnostics: Some(vec![lsp_diag]),
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
    if !in_frontmatter_region
        && let Some(offset) = position_to_offset(&text, request_range.start)
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

        match list_conversion::detect_list_type(&list_node) {
            Some(crate::syntax::ListKind::Bullet) => {
                let edits = list_conversion::convert_to_ordered(&list_node, &text);
                if !edits.is_empty() {
                    let mut changes = HashMap::new();
                    changes.insert(uri.clone(), edits);

                    let action = CodeAction {
                        title: "Convert to ordered list".to_string(),
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

                let edits = list_conversion::convert_to_task(&list_node, &text);
                if !edits.is_empty() {
                    let mut changes = HashMap::new();
                    changes.insert(uri.clone(), edits);

                    let action = CodeAction {
                        title: "Convert to task list".to_string(),
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
            Some(crate::syntax::ListKind::Ordered) => {
                let edits = list_conversion::convert_to_bullet(&list_node, &text);
                if !edits.is_empty() {
                    let mut changes = HashMap::new();
                    changes.insert(uri.clone(), edits);

                    let action = CodeAction {
                        title: "Convert to bullet list".to_string(),
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

                let edits = list_conversion::convert_to_task(&list_node, &text);
                if !edits.is_empty() {
                    let mut changes = HashMap::new();
                    changes.insert(uri.clone(), edits);

                    let action = CodeAction {
                        title: "Convert to task list".to_string(),
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
            Some(crate::syntax::ListKind::Task) => {
                let edits = list_conversion::convert_to_bullet(&list_node, &text);
                if !edits.is_empty() {
                    let mut changes = HashMap::new();
                    changes.insert(uri.clone(), edits);

                    let action = CodeAction {
                        title: "Convert to bullet list".to_string(),
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

                let edits = list_conversion::convert_task_to_ordered(&list_node, &text);
                if !edits.is_empty() {
                    let mut changes = HashMap::new();
                    changes.insert(uri.clone(), edits);

                    let action = CodeAction {
                        title: "Convert to ordered list".to_string(),
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
            None => {}
        }
    }

    // Add footnote conversion code actions (refactoring)
    if !in_frontmatter_region && let Some(offset) = position_to_offset(&text, request_range.start) {
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

    // Add heading link conversion code actions (refactoring)
    if !in_frontmatter_region
        && let Some(offset) = position_to_offset(&text, request_range.start)
        && let Some(link_node) =
            heading_link_conversion::find_implicit_heading_link_at_position(&tree, offset)
    {
        let edits = heading_link_conversion::convert_to_explicit_heading_link(
            &link_node,
            &tree,
            &text,
            &config.extensions,
        );
        if !edits.is_empty() {
            let mut changes = HashMap::new();
            changes.insert(uri.clone(), edits);

            let action = CodeAction {
                title: "Convert to explicit heading link".to_string(),
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

    Ok(Some(actions))
}

fn ranges_overlap(a: Range, b: Range) -> bool {
    (a.start.line, a.start.character) < (b.end.line, b.end.character)
        && (b.start.line, b.start.character) < (a.end.line, a.end.character)
}
