//! Chunk option AST wrappers for type-safe access to chunk options in executable code blocks.

use crate::syntax::{AstNode, SyntaxKind, SyntaxNode};

/// A chunk option in an executable code block (e.g., `echo=TRUE` or `fig.cap="text"`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChunkOption(SyntaxNode);

impl AstNode for ChunkOption {
    fn kind() -> SyntaxKind {
        SyntaxKind::CHUNK_OPTION
    }

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::CHUNK_OPTION
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        Self::can_cast(node.kind()).then(|| ChunkOption(node))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl ChunkOption {
    /// Get the option key (e.g., "echo", "fig.cap").
    pub fn key(&self) -> Option<String> {
        self.0.children_with_tokens().find_map(|child| {
            if let rowan::NodeOrToken::Token(token) = child
                && token.kind() == SyntaxKind::CHUNK_OPTION_KEY
            {
                return Some(token.text().to_string());
            }
            None
        })
    }

    /// Get the option value (e.g., "TRUE", "A nice plot").
    /// Returns None for options without values.
    pub fn value(&self) -> Option<String> {
        self.0.children_with_tokens().find_map(|child| {
            if let rowan::NodeOrToken::Token(token) = child
                && token.kind() == SyntaxKind::CHUNK_OPTION_VALUE
            {
                return Some(token.text().to_string());
            }
            None
        })
    }

    /// Check if the value is quoted (has CHUNK_OPTION_QUOTE nodes).
    pub fn is_quoted(&self) -> bool {
        self.0.children_with_tokens().any(|child| {
            if let rowan::NodeOrToken::Token(token) = child {
                token.kind() == SyntaxKind::CHUNK_OPTION_QUOTE
            } else {
                false
            }
        })
    }

    /// Get the quote character if the value is quoted.
    pub fn quote_char(&self) -> Option<char> {
        self.0.children_with_tokens().find_map(|child| {
            if let rowan::NodeOrToken::Token(token) = child
                && token.kind() == SyntaxKind::CHUNK_OPTION_QUOTE
            {
                return token.text().chars().next();
            }
            None
        })
    }
}

/// A chunk label in an executable code block (e.g., `mylabel` in `{r mylabel}`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChunkLabel(SyntaxNode);

impl AstNode for ChunkLabel {
    fn kind() -> SyntaxKind {
        SyntaxKind::CHUNK_LABEL
    }

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::CHUNK_LABEL
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        Self::can_cast(node.kind()).then(|| ChunkLabel(node))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl ChunkLabel {
    /// Get the label text.
    pub fn text(&self) -> String {
        self.0.text().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn test_chunk_option_quoted() {
        let tree = parse(
            r#"```{r, fig.cap="A nice plot"}
x <- 1
```"#,
            None,
        );

        let option = tree
            .descendants()
            .find_map(ChunkOption::cast)
            .expect("Should find chunk option");

        assert_eq!(option.key(), Some("fig.cap".to_string()));
        assert_eq!(option.value(), Some("A nice plot".to_string()));
        assert!(option.is_quoted());
        assert_eq!(option.quote_char(), Some('"'));
    }

    #[test]
    fn test_chunk_option_unquoted() {
        let tree = parse("```{r, echo=TRUE}\nx <- 1\n```", None);

        let option = tree
            .descendants()
            .find_map(ChunkOption::cast)
            .expect("Should find chunk option");

        assert_eq!(option.key(), Some("echo".to_string()));
        assert_eq!(option.value(), Some("TRUE".to_string()));
        assert!(!option.is_quoted());
    }

    #[test]
    fn test_chunk_label() {
        let tree = parse("```{r mylabel}\nx <- 1\n```", None);

        let label = tree
            .descendants()
            .find_map(ChunkLabel::cast)
            .expect("Should find chunk label");

        assert_eq!(label.text(), "mylabel");
    }
}
