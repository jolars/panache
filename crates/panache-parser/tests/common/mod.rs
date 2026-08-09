//! Shared adapter for the incremental-reparse test suites.
//!
//! `reparse` is refusal-first: it returns `None` and leaves the full parse to
//! the caller. These suites *are* that caller, and they all want the same
//! policy -- "splice if you can, otherwise full-parse" -- plus a label saying
//! which happened. That policy lives here rather than in the parser, so the
//! production entry point keeps exactly one meaning.

use panache_parser::ParserOptions;
use panache_parser::SyntaxNode;
use panache_parser::parser::{
    CostGuards, Edit, SyntaxError, parse_with_errors, reparse_with_cost_guards,
};

/// The outcome of a reparse attempt plus its full-parse fallback.
///
/// Each test binary compiles this module separately and reads a different
/// subset of the fields, so the whole struct opts out of dead-code warnings.
#[allow(dead_code)]
pub struct Spliced {
    pub tree: SyntaxNode,
    pub errors: Vec<SyntaxError>,
    pub reparse_range: (usize, usize),
    /// `"section_window"`, `"suffix_window"`, or `"full_reparse"` when every
    /// guard declined.
    pub strategy: &'static str,
}

fn full_parse(input: &str, options: Option<ParserOptions>) -> Spliced {
    let (tree, errors) = parse_with_errors(input, options);
    let len: usize = tree.text_range().end().into();
    Spliced {
        tree,
        errors,
        reparse_range: (0, len),
        strategy: "full_reparse",
    }
}

/// Reparse `input` against `old_tree`, falling back to a full parse when the
/// guard cascade declines.
///
/// `old_edit` is in the old text's coordinates and `new_edit` in `input`'s. A
/// pair that cannot be expressed as one [`Edit`] -- a new range that does not
/// start where the old one did, or one that does not fit `input` -- is a shape
/// the entry point rejects, so it full-parses.
#[allow(dead_code)]
pub fn reparse_or_full(
    input: &str,
    options: Option<ParserOptions>,
    old_tree: &SyntaxNode,
    old_errors: &[SyntaxError],
    old_edit: (usize, usize),
    new_edit: (usize, usize),
) -> Spliced {
    reparse_or_full_with_cost_guards(
        input,
        options,
        old_tree,
        old_errors,
        old_edit,
        new_edit,
        CostGuards::Enforced,
    )
}

/// [`reparse_or_full`], with the window-size cutoff under the caller's control.
///
/// Only the fuzz harness passes [`CostGuards::Ignored`], and only for its
/// hazard snippets --- see the type's own documentation for why a correctness
/// harness wants a *cost* guard out of its way.
#[allow(dead_code)]
pub fn reparse_or_full_with_cost_guards(
    input: &str,
    options: Option<ParserOptions>,
    old_tree: &SyntaxNode,
    old_errors: &[SyntaxError],
    old_edit: (usize, usize),
    new_edit: (usize, usize),
    cutoff: CostGuards,
) -> Spliced {
    let resolved = options.clone().unwrap_or_default();
    let Some(insert) = input
        .get(new_edit.0..new_edit.1)
        .filter(|_| new_edit.0 == old_edit.0)
    else {
        return full_parse(input, options);
    };
    let edit = Edit {
        range: old_edit.0..old_edit.1,
        insert: insert.to_string(),
    };
    match reparse_with_cost_guards(
        &old_tree.green().to_owned(),
        old_errors,
        &edit,
        input,
        &resolved,
        cutoff,
    ) {
        Some(reparsed) => Spliced {
            tree: SyntaxNode::new_root(reparsed.green),
            errors: reparsed.errors,
            reparse_range: reparsed.reparse_range,
            strategy: reparsed.strategy.as_str(),
        },
        None => full_parse(input, options),
    }
}
