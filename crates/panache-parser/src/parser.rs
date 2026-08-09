//! Parser module for Pandoc/Quarto documents.
//!
//! This module implements a single-pass parser that constructs a lossless syntax tree (CST) for
//! Quarto documents.

use crate::options::ParserOptions;
use crate::parser::inlines::refdef_map::{RefdefMap, collect_refdef_labels};
use crate::syntax::SyntaxNode;

pub mod blocks;
pub mod diagnostics;
pub mod inlines;
pub mod math;
pub mod reparse;
pub mod utils;
pub mod yaml;

mod block_dispatcher;
mod core;
mod verify;

// Re-export main parser
pub use core::Parser;
pub use diagnostics::{Diagnostics, SyntaxError, SyntaxErrorSource};
pub use reparse::{
    CostGuards, Edit, ReparseStrategy, Reparsed, diff_edit, reparse, reparse_with_cost_guards,
};
pub use verify::fingerprint;

/// Parses a Quarto document string into a syntax tree.
///
/// Single-pass architecture: blocks emit inline structure during parsing.
///
/// Convenience wrapper that scans the input for reference-definition
/// labels via [`collect_refdef_labels`] before parsing. Callers that
/// already have a precomputed [`RefdefMap`] (e.g. salsa-cached) should
/// use [`parse_with_refdefs`] instead to skip the scan.
///
/// # Examples
///
/// ```rust
/// use panache_parser::parser::parse;
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
pub fn parse(input: &str, config: Option<ParserOptions>) -> SyntaxNode {
    parse_with_errors(input, config).0
}

/// Like [`parse`], but also returns the syntax errors the parser found in
/// embedded sublanguages (currently malformed frontmatter / hashpipe YAML).
///
/// The errors carry host-aligned ranges, so consumers (the linter) can turn
/// them straight into diagnostics without re-parsing the region or remapping
/// offsets. For pure Markdown the error list is empty.
pub fn parse_with_errors(
    input: &str,
    config: Option<ParserOptions>,
) -> (SyntaxNode, Vec<SyntaxError>) {
    let mut config = config.unwrap_or_default();
    populate_refdef_labels(input, &mut config);
    Parser::new(input, &config).parse_with_errors()
}

/// Parse with a caller-supplied refdef set.
///
/// Skips the [`collect_refdef_labels`] scan that [`parse`] performs.
/// Use this when the caller already has a cached [`RefdefMap`] for
/// `input` — e.g. from a salsa-tracked query — to avoid a redundant
/// document-level scan on every parse.
///
/// The supplied `refdefs` becomes the parser's refdef set, overriding
/// any value previously set on `options.refdef_labels`.
pub fn parse_with_refdefs(
    input: &str,
    options: Option<ParserOptions>,
    refdefs: RefdefMap,
) -> SyntaxNode {
    parse_with_refdefs_and_errors(input, options, refdefs).0
}

/// Like [`parse_with_refdefs`], but also returns embedded-sublanguage syntax
/// errors (see [`parse_with_errors`]). Used by the salsa parse query, which
/// caches the tree and the errors together from a single parse.
pub fn parse_with_refdefs_and_errors(
    input: &str,
    options: Option<ParserOptions>,
    refdefs: RefdefMap,
) -> (SyntaxNode, Vec<SyntaxError>) {
    let mut options = options.unwrap_or_default();
    options.refdef_labels = Some(refdefs);
    Parser::new(input, &options).parse_with_errors()
}

/// Pre-compute the document-level reference link label set.
///
/// CommonMark §6.3 makes reference link resolution depend on whether
/// the label matches a definition that may appear anywhere in the
/// document (including after the use site). The IR-based bracket
/// resolution pass in `inlines::inline_ir` consults this set to
/// distinguish a real shortcut/reference link from bracket-shaped
/// literal text.
///
/// Pandoc-markdown agrees on the document-scoped lookup rule: a
/// `[foo][bar]` shape with no `[bar]: ...` definition is literal text.
/// Both dialects populate this set so the dispatcher's reference-link
/// branch (under Pandoc) and the IR's `process_brackets` pass (under
/// CommonMark) can consult it uniformly.
///
/// Only populated when the caller hasn't already supplied one.
fn populate_refdef_labels(input: &str, config: &mut ParserOptions) {
    if config.refdef_labels.is_some() {
        return;
    }
    config.refdef_labels = Some(collect_refdef_labels(input, config.dialect));
}
