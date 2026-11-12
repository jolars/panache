use std::collections::HashMap;

use crate::linter;
use crate::lsp::uri_ext::UriExt;
use lsp_types::*;

use crate::lsp::global_state::StateSnapshot;
use crate::syntax::{AstNode, List};

use super::super::conversions::{convert_diagnostic, offset_to_position, position_to_offset};
use super::{footnote_conversion, heading_link_conversion, link_conversion, list_conversion};

/// Handle textDocument/codeAction request
pub(crate) fn code_action(
    snap: &StateSnapshot,
    params: CodeActionParams,
) -> Option<CodeActionResponse> {
    let uri = params.text_document.uri.clone();
    if snap
        .document_state(&uri)
        .is_some_and(|s| s.notebook_uri.is_some())
    {
        return None;
    }
    let (text, config) = snap.document_and_config(&uri)?;
    let line_index = snap.line_index(&uri)?;
    let request_range = params.range;
    let parsed_yaml_regions = snap.parsed_yaml_regions(&uri);
    let request_start_offset = position_to_offset(&line_index, request_range.start);
    let request_end_offset = position_to_offset(&line_index, request_range.end)
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

    let state = snap.document_state(&uri)?;
    let plan = crate::salsa::built_in_lint_plan(snap.db(), state.salsa_file, state.salsa_config);
    let mut diagnostics = plan.diagnostics.clone();
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(batch) = snap.execution_batch_for(&uri) {
        snap.execution_cache.invalidate_project_config(&batch);
        let results = snap.execution_cache.get_or_run(&batch, true);
        snap.db().unwind_if_revision_cancelled();
        if let Some(path) = uri.to_file_path()
            && let Some(items) = results
                .diagnostics
                .get(&crate::linter::execution_context::normalize_path(&path))
        {
            diagnostics.extend(items.iter().cloned());
        }
    } else {
        for job in &plan.external_jobs {
            match crate::linter::external_linters_sync::run_linter_job_sync(job, &text) {
                Ok(items) => diagnostics.extend(items),
                Err(error) => log::warn!("External linter failed: {error}"),
            }
        }
    }
    diagnostics.sort_by_key(|diagnostic| (diagnostic.location.line, diagnostic.location.column));

    let mut actions = Vec::new();
    let mut fix_all_edits: Vec<(usize, usize, String)> = Vec::new();

    // Add lint fix code actions
    for diag in &diagnostics {
        if let Some(ref fix) = diag.fix {
            let lsp_diag = convert_diagnostic(diag, &line_index);
            if !should_offer_quickfix(request_range, lsp_diag.range) {
                continue;
            }
            // Unsafe fixes may change the document's meaning, so they are still
            // offered individually (labeled) but excluded from the aggregate
            // "fix all" action, matching the CLI's safe-by-default `--fix`.
            let is_unsafe = fix.safety == linter::FixSafety::Unsafe;
            let mut changes = HashMap::new();
            let text_edits: Vec<TextEdit> = fix
                .edits
                .iter()
                .map(|edit| {
                    let start_offset: usize = edit.range.start().into();
                    let end_offset: usize = edit.range.end().into();
                    let start = offset_to_position(&line_index, start_offset);
                    let end = offset_to_position(&line_index, end_offset);
                    if !is_unsafe {
                        fix_all_edits.push((start_offset, end_offset, edit.replacement.clone()));
                    }
                    TextEdit {
                        range: Range { start, end },
                        new_text: edit.replacement.clone(),
                    }
                })
                .collect();

            changes.insert(uri.clone(), text_edits);

            let title = if is_unsafe {
                format!("{} (unsafe)", fix.message)
            } else {
                fix.message.clone()
            };
            let action = CodeAction {
                title,
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

    if !fix_all_edits.is_empty() {
        fix_all_edits.sort_by_key(|a| (a.0, a.1));
        let mut selected_edits: Vec<(usize, usize, String)> = Vec::new();
        for edit in fix_all_edits {
            if selected_edits
                .last()
                .is_some_and(|prev| edit.0 < prev.1 || edit == *prev)
            {
                continue;
            }
            selected_edits.push(edit);
        }

        if !selected_edits.is_empty() {
            let mut changes = HashMap::new();
            let text_edits: Vec<TextEdit> = selected_edits
                .into_iter()
                .map(|(start_offset, end_offset, replacement)| TextEdit {
                    range: Range {
                        start: offset_to_position(&line_index, start_offset),
                        end: offset_to_position(&line_index, end_offset),
                    },
                    new_text: replacement,
                })
                .collect();
            changes.insert(uri.clone(), text_edits);

            let fix_all_action = CodeAction {
                title: "Fix all auto-fixable lint issues".to_string(),
                kind: Some(CodeActionKind::SOURCE_FIX_ALL),
                diagnostics: None,
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    ..Default::default()
                }),
                ..Default::default()
            };
            actions.push(CodeActionOrCommand::CodeAction(fix_all_action));
        }
    }

    let tree = crate::parse(&text, Some(config.clone()));

    if !in_frontmatter_region {
        actions.extend(super::table_conversion::code_actions(
            snap,
            &params,
            &tree,
            &text,
            &config,
            &line_index,
        ));
    }

    // Add list conversion code actions (refactoring)
    // Parse tree synchronously (SyntaxNode is not Send, can't use spawn_blocking)
    if !in_frontmatter_region
        && let Some(offset) = position_to_offset(&line_index, request_range.start)
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
    if !in_frontmatter_region
        && let Some(offset) = position_to_offset(&line_index, request_range.start)
    {
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

    // Add link inline/reference conversion code actions (refactoring)
    if !in_frontmatter_region
        && let Some(offset) = position_to_offset(&line_index, request_range.start)
        && let Some(link) = link_conversion::find_link_at_position(&tree, offset)
    {
        if link_conversion::can_convert_to_inline(&link, &tree) {
            let edits = link_conversion::convert_to_inline(&link, &tree, &text);
            if !edits.is_empty() {
                let mut changes = HashMap::new();
                changes.insert(uri.clone(), edits);

                let action = CodeAction {
                    title: "Convert to inline link".to_string(),
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

        if link_conversion::can_convert_to_reference(&link) {
            let edits = link_conversion::convert_to_reference(&link, &tree, &text);
            if !edits.is_empty() {
                let mut changes = HashMap::new();
                changes.insert(uri.clone(), edits);

                let action = CodeAction {
                    title: "Convert to reference link".to_string(),
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
        && let Some(offset) = position_to_offset(&line_index, request_range.start)
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

    Some(actions)
}

fn range_contains(outer: Range, inner: Range) -> bool {
    (outer.start.line, outer.start.character) <= (inner.start.line, inner.start.character)
        && (inner.end.line, inner.end.character) <= (outer.end.line, outer.end.character)
}

fn should_offer_quickfix(request: Range, diagnostic: Range) -> bool {
    if request.start == request.end {
        return position_in_range(request.start, diagnostic);
    }
    range_contains(request, diagnostic)
}

fn position_in_range(position: Position, range: Range) -> bool {
    (range.start.line, range.start.character) <= (position.line, position.character)
        && (position.line, position.character) <= (range.end.line, range.end.character)
}
