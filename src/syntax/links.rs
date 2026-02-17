//! Link and image AST node wrappers.

use super::ast::support;
use super::{AstNode, SyntaxKind, SyntaxNode};

pub struct Link(SyntaxNode);

impl AstNode for Link {
    fn kind() -> SyntaxKind {
        SyntaxKind::LINK
    }

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::LINK
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

impl Link {
    /// Returns the link text node.
    pub fn text(&self) -> Option<LinkText> {
        support::child(&self.0)
    }

    /// Returns the link destination node.
    pub fn dest(&self) -> Option<LinkDest> {
        support::child(&self.0)
    }

    /// Returns the reference label for reference-style links.
    pub fn reference(&self) -> Option<LinkRef> {
        support::child(&self.0)
    }
}

pub struct LinkText(SyntaxNode);

impl AstNode for LinkText {
    fn kind() -> SyntaxKind {
        SyntaxKind::LINK_TEXT
    }

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::LINK_TEXT
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

impl LinkText {
    /// Returns the text content.
    pub fn text_content(&self) -> String {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|token| token.kind() == SyntaxKind::TEXT)
            .map(|token| token.text().to_string())
            .collect()
    }
}

pub struct LinkDest(SyntaxNode);

impl AstNode for LinkDest {
    fn kind() -> SyntaxKind {
        SyntaxKind::LINK_DEST
    }

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::LINK_DEST
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

impl LinkDest {
    /// Returns the URL/destination as a string (with surrounding parentheses).
    pub fn url(&self) -> String {
        self.0.text().to_string()
    }

    /// Returns the URL without parentheses.
    pub fn url_content(&self) -> String {
        let text = self.0.text().to_string();
        text.trim_start_matches('(')
            .trim_end_matches(')')
            .to_string()
    }
}

pub struct LinkRef(SyntaxNode);

impl AstNode for LinkRef {
    fn kind() -> SyntaxKind {
        SyntaxKind::LINK_REF
    }

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::LINK_REF
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

impl LinkRef {
    /// Returns the reference label text.
    pub fn label(&self) -> String {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|token| token.kind() == SyntaxKind::TEXT)
            .map(|token| token.text().to_string())
            .collect()
    }
}

pub struct ImageLink(SyntaxNode);

impl AstNode for ImageLink {
    fn kind() -> SyntaxKind {
        SyntaxKind::IMAGE_LINK
    }

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::IMAGE_LINK
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

impl ImageLink {
    /// Returns the alt text node.
    pub fn alt(&self) -> Option<ImageAlt> {
        support::child(&self.0)
    }

    /// Returns the image destination.
    pub fn dest(&self) -> Option<LinkDest> {
        support::child(&self.0)
    }
}

pub struct ImageAlt(SyntaxNode);

impl AstNode for ImageAlt {
    fn kind() -> SyntaxKind {
        SyntaxKind::IMAGE_ALT
    }

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::IMAGE_ALT
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

impl ImageAlt {
    /// Returns the alt text content.
    pub fn text(&self) -> String {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|token| token.kind() == SyntaxKind::TEXT)
            .map(|token| token.text().to_string())
            .collect()
    }
}

pub struct Figure(SyntaxNode);

impl AstNode for Figure {
    fn kind() -> SyntaxKind {
        SyntaxKind::FIGURE
    }

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::FIGURE
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

impl Figure {
    /// Returns the image link within the figure.
    pub fn image(&self) -> Option<ImageLink> {
        support::child(&self.0)
    }
}
