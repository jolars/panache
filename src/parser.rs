//! Compatibility wrapper around the internal panache-parser crate.

pub use panache_parser::parser::Parser;
pub use panache_parser::parser::blocks;
pub use panache_parser::parser::inlines;
pub use panache_parser::parser::utils;
pub use panache_parser::parser::yaml;
pub use panache_parser::parser::{Diagnostics, SyntaxError, SyntaxErrorSource};
pub use panache_parser::parser::{Edit, ReparseStrategy, Reparsed, diff_edit};
pub use panache_parser::{RefdefMap, collect_refdef_labels};
pub use panache_parser::{to_pandoc_ast, to_pandoc_json};

use crate::config::Config;
use crate::syntax::SyntaxNode;

pub fn parse(input: &str, config: Option<Config>) -> SyntaxNode {
    let parser_config = config.map(|c| c.parser_options());
    panache_parser::parser::parse(input, parser_config)
}

/// Parse, also returning the embedded-sublanguage syntax errors (host-ranged
/// malformed YAML). See [`panache_parser::parser::parse_with_errors`].
pub fn parse_with_errors(input: &str, config: Option<Config>) -> (SyntaxNode, Vec<SyntaxError>) {
    let parser_config = config.map(|c| c.parser_options());
    panache_parser::parser::parse_with_errors(input, parser_config)
}

/// Parse with a caller-supplied refdef set, skipping the
/// `collect_refdef_labels` scan. See
/// [`panache_parser::parse_with_refdefs`].
pub fn parse_with_refdefs(input: &str, config: Option<Config>, refdefs: RefdefMap) -> SyntaxNode {
    let parser_config = config.map(|c| c.parser_options());
    panache_parser::parser::parse_with_refdefs(input, parser_config, refdefs)
}

/// Parse with a caller-supplied refdef set, also returning the
/// embedded-sublanguage syntax errors (host-ranged malformed YAML). See
/// [`panache_parser::parser::parse_with_refdefs_and_errors`].
pub fn parse_with_refdefs_and_errors(
    input: &str,
    config: Option<Config>,
    refdefs: RefdefMap,
) -> (SyntaxNode, Vec<SyntaxError>) {
    let parser_config = config.map(|c| c.parser_options());
    panache_parser::parser::parse_with_refdefs_and_errors(input, parser_config, refdefs)
}

/// Attempt an incremental reparse against a previous parse, with a
/// caller-supplied refdef set. `None` means "no reuse -- full-parse instead";
/// see [`panache_parser::parser::reparse`].
pub fn reparse_with_refdefs(
    prev_green: &rowan::GreenNode,
    prev_errors: &[SyntaxError],
    edit: &Edit,
    new_text: &str,
    config: Option<Config>,
    refdefs: RefdefMap,
) -> Option<Reparsed> {
    let mut options = config.map(|c| c.parser_options()).unwrap_or_default();
    options.refdef_labels = Some(refdefs);
    panache_parser::parser::reparse(prev_green, prev_errors, edit, new_text, &options)
}
