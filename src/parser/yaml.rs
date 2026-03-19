//! YAML parser groundwork for long-term Panache integration.
//!
//! This module is intentionally minimal and currently acts as a placeholder for a
//! future in-tree YAML parser that can produce Panache-compatible CST structures.
//! Initial goals:
//! - support plain YAML and hashpipe-prefixed YAML from shared parsing primitives,
//! - preserve lossless syntax/trivia needed for exact host document ranges,
//! - enable shadow-mode comparison against the existing YAML engine before rollout.
//! - prepare for first-class YAML formatting support once parser parity is proven.

use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::GreenNodeBuilder;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BasicYamlEntry<'a> {
    pub key: &'a str,
    pub value: &'a str,
}

/// Parse a single-line YAML mapping entry like `title: My Title`.
///
/// This is intentionally minimal groundwork and currently supports exactly one
/// `key: value` line.
pub fn parse_basic_entry(input: &str) -> Option<BasicYamlEntry<'_>> {
    if input.contains('\n') {
        return None;
    }

    let (raw_key, raw_value) = input.split_once(':')?;
    let key = raw_key.trim();
    let value = raw_value.trim();

    if key.is_empty() || value.is_empty() {
        return None;
    }

    Some(BasicYamlEntry { key, value })
}

/// Parse a single-line YAML mapping entry and emit a tiny Rowan CST.
///
/// The current prototype emits:
/// DOCUMENT
///   YAML_METADATA_CONTENT
///     TEXT(key)
///     TEXT(":")
///     [WHITESPACE(" ")] // when present in the original input
///     TEXT(value)
pub fn parse_basic_entry_tree(input: &str) -> Option<SyntaxNode> {
    let entry = parse_basic_entry(input)?;
    let (_, raw_value) = input.split_once(':')?;

    let mut builder = GreenNodeBuilder::new();
    builder.start_node(SyntaxKind::DOCUMENT.into());
    builder.start_node(SyntaxKind::YAML_METADATA_CONTENT.into());
    builder.token(SyntaxKind::TEXT.into(), entry.key);
    builder.token(SyntaxKind::TEXT.into(), ":");

    let leading_spaces = raw_value.len() - raw_value.trim_start_matches(' ').len();
    if leading_spaces > 0 {
        builder.token(SyntaxKind::WHITESPACE.into(), &raw_value[..leading_spaces]);
    }
    builder.token(SyntaxKind::TEXT.into(), entry.value);
    builder.finish_node(); // YAML_METADATA_CONTENT
    builder.finish_node(); // DOCUMENT

    Some(SyntaxNode::new_root(builder.finish()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_title_entry() {
        let parsed = parse_basic_entry("title: My Title");
        assert_eq!(
            parsed,
            Some(BasicYamlEntry {
                key: "title",
                value: "My Title"
            })
        );
    }

    #[test]
    fn rejects_missing_value() {
        assert_eq!(parse_basic_entry("title:"), None);
    }

    #[test]
    fn rejects_multiline_input() {
        assert_eq!(parse_basic_entry("title: My Title\nauthor: Me"), None);
    }

    #[test]
    fn accepts_single_line_with_crlf_terminator() {
        let parsed = parse_basic_entry("title: My Title\r");
        assert_eq!(
            parsed,
            Some(BasicYamlEntry {
                key: "title",
                value: "My Title"
            })
        );
    }

    #[test]
    fn builds_basic_rowan_tree() {
        let tree = parse_basic_entry_tree("title: My Title").expect("tree");
        assert_eq!(tree.kind(), SyntaxKind::DOCUMENT);
        assert_eq!(tree.text().to_string(), "title: My Title");

        let content = tree
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_METADATA_CONTENT)
            .expect("yaml metadata content");
        assert_eq!(content.text().to_string(), "title: My Title");
    }

    #[test]
    fn rejects_tree_for_invalid_input() {
        assert!(parse_basic_entry_tree("title:").is_none());
    }
}
