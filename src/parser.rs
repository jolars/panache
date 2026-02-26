//! Parser module for Pandoc/Quarto documents.
//!
//! This module implements a single-pass parser that constructs a lossless syntax tree (CST) for
//! Quarto documents.

use crate::config::Config;
use crate::syntax::SyntaxNode;

pub mod blocks;
pub mod inlines;
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
    Parser::new(input, &config).parse()
}
