pub use panache_parser::parser::blocks;
pub use panache_parser::parser::inlines;
pub use panache_parser::parser::utils;
pub use panache_parser::parser::yaml;

use crate::config::ParserOptions;
use crate::syntax::SyntaxNode;

pub fn parse(input: &str, config: Option<ParserOptions>) -> SyntaxNode {
    panache_parser::parser::parse(input, config)
}
