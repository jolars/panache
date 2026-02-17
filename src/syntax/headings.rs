//! Heading AST node wrappers.

use super::ast::support;
use super::{AstNode, SyntaxKind, SyntaxNode};

pub struct Heading(SyntaxNode);

impl AstNode for Heading {
    fn kind() -> SyntaxKind {
        SyntaxKind::HEADING
    }

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::HEADING
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

impl Heading {
    /// Returns the heading level (1-6).
    pub fn level(&self) -> usize {
        // Look for ATX_HEADING_MARKER node which contains the token
        for child in self.0.children() {
            if child.kind() == SyntaxKind::ATX_HEADING_MARKER {
                // Count '#' in the token child
                for token in child.children_with_tokens() {
                    if let Some(t) = token.as_token()
                        && t.kind() == SyntaxKind::ATX_HEADING_MARKER
                    {
                        return t.text().chars().filter(|&c| c == '#').count();
                    }
                }
            }
        }

        // Check for setext underline
        if let Some(underline) = support::token(&self.0, SyntaxKind::SETEXT_HEADING_UNDERLINE) {
            // Setext headings: '=' is level 1, '-' is level 2
            if underline.text().starts_with('=') {
                1
            } else {
                2
            }
        } else {
            1 // Default to level 1
        }
    }

    /// Returns the heading content node if present.
    pub fn content(&self) -> Option<HeadingContent> {
        support::child(&self.0)
    }

    /// Returns the heading text as a string.
    pub fn text(&self) -> String {
        self.content().map(|c| c.text()).unwrap_or_default()
    }
}

pub struct HeadingContent(SyntaxNode);

impl AstNode for HeadingContent {
    fn kind() -> SyntaxKind {
        SyntaxKind::HEADING_CONTENT
    }

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::HEADING_CONTENT
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

impl HeadingContent {
    /// Returns the text content of the heading.
    pub fn text(&self) -> String {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|token| token.kind() == SyntaxKind::TEXT)
            .map(|token| token.text().to_string())
            .collect()
    }
}
