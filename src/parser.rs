//! Compatibility wrapper around the internal panache-parser crate.

pub use panache_parser::parser::IncrementalParseResult;
pub use panache_parser::parser::Parser;
pub use panache_parser::parser::blocks;
pub use panache_parser::parser::inlines;
pub use panache_parser::parser::parse_incremental_suffix;
pub use panache_parser::parser::utils;
pub use panache_parser::parser::yaml;

use crate::config::Config;
use crate::syntax::SyntaxNode;

pub fn parse(input: &str, config: Option<Config>) -> SyntaxNode {
    panache_parser::parser::parse(input, config)
}
