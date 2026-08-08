use std::path::PathBuf;
use std::sync::Arc;

use lsp_types::Uri;

use crate::lsp::global_state::StateSnapshot;
use crate::lsp::line_index::LineIndex;
use crate::syntax::SyntaxNode;

#[derive(Clone)]
pub(crate) struct OpenDocumentContext {
    pub(crate) salsa_file: crate::salsa::FileText,
    pub(crate) salsa_config: crate::salsa::FileConfig,
    pub(crate) path: Option<PathBuf>,
    pub(crate) tree: rowan::GreenNode,
    pub(crate) content: String,
    /// Salsa-cached line index for O(log n) position <-> offset conversion.
    pub(crate) line_index: Arc<LineIndex>,
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
    let line_index = crate::lsp::line_index::line_index(snap.db(), state.salsa_file).clone();
    // The tree comes from salsa, not from `DocumentState`: salsa's is the
    // authoritative parse (diagnostics and the linter already read it), and
    // taking it here keeps every handler on the same tree as the text above.
    let tree = crate::salsa::parsed_tree(snap.db(), state.salsa_file, state.salsa_config).clone();

    Some(OpenDocumentContext {
        salsa_file: state.salsa_file,
        salsa_config: state.salsa_config,
        path: crate::salsa::Db::path_of_id(snap.db(), state.file_id),
        tree,
        content,
        line_index,
    })
}
