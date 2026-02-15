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
use crate::parser::parse;

use super::super::{conversions, helpers};

/// Handle textDocument/definition request
pub(crate) async fn goto_definition(
    client: &tower_lsp_server::Client,
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    workspace_root: Arc<Mutex<Option<PathBuf>>>,
    params: GotoDefinitionParams,
) -> Result<Option<GotoDefinitionResponse>> {
    let uri = &params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;

    // Get document content and config
    let Some((content, config)) =
        helpers::get_document_and_config(client, &document_map, &workspace_root, uri).await
    else {
        return Ok(None);
    };

    // Parse the document
    let root = parse(&content, Some(config));

    // Convert LSP position to byte offset
    let Some(offset) = conversions::position_to_offset(&content, position) else {
        return Ok(None);
    };

    // Find the definition at this offset
    let Some(definition_range) = helpers::find_definition_at_offset(&root, offset) else {
        return Ok(None);
    };

    // Convert the definition's TextRange to LSP Location
    let start_offset: usize = definition_range.start().into();
    let end_offset: usize = definition_range.end().into();

    let start_position = conversions::offset_to_position(&content, start_offset);
    let end_position = conversions::offset_to_position(&content, end_offset);

    let location = Location {
        uri: uri.clone(),
        range: Range {
            start: start_position,
            end: end_position,
        },
    };

    Ok(Some(GotoDefinitionResponse::Scalar(location)))
}
