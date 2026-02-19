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

    /// Extracts the content of the footnote definition.
    /// Returns the text content after the `[^id]:` marker.
    pub fn content(&self) -> String {
        // Skip the FOOTNOTE_REFERENCE token and collect all other content
        self.0
            .children()
            .filter(|child| child.kind() != SyntaxKind::FOOTNOTE_REFERENCE)
            .map(|child| child.text().to_string())
            .collect::<Vec<_>>()
            .join("")
    }

    /// Check if this footnote definition is simple (single paragraph, no complex blocks).
    /// Simple footnotes can be converted to inline style.
    pub fn is_simple(&self) -> bool {
        // Simple footnote has:
        // - No blank lines in content (single paragraph)
        // - No code blocks, lists, or other block elements
        let content = self.content();

        // Check for blank lines (indicates multi-paragraph)
        if content.contains("\n\n") {
            return false;
        }

        // Check for code blocks (need to distinguish from continuation lines)
        // Code blocks have 8+ spaces (4 for footnote + 4 for code)
        if content
            .lines()
            .skip(1)
            .any(|line| line.len() > 8 && line.starts_with("        "))
        {
            return false;
        }

        // Check for list markers in continuation lines (after first line)
        for line in content.lines().skip(1) {
            let trimmed = line.trim_start();
            if trimmed.starts_with("- ")
                || trimmed.starts_with("* ")
                || trimmed.starts_with("+ ")
                || (trimmed
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_digit())
                    .unwrap_or(false)
                    && trimmed.chars().skip(1).any(|c| c == '.'))
            {
                return false;
            }
        }

        true
    }
}

pub struct InlineFootnote(SyntaxNode);

impl AstNode for InlineFootnote {
    fn kind() -> SyntaxKind {
        SyntaxKind::INLINE_FOOTNOTE
    }

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::INLINE_FOOTNOTE
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

impl InlineFootnote {
    /// Extracts the content of the inline footnote (text between ^[ and ]).
    pub fn content(&self) -> String {
        self.0
            .children_with_tokens()
            .filter_map(|child| {
                if let Some(token) = child.as_token() {
                    // Skip the start and end markers
                    if token.kind() != SyntaxKind::INLINE_FOOTNOTE_START
                        && token.kind() != SyntaxKind::INLINE_FOOTNOTE_END
                    {
                        Some(token.text().to_string())
                    } else {
                        None
                    }
                } else {
                    // Include nested nodes (emphasis, code, etc.)
                    child.as_node().map(|node| node.text().to_string())
                }
            })
            .collect::<Vec<_>>()
            .join("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn test_footnote_definition_single_line() {
        let input = "[^1]: This is a simple footnote.";
        let root = parse(input, None);
        let def = root
            .descendants()
            .find_map(FootnoteDefinition::cast)
            .expect("Should find FootnoteDefinition");

        assert_eq!(def.id(), "1");
        assert_eq!(def.content().trim(), "This is a simple footnote.");
        assert!(def.is_simple(), "Single line footnote should be simple");
    }

    #[test]
    fn test_footnote_definition_multiline() {
        let input = "[^1]: First line\n    Second line";
        let root = parse(input, None);
        let def = root
            .descendants()
            .find_map(FootnoteDefinition::cast)
            .expect("Should find FootnoteDefinition");

        assert_eq!(def.id(), "1");
        let content = def.content();
        assert!(content.contains("First line"));
        assert!(content.contains("Second line"));
        assert!(def.is_simple(), "Continuation lines should still be simple");
    }

    #[test]
    fn test_footnote_definition_with_formatting() {
        let input = "[^note]: Text with *emphasis* and `code`.";
        let root = parse(input, None);
        let def = root
            .descendants()
            .find_map(FootnoteDefinition::cast)
            .expect("Should find FootnoteDefinition");

        assert_eq!(def.id(), "note");
        let content = def.content();
        assert!(content.contains("*emphasis*"));
        assert!(content.contains("`code`"));
    }

    #[test]
    fn test_footnote_definition_empty() {
        let input = "[^1]: ";
        let root = parse(input, None);
        let def = root
            .descendants()
            .find_map(FootnoteDefinition::cast)
            .expect("Should find FootnoteDefinition");

        assert_eq!(def.id(), "1");
        assert!(def.content().trim().is_empty());
    }

    #[test]
    fn test_footnote_reference_id() {
        let input = "[^test]";
        let root = parse(input, None);
        let ref_node = root
            .descendants()
            .find_map(FootnoteReference::cast)
            .expect("Should find FootnoteReference");

        assert_eq!(ref_node.id(), "test");
    }

    #[test]
    fn test_footnote_definition_is_simple() {
        // Simple single-line
        let input = "[^1]: Simple text.";
        let root = parse(input, None);
        let def = root
            .descendants()
            .find_map(FootnoteDefinition::cast)
            .unwrap();
        assert!(def.is_simple());

        // Simple with continuation
        let input2 = "[^1]: First line\n    continuation.";
        let root2 = parse(input2, None);
        let def2 = root2
            .descendants()
            .find_map(FootnoteDefinition::cast)
            .unwrap();
        assert!(def2.is_simple());
    }

    #[test]
    fn test_footnote_definition_is_complex() {
        // Multi-paragraph (blank line)
        let input = "[^1]: First para.\n\n    Second para.";
        let root = parse(input, None);
        let def = root
            .descendants()
            .find_map(FootnoteDefinition::cast)
            .unwrap();
        assert!(!def.is_simple(), "Multi-paragraph should not be simple");

        // With list
        let input2 = "[^1]: Text\n    - Item 1\n    - Item 2";
        let root2 = parse(input2, None);
        let def2 = root2
            .descendants()
            .find_map(FootnoteDefinition::cast)
            .unwrap();
        assert!(!def2.is_simple(), "Footnote with list should not be simple");

        // With code block
        let input3 = "[^1]: Text\n\n        code block";
        let root3 = parse(input3, None);
        let def3 = root3
            .descendants()
            .find_map(FootnoteDefinition::cast)
            .unwrap();
        assert!(
            !def3.is_simple(),
            "Footnote with code block should not be simple"
        );
    }

    #[test]
    fn test_inline_footnote_content() {
        let input = "Text^[This is an inline note] more text.";
        let root = parse(input, None);
        let inline = root
            .descendants()
            .find_map(InlineFootnote::cast)
            .expect("Should find InlineFootnote");

        assert_eq!(inline.content(), "This is an inline note");
    }

    #[test]
    fn test_inline_footnote_with_formatting() {
        let input = "Text^[Note with *emphasis* and `code`] more.";
        let root = parse(input, None);
        let inline = root
            .descendants()
            .find_map(InlineFootnote::cast)
            .expect("Should find InlineFootnote");

        let content = inline.content();
        assert!(content.contains("emphasis"));
        assert!(content.contains("code"));
    }
}
