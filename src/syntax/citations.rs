//! Citation AST node wrappers.

use super::{AstNode, SyntaxKind, SyntaxNode, SyntaxToken};

pub struct Citation(SyntaxNode);

impl AstNode for Citation {
    fn kind() -> SyntaxKind {
        SyntaxKind::CITATION
    }

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::CITATION
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self(syntax))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl Citation {
    pub fn keys(&self) -> Vec<CitationKey> {
        self.0
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .filter(|token| token.kind() == SyntaxKind::CITATION_KEY)
            .map(CitationKey)
            .collect()
    }

    pub fn key_texts(&self) -> Vec<String> {
        self.keys().into_iter().map(|key| key.text()).collect()
    }
}

pub struct Crossref(SyntaxNode);

impl AstNode for Crossref {
    fn kind() -> SyntaxKind {
        SyntaxKind::CROSSREF
    }

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::CROSSREF
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self(syntax))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl Crossref {
    pub fn keys(&self) -> Vec<CitationKey> {
        self.0
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .filter(|token| token.kind() == SyntaxKind::CITATION_KEY)
            .map(CitationKey)
            .collect()
    }

    pub fn key_texts(&self) -> Vec<String> {
        self.keys().into_iter().map(|key| key.text()).collect()
    }
}

pub struct CitationKey(SyntaxToken);

impl CitationKey {
    pub fn text(&self) -> String {
        self.0.text().to_string()
    }

    pub fn text_range(&self) -> rowan::TextRange {
        self.0.text_range()
    }
}
