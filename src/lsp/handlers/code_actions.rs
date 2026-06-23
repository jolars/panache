use std::collections::HashMap;

use crate::lsp::uri_ext::UriExt;
use lsp_types::*;

use crate::linter;
use crate::lsp::global_state::StateSnapshot;
use crate::syntax::{AstNode, List};

use super::super::conversions::{convert_diagnostic, offset_to_position, position_to_offset};
use super::{footnote_conversion, heading_link_conversion, link_conversion, list_conversion};

/// Handle textDocument/codeAction request
pub(crate) fn code_action(
    snap: &StateSnapshot,
    params: CodeActionParams,
) -> Option<CodeActionResponse> {
    let uri = params.text_document.uri;
    let (text, config) = snap.document_and_config(&uri)?;
    let request_range = params.range;
    let parsed_yaml_regions = snap.parsed_yaml_regions(&uri);
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
        language: String,
        content: String,
        mappings: Vec<crate::linter::code_block_collector::BlockMapping>,
    }

    // Parse + built-in lint + collect external jobs (synchronous).
    let doc_path = uri.to_file_path().map(|path| path.into_owned());
    let (mut diagnostics, external_jobs) = {
        let tree = crate::parse(&text, Some(config.clone()));
        let metadata = doc_path
            .as_ref()
            .and_then(|path| crate::metadata::extract_project_metadata(&tree, path).ok());

        let mut diagnostics = linter::lint_with_metadata(&tree, &text, &config, metadata.as_ref());
        let mut jobs = Vec::new();

        if !config.linters.is_empty() {
            let code_blocks = crate::utils::collect_code_blocks(&tree, &text);
            for (language, linter_name) in &config.linters {
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
                    language: language.clone(),
                    content: concatenated.content,
                    mappings: concatenated.mappings,
                });
            }
        }

        diagnostics.sort_by_key(|d| (d.location.line, d.location.column));
        (diagnostics, jobs)
    };

    #[cfg(not(target_arch = "wasm32"))]
    if !external_jobs.is_empty() {
        let registry = crate::linter::external_linters::ExternalLinterRegistry::new();
        for job in external_jobs {
            match crate::linter::external_linters_sync::run_linter_sync(
                &job.linter_name,
                &job.language,
                &job.content,
                &text,
                &registry,
                Some(job.mappings.as_slice()),
            ) {
                Ok(diags) => diagnostics.extend(diags),
                Err(e) => log::warn!("External linter failed: {e}"),
            }
        }
        diagnostics.sort_by_key(|d| (d.location.line, d.location.column));
    }

    let mut actions = Vec::new();
    let mut fix_all_edits: Vec<(usize, usize, String)> = Vec::new();

    // Add lint fix code actions
    for diag in &diagnostics {
        if let Some(ref fix) = diag.fix {
            let lsp_diag = convert_diagnostic(diag, &text);
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
                    let start = offset_to_position(&text, start_offset);
                    let end = offset_to_position(&text, end_offset);
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
                        start: offset_to_position(&text, start_offset),
                        end: offset_to_position(&text, end_offset),
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

    // Add link inline/reference conversion code actions (refactoring)
    if !in_frontmatter_region
        && let Some(offset) = position_to_offset(&text, request_range.start)
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
