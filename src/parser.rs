//! Parser module for Pandoc/Quarto documents.
//!
//! This module implements a single-pass parser that constructs a lossless syntax tree (CST) for
//! Quarto documents.

use crate::config::Config;
use crate::syntax::SyntaxNode;

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

/// Incrementally update a syntax tree by reparsing from a safe restart boundary
/// to EOF and rebuilding the document root from old prefix + reparsed suffix.
///
/// Current implementation is intentionally conservative: it is a facade over a
/// full-document parse while incremental design work is in progress.
pub fn parse_incremental_suffix(
    input: &str,
    config: Option<Config>,
    _old_tree: &SyntaxNode,
    _old_edit_range: (usize, usize),
    _new_edit_range: (usize, usize),
) -> IncrementalParseResult {
    let config = config.unwrap_or_default();
    let tree = Parser::new(input, &config).parse();
    let len: usize = tree.text_range().end().into();

    IncrementalParseResult {
        tree,
        reparse_range: (0, len),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply_edit(text: &str, old: (usize, usize), insert: &str) -> String {
        let mut out = String::with_capacity(text.len() - (old.1 - old.0) + insert.len());
        out.push_str(&text[..old.0]);
        out.push_str(insert);
        out.push_str(&text[old.1..]);
        out
    }

    #[test]
    fn incremental_suffix_matches_full_parse_for_tail_edit() {
        let input = "# H\n\npara one\n\npara two\n\npara three\n";
        let old_tree = parse(input, None);
        let old_edit = (30, 35);
        let updated = apply_edit(input, old_edit, "tail section");
        let new_edit = (30, 42);

        let inc = parse_incremental_suffix(&updated, None, &old_tree, old_edit, new_edit).tree;
        let full = parse(&updated, None);
        assert_eq!(inc.to_string(), full.to_string());
    }

    #[test]
    fn incremental_suffix_matches_full_parse_for_middle_edit() {
        let input = "# H\n\n- a\n- b\n\nfinal para\n";
        let old_tree = parse(input, None);
        let old_edit = (10, 11);
        let updated = apply_edit(input, old_edit, "alpha");
        let new_edit = (10, 15);

        let inc = parse_incremental_suffix(&updated, None, &old_tree, old_edit, new_edit).tree;
        let full = parse(&updated, None);
        assert_eq!(inc.to_string(), full.to_string());
    }
}
