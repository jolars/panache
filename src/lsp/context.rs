use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;
use tower_lsp_server::ls_types::Uri;

use crate::lsp::DocumentState;
use crate::syntax::{ParsedYamlRegionSnapshot, SyntaxNode};

#[derive(Clone)]
pub(crate) struct OpenDocumentContext {
    pub(crate) salsa_file: crate::salsa::FileText,
    pub(crate) salsa_config: crate::salsa::FileConfig,
    pub(crate) path: Option<PathBuf>,
    pub(crate) parsed_yaml_regions: Vec<ParsedYamlRegionSnapshot>,
    pub(crate) tree: rowan::GreenNode,
    pub(crate) content: String,
}

impl OpenDocumentContext {
    pub(crate) fn syntax_root(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.tree.clone())
    }
}

pub(crate) async fn get_open_document_context(
    document_map: &Arc<Mutex<HashMap<String, DocumentState>>>,
    salsa_db: &Arc<Mutex<crate::salsa::SalsaDb>>,
    uri: &Uri,
) -> Option<OpenDocumentContext> {
    let state = {
        let map = document_map.lock().await;
        map.get(&uri.to_string())?.clone()
    };

    let content = {
        let db = salsa_db.lock().await;
        state.salsa_file.text(&*db).clone()
    };

    Some(OpenDocumentContext {
        salsa_file: state.salsa_file,
        salsa_config: state.salsa_config,
        path: state.path,
        parsed_yaml_regions: state.parsed_yaml_regions,
        tree: state.tree,
        content,
    })
}
