//! Heading AST node wrappers.

use super::ast::support;
use super::{AstNode, PanacheLanguage, SyntaxKind, SyntaxNode};

pub struct Heading(SyntaxNode);

impl AstNode for Heading {
    type Language = PanacheLanguage;

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

    /// Returns the heading text range.
    pub fn text_range(&self) -> rowan::TextRange {
        self.0.text_range()
    }

    /// Returns heading text, or a placeholder when empty.
    pub fn title_or(&self, placeholder: &str) -> String {
        let text = self.text();
        if text.is_empty() {
            placeholder.to_string()
        } else {
            text
        }
    }

    /// Returns the text range of the ATX marker token (e.g. `###`), if this is an ATX heading.
    pub fn atx_marker_range(&self) -> Option<rowan::TextRange> {
        self.0
            .children()
            .find(|child| child.kind() == SyntaxKind::ATX_HEADING_MARKER)
            .and_then(|marker_node| {
                marker_node
                    .children_with_tokens()
                    .find_map(|el| el.as_token().map(|token| token.text_range()))
            })
    }
}

pub struct HeadingContent(SyntaxNode);

impl AstNode for HeadingContent {
    type Language = PanacheLanguage;

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
            .descendants_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|token| {
                matches!(
                    token.kind(),
                    SyntaxKind::TEXT
                        | SyntaxKind::INLINE_CODE_CONTENT
                        | SyntaxKind::INLINE_EXEC_CONTENT
                )
            })
            .map(|token| token.text().to_string())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_title_or_returns_placeholder_for_empty_heading() {
        let tree = crate::parse("# \n", None);
        let heading = tree.descendants().find_map(Heading::cast).expect("heading");
        assert_eq!(heading.title_or("(empty)"), "(empty)");
    }

    #[test]
    fn heading_atx_marker_range_points_to_hashes() {
        let input = "### Title\n";
        let tree = crate::parse(input, None);
        let heading = tree.descendants().find_map(Heading::cast).expect("heading");
        let range = heading.atx_marker_range().expect("marker range");
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(&input[start..end], "###");
    }
}
