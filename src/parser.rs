//! Compatibility wrapper around the internal panache-parser crate.

pub use panache_parser::parser::IncrementalParseResult;
pub use panache_parser::parser::Parser;
pub use panache_parser::parser::blocks;
pub use panache_parser::parser::inlines;
pub use panache_parser::parser::utils;
pub use panache_parser::parser::yaml;
pub use panache_parser::to_pandoc_ast;
pub use panache_parser::{RefdefMap, collect_refdef_labels};

use crate::config::Config;
use crate::syntax::SyntaxNode;

pub fn parse(input: &str, config: Option<Config>) -> SyntaxNode {
    let parser_config = config.map(|c| c.parser_options());
    panache_parser::parser::parse(input, parser_config)
}

/// Parse with a caller-supplied refdef set, skipping the
/// `collect_refdef_labels` scan. See
/// [`panache_parser::parse_with_refdefs`].
pub fn parse_with_refdefs(input: &str, config: Option<Config>, refdefs: RefdefMap) -> SyntaxNode {
    let parser_config = config.map(|c| c.parser_options());
    panache_parser::parser::parse_with_refdefs(input, parser_config, refdefs)
}

pub fn parse_incremental_suffix(
    input: &str,
    config: Option<Config>,
    old_tree: &SyntaxNode,
    old_edit_range: (usize, usize),
    new_edit_range: (usize, usize),
) -> IncrementalParseResult {
    let parser_config = config.map(|c| c.parser_options());
    panache_parser::parser::parse_incremental_suffix(
        input,
        parser_config,
        old_tree,
        old_edit_range,
        new_edit_range,
    )
}

/// Incremental reparse with a caller-supplied refdef set. See
/// [`panache_parser::parser::parse_incremental_suffix_with_refdefs`].
pub fn parse_incremental_suffix_with_refdefs(
    input: &str,
    config: Option<Config>,
    refdefs: RefdefMap,
    old_tree: &SyntaxNode,
    old_edit_range: (usize, usize),
    new_edit_range: (usize, usize),
) -> IncrementalParseResult {
    let parser_config = config.map(|c| c.parser_options());
    panache_parser::parser::parse_incremental_suffix_with_refdefs(
        input,
        parser_config,
        refdefs,
        old_tree,
        old_edit_range,
        new_edit_range,
    )
}
