//! Parser module for Pandoc/Quarto documents.
//!
//! Single-pass parsing architecture: blocks emit inline structure during parsing.

use crate::config::Config;
use crate::syntax::SyntaxNode;

pub mod blocks;
pub mod inlines;
pub mod list_postprocessor;
pub mod utils;

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
    let block_tree = Parser::new(input, &config).parse();

    // Post-process to wrap list item content in Plain/PARAGRAPH blocks
    let green = list_postprocessor::wrap_list_item_content(block_tree, &config);

    SyntaxNode::new_root(green)
}
