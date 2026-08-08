//! The incremental-reparse entry point and its edit currency.
//!
//! [`reparse`] is a *hint*, not a parser: it either produces a spliced tree
//! that the governing invariant guarantees is byte-identical to a full parse of
//! the edited text, or it declines with [`None`] and leaves the full parse to
//! the caller. Every guard in the cascade is a decline, never an error and
//! never a best effort -- "when in doubt, return `None`" is the whole
//! correctness argument, and it is what lets the caller (salsa's
//! `parsed_document` on the host side) treat the reparse as a pure
//! optimization.
//!
//! [`Edit`] is the currency the caller speaks. Conversion from LSP `didChange`
//! content changes lives host-side; this crate only ever sees byte ranges.
//! [`diff_edit`] recovers a single contiguous edit from two whole texts, which
//! is how a caller with no edit information at all (a disk revert, a coalesced
//! `didChange` batch) still gets an incremental attempt.

use std::ops::Range;

use crate::options::ParserOptions;
use crate::parser::SyntaxError;
use crate::syntax::SyntaxNode;

/// A single contiguous text edit: replace `range` (a byte range in the *old*
/// text) with `insert`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub range: Range<usize>,
    pub insert: String,
}

impl Edit {
    /// The signed length change this edit applies to text after `range`.
    pub fn delta(&self) -> isize {
        self.insert.len() as isize - (self.range.end - self.range.start) as isize
    }

    /// Apply the edit to `old`, producing the new text.
    pub fn apply(&self, old: &str) -> String {
        let mut out =
            String::with_capacity(old.len().saturating_sub(self.range.len()) + self.insert.len());
        out.push_str(&old[..self.range.start]);
        out.push_str(&self.insert);
        out.push_str(&old[self.range.end..]);
        out
    }

    /// The edit's footprint in the *new* text: where `insert` landed.
    fn new_range(&self) -> (usize, usize) {
        (self.range.start, self.range.start + self.insert.len())
    }
}

/// Recover a single contiguous [`Edit`] from a pair of whole texts by stripping
/// the common prefix and suffix. Multiple disjoint edits collapse into one
/// spanning edit -- still a correct transform, just coarser (and one the guard
/// cascade is more likely to decline). Boundaries are clamped to char
/// boundaries of both texts.
pub fn diff_edit(old: &str, new: &str) -> Edit {
    let ob = old.as_bytes();
    let nb = new.as_bytes();

    let mut prefix = 0;
    let max_prefix = ob.len().min(nb.len());
    while prefix < max_prefix && ob[prefix] == nb[prefix] {
        prefix += 1;
    }
    while prefix > 0 && !old.is_char_boundary(prefix) {
        prefix -= 1;
    }

    let mut suffix = 0;
    let max_suffix = (ob.len() - prefix).min(nb.len() - prefix);
    while suffix < max_suffix && ob[ob.len() - 1 - suffix] == nb[nb.len() - 1 - suffix] {
        suffix += 1;
    }
    while suffix > 0
        && (!old.is_char_boundary(old.len() - suffix) || !new.is_char_boundary(new.len() - suffix))
    {
        suffix -= 1;
    }

    Edit {
        range: prefix..(old.len() - suffix),
        insert: new[prefix..(new.len() - suffix)].to_string(),
    }
}

/// Which window a successful reparse re-derived.
///
/// Both strategies parse their window to EOF -- list-item buffering depends on
/// unbounded lookahead, so a bounded standalone window parse is untrustworthy.
/// The section window differs in that it re-adopts the old suffix children when
/// they come back structurally equal, preserving their `Arc` identity; when
/// they don't, it degrades to the wholesale suffix splice and reports itself as
/// [`ReparseStrategy::SuffixWindow`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReparseStrategy {
    SectionWindow,
    SuffixWindow,
}

impl ReparseStrategy {
    /// The stable label used in logs and in the legacy
    /// [`IncrementalParseResult`](super::IncrementalParseResult) shape.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SectionWindow => "section_window",
            Self::SuffixWindow => "suffix_window",
        }
    }
}

/// A successful incremental reparse.
///
/// `errors` covers the **whole** document, not just the reparsed window: the
/// retained prefix's share is carried over from the caller-supplied previous
/// errors and the window's are shifted into document coordinates. The governing
/// invariant covers this vector as much as the tree.
pub struct Reparsed {
    pub green: rowan::GreenNode,
    pub errors: Vec<SyntaxError>,
    pub reparse_range: (usize, usize),
    pub strategy: ReparseStrategy,
}

/// Attempt an incremental reparse of `new_text` off the previous parse.
///
/// Returns [`None`] whenever any guard declines -- the caller must then full-
/// parse. A [`Some`] result is, by the governing invariant, byte-identical to
/// that full parse (tree and errors), which the debug oracle asserts on every
/// success.
///
/// `prev_errors` are the syntax errors of the parse that produced `prev_green`;
/// the retained prefix's share of them is carried into the result. Passing an
/// under-reported vector is a divergence the oracle will catch.
///
/// `options.refdef_labels` must already be populated for `new_text` -- this
/// entry point does not scan, because every caller that has a previous parse
/// also has a cached refdef set.
pub fn reparse(
    prev_green: &rowan::GreenNode,
    prev_errors: &[SyntaxError],
    edit: &Edit,
    new_text: &str,
    options: &ParserOptions,
) -> Option<Reparsed> {
    let prev_tree = SyntaxNode::new_root(prev_green.clone());
    super::reparse_ranges(
        new_text,
        options,
        &prev_tree,
        prev_errors,
        (edit.range.start, edit.range.end),
        edit.new_range(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(range: Range<usize>, insert: &str) -> Edit {
        Edit {
            range,
            insert: insert.to_string(),
        }
    }

    #[test]
    fn diff_edit_recovers_a_noop() {
        assert_eq!(diff_edit("# Title\n", "# Title\n"), edit(8..8, ""));
    }

    #[test]
    fn diff_edit_recovers_an_insertion() {
        let e = diff_edit("# Title\n", "# Titles\n");
        assert_eq!(e, edit(7..7, "s"));
        assert_eq!(e.apply("# Title\n"), "# Titles\n");
        assert_eq!(e.delta(), 1);
        assert_eq!(e.new_range(), (7, 8));
    }

    #[test]
    fn diff_edit_recovers_a_deletion() {
        let e = diff_edit("# Titles\n", "# Title\n");
        assert_eq!(e, edit(7..8, ""));
        assert_eq!(e.apply("# Titles\n"), "# Title\n");
        assert_eq!(e.delta(), -1);
        assert_eq!(e.new_range(), (7, 7));
    }

    #[test]
    fn diff_edit_recovers_a_replacement() {
        let e = diff_edit("a *b* c\n", "a *z* c\n");
        assert_eq!(e, edit(3..4, "z"));
        assert_eq!(e.apply("a *b* c\n"), "a *z* c\n");
        assert_eq!(e.delta(), 0);
    }

    #[test]
    fn diff_edit_collapses_disjoint_edits_into_one_span() {
        // Two edits (a -> z at 0, c -> z at 4) collapse into one spanning edit.
        let e = diff_edit("a b c\n", "z b z\n");
        assert_eq!(e, edit(0..5, "z b z"));
        assert_eq!(e.apply("a b c\n"), "z b z\n");
    }

    #[test]
    fn diff_edit_handles_whole_replacement_and_empty_texts() {
        assert_eq!(diff_edit("", "# Title\n"), edit(0..0, "# Title\n"));
        assert_eq!(diff_edit("# Title\n", ""), edit(0..8, ""));
        assert_eq!(diff_edit("abc", "xyz"), edit(0..3, "xyz"));
    }

    #[test]
    fn diff_edit_clamps_to_char_boundaries() {
        // A (0xCE 0xB1) and B (0xCE 0xB2) share their lead byte; a naive byte
        // prefix would split the code point.
        let e = diff_edit("\u{3B1} text\n", "\u{3B2} text\n");
        assert_eq!(e, edit(0..2, "\u{3B2}"));
        assert_eq!(e.apply("\u{3B1} text\n"), "\u{3B2} text\n");

        // Shared trail bytes mid-character.
        let old = "x\u{3AC}";
        let new = "x\u{1FB6}";
        let e = diff_edit(old, new);
        assert!(old.is_char_boundary(e.range.start));
        assert!(old.is_char_boundary(e.range.end));
        assert_eq!(e.apply(old), new);
    }

    #[test]
    fn reparse_splices_a_paragraph_edit() {
        let old_text = "# One\n\nAlpha.\n\n# Two\n\nBeta.\n";
        let new_text = "# One\n\nAlpha.\n\n# Two\n\nBetas.\n";
        let options = ParserOptions::default();
        let (old_tree, old_errors) =
            super::super::Parser::new(old_text, &options).parse_with_errors();

        let edit = diff_edit(old_text, new_text);
        let result = reparse(
            &old_tree.green().to_owned(),
            &old_errors,
            &edit,
            new_text,
            &options,
        )
        .expect("a plain paragraph edit must splice");

        assert_eq!(
            crate::parser::fingerprint(&SyntaxNode::new_root(result.green)),
            crate::parser::fingerprint(&super::super::parse(new_text, Some(options))),
        );
    }

    #[test]
    fn reparse_declines_an_edit_past_the_previous_tree() {
        let old_text = "Alpha.\n";
        let options = ParserOptions::default();
        let (old_tree, old_errors) =
            super::super::Parser::new(old_text, &options).parse_with_errors();

        // A range beyond the old text: the caller's tree and text disagree.
        let stale = Edit {
            range: 50..60,
            insert: String::new(),
        };
        assert!(
            reparse(
                &old_tree.green().to_owned(),
                &old_errors,
                &stale,
                "Alpha.\n",
                &options,
            )
            .is_none()
        );
    }
}
