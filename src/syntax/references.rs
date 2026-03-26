//! Reference link and footnote AST node wrappers.

use super::ast::support;
use super::links::Link;
use super::{AstNode, PanacheLanguage, SyntaxKind, SyntaxNode};

pub struct ReferenceDefinition(SyntaxNode);

impl AstNode for ReferenceDefinition {
    type Language = PanacheLanguage;

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

    /// Extracts raw destination text from a reference definition body.
    pub fn destination(&self) -> Option<String> {
        let tail = self
            .0
            .children_with_tokens()
            .filter_map(|child| child.into_token())
            .find(|token| token.kind() == SyntaxKind::TEXT)?
            .text()
            .to_string();

        let after_colon = tail.trim_start().strip_prefix(':')?.trim_start();
        if after_colon.is_empty() {
            return None;
        }

        Some(after_colon.to_string())
    }

    /// Returns the text range for the definition label value.
    pub fn label_value_range(&self) -> Option<rowan::TextRange> {
        let link = self.link()?;

        if let Some(range) = link
            .reference()
            .and_then(|reference| reference.label_value_range())
        {
            return Some(range);
        }

        link.text()?
            .syntax()
            .descendants_with_tokens()
            .find_map(|elem| {
                elem.into_token()
                    .filter(|token| token.kind() == SyntaxKind::TEXT)
                    .map(|token| token.text_range())
            })
    }
}

pub struct FootnoteReference(SyntaxNode);

impl AstNode for FootnoteReference {
    type Language = PanacheLanguage;

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
        if let Some(id) = self
            .0
            .children_with_tokens()
            .filter_map(|child| child.into_token())
            .find(|token| token.kind() == SyntaxKind::FOOTNOTE_LABEL_ID)
        {
            return id.text().to_string();
        }

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

    /// Returns the full text range of this reference token.
    pub fn id_range(&self) -> rowan::TextRange {
        self.0.text_range()
    }

    /// Returns the text range for the footnote ID only (excluding `[^` and `]`).
    pub fn id_value_range(&self) -> Option<rowan::TextRange> {
        if let Some(id) = self
            .0
            .children_with_tokens()
            .filter_map(|child| child.into_token())
            .find(|token| token.kind() == SyntaxKind::FOOTNOTE_LABEL_ID)
        {
            return Some(id.text_range());
        }

        let tokens: Vec<_> = self
            .0
            .children_with_tokens()
            .filter_map(|child| child.into_token())
            .filter(|token| token.kind() == SyntaxKind::TEXT)
            .collect();

        if tokens.len() >= 2 && tokens[0].text() == "[^" {
            Some(tokens[1].text_range())
        } else {
            None
        }
    }
}

pub struct FootnoteDefinition(SyntaxNode);

impl AstNode for FootnoteDefinition {
    type Language = PanacheLanguage;

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
        if let Some(id) = self
            .0
            .children_with_tokens()
            .filter_map(|child| child.into_token())
            .find(|token| token.kind() == SyntaxKind::FOOTNOTE_LABEL_ID)
        {
            return id.text().to_string();
        }

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

    /// Returns the text range for the footnote ID only (excluding `[^`, `]`, and `:`).
    pub fn id_value_range(&self) -> Option<rowan::TextRange> {
        if let Some(id) = self
            .0
            .children_with_tokens()
            .filter_map(|child| child.into_token())
            .find(|token| token.kind() == SyntaxKind::FOOTNOTE_LABEL_ID)
        {
            return Some(id.text_range());
        }

        let marker = self
            .0
            .children_with_tokens()
            .filter_map(|child| child.into_token())
            .find(|token| token.kind() == SyntaxKind::FOOTNOTE_REFERENCE)?;

        let marker_text = marker.text();
        if !marker_text.starts_with("[^") {
            return None;
        }

        let close_bracket = marker_text.find(']')?;
        if close_bracket <= 2 {
            return None;
        }

        if marker_text.as_bytes().get(close_bracket + 1) != Some(&b':') {
            return None;
        }

        let token_start = marker.text_range().start();
        let id_start = token_start + rowan::TextSize::from(2);
        let id_end = token_start + rowan::TextSize::from(close_bracket as u32);
        Some(rowan::TextRange::new(id_start, id_end))
    }

    /// Extracts the content of the footnote definition.
    /// Returns the text content after the `[^id]:` marker.
    pub fn content(&self) -> String {
        // Skip the definition marker tokens and collect all other content
        self.0
            .children_with_tokens()
            .filter_map(|child| match child {
                rowan::NodeOrToken::Node(node) => Some(node.text().to_string()),
                rowan::NodeOrToken::Token(token)
                    if !matches!(
                        token.kind(),
                        SyntaxKind::FOOTNOTE_REFERENCE
                            | SyntaxKind::FOOTNOTE_LABEL_START
                            | SyntaxKind::FOOTNOTE_LABEL_ID
                            | SyntaxKind::FOOTNOTE_LABEL_END
                            | SyntaxKind::FOOTNOTE_LABEL_COLON
                    ) =>
                {
                    Some(token.text().to_string())
                }
                _ => None,
            })
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

        // Check for list nodes in the CST (handles nested lists reliably).
        if self
            .0
            .descendants()
            .any(|node| node.kind() == SyntaxKind::LIST)
        {
            return false;
        }

        true
    }
}

pub struct InlineFootnote(SyntaxNode);

impl AstNode for InlineFootnote {
    type Language = PanacheLanguage;

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
    fn test_reference_definition_destination() {
        let input = "[ref]: https://example.com \"Title\"";
        let root = parse(input, None);
        let def = root
            .descendants()
            .find_map(ReferenceDefinition::cast)
            .expect("Should find ReferenceDefinition");

        assert_eq!(def.label(), "ref");
        assert_eq!(
            def.destination().as_deref(),
            Some("https://example.com \"Title\"")
        );
        assert!(def.label_value_range().is_some());
    }

    #[test]
    fn test_footnote_definition_single_line() {
        let input = "[^1]: This is a simple footnote.";
        let root = parse(input, None);
        let def = root
            .descendants()
            .find_map(FootnoteDefinition::cast)
            .expect("Should find FootnoteDefinition");

        assert_eq!(def.id(), "1");
        assert_eq!(
            def.id_value_range()
                .map(|range| {
                    let start: usize = range.start().into();
                    let end: usize = range.end().into();
                    input[start..end].to_string()
                })
                .as_deref(),
            Some("1")
        );
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
        assert_eq!(
            def.id_value_range()
                .map(|range| {
                    let start: usize = range.start().into();
                    let end: usize = range.end().into();
                    input[start..end].to_string()
                })
                .as_deref(),
            Some("note")
        );
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
        assert_eq!(
            ref_node
                .id_value_range()
                .map(|range| {
                    let start: usize = range.start().into();
                    let end: usize = range.end().into();
                    input[start..end].to_string()
                })
                .as_deref(),
            Some("test")
        );
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
