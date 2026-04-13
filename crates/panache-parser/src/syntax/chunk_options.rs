//! Chunk option AST wrappers for type-safe access to chunk options in executable code blocks.

use crate::syntax::ast::support;
use crate::syntax::{AstNode, PanacheLanguage, SyntaxKind, SyntaxNode};

/// A chunk option in an executable code block (e.g., `echo=TRUE` or `fig.cap="text"`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChunkOption(SyntaxNode);

impl AstNode for ChunkOption {
    type Language = PanacheLanguage;

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

    /// Get the value token range, if present.
    pub fn value_range(&self) -> Option<rowan::TextRange> {
        self.0.children_with_tokens().find_map(|child| {
            if let rowan::NodeOrToken::Token(token) = child
                && token.kind() == SyntaxKind::CHUNK_OPTION_VALUE
            {
                return Some(token.text_range());
            }
            None
        })
    }

    /// Get the key token range, if present.
    pub fn key_range(&self) -> Option<rowan::TextRange> {
        self.0.children_with_tokens().find_map(|child| {
            if let rowan::NodeOrToken::Token(token) = child
                && token.kind() == SyntaxKind::CHUNK_OPTION_KEY
            {
                return Some(token.text_range());
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
    type Language = PanacheLanguage;

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

    /// Get the label text range.
    pub fn range(&self) -> rowan::TextRange {
        self.0.text_range()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChunkOptionSource {
    InlineInfo,
    HashpipeYaml,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChunkOptionEntry {
    option: ChunkOption,
    source: ChunkOptionSource,
}

impl ChunkOptionEntry {
    pub fn new(option: ChunkOption, source: ChunkOptionSource) -> Self {
        Self { option, source }
    }

    pub fn option(&self) -> &ChunkOption {
        &self.option
    }

    pub fn into_option(self) -> ChunkOption {
        self.option
    }

    pub fn source(&self) -> ChunkOptionSource {
        self.source
    }

    pub fn key(&self) -> Option<String> {
        self.option.key()
    }

    pub fn key_range(&self) -> Option<rowan::TextRange> {
        self.option.key_range()
    }

    pub fn value(&self) -> Option<String> {
        self.option.value()
    }

    pub fn value_range(&self) -> Option<rowan::TextRange> {
        self.option.value_range()
    }

    pub fn is_quoted(&self) -> bool {
        self.option.is_quoted()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChunkLabelSource {
    InlineLabel,
    LabelOption,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChunkLabelEntry {
    value: String,
    declaration_range: rowan::TextRange,
    value_range: rowan::TextRange,
    source: ChunkLabelSource,
}

impl ChunkLabelEntry {
    pub fn new(
        value: String,
        declaration_range: rowan::TextRange,
        value_range: rowan::TextRange,
        source: ChunkLabelSource,
    ) -> Self {
        Self {
            value,
            declaration_range,
            value_range,
            source,
        }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn source(&self) -> ChunkLabelSource {
        self.source
    }

    pub fn declaration_range(&self) -> rowan::TextRange {
        self.declaration_range
    }

    pub fn value_range(&self) -> rowan::TextRange {
        self.value_range
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChunkInfoItem {
    Label(ChunkLabel),
    Option(ChunkOption),
}

pub struct ChunkOptions(SyntaxNode);

impl AstNode for ChunkOptions {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::CHUNK_OPTIONS
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

impl ChunkOptions {
    pub fn options(&self) -> impl Iterator<Item = ChunkOption> {
        support::children(&self.0)
    }

    pub fn labels(&self) -> impl Iterator<Item = ChunkLabel> {
        self.0.children().filter_map(ChunkLabel::cast)
    }

    pub fn items(&self) -> impl Iterator<Item = ChunkInfoItem> {
        self.0.children().filter_map(|child| {
            if let Some(label) = ChunkLabel::cast(child.clone()) {
                return Some(ChunkInfoItem::Label(label));
            }
            ChunkOption::cast(child).map(ChunkInfoItem::Option)
        })
    }

    pub fn option_entries(&self, source: ChunkOptionSource) -> Vec<ChunkOptionEntry> {
        self.options()
            .map(|option| ChunkOptionEntry::new(option, source))
            .collect()
    }
}

pub fn collect_option_entries_from_descendants(
    root: &SyntaxNode,
    source: ChunkOptionSource,
) -> Vec<ChunkOptionEntry> {
    root.descendants()
        .filter_map(ChunkOption::cast)
        .map(|option| ChunkOptionEntry::new(option, source))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::{Flavor, ParserOptions};
    use crate::parse;

    #[test]
    fn test_chunk_option_quoted() {
        let config = ParserOptions {
            flavor: Flavor::Quarto,
            extensions: crate::options::Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        };
        let tree = parse(
            r#"```{r, fig.cap="A nice plot"}
x <- 1
```"#,
            Some(config),
        );

        let option = tree
            .descendants()
            .find_map(ChunkOption::cast)
            .expect("Should find chunk option");

        assert_eq!(option.key(), Some("fig.cap".to_string()));
        assert_eq!(option.value(), Some("A nice plot".to_string()));
        assert!(option.key_range().is_some());
        assert!(option.value_range().is_some());
        assert!(option.is_quoted());
        assert_eq!(option.quote_char(), Some('"'));
    }

    #[test]
    fn test_chunk_option_unquoted() {
        let config = ParserOptions {
            flavor: Flavor::Quarto,
            extensions: crate::options::Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        };
        let tree = parse("```{r, echo=TRUE}\nx <- 1\n```", Some(config));

        let option = tree
            .descendants()
            .find_map(ChunkOption::cast)
            .expect("Should find chunk option");

        assert_eq!(option.key(), Some("echo".to_string()));
        assert_eq!(option.value(), Some("TRUE".to_string()));
        assert!(option.key_range().is_some());
        assert!(option.value_range().is_some());
        assert!(!option.is_quoted());
    }

    #[test]
    fn test_chunk_label() {
        let config = ParserOptions {
            flavor: Flavor::Quarto,
            extensions: crate::options::Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        };
        let tree = parse("```{r mylabel}\nx <- 1\n```", Some(config));

        let label = tree
            .descendants()
            .find_map(ChunkLabel::cast)
            .expect("Should find chunk label");

        assert_eq!(label.text(), "mylabel");
        assert!(!label.range().is_empty());
    }
}
