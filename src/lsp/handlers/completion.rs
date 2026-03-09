//! Handler for textDocument/completion LSP requests.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use crate::lsp::DocumentState;

use super::super::helpers;
use crate::metadata::inline_reference_map;

pub(crate) async fn completion(
    _client: &tower_lsp_server::Client,
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    salsa_db: Arc<Mutex<crate::salsa::SalsaDb>>,
    _workspace_root: Arc<Mutex<Option<PathBuf>>>,
    params: CompletionParams,
) -> Result<Option<CompletionResponse>> {
    let uri = &params.text_document_position.text_document.uri;
    let position = params.text_document_position.position;

    let Some(text) = helpers::get_document_content(&document_map, &salsa_db, uri).await else {
        return Ok(None);
    };

    let Some(offset) = super::super::conversions::position_to_offset(&text, position) else {
        return Ok(None);
    };

    if !is_citation_context(&text, offset) {
        return Ok(None);
    }

    let (salsa_file, salsa_config, doc_path) = {
        let map = document_map.lock().await;
        match map.get(&uri.to_string()) {
            Some(state) => (state.salsa_file, state.salsa_config, state.path.clone()),
            None => return Ok(None),
        }
    };

    let Some(doc_path) = doc_path else {
        return Ok(None);
    };
    let yaml_ok = {
        let db = salsa_db.lock().await;
        crate::salsa::yaml_metadata_parse_result(&*db, salsa_file, salsa_config, doc_path.clone())
            .is_ok()
    };
    if !yaml_ok {
        return Ok(None);
    }

    let metadata = {
        let db = salsa_db.lock().await;
        crate::salsa::metadata(&*db, salsa_file, salsa_config, doc_path).clone()
    };
    let parse = metadata.bibliography_parse.as_ref();
    if parse.is_none() && metadata.inline_references.is_empty() {
        return Ok(None);
    }

    let mut seen = std::collections::HashSet::new();
    let mut items = Vec::new();
    if let Some(parse) = parse {
        for key in parse.index.iter_keys() {
            if !seen.insert(key.to_lowercase()) {
                continue;
            }
            items.push(CompletionItem {
                label: key.clone(),
                kind: Some(CompletionItemKind::REFERENCE),
                insert_text: Some(key.clone()),
                insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
                ..Default::default()
            });
        }
    }
    for (key, entries) in inline_reference_map(&metadata.inline_references) {
        if entries.is_empty() || !seen.insert(key.clone()) {
            continue;
        }
        let label = entries[0].id.clone();
        items.push(CompletionItem {
            label: label.clone(),
            kind: Some(CompletionItemKind::REFERENCE),
            insert_text: Some(label),
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
