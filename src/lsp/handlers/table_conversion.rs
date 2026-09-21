//! LSP range and container handling for explicit table conversions.

use std::collections::HashMap;

use lsp_types::{
    CodeAction, CodeActionDisabled, CodeActionKind, CodeActionOrCommand, CodeActionParams, Range,
    TextEdit, WorkspaceEdit,
};
use panache_formatter::formatter::table_conversion::equivalent_tables;
use panache_formatter::{TableConversionError, TableStyle, convert_table};
use unicode_width::UnicodeWidthStr;

use super::super::conversions::{offset_to_position, position_to_offset};
use crate::lsp::{global_state::StateSnapshot, line_index::LineIndex};
use crate::{
    Config,
    syntax::{SyntaxKind, SyntaxNode, Table},
};

pub(super) fn code_actions(
    snap: &StateSnapshot,
    params: &CodeActionParams,
    tree: &SyntaxNode,
    text: &str,
    config: &Config,
    index: &LineIndex,
) -> Vec<CodeActionOrCommand> {
    if params.context.only.as_ref().is_some_and(|kinds| {
        !kinds.iter().any(|kind| {
            let kind = kind.as_str();
            kind.is_empty() || kind == "refactor" || kind == "refactor.rewrite"
        })
    }) {
        return Vec::new();
    }
    let (Some(start), Some(end)) = (
        position_to_offset(index, params.range.start),
        position_to_offset(index, params.range.end),
    ) else {
        return Vec::new();
    };
    let Some(table) = tree
        .token_at_offset((start as u32).into())
        .right_biased()
        .and_then(|token| token.parent_ancestors().find_map(Table::cast))
    else {
        return Vec::new();
    };
    let range = table.syntax().text_range();
    if end < start || end > usize::from(range.end()) {
        return Vec::new();
    }
    let config = crate::formatter::to_formatter_config(config);
    let mut actions = Vec::new();
    for (target, title) in [
        (TableStyle::Pipe, "Convert to pipe table"),
        (TableStyle::Simple, "Convert to simple table"),
        (TableStyle::Multiline, "Convert to multiline table"),
        (TableStyle::Grid, "Convert to grid table"),
    ] {
        if table.syntax().kind() == target.syntax_kind() {
            continue;
        }
        let mut action = CodeAction {
            title: title.to_string(),
            kind: Some(CodeActionKind::REFACTOR_REWRITE),
            ..Default::default()
        };
        match conversion_edit(&table, tree, text, &config, index, target) {
            Ok(edit) => {
                action.edit = Some(WorkspaceEdit {
                    changes: Some(HashMap::from([(
                        params.text_document.uri.clone(),
                        vec![edit],
                    )])),
                    ..Default::default()
                })
            }
            Err(reason) if snap.supports_disabled_code_actions => {
                action.disabled = Some(CodeActionDisabled {
                    reason: reason.to_string(),
                });
            }
            Err(_) => continue,
        }
        actions.push(CodeActionOrCommand::CodeAction(action));
    }
    actions
}

fn conversion_edit(
    table: &Table,
    tree: &SyntaxNode,
    text: &str,
    config: &panache_formatter::Config,
    index: &LineIndex,
    target: TableStyle,
) -> Result<TextEdit, TableConversionError> {
    let range = table.syntax().text_range();
    let start = usize::from(range.start());
    let end = usize::from(range.end());
    // A table on a marker line borrows its first prefix from the parent. A
    // table starting later in the container owns that prefix itself.
    let first_prefix: String = table
        .syntax()
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .take_while(|token| token.kind() == SyntaxKind::LINE_PREFIX)
        .map(|token| token.text().to_string())
        .collect();
    // A grid's first separator may borrow the container marker from its
    // parent. Find the first complete prefix owned by the table instead.
    let prefix: String = table
        .syntax()
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .skip_while(|token| token.kind() != SyntaxKind::LINE_PREFIX)
        .take_while(|token| token.kind() == SyntaxKind::LINE_PREFIX)
        .map(|token| token.text().to_string())
        .collect();
    let table_indent = if target == TableStyle::Grid {
        0
    } else {
        config.table_indent
    };
    let width = config
        .line_width
        .saturating_sub(prefix.width() + table_indent);
    let converted = convert_table(table, target, config, width)?;
    let mut replacement = String::new();
    for (i, line) in converted.split_inclusive('\n').enumerate() {
        replacement.push_str(if i == 0 { &first_prefix } else { &prefix });
        replacement.push_str(line);
    }
    if !text[start..end].ends_with('\n') {
        replacement.truncate(replacement.trim_end_matches('\n').len());
    }
    let ending = if text
        .find("\r\n")
        .is_some_and(|crlf| text.find('\n') == Some(crlf + 1))
    {
        "\r\n"
    } else {
        "\n"
    };
    if ending == "\r\n" {
        replacement = replacement.replace('\n', "\r\n");
    }
    let mut candidate_text = text.to_string();
    candidate_text.replace_range(start..end, &replacement);
    let parsed = panache_formatter::parser::parse(&candidate_text, Some(config.parser_options()));
    let candidate = parsed
        .descendants()
        .filter_map(Table::cast)
        .find(|table| usize::from(table.syntax().text_range().start()) == start)
        .ok_or(TableConversionError::ContainerLayout)?;
    let expected_end = start + replacement.len();
    if candidate.syntax().kind() != target.syntax_kind()
        || usize::from(candidate.syntax().text_range().end()) != expected_end
        || ancestors(table.syntax()) != ancestors(candidate.syntax())
        || tree.descendants().filter_map(Table::cast).count()
            != parsed.descendants().filter_map(Table::cast).count()
    {
        return Err(TableConversionError::ContainerLayout);
    }
    if !equivalent_tables(table, &candidate, config) {
        return Err(TableConversionError::InvalidOutput);
    }
    Ok(TextEdit {
        range: Range {
            start: offset_to_position(index, start),
            end: offset_to_position(index, end),
        },
        new_text: replacement,
    })
}

fn ancestors(node: &SyntaxNode) -> Vec<SyntaxKind> {
    node.ancestors().skip(1).map(|node| node.kind()).collect()
}
