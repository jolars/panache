//! Handler for textDocument/completion LSP requests.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use crate::lsp::DocumentState;

use super::super::helpers;

pub(crate) async fn completion(
    _client: &tower_lsp_server::Client,
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    _workspace_root: Arc<Mutex<Option<PathBuf>>>,
    params: CompletionParams,
) -> Result<Option<CompletionResponse>> {
    let uri = &params.text_document_position.text_document.uri;
    let position = params.text_document_position.position;

    let Some(text) = helpers::get_document_content(&document_map, uri).await else {
        return Ok(None);
    };

    let Some(offset) = super::super::conversions::position_to_offset(&text, position) else {
        return Ok(None);
    };

    if !is_citation_context(&text, offset) {
        return Ok(None);
    }

    let metadata = {
        let map = document_map.lock().await;
        map.get(&uri.to_string())
            .and_then(|state| state.metadata.clone())
    };

    let Some(metadata) = metadata else {
        return Ok(None);
    };
    let Some(parse) = metadata.bibliography_parse else {
        return Ok(None);
    };

    let mut items = Vec::new();
    for key in parse.index.iter_keys() {
        items.push(CompletionItem {
            label: key.clone(),
            kind: Some(CompletionItemKind::REFERENCE),
            insert_text: Some(key.clone()),
            insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
            ..Default::default()
        });
    }

    Ok(Some(CompletionResponse::Array(items)))
}

fn is_citation_context(text: &str, offset: usize) -> bool {
    let start = offset.saturating_sub(8);
    let snippet = &text[start..offset];
    snippet.contains("@")
}
