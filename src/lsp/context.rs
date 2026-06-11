use std::path::PathBuf;

use lsp_types::Uri;

use crate::lsp::global_state::StateSnapshot;
use crate::syntax::SyntaxNode;

#[derive(Clone)]
pub(crate) struct OpenDocumentContext {
    pub(crate) salsa_file: crate::salsa::FileText,
    pub(crate) salsa_config: crate::salsa::FileConfig,
    pub(crate) path: Option<PathBuf>,
    pub(crate) tree: rowan::GreenNode,
    pub(crate) content: String,
}

impl OpenDocumentContext {
    pub(crate) fn syntax_root(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.tree.clone())
    }
}

pub(crate) fn get_open_document_context(
    snap: &StateSnapshot,
    uri: &Uri,
) -> Option<OpenDocumentContext> {
    let state = snap.document_map.get(&uri.to_string())?.clone();
    let content = state.salsa_file.content_or_empty(snap.db()).to_string();

    Some(OpenDocumentContext {
        salsa_file: state.salsa_file,
        salsa_config: state.salsa_config,
        path: state.path,
        tree: state.tree,
        content,
    })
}
