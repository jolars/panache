//! Typed AST wrappers over the in-tree YAML CST.
//!
//! These wrappers give value-extraction consumers (metadata, bibliography,
//! includes, hashpipe) a typed traversal API over the rowan CST produced by
//! [`crate::parser::yaml::parse_yaml_tree`], replacing the external
//! `yaml_parser` crate's `ast` module. They follow the house pattern
//! ([`super::headings`], [`super::references`]): newtype wrappers with
//! hand-written [`AstNode`] impls and `rowan::ast::support`-based accessors.
//!
//! Two CST facts shape the API:
//!
//! - The parser wraps every parse in a
//!   `DOCUMENT > YAML_METADATA_CONTENT > YAML_STREAM > YAML_DOCUMENT` envelope.
//!   [`parse_yaml_document`] centralizes the descent so no consumer
//!   re-implements it.
//! - `YAML_BLOCK_MAP_KEY` includes the trailing `:` (`YAML_COLON`) token, so
//!   [`YamlBlockMapKey::scalar`] reads the `YAML_SCALAR` token child rather
//!   than the node text.
//!
//! Scalar style is not a CST kind — every style emits `YAML_SCALAR` — so
//! [`YamlScalar`] detects the style from the leading byte and cooks via the
//! shared [`crate::parser::yaml::cook`] primitives.

use rowan::TextRange;

use super::ast::{AstChildren, AstNode, support};
use super::{PanacheLanguage, SyntaxKind, SyntaxNode, SyntaxToken};
use crate::parser::yaml::{ScalarStyle, cook, parse_yaml_tree};

/// Parse `input` and return the first YAML document, descending the host
/// `DOCUMENT > YAML_METADATA_CONTENT > YAML_STREAM` envelope. Returns `None`
/// when the input fails the structural validator (no tree) or has no document.
pub fn parse_yaml_document(input: &str) -> Option<YamlDocument> {
    first_document(&parse_yaml_tree(input)?)
}

/// Parse `input` and return every YAML document in the stream. Most consumers
/// only need the first ([`parse_yaml_document`]); this exists for multi-document
/// completeness (`a: 1\n---\nb: 2`).
pub fn parse_yaml_documents(input: &str) -> Vec<YamlDocument> {
    let Some(tree) = parse_yaml_tree(input) else {
        return Vec::new();
    };
    tree.descendants()
        .find(|n| n.kind() == SyntaxKind::YAML_STREAM)
        .map(|stream| stream.children().filter_map(YamlDocument::cast).collect())
        .unwrap_or_default()
}

fn first_document(tree: &SyntaxNode) -> Option<YamlDocument> {
    tree.descendants()
        .find(|n| n.kind() == SyntaxKind::YAML_STREAM)?
        .children()
        .find_map(YamlDocument::cast)
}

/// The five concrete node shapes a value, sequence item, or document body can
/// take. `None` (i.e. an absent `YamlNode`) models an empty YAML value.
#[derive(Debug, Clone)]
pub enum YamlNode {
    BlockMap(YamlBlockMap),
    BlockSequence(YamlBlockSequence),
    FlowMap(YamlFlowMap),
    FlowSequence(YamlFlowSequence),
    Scalar(YamlScalar),
}

/// Resolve the single content node held by a value / item / document wrapper.
/// Container children take precedence; a bare scalar value resolves to the
/// first `YAML_SCALAR` token (anchors/tags/aliases are skipped). Returns `None`
/// for an empty value.
fn node_child(parent: &SyntaxNode) -> Option<YamlNode> {
    for child in parent.children() {
        match child.kind() {
            SyntaxKind::YAML_BLOCK_MAP => return YamlBlockMap::cast(child).map(YamlNode::BlockMap),
            SyntaxKind::YAML_BLOCK_SEQUENCE => {
                return YamlBlockSequence::cast(child).map(YamlNode::BlockSequence);
            }
            SyntaxKind::YAML_FLOW_MAP => return YamlFlowMap::cast(child).map(YamlNode::FlowMap),
            SyntaxKind::YAML_FLOW_SEQUENCE => {
                return YamlFlowSequence::cast(child).map(YamlNode::FlowSequence);
            }
            _ => {}
        }
    }
    scalar_token(parent).map(YamlNode::Scalar)
}

/// First direct `YAML_SCALAR` token child of `parent`, wrapped. Note flow
/// containers emit their `[`/`]`/`{`/`}`/`,` as `YAML_SCALAR` tokens too, but
/// those live inside the flow node, not as direct children of a value/key.
fn scalar_token(parent: &SyntaxNode) -> Option<YamlScalar> {
    parent
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| t.kind() == SyntaxKind::YAML_SCALAR)
        .and_then(YamlScalar::cast)
}

fn token_of(parent: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxToken> {
    parent
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| t.kind() == kind)
}

/// Projections shared by value / item / document body wrappers. Implemented as
/// a macro so each wrapper gets the same `as_*` surface without repetition.
macro_rules! node_projections {
    () => {
        /// The single content node, or `None` for an empty value.
        pub fn as_node(&self) -> Option<YamlNode> {
            node_child(&self.0)
        }

        /// The value as a scalar, or `None` if it is a container or empty.
        pub fn as_scalar(&self) -> Option<YamlScalar> {
            match self.as_node()? {
                YamlNode::Scalar(s) => Some(s),
                _ => None,
            }
        }

        pub fn as_block_map(&self) -> Option<YamlBlockMap> {
            match self.as_node()? {
                YamlNode::BlockMap(m) => Some(m),
                _ => None,
            }
        }

        pub fn as_block_sequence(&self) -> Option<YamlBlockSequence> {
            match self.as_node()? {
                YamlNode::BlockSequence(s) => Some(s),
                _ => None,
            }
        }

        pub fn as_flow_map(&self) -> Option<YamlFlowMap> {
            match self.as_node()? {
                YamlNode::FlowMap(m) => Some(m),
                _ => None,
            }
        }

        pub fn as_flow_sequence(&self) -> Option<YamlFlowSequence> {
            match self.as_node()? {
                YamlNode::FlowSequence(s) => Some(s),
                _ => None,
            }
        }

        /// Whether this value is empty (no scalar and no container child).
        pub fn is_empty(&self) -> bool {
            self.as_node().is_none()
        }

        /// The explicit `YAML_TAG` token decorating this value (e.g. `!expr`),
        /// if any. Used by the hashpipe formatter to preserve chunk-option tags.
        pub fn tag(&self) -> Option<SyntaxToken> {
            token_of(&self.0, SyntaxKind::YAML_TAG)
        }
    };
}

/// Declare a newtype CST-node wrapper with the standard hand-written
/// [`AstNode`] impl for a single `SyntaxKind`.
macro_rules! ast_node {
    ($(#[$meta:meta])* $name:ident, $kind:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(SyntaxNode);

        impl AstNode for $name {
            type Language = PanacheLanguage;

            fn can_cast(kind: SyntaxKind) -> bool {
                kind == SyntaxKind::$kind
            }

            fn cast(syntax: SyntaxNode) -> Option<Self> {
                Self::can_cast(syntax.kind()).then_some(Self(syntax))
            }

            fn syntax(&self) -> &SyntaxNode {
                &self.0
            }
        }
    };
}

ast_node!(
    /// A single YAML document inside the stream.
    YamlDocument, YAML_DOCUMENT
);

impl YamlDocument {
    pub fn block_map(&self) -> Option<YamlBlockMap> {
        support::child(&self.0)
    }

    pub fn block_sequence(&self) -> Option<YamlBlockSequence> {
        support::child(&self.0)
    }

    pub fn flow_map(&self) -> Option<YamlFlowMap> {
        support::child(&self.0)
    }

    pub fn flow_sequence(&self) -> Option<YamlFlowSequence> {
        support::child(&self.0)
    }

    /// A top-level bare scalar document (`"just a string"`).
    pub fn scalar(&self) -> Option<YamlScalar> {
        scalar_token(&self.0)
    }

    pub fn as_node(&self) -> Option<YamlNode> {
        node_child(&self.0)
    }
}

ast_node!(
    /// A block mapping (`key: value` entries).
    YamlBlockMap, YAML_BLOCK_MAP
);

impl YamlBlockMap {
    pub fn entries(&self) -> AstChildren<YamlBlockMapEntry> {
        support::children(&self.0)
    }

    /// The first entry whose (cooked) key text equals `key`.
    pub fn entry(&self, key: &str) -> Option<YamlBlockMapEntry> {
        self.entries()
            .find(|entry| entry.key_text().as_deref() == Some(key))
    }

    pub fn value_of(&self, key: &str) -> Option<YamlBlockMapValue> {
        self.entry(key)?.value()
    }
}

ast_node!(
    /// One `key: value` pair in a block mapping.
    YamlBlockMapEntry, YAML_BLOCK_MAP_ENTRY
);

impl YamlBlockMapEntry {
    pub fn key(&self) -> Option<YamlBlockMapKey> {
        support::child(&self.0)
    }

    /// The cooked key text. Reads the scalar child of `YAML_BLOCK_MAP_KEY`, so
    /// the trailing `:` token is excluded.
    pub fn key_text(&self) -> Option<String> {
        self.key()?.scalar().map(|s| s.value())
    }

    pub fn value(&self) -> Option<YamlBlockMapValue> {
        support::child(&self.0)
    }
}

ast_node!(
    /// The key side of a block-map entry. Holds the `YAML_SCALAR` token AND the
    /// trailing `YAML_COLON` token.
    YamlBlockMapKey, YAML_BLOCK_MAP_KEY
);

impl YamlBlockMapKey {
    /// The key's scalar token (excluding the `:` colon).
    pub fn scalar(&self) -> Option<YamlScalar> {
        scalar_token(&self.0)
    }
}

ast_node!(
    /// The value side of a block-map entry: a scalar, a nested container, or
    /// empty.
    YamlBlockMapValue, YAML_BLOCK_MAP_VALUE
);

impl YamlBlockMapValue {
    node_projections!();
}

ast_node!(
    /// A block sequence (`- item` entries).
    YamlBlockSequence, YAML_BLOCK_SEQUENCE
);

impl YamlBlockSequence {
    pub fn items(&self) -> AstChildren<YamlBlockSequenceItem> {
        support::children(&self.0)
    }
}

ast_node!(
    /// One `- item` in a block sequence. The leading `-` is a
    /// `YAML_BLOCK_SEQ_ENTRY` token, skipped by the content projections.
    YamlBlockSequenceItem, YAML_BLOCK_SEQUENCE_ITEM
);

impl YamlBlockSequenceItem {
    node_projections!();
}

ast_node!(
    /// A flow sequence (`[a, b, c]`).
    YamlFlowSequence, YAML_FLOW_SEQUENCE
);

impl YamlFlowSequence {
    pub fn items(&self) -> AstChildren<YamlFlowSequenceItem> {
        support::children(&self.0)
    }
}

ast_node!(
    /// One item in a flow sequence.
    YamlFlowSequenceItem, YAML_FLOW_SEQUENCE_ITEM
);

impl YamlFlowSequenceItem {
    node_projections!();
}

ast_node!(
    /// A flow mapping (`{k: v, ...}`).
    YamlFlowMap, YAML_FLOW_MAP
);

impl YamlFlowMap {
    pub fn entries(&self) -> AstChildren<YamlFlowMapEntry> {
        support::children(&self.0)
    }

    pub fn entry(&self, key: &str) -> Option<YamlFlowMapEntry> {
        self.entries()
            .find(|entry| entry.key_text().as_deref() == Some(key))
    }

    pub fn value_of(&self, key: &str) -> Option<YamlFlowMapValue> {
        self.entry(key)?.value()
    }
}

ast_node!(
    /// One `k: v` pair in a flow mapping.
    YamlFlowMapEntry, YAML_FLOW_MAP_ENTRY
);

impl YamlFlowMapEntry {
    pub fn key(&self) -> Option<YamlFlowMapKey> {
        support::child(&self.0)
    }

    pub fn key_text(&self) -> Option<String> {
        self.key()?.scalar().map(|s| s.value())
    }

    pub fn value(&self) -> Option<YamlFlowMapValue> {
        support::child(&self.0)
    }
}

ast_node!(
    /// The key side of a flow-map entry.
    YamlFlowMapKey, YAML_FLOW_MAP_KEY
);

impl YamlFlowMapKey {
    pub fn scalar(&self) -> Option<YamlScalar> {
        scalar_token(&self.0)
    }
}

ast_node!(
    /// The value side of a flow-map entry.
    YamlFlowMapValue, YAML_FLOW_MAP_VALUE
);

impl YamlFlowMapValue {
    node_projections!();
}

/// The lexical style of a scalar, detected from its raw source. (The CST does
/// not record style as a distinct kind — every style is a `YAML_SCALAR` token.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum YamlScalarStyle {
    Plain,
    SingleQuoted,
    DoubleQuoted,
    Literal,
    Folded,
}

impl YamlScalarStyle {
    fn to_cook_style(self) -> ScalarStyle {
        match self {
            YamlScalarStyle::Plain => ScalarStyle::Plain,
            YamlScalarStyle::SingleQuoted => ScalarStyle::SingleQuoted,
            YamlScalarStyle::DoubleQuoted => ScalarStyle::DoubleQuoted,
            YamlScalarStyle::Literal => ScalarStyle::Literal,
            YamlScalarStyle::Folded => ScalarStyle::Folded,
        }
    }
}

fn detect_style(raw: &str) -> YamlScalarStyle {
    match raw.trim_start().as_bytes().first() {
        Some(b'\'') => YamlScalarStyle::SingleQuoted,
        Some(b'"') => YamlScalarStyle::DoubleQuoted,
        Some(b'|') => YamlScalarStyle::Literal,
        Some(b'>') => YamlScalarStyle::Folded,
        _ => YamlScalarStyle::Plain,
    }
}

/// A scalar token. Unlike the node wrappers, this wraps a `SyntaxToken`
/// (`YAML_SCALAR` is a token, not a node), so it is not an [`AstNode`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct YamlScalar(SyntaxToken);

impl YamlScalar {
    pub fn cast(token: SyntaxToken) -> Option<Self> {
        (token.kind() == SyntaxKind::YAML_SCALAR).then_some(Self(token))
    }

    /// The raw source bytes of the scalar, including any quotes / block header.
    pub fn raw(&self) -> &str {
        self.0.text()
    }

    pub fn style(&self) -> YamlScalarStyle {
        detect_style(self.raw())
    }

    /// The cooked logical string: quotes stripped, escapes decoded, multi-line
    /// scalars folded per YAML 1.2. Block scalars (`|`/`>`) are returned raw
    /// (their cooking needs parent indent context).
    pub fn value(&self) -> String {
        let raw = self.raw();
        cook(detect_style(raw).to_cook_style(), raw)
    }

    pub fn text_range(&self) -> TextRange {
        self.0.text_range()
    }

    pub fn syntax(&self) -> &SyntaxToken {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_yaml_document_descends_envelope() {
        let doc = parse_yaml_document("title: x\n").expect("document");
        let map = doc.block_map().expect("block map");
        assert_eq!(map.entries().count(), 1);
    }

    #[test]
    fn key_text_strips_colon() {
        let doc = parse_yaml_document("key: value\n").expect("document");
        let entry = doc.block_map().unwrap().entries().next().unwrap();
        assert_eq!(entry.key_text().as_deref(), Some("key"));
    }

    #[test]
    fn value_is_cooked() {
        let doc = parse_yaml_document("k: 'it''s'\n").expect("document");
        let value = doc.block_map().unwrap().value_of("k").unwrap();
        assert_eq!(value.as_scalar().unwrap().value(), "it's");

        let doc = parse_yaml_document("k: \"a\\nb\"\n").expect("document");
        let value = doc.block_map().unwrap().value_of("k").unwrap();
        assert_eq!(value.as_scalar().unwrap().value(), "a\nb");
    }

    #[test]
    fn raw_preserves_quotes() {
        let doc = parse_yaml_document("k: 'it''s'\n").expect("document");
        let scalar = doc
            .block_map()
            .unwrap()
            .value_of("k")
            .unwrap()
            .as_scalar()
            .unwrap();
        assert_eq!(scalar.raw(), "'it''s'");
        assert_eq!(scalar.style(), YamlScalarStyle::SingleQuoted);
    }

    #[test]
    fn scalar_text_range_is_content_relative() {
        let input = "k: value\n";
        let doc = parse_yaml_document(input).expect("document");
        let scalar = doc
            .block_map()
            .unwrap()
            .value_of("k")
            .unwrap()
            .as_scalar()
            .unwrap();
        let range = scalar.text_range();
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(&input[start..end], "value");
    }

    #[test]
    fn empty_value_has_no_scalar() {
        let doc = parse_yaml_document("k:\n").expect("document");
        let value = doc.block_map().unwrap().value_of("k").unwrap();
        assert!(value.is_empty());
        assert!(value.as_scalar().is_none());
    }

    #[test]
    fn block_sequence_items_yield_scalars() {
        let doc = parse_yaml_document("k:\n  - a\n  - b\n").expect("document");
        let seq = doc
            .block_map()
            .unwrap()
            .value_of("k")
            .unwrap()
            .as_block_sequence()
            .expect("block sequence");
        let items: Vec<String> = seq
            .items()
            .filter_map(|item| item.as_scalar().map(|s| s.value()))
            .collect();
        assert_eq!(items, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn flow_sequence_items_yield_scalars() {
        let doc = parse_yaml_document("k: [a, b]\n").expect("document");
        let seq = doc
            .block_map()
            .unwrap()
            .value_of("k")
            .unwrap()
            .as_flow_sequence()
            .expect("flow sequence");
        let items: Vec<String> = seq
            .items()
            .filter_map(|item| item.as_scalar().map(|s| s.value()))
            .collect();
        assert_eq!(items, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn tag_token_is_exposed_and_scalar_ignores_it() {
        let doc = parse_yaml_document("k: !expr foo\n").expect("document");
        let value = doc.block_map().unwrap().value_of("k").unwrap();
        assert_eq!(
            value.tag().map(|t| t.text().to_string()),
            Some("!expr".to_string())
        );
        assert_eq!(value.as_scalar().unwrap().raw(), "foo");
    }

    #[test]
    fn quoted_key_with_colon_round_trips() {
        let doc = parse_yaml_document("\"foo:bar\": 1\n").expect("document");
        let entry = doc.block_map().unwrap().entries().next().unwrap();
        assert_eq!(entry.key_text().as_deref(), Some("foo:bar"));
        assert_eq!(entry.key().unwrap().scalar().unwrap().raw(), "\"foo:bar\"");
    }

    #[test]
    fn parse_yaml_documents_returns_all_documents() {
        let docs = parse_yaml_documents("a: 1\n---\nb: 2\n");
        assert_eq!(docs.len(), 2);
    }

    #[test]
    fn invalid_yaml_yields_no_document() {
        assert!(parse_yaml_document("k: [\n").is_none());
    }
}
