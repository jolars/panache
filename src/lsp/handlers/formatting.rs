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
    log::debug!("format_document uri={}", uri.as_str());

    let (text, config, source, workspace_root) = snap.document_config_and_source(&uri)?;

    if is_uri_excluded(&uri, &config, &source, workspace_root.as_deref()) {
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
    let end_position = offset_to_position(&text, text.len());
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
    let position = params.text_document_position.position;
    log::debug!("format_on_type uri={} pos={:?}", uri.as_str(), position);

    let text = snap.document_content(&uri)?;
    // Salsa-cached tree; already reflects the just-typed newline. Probing the
    // *previous* line's marker (below) keeps us off the unstable new line.
    let tree = snap.parsed_tree(&uri)?;

    let cursor = position_to_offset(&text, position)?;
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
        start: offset_to_position(&text, line_start),
        end: offset_to_position(&text, line_start + ws_len),
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
    let range = params.range;
    log::debug!(
        "format_range uri={} start={:?} end={:?}",
        uri.as_str(),
        range.start,
        range.end
    );

    let (text, config) = snap.document_and_config(&uri)?;

    // Convert LSP range (0-indexed lines, end-exclusive) to panache range
    // (1-indexed, inclusive).
    let start_line = (range.start.line + 1) as usize;
    let mut end_line = (range.end.line + 1) as usize;
    if range.end.character == 0 && range.end.line > range.start.line {
        end_line = range.end.line as usize;
    }

    let _ = (
        position_to_offset(&text, range.start),
        position_to_offset(&text, range.end),
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
        start: offset_to_position(&text, start_offset),
        end: offset_to_position(&text, end_offset.min(text.len())),
    };

    Some(vec![TextEdit {
        range: edit_range,
        new_text: formatted,
    }])
}
