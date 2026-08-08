//! Incremental-reparse verification helpers.
//!
//! [`fingerprint`] renders every element of a tree as a `kind@range "text"`
//! line; two trees with equal fingerprints are structurally identical, token
//! boundaries included. It is `pub` so the in-crate debug oracle
//! ([`assert_matches_full_parse`]) and external test harnesses share one
//! definition of "identical" and can never diverge on it.
//!
//! The oracle enforces the governing incremental-parsing invariant: a
//! successful incremental reparse must be byte-for-byte structurally
//! identical to a full parse of the edited text. It runs on every
//! non-fallback reparse in debug builds and is compiled out of release
//! builds.

use std::fmt::Write;

use crate::syntax::SyntaxNode;

/// Render every element (nodes and tokens) of `node` as one line of
/// `kind@range "text"`, in preorder.
///
/// Structural equality of two trees is equality of their fingerprints:
/// every kind, every range, and every token's text participates.
pub fn fingerprint(node: &SyntaxNode) -> String {
    let mut out = String::new();
    for element in node.descendants_with_tokens() {
        let text = element
            .as_token()
            .map(|token| token.text().to_string())
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "{:?}@{:?} {:?}",
            element.kind(),
            element.text_range(),
            text
        );
    }
    out
}

/// Debug-build oracle: a successful incremental reparse must equal a full
/// parse of the same input under the same options -- tree *and* syntax
/// errors.
///
/// Panics (debug builds only) when the incrementally spliced tree or its
/// error vector diverges from a from-scratch parse. Every failure here is an
/// incremental-parser bug whose fix is a new bail-to-full-parse condition,
/// never a relaxation of this assert.
#[cfg(debug_assertions)]
pub(crate) fn assert_matches_full_parse(
    result: &super::Reparsed,
    input: &str,
    options: &crate::options::ParserOptions,
) {
    let (full, full_errors) = super::Parser::new(input, options).parse_with_errors();
    assert_eq!(
        fingerprint(&SyntaxNode::new_root(result.green.clone())),
        fingerprint(&full),
        "incremental reparse (strategy {:?}, reparse_range {:?}) diverged from full parse",
        result.strategy,
        result.reparse_range,
    );
    assert_eq!(
        result.errors, full_errors,
        "incremental reparse (strategy {:?}, reparse_range {:?}) diverged from full parse \
         on syntax errors",
        result.strategy, result.reparse_range,
    );
}

#[cfg(not(debug_assertions))]
#[inline]
pub(crate) fn assert_matches_full_parse(
    _result: &super::Reparsed,
    _input: &str,
    _options: &crate::options::ParserOptions,
) {
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{ReparseStrategy, Reparsed, parse};

    #[test]
    fn fingerprint_distinguishes_structural_difference() {
        let paragraph = parse("plain text\n", None);
        let heading = parse("# plain text\n", None);
        assert_ne!(fingerprint(&paragraph), fingerprint(&heading));
    }

    #[test]
    fn fingerprint_equal_for_identical_parses() {
        let a = parse("# Title\n\nBody with `code`.\n", None);
        let b = parse("# Title\n\nBody with `code`.\n", None);
        assert_eq!(fingerprint(&a), fingerprint(&b));
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "diverged from full parse")]
    fn oracle_panics_on_injected_divergence() {
        // A tree parsed from different text stands in for a bad splice.
        let input = "# Title\n\nParagraph.\n";
        let wrong = Reparsed {
            green: parse("# Title\n\n> quoted\n", None).green().to_owned(),
            errors: Vec::new(),
            reparse_range: (0, input.len()),
            strategy: ReparseStrategy::SuffixWindow,
        };
        assert_matches_full_parse(&wrong, input, &crate::options::ParserOptions::default());
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "diverged from full parse on syntax errors")]
    fn oracle_panics_when_only_the_error_vector_diverges() {
        // Identical tree, dropped error: the shape of a splice that reuses a
        // prefix without carrying the prefix's errors.
        let input = "---\ntitle: [\n---\n\nParagraph.\n";
        let options = crate::options::ParserOptions::default();
        let (tree, errors) = crate::parser::Parser::new(input, &options).parse_with_errors();
        assert_eq!(errors.len(), 1, "fixture must have exactly one error");
        let wrong = Reparsed {
            green: tree.green().to_owned(),
            errors: Vec::new(),
            reparse_range: (0, input.len()),
            strategy: ReparseStrategy::SuffixWindow,
        };
        assert_matches_full_parse(&wrong, input, &options);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn oracle_accepts_identical_tree() {
        let input = "# Title\n\nParagraph.\n";
        let options = crate::options::ParserOptions::default();
        let result = Reparsed {
            green: crate::parser::Parser::new(input, &options)
                .parse()
                .green()
                .to_owned(),
            errors: Vec::new(),
            reparse_range: (0, input.len()),
            strategy: ReparseStrategy::SuffixWindow,
        };
        assert_matches_full_parse(&result, input, &options);
    }
}
