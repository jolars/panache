//! `textDocument/formatting` and `rangeFormatting`.
//!
//! Runs on a [`TaskPool`](crate::lsp::task_pool) worker over a
//! [`StateSnapshot`]; formatting itself is the synchronous [`crate::format`],
//! which routes through the synchronous external-formatter path.

use lsp_types::{
    DocumentFormattingParams, DocumentOnTypeFormattingParams, DocumentRangeFormattingParams,
    Position, Range, TextEdit,
};

use super::super::conversions::{offset_to_position, position_to_offset};
use super::super::helpers::is_uri_excluded;
use crate::lsp::global_state::StateSnapshot;
use crate::{parser, range_utils};

/// Handle `textDocument/formatting`.
pub(crate) fn format_document(
    snap: &StateSnapshot,
    params: DocumentFormattingParams,
) -> Option<Vec<TextEdit>> {
    let uri = params.text_document.uri;
    if snap
        .document_state(&uri)
        .is_some_and(|s| s.notebook_cell_kind == Some(lsp_types::NotebookCellKind::Code))
    {
        return None;
    }
    log::debug!("format_document uri={}", uri.as_str());

    let config_uri = snap
        .document_state(&uri)
        .and_then(|s| s.notebook_uri)
        .map(|u| crate::lsp::notebooks::config_uri(&snap.workspace_folders, &u));
    let config_uri = config_uri.as_ref().unwrap_or(&uri);
    let text = snap.document_content(&uri)?;
    if text.is_empty()
        && snap
            .document_state(&uri)
            .is_some_and(|s| s.notebook_uri.is_some())
    {
        return None;
    }
    // Refuse to format under a broken `panache.toml` rather than silently
    // applying default formatting. The error is surfaced as a diagnostic on the
    // config file (settle pass) and a one-shot toast (main-loop config reload).
    let (config, source) =
        match crate::lsp::config::try_load_config(&snap.workspace_folders, Some(config_uri)) {
            Ok(loaded) => loaded,
            Err(err) => {
                log::warn!("Refusing to format {}: {err}", uri.as_str());
                return None;
            }
        };
    let workspace_root = snap.workspace_root_for(config_uri);

    if is_uri_excluded(config_uri, &config, &source, workspace_root.as_deref()) {
        log::info!(
            "Skipping formatting (matched exclude pattern): {}",
            uri.as_str()
        );
        return None;
    }

    // Reuse the salsa-cached parse (the one hover/symbols read) instead of
    // parsing afresh, saving a parse per format request. Falls back to a fresh
    // parse only if the document somehow isn't open.
    let formatted = match snap.parsed_tree(&uri) {
        Some(tree) => crate::format_with_tree(&text, &tree, &config, None),
        None => crate::format(&text, Some(config), None),
    };

    if formatted == text {
        return None;
    }

    // Replace the entire document; use text.len() to include trailing newlines.
    let line_index = snap.line_index(&uri)?;
    let end_position = offset_to_position(&line_index, text.len());
    let range = Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: end_position,
    };

    Some(vec![TextEdit {
        range,
        new_text: formatted,
    }])
}

/// Handle `textDocument/onTypeFormatting`.
///
/// Scoped narrowly to continuation indentation after Enter inside a list item:
/// when the trigger is a newline, align the new line to the enclosing list
/// item's continuation column (the same column the formatter would use, so
/// on-type help and a later format pass never disagree). Whitespace only — it
/// never inserts a list marker and never guesses new-item-vs-exit intent.
pub(crate) fn format_on_type(
    snap: &StateSnapshot,
    params: DocumentOnTypeFormattingParams,
) -> Option<Vec<TextEdit>> {
    // Newline is the only trigger we register, but guard defensively.
    if params.ch != "\n" {
        return None;
    }

    let uri = params.text_document_position.text_document.uri;
    if snap
        .document_state(&uri)
        .is_some_and(|s| s.notebook_cell_kind == Some(lsp_types::NotebookCellKind::Code))
    {
        return None;
    }
    let position = params.text_document_position.position;
    log::debug!("format_on_type uri={} pos={:?}", uri.as_str(), position);

    let text = snap.document_content(&uri)?;
    if text.is_empty()
        && snap
            .document_state(&uri)
            .is_some_and(|s| s.notebook_uri.is_some())
    {
        return None;
    }
    // Salsa-cached tree; already reflects the just-typed newline. Probing the
    // *previous* line's marker (below) keeps us off the unstable new line.
    let tree = snap.parsed_tree(&uri)?;

    let line_index = snap.line_index(&uri)?;
    let cursor = position_to_offset(&line_index, position)?;
    let line_start = text[..cursor].rfind('\n').map_or(0, |i| i + 1);
    // An offset on the line the newline was typed after.
    let probe = line_start.saturating_sub(1);

    let want = panache_formatter::continuation_indent_at(&tree, &text, probe)?;

    // Replace only the new line's existing leading whitespace.
    let line = &text[line_start..cursor];
    let ws_len = line.len() - line.trim_start_matches([' ', '\t']).len();
    let new_text = " ".repeat(want);
    if line[..ws_len] == new_text {
        return None;
    }

    let range = Range {
        start: offset_to_position(&line_index, line_start),
        end: offset_to_position(&line_index, line_start + ws_len),
    };
    Some(vec![TextEdit { range, new_text }])
}

/// Handle `textDocument/rangeFormatting`.
///
/// Range formatting intentionally bypasses `exclude`/`extend-exclude`: it only
/// fires when the user explicitly selects text and asks to format it, mirroring
/// the CLI's "explicit file target bypasses excludes" rule.
pub(crate) fn format_range(
    snap: &StateSnapshot,
    params: DocumentRangeFormattingParams,
) -> Option<Vec<TextEdit>> {
    let uri = params.text_document.uri;
    if snap
        .document_state(&uri)
        .is_some_and(|s| s.notebook_cell_kind == Some(lsp_types::NotebookCellKind::Code))
    {
        return None;
    }
    let range = params.range;
    log::debug!(
        "format_range uri={} start={:?} end={:?}",
        uri.as_str(),
        range.start,
        range.end
    );

    let config_uri = snap
        .document_state(&uri)
        .and_then(|s| s.notebook_uri)
        .map(|u| crate::lsp::notebooks::config_uri(&snap.workspace_folders, &u));
    let config_uri = config_uri.as_ref().unwrap_or(&uri);
    let text = snap.document_content(&uri)?;
    if text.is_empty()
        && snap
            .document_state(&uri)
            .is_some_and(|s| s.notebook_uri.is_some())
    {
        return None;
    }
    // Refuse to range-format under a broken config (see `format_document`).
    let config =
        match crate::lsp::config::try_load_config(&snap.workspace_folders, Some(config_uri)) {
            Ok((config, _source)) => config,
            Err(err) => {
                log::warn!("Refusing to range-format {}: {err}", uri.as_str());
                return None;
            }
        };

    // Convert LSP range (0-indexed lines, end-exclusive) to panache range
    // (1-indexed, inclusive).
    let start_line = (range.start.line + 1) as usize;
    let mut end_line = (range.end.line + 1) as usize;
    if range.end.character == 0 && range.end.line > range.start.line {
        end_line = range.end.line as usize;
    }

    let line_index = snap.line_index(&uri)?;
    let _ = (
        position_to_offset(&line_index, range.start),
        position_to_offset(&line_index, range.end),
    );

    // Reuse the salsa-cached parse for both range expansion and formatting,
    // rather than parsing twice (once here, once inside `format`).
    let tree = snap
        .parsed_tree(&uri)
        .unwrap_or_else(|| parser::parse(&text, Some(config.clone())));
    let expanded_range =
        range_utils::expand_line_range_to_blocks(&tree, &text, start_line, end_line);
    let formatted = crate::format_with_tree(&text, &tree, &config, Some((start_line, end_line)));

    if formatted.is_empty() || formatted == text {
        return None;
    }

    let (start_offset, end_offset) = expanded_range?;

    let edit_range = Range {
        start: offset_to_position(&line_index, start_offset),
        end: offset_to_position(&line_index, end_offset.min(text.len())),
    };

    Some(vec![TextEdit {
        range: edit_range,
        new_text: formatted,
    }])
}
