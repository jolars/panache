//! Citation extraction from CST.

use crate::syntax::{AstNode, Citation, SyntaxNode};

#[derive(Debug, Clone)]
pub struct CitationInfo {
    pub keys: Vec<String>,
}

pub fn extract_citations(tree: &SyntaxNode) -> CitationInfo {
    let mut keys = Vec::new();

    for citation in tree.descendants().filter_map(Citation::cast) {
        for key in citation.key_texts() {
            keys.push(key);
        }
    }

    CitationInfo { keys }
}
