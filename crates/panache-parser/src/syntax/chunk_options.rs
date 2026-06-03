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

/// A class attribute in an executable code block (e.g., `.marimo` in
/// `{python .marimo}`). The text accessor includes the leading `.`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChunkClass(SyntaxNode);

impl AstNode for ChunkClass {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::ATTR_CLASS
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        Self::can_cast(node.kind()).then(|| ChunkClass(node))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl ChunkClass {
    pub fn text(&self) -> String {
        self.0.text().to_string()
    }

    pub fn range(&self) -> rowan::TextRange {
        self.0.text_range()
    }
}

/// An id attribute in an executable code block (e.g., `#setup` in
/// `{r #setup}`). The text accessor includes the leading `#`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChunkId(SyntaxNode);

impl AstNode for ChunkId {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::ATTR_ID
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        Self::can_cast(node.kind()).then(|| ChunkId(node))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl ChunkId {
    pub fn text(&self) -> String {
        self.0.text().to_string()
    }

    pub fn range(&self) -> rowan::TextRange {
        self.0.text_range()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChunkOptionSource {
    InlineInfo,
    HashpipeYaml,
}

/// A single chunk option, decoupled from its source CST shape. Inline
/// options come from `CHUNK_OPTION` nodes on the fence info line; hashpipe
/// options come from the embedded YAML block map under
/// `HASHPIPE_YAML_CONTENT`. Both project to the same key/value/range view so
/// `CodeBlock::merged_chunk_option_entries` can mix them.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChunkOptionEntry {
    key: Option<String>,
    value: Option<String>,
    key_range: Option<rowan::TextRange>,
    value_range: Option<rowan::TextRange>,
    is_quoted: bool,
    declaration_range: rowan::TextRange,
    source: ChunkOptionSource,
}

impl ChunkOptionEntry {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        key: Option<String>,
        value: Option<String>,
        key_range: Option<rowan::TextRange>,
        value_range: Option<rowan::TextRange>,
        is_quoted: bool,
        declaration_range: rowan::TextRange,
        source: ChunkOptionSource,
    ) -> Self {
        Self {
            key,
            value,
            key_range,
            value_range,
            is_quoted,
            declaration_range,
            source,
        }
    }

    /// Build an entry from an inline `CHUNK_OPTION` node (fence info line).
    pub fn from_inline_option(option: &ChunkOption, source: ChunkOptionSource) -> Self {
        Self::new(
            option.key(),
            option.value(),
            option.key_range(),
            option.value_range(),
            option.is_quoted(),
            option.syntax().text_range(),
            source,
        )
    }

    pub fn source(&self) -> ChunkOptionSource {
        self.source
    }

    pub fn key(&self) -> Option<String> {
        self.key.clone()
    }

    pub fn key_range(&self) -> Option<rowan::TextRange> {
        self.key_range
    }

    pub fn value(&self) -> Option<String> {
        self.value.clone()
    }

    pub fn value_range(&self) -> Option<rowan::TextRange> {
        self.value_range
    }

    pub fn is_quoted(&self) -> bool {
        self.is_quoted
    }

    /// The full source range of the option declaration (the `CHUNK_OPTION`
    /// node for inline options, the `YAML_BLOCK_MAP_ENTRY` for hashpipe).
    pub fn declaration_range(&self) -> rowan::TextRange {
        self.declaration_range
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
    Class(ChunkClass),
    Id(ChunkId),
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
            if let Some(class) = ChunkClass::cast(child.clone()) {
                return Some(ChunkInfoItem::Class(class));
            }
            if let Some(id) = ChunkId::cast(child.clone()) {
                return Some(ChunkInfoItem::Id(id));
            }
            ChunkOption::cast(child).map(ChunkInfoItem::Option)
        })
    }

    pub fn option_entries(&self, source: ChunkOptionSource) -> Vec<ChunkOptionEntry> {
        self.options()
            .map(|option| ChunkOptionEntry::from_inline_option(&option, source))
            .collect()
    }
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
