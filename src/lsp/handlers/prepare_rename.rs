use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use crate::lsp::DocumentState;
use crate::syntax::SyntaxNode;

use super::super::conversions::{offset_to_position, position_to_offset};
use super::super::helpers;

pub(crate) async fn prepare_rename(
    client: &tower_lsp_server::Client,
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    salsa_db: Arc<Mutex<crate::salsa::SalsaDb>>,
    workspace_root: Arc<Mutex<Option<PathBuf>>>,
    params: TextDocumentPositionParams,
) -> Result<Option<PrepareRenameResponse>> {
    let uri = params.text_document.uri;
    let position = params.position;
    let _config = helpers::get_config(client, &workspace_root, &uri).await;

    let (content, green_tree, parsed_yaml_regions) = {
        let map = document_map.lock().await;
        let Some(state) = map.get(&uri.to_string()) else {
            return Ok(None);
        };
        let db = salsa_db.lock().await;
        (
            state.salsa_file.text(&*db).clone(),
            state.tree.clone(),
            state.parsed_yaml_regions.clone(),
        )
    };

    let Some(offset) = position_to_offset(&content, position) else {
        return Ok(None);
    };
    if helpers::is_offset_in_yaml_frontmatter(&parsed_yaml_regions, offset) {
        return Ok(None);
    }

    let root = SyntaxNode::new_root(green_tree);
    let Some(range) = helpers::find_symbol_text_range_at_offset(&root, offset) else {
        return Ok(None);
    };

    let start = offset_to_position(&content, range.start().into());
    let end = offset_to_position(&content, range.end().into());
    Ok(Some(PrepareRenameResponse::Range(Range { start, end })))
}
