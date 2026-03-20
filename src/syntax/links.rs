//! Link and image AST node wrappers.

use super::ast::support;
use super::{AstNode, PanacheLanguage, SyntaxKind, SyntaxNode};

pub struct Link(SyntaxNode);

impl AstNode for Link {
    type Language = PanacheLanguage;

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

pub struct AutoLink(SyntaxNode);

impl AstNode for AutoLink {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::AUTO_LINK
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

impl AutoLink {
    /// Returns the autolink target text without angle brackets.
    pub fn target(&self) -> String {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|token| token.kind() == SyntaxKind::TEXT)
            .map(|token| token.text().to_string())
            .collect()
    }
}

pub struct LinkText(SyntaxNode);

impl AstNode for LinkText {
    type Language = PanacheLanguage;

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
            .descendants_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|token| token.kind() == SyntaxKind::TEXT)
            .map(|token| token.text().to_string())
            .collect()
    }
}

pub struct LinkDest(SyntaxNode);

impl AstNode for LinkDest {
    type Language = PanacheLanguage;

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

    /// Returns the range for a hash-anchor id within destination text (without '#').
    pub fn hash_anchor_id_range(&self) -> Option<rowan::TextRange> {
        let text = self.0.text().to_string();
        let hash_idx = text.find('#')?;
        let after_hash = &text[hash_idx + 1..];
        let id_len = after_hash
            .chars()
            .take_while(|ch| !ch.is_whitespace() && *ch != ')')
            .map(char::len_utf8)
            .sum::<usize>();
        if id_len == 0 {
            return None;
        }
        let node_start: usize = self.0.text_range().start().into();
        let start = rowan::TextSize::from((node_start + hash_idx + 1) as u32);
        let end = rowan::TextSize::from((node_start + hash_idx + 1 + id_len) as u32);
        Some(rowan::TextRange::new(start, end))
    }

    /// Returns the hash-anchor id within destination text (without '#').
    pub fn hash_anchor_id(&self) -> Option<String> {
        let text = self.0.text().to_string();
        let hash_idx = text.find('#')?;
        let after_hash = &text[hash_idx + 1..];
        let id_len = after_hash
            .chars()
            .take_while(|ch| !ch.is_whitespace() && *ch != ')')
            .map(char::len_utf8)
            .sum::<usize>();
        if id_len == 0 {
            return None;
        }
        Some(after_hash[..id_len].to_string())
    }
}

pub struct LinkRef(SyntaxNode);

impl AstNode for LinkRef {
    type Language = PanacheLanguage;

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

    /// Returns the text range for the reference label (without brackets).
    pub fn label_range(&self) -> Option<rowan::TextRange> {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|token| token.kind() == SyntaxKind::TEXT)
            .map(|token| token.text_range())
    }
}

pub struct ImageLink(SyntaxNode);

impl AstNode for ImageLink {
    type Language = PanacheLanguage;

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

    /// Returns the reference label for reference-style images.
    pub fn reference(&self) -> Option<LinkRef> {
        support::child(&self.0)
    }

    /// Returns the reference label text for reference-style images.
    pub fn reference_label(&self) -> Option<String> {
        self.reference().map(|link_ref| link_ref.label())
    }

    /// Returns the text range for the reference label in reference-style images.
    pub fn reference_label_range(&self) -> Option<rowan::TextRange> {
        self.reference().and_then(|link_ref| link_ref.label_range())
    }
}

pub struct ImageAlt(SyntaxNode);

impl AstNode for ImageAlt {
    type Language = PanacheLanguage;

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
            .descendants_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|token| token.kind() == SyntaxKind::TEXT)
            .map(|token| token.text().to_string())
            .collect()
    }
}

pub struct Figure(SyntaxNode);

impl AstNode for Figure {
    type Language = PanacheLanguage;

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

#[cfg(test)]
mod tests {
    use super::{AstNode, ImageLink};

    #[test]
    fn image_reference_label_and_range_are_extracted() {
        let input = "![Alt text][img]";
        let tree = crate::parse(input, None);
        let image = tree
            .descendants()
            .find_map(ImageLink::cast)
            .expect("image link");

        assert_eq!(image.reference_label().as_deref(), Some("img"));

        let range = image.reference_label_range().expect("label range");
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(&input[start..end], "img");
    }
}
