//! Parser module containing block and inline parsers.

use crate::config::Config;
use crate::syntax::SyntaxNode;

pub mod block_parser;
pub mod inline_parser;

// Re-export commonly used types
pub use block_parser::{BlockParser, ReferenceRegistry};
pub use inline_parser::{InlineParser, parse_inline_text};

/// Parses a Quarto document string into a syntax tree.
///
/// This function runs both the block parser and inline parser to produce
/// a complete concrete syntax tree (CST). Line endings (LF or CRLF) are
/// preserved exactly as they appear in the input.
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
    let (block_tree, reference_registry) = BlockParser::new(input, &config).parse();
    InlineParser::new(block_tree, config, reference_registry).parse()
}
