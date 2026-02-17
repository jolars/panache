//! Reference link and footnote AST node wrappers.

use super::ast::support;
use super::links::Link;
use super::{AstNode, SyntaxKind, SyntaxNode};

pub struct ReferenceDefinition(SyntaxNode);

impl AstNode for ReferenceDefinition {
    fn kind() -> SyntaxKind {
        SyntaxKind::REFERENCE_DEFINITION
    }

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::REFERENCE_DEFINITION
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

impl ReferenceDefinition {
    /// Returns the link containing the label and URL.
    pub fn link(&self) -> Option<Link> {
        support::child(&self.0)
    }

    /// Extracts the label text.
    pub fn label(&self) -> String {
        self.link()
            .and_then(|link| link.text())
            .map(|text| text.text_content())
            .unwrap_or_default()
    }
}

pub struct FootnoteReference(SyntaxNode);

impl AstNode for FootnoteReference {
    fn kind() -> SyntaxKind {
        SyntaxKind::FOOTNOTE_REFERENCE
    }

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::FOOTNOTE_REFERENCE
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

impl FootnoteReference {
    /// Extracts the footnote ID (e.g., "1" from a footnote reference).
    pub fn id(&self) -> String {
        let tokens: Vec<_> = self
            .0
            .children_with_tokens()
            .filter_map(|child| child.into_token())
            .filter(|token| token.kind() == SyntaxKind::TEXT)
            .map(|token| token.text().to_string())
            .collect();

        if tokens.len() >= 2 && tokens[0] == "[^" {
            tokens[1].clone()
        } else {
            String::new()
        }
    }
}

pub struct FootnoteDefinition(SyntaxNode);

impl AstNode for FootnoteDefinition {
    fn kind() -> SyntaxKind {
        SyntaxKind::FOOTNOTE_DEFINITION
    }

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::FOOTNOTE_DEFINITION
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

impl FootnoteDefinition {
    /// Extracts the footnote ID from the definition marker.
    pub fn id(&self) -> String {
        self.0
            .children_with_tokens()
            .filter_map(|child| child.into_token())
            .find(|token| token.kind() == SyntaxKind::FOOTNOTE_REFERENCE)
            .and_then(|token| {
                let text = token.text();
                if text.starts_with("[^") && text.contains("]:") {
                    text.trim_start_matches("[^")
                        .split(']')
                        .next()
                        .map(String::from)
                } else {
                    None
                }
            })
            .unwrap_or_default()
    }
}
