use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::Client;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use super::super::config::load_config;
use super::super::conversions::offset_to_position;

/// Handle textDocument/formatting request
pub(crate) async fn format_document(
    client: &Client,
    document_map: Arc<Mutex<HashMap<String, String>>>,
    workspace_root: Arc<Mutex<Option<PathBuf>>>,
    params: DocumentFormattingParams,
) -> Result<Option<Vec<TextEdit>>> {
    let uri = params.text_document.uri;
    let uri_string = uri.to_string();

    client
        .log_message(
            MessageType::INFO,
            format!("Formatting request for {}", uri_string),
        )
        .await;

    // Get document content (clone to avoid holding lock across await)
    let text = {
        let document_map = document_map.lock().await;
        match document_map.get(&uri_string) {
            Some(t) => t.clone(),
            None => {
                client
                    .log_message(
                        MessageType::ERROR,
                        format!("Document not found: {}", uri_string),
                    )
                    .await;
                return Ok(None);
            }
        }
    };

    // Load config
    let workspace_root = workspace_root.lock().await.clone();
    let config = load_config(client, &workspace_root).await;

    // Run formatting in a blocking task (because rowan::SyntaxNode isn't Send)
    // but use format_async inside to support external formatters
    let text_clone = text.clone();
    let formatted = tokio::task::spawn_blocking(move || {
        // Create a new tokio runtime for async external formatters
        tokio::runtime::Runtime::new()
            .expect("Failed to create runtime")
            .block_on(crate::format_async(&text_clone, Some(config), None))
    })
    .await
    .map_err(|_| tower_lsp_server::jsonrpc::Error::internal_error())?;

    // If the content didn't change, return None
    if formatted == text {
        return Ok(None);
    }

    // Calculate the range to replace (entire document)
    // Use text.len() to ensure we include any trailing newlines
    let end_position = offset_to_position(&text, text.len());

    let range = Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: end_position,
    };

    Ok(Some(vec![TextEdit {
        range,
        new_text: formatted,
    }]))
}

/// Handle textDocument/rangeFormatting request
pub(crate) async fn format_range(
    client: &Client,
    document_map: Arc<Mutex<HashMap<String, String>>>,
    workspace_root: Arc<Mutex<Option<PathBuf>>>,
    params: DocumentRangeFormattingParams,
) -> Result<Option<Vec<TextEdit>>> {
    let uri = params.text_document.uri;
    let uri_string = uri.to_string();
    let range = params.range;

    client
        .log_message(
            MessageType::INFO,
            format!(
                "Range formatting request for {} (lines {}-{})",
                uri_string,
                range.start.line + 1,
                range.end.line + 1
            ),
        )
        .await;

    // Get document content (clone to avoid holding lock across await)
    let text = {
        let document_map = document_map.lock().await;
        match document_map.get(&uri_string) {
            Some(t) => t.clone(),
            None => {
                client
                    .log_message(
                        MessageType::ERROR,
                        format!("Document not found: {}", uri_string),
                    )
                    .await;
                return Ok(None);
            }
        }
    };

    // Convert LSP range (0-indexed lines) to panache range (1-indexed lines)
    let start_line = (range.start.line + 1) as usize;
    let end_line = (range.end.line + 1) as usize;

    // Load config
    let workspace_root = workspace_root.lock().await.clone();
    let config = load_config(client, &workspace_root).await;

    // Run range formatting in a blocking task
    let text_clone = text.clone();
    let formatted = tokio::task::spawn_blocking(move || {
        tokio::runtime::Runtime::new()
            .expect("Failed to create runtime")
            .block_on(crate::format_async(
                &text_clone,
                Some(config),
                Some((start_line, end_line)),
            ))
    })
    .await
    .map_err(|_| tower_lsp_server::jsonrpc::Error::internal_error())?;

    // If the formatted range is empty or unchanged, return None
    if formatted.is_empty() || formatted == text {
        return Ok(None);
    }

    // Calculate the actual range that was formatted (expanded to block boundaries)
    // For simplicity, we'll replace the entire selected range with the formatted output
    // The range expansion is already handled by panache's range_utils

    // Find where the formatted text should be placed
    // Since range formatting returns only the formatted blocks, we need to determine
    // the byte offsets in the original text to replace

    // Convert line range to byte offsets in original text
    let start_offset = text
        .lines()
        .take(start_line.saturating_sub(1))
        .map(|l| l.len() + 1) // +1 for newline
        .sum::<usize>();

    let end_offset = text
        .lines()
        .take(end_line)
        .map(|l| l.len() + 1)
        .sum::<usize>()
        .min(text.len());

    // Create the edit range
    let edit_range = Range {
        start: offset_to_position(&text, start_offset),
        end: offset_to_position(&text, end_offset),
    };

    Ok(Some(vec![TextEdit {
        range: edit_range,
        new_text: formatted,
    }]))
}
