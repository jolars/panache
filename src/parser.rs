//! Parser module for Pandoc/Quarto documents.
//!
//! This module implements a single-pass parser that constructs a lossless syntax tree (CST) for
//! Quarto documents.

use crate::config::Config;
use crate::range_utils::expand_byte_range_to_blocks;
use crate::syntax::SyntaxNode;
use rowan::{GreenNode, NodeOrToken, TextRange, TextSize};

pub mod blocks;
pub mod inlines;
pub mod utils;
pub mod yaml;

mod block_dispatcher;
mod core;

// Re-export main parser
pub use core::Parser;

/// Parses a Quarto document string into a syntax tree.
///
/// Single-pass architecture: blocks emit inline structure during parsing.
///
/// # Examples
///
/// ```rust
/// use panache::parser::parse;
///
/// let input = "# Heading\n\nParagraph text.";
/// let tree = parse(input, None);
/// println!("{:#?}", tree);
/// ```
///
/// # Arguments
///
/// * `input` - The Quarto document content to parse
/// * `config` - Optional configuration. If None, uses default config.
pub fn parse(input: &str, config: Option<Config>) -> SyntaxNode {
    let config = config.unwrap_or_default();
    Parser::new(input, &config).parse()
}

pub struct IncrementalParseResult {
    pub tree: SyntaxNode,
    pub reparse_range: (usize, usize),
}

/// Incrementally update a syntax tree by reparsing an expanded block range
/// and splicing the new subtree into the existing tree.
pub fn parse_incremental(
    input: &str,
    config: Option<Config>,
    old_tree: &SyntaxNode,
    old_edit_range: (usize, usize),
    new_edit_range: (usize, usize),
) -> IncrementalParseResult {
    let config = config.unwrap_or_default();
    let old_root = old_tree.clone();
    let old_expanded = expand_byte_range_to_blocks(old_tree, old_edit_range.0, old_edit_range.1);
    let old_len: usize = old_root.text_range().end().into();
    let new_tree = Parser::new(input, &config).parse();
    let new_root = new_tree.clone();
    let new_expanded = expand_byte_range_to_blocks(&new_root, new_edit_range.0, new_edit_range.1);
    let new_len: usize = new_root.text_range().end().into();

    let old_range = TextRange::new(
        TextSize::from(old_expanded.0.min(old_len) as u32),
        TextSize::from(old_expanded.1.min(old_len) as u32),
    );
    let new_range = TextRange::new(
        TextSize::from(new_expanded.0.min(new_len) as u32),
        TextSize::from(new_expanded.1.min(new_len) as u32),
    );

    let old_element = old_root.covering_element(old_range);
    let new_element = new_root.covering_element(new_range);

    let old_node = match old_element {
        NodeOrToken::Node(node) => node,
        NodeOrToken::Token(token) => token.parent().unwrap_or_else(|| old_root.clone()),
    };

    let new_node = match new_element {
        NodeOrToken::Node(node) => node,
        NodeOrToken::Token(token) => token.parent().unwrap_or_else(|| new_root.clone()),
    };

    let replacement: GreenNode = GreenNode::from(new_node.green());

    let updated_tree = if old_node.kind() == new_node.kind() {
        let new_green = old_node.replace_with(replacement);
        SyntaxNode::new_root(new_green)
    } else {
        new_tree
    };

    IncrementalParseResult {
        tree: updated_tree,
        reparse_range: old_expanded,
    }
}
