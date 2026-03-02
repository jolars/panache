//! Handler for textDocument/definition LSP requests.
//!
//! Provides "go to definition" functionality for:
//! - Reference links: `[text][ref]` → `[ref]: url`
//! - Reference images: `![alt][ref]` → `[ref]: url`
//! - Footnote references: `[^id]` → `[^id]: content`

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use crate::lsp::DocumentState;

use super::super::{conversions, helpers};

/// Handle textDocument/definition request
pub(crate) async fn goto_definition(
    _client: &tower_lsp_server::Client,
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    _workspace_root: Arc<Mutex<Option<PathBuf>>>,
    params: GotoDefinitionParams,
) -> Result<Option<GotoDefinitionResponse>> {
    let uri = &params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;

    let metadata = {
        let map = document_map.lock().await;
        map.get(&uri.to_string())
            .and_then(|state| state.metadata.clone())
    };

    let Some((content, root)) = helpers::get_document_content_and_tree(&document_map, uri).await
    else {
        return Ok(None);
    };

    // Convert LSP position to byte offset
    let Some(offset) = conversions::position_to_offset(&content, position) else {
        return Ok(None);
    };

    // Find the node at this offset
    let Some(mut node) = helpers::find_node_at_offset(&root, offset) else {
        return Ok(None);
    };

    // Walk up the tree to find a citation, reference, or footnote
    loop {
        // First: citation definitions in bibliography
        if let Some(key) = helpers::extract_citation_key(&node)
            && let Some(metadata) = metadata.clone()
            && let Some(parse) = metadata.bibliography_parse
            && let Some(location) = parse.index.get(&key)
        {
            let target_uri = Uri::from_file_path(&location.file).unwrap_or_else(|| uri.clone());
            let (target_text, target_uri) =
                if let Ok(text) = std::fs::read_to_string(&location.file) {
                    (text, target_uri)
                } else {
                    (content.clone(), uri.clone())
                };
            let start = conversions::offset_to_position(&target_text, location.span.start);
            let end = conversions::offset_to_position(&target_text, location.span.end);
            let location = Location {
                uri: target_uri,
                range: Range { start, end },
            };
            return Ok(Some(GotoDefinitionResponse::Scalar(location)));
        }

        // Fallback: find reference/footnote definition at this node
        if let Some((label, is_footnote)) = helpers::extract_reference_label(&node)
            && let Some(definition) = helpers::find_definition_node(&root, &label, is_footnote)
        {
            let start_offset: usize = definition.text_range().start().into();
            let end_offset: usize = definition.text_range().end().into();

            let start_position = conversions::offset_to_position(&content, start_offset);
            let end_position = conversions::offset_to_position(&content, end_offset);

            let location = Location {
                uri: uri.clone(),
                range: Range {
                    start: start_position,
                    end: end_position,
                },
            };

            return Ok(Some(GotoDefinitionResponse::Scalar(location)));
        }

        // Move up to parent, or return None if at root
        match node.parent() {
            Some(parent) => node = parent,
            None => return Ok(None),
        }
    }
}
