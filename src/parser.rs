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
/// This function normalizes line endings and runs both the block parser
/// and inline parser to produce a complete concrete syntax tree (CST).
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
    let normalized_input = input.replace("\r\n", "\n");
    let config = config.unwrap_or_default();
    let (block_tree, reference_registry) = BlockParser::new(&normalized_input, &config).parse();
    InlineParser::new(block_tree, config, reference_registry).parse()
}
