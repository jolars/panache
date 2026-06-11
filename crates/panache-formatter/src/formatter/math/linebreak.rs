//! Semantic line-breaking for over-width display **free rows** (`$$…$$`
//! non-environment content).
//!
//! A logical free row wider than the target `line_width` is broken at its
//! top-level operators, with a two-level hierarchy mirroring the amsmath
//! convention: **relations** (`=`, `\leq`, `\to`, …) first, then the **binary**
//! operators (`+`, `\cdot`, …) inside each over-width relation segment, indented
//! one level deeper so they nest under the relation's right-hand side:
//!
//! ```text
//! A = aaaaaaaaaa
//!     + bbbbbbbbbb
//!   = cccccccccc
//!     + dddddddddd
//! ```
//!
//! ## What "top-level" means, and why groups are opaque
//!
//! Breaks are only ever offered at operators sitting at **delimiter depth 0**:
//! an operator inside `(…)`, `[…]`, or a `\left…\right` pair is not a candidate
//! (we track an open/close depth counter, since — unlike `{…}` brace groups —
//! those delimiter pairs are *flat token runs* in the CST, not nesting nodes).
//! Brace groups (`{…}`, and therefore `\frac{…}{…}` arguments) are whole nodes
//! we never descend into for break points. This is a *layout policy*, not a hard
//! constraint (math ignores whitespace, so one could break inside `{…}`), chosen
//! because (a) some interiors are whitespace/newline-sensitive (`\text{…}`,
//! trailing `%` comments), (b) one breaks at the outermost structure by
//! convention, and (c) it keeps the walk and its idempotency simple. The
//! consequence we accept: a sub-unit wider than the line with no top-level
//! operator stays over-width, like an unbreakable long word in prose reflow.
//!
//! ## Unary coercion
//!
//! Break candidates are *spaced* operators (`operators::is_spaced` after
//! `operators::coerce`): a unary `+`/`-` (`-x`, `e^{-t}`) is `Ord` and never a
//! break site. A relation continuation starts with a relation, which never
//! coerces, so it re-spaces correctly rendered in isolation; a binary
//! continuation starts with a binary operator, which *would* coerce to unary in
//! isolation, so it is rendered with a seeded closing-operand class
//! (`render_inline_seeded`) to stay binary.
//!
//! ## Scope (current)
//!
//! Every over-width free row with a top-level relation **or** binary operator is
//! broken. A relation chain (≥ 2 relations) breaks at its relations first, then
//! at the binary operators inside each over-width segment. A single-relation row
//! breaks its over-width binary RHS, each `+ term` nested under the RHS. A
//! standalone binary chain (no relation) breaks with the first term as the head
//! and each `+ term` flush under it. The unifying rule across all three: a binary
//! continuation aligns under the first term of its operand sequence. A row with
//! no top-level relation or binary operator (e.g. a lone wide `\frac{…}{…}`)
//! stays on one line, like an unbreakable long word in prose reflow. Inline and
//! environment-body math are not line-broken.
//!
//! ## Idempotency
//!
//! Indents are derived from the *logical* row (recomputed every pass), never
//! measured from the source. On a second pass the breaker's own soft newlines
//! and alignment spaces have collapsed back into the single logical row (see
//! [`super::render`]'s `split_logical_rows`), so the identical break points and
//! indents are reproduced and the output is a fixed point.

use super::operators::{self, AtomClass};
use super::render;
use crate::syntax::{SyntaxElement, SyntaxKind};

/// One indent step for a nested binary continuation, relative to its relation.
const BINARY_NEST: usize = 2;

/// A top-level (depth-0), *spaced* operator break candidate.
struct Break {
    /// Element index of the atom's first token (where a break lands before it).
    index: usize,
    /// The atom's coerced class — only `Bin`/`Rel` ever reach here.
    class: AtomClass,
}

/// Break one logical free-display row into physical content lines (no base
/// math-indent, no trailing `\\` — the caller adds those). A row that fits, or
/// that has no usable relation chain, is returned on one line unchanged.
pub(super) fn break_free_row(elems: &[SyntaxElement], line_width: usize) -> Vec<String> {
    // The unbroken, canonical single-line form — also the exact bytes the old
    // code emitted, so a row that fits is byte-identical to before.
    let single = render::render_inline(elems).trim().to_string();
    if single.chars().count() <= line_width {
        return vec![single];
    }

    let breaks = spaced_operator_breaks(elems);
    let rels: Vec<usize> = breaks
        .iter()
        .filter(|b| b.class == AtomClass::Rel)
        .map(|b| b.index)
        .collect();

    // Zero relations: a standalone binary chain. With no relation, the chain's
    // first term *is* the head, so each `+ term` aligns flush under it (bin_indent
    // 0) — the same rule the relation cases follow (a binary continuation aligns
    // under the first term of its right-hand side). A row with no top-level binary
    // op either (e.g. a lone wide `\frac`) falls through `break_binary_segment`'s
    // empty-`bins` guard and stays on one (over-width) line.
    if rels.is_empty() {
        return break_binary_segment(elems, 0, 0, line_width);
    }

    // Relation continuations align under the first relation's column; binary
    // continuations nest one step deeper, under the relation's right-hand side.
    let prefix_width = render::render_inline(&elems[..rels[0]])
        .trim()
        .chars()
        .count();
    let rel_indent = if prefix_width == 0 {
        0
    } else {
        prefix_width + 1
    };
    let bin_indent = rel_indent + BINARY_NEST;

    // One relation: the whole row is a single segment. It keeps the lone relation
    // on the opening line; an over-width binary RHS breaks before each `+`, each
    // term nested at `bin_indent` (under the RHS for a single-char LHS).
    if rels.len() == 1 {
        return break_binary_segment(elems, 0, bin_indent, line_width);
    }

    // ≥ 2 relations: a relation chain. The first relation stays on the opening
    // line; every later one starts a continuation aligned under the first
    // relation's column. Segment boundaries: [0, rels[1], rels[2], …, len].
    let bounds: Vec<usize> = std::iter::once(0)
        .chain(rels[1..].iter().copied())
        .chain(std::iter::once(elems.len()))
        .collect();

    let mut out: Vec<String> = Vec::new();
    for w in 0..bounds.len() - 1 {
        let seg = &elems[bounds[w]..bounds[w + 1]];
        let seg_indent = if w == 0 { 0 } else { rel_indent };
        out.extend(break_binary_segment(
            seg, seg_indent, bin_indent, line_width,
        ));
    }
    out
}

/// Lay out one relation segment: keep it on a single line at `base_indent` if it
/// fits, otherwise split it before each top-level binary operator, putting each
/// `+ term` on its own line at `bin_indent`.
fn break_binary_segment(
    seg: &[SyntaxElement],
    base_indent: usize,
    bin_indent: usize,
    line_width: usize,
) -> Vec<String> {
    let single = render::render_inline(seg).trim().to_string();
    let base_pad = " ".repeat(base_indent);
    if base_indent + single.chars().count() <= line_width {
        return vec![format!("{base_pad}{single}")];
    }

    let bins: Vec<usize> = spaced_operator_breaks(seg)
        .iter()
        .filter(|b| b.class == AtomClass::Bin)
        .map(|b| b.index)
        .collect();
    if bins.is_empty() {
        // Nothing to break against — leave it (over-width) on one line.
        return vec![format!("{base_pad}{single}")];
    }

    let bin_pad = " ".repeat(bin_indent);
    let mut out: Vec<String> = Vec::new();
    // Head: everything before the first binary operator (keeps the leading
    // relation, e.g. `A = aaaa` or a continuation's `= cccc`).
    let head = render::render_inline(&seg[..bins[0]]).trim().to_string();
    if !head.is_empty() {
        out.push(format!("{base_pad}{head}"));
    }
    for w in 0..bins.len() {
        let start = bins[w];
        let end = bins.get(w + 1).copied().unwrap_or(seg.len());
        // Seed a closing-operand class so the leading binary operator stays
        // binary (`+ term`) instead of coercing to a unary sign in isolation.
        let cont = render::render_inline_seeded(&seg[start..end], Some(AtomClass::Close))
            .trim()
            .to_string();
        out.push(format!("{bin_pad}{cont}"));
    }
    out
}

/// Top-level (depth-0) **spaced** operator break candidates, in document order,
/// each with its coerced [`AtomClass`]. Mirrors the class/coercion bookkeeping of
/// [`super::render`]'s spacing pass at the top-level element granularity, while
/// tracking an open/close delimiter depth so only depth-0 operators qualify and
/// excluding unary (coerced-to-`Ord`) signs. Brace groups and environments are
/// opaque operand nodes — their interior operators never appear here.
fn spaced_operator_breaks(elems: &[SyntaxElement]) -> Vec<Break> {
    let mut out: Vec<Break> = Vec::new();
    let mut depth: i32 = 0;
    let mut prev: Option<AtomClass> = None;
    let mut i = 0;
    while i < elems.len() {
        let el = &elems[i];
        match el.kind() {
            SyntaxKind::MATH_OPEN => {
                depth += 1;
                prev = Some(AtomClass::Open);
                i += 1;
            }
            SyntaxKind::MATH_CLOSE => {
                depth -= 1;
                prev = Some(AtomClass::Close);
                i += 1;
            }
            SyntaxKind::MATH_PUNCT => {
                prev = Some(AtomClass::Punct);
                i += 1;
            }
            SyntaxKind::MATH_TEXT => {
                prev = Some(AtomClass::Ord);
                i += 1;
            }
            // `^`/`_` bind tightly and `&` opens a cell — all unary-inducing.
            SyntaxKind::MATH_SCRIPT | SyntaxKind::MATH_ALIGN => {
                prev = Some(AtomClass::Open);
                i += 1;
            }
            // Operand nodes (a `{…}` group or a nested environment).
            SyntaxKind::MATH_GROUP | SyntaxKind::MATH_ENVIRONMENT => {
                prev = Some(AtomClass::Close);
                i += 1;
            }
            // Whitespace and comments are transparent to atom class.
            SyntaxKind::MATH_SPACE | SyntaxKind::MATH_NEWLINE | SyntaxKind::MATH_COMMENT => {
                i += 1;
            }
            // Defensive: a hard break would reset context (shouldn't occur in a
            // logical row, which is split on `\\`).
            SyntaxKind::MATH_LINE_BREAK => {
                prev = None;
                i += 1;
            }
            SyntaxKind::MATH_COMMAND => {
                let text = el
                    .as_token()
                    .map(|t| t.text().to_string())
                    .unwrap_or_default();
                let name = text.strip_prefix('\\').unwrap_or(&text);
                if name == "left" {
                    depth += 1;
                    prev = Some(AtomClass::Open);
                } else if name == "right" {
                    depth -= 1;
                    prev = Some(AtomClass::Close);
                } else if let Some(raw) = operators::command_class(name) {
                    let class = operators::coerce(raw, prev);
                    if depth == 0 && operators::is_spaced(class) {
                        out.push(Break { index: i, class });
                    }
                    prev = Some(class);
                } else {
                    prev = Some(AtomClass::Ord);
                }
                i += 1;
            }
            // A maximal run of adjacent operator tokens (one char each) splits
            // into atoms; each *spaced* atom at depth 0 is a break candidate at
            // its first token.
            SyntaxKind::MATH_OPERATOR => {
                let run_start = i;
                let mut run = String::new();
                while i < elems.len() && elems[i].kind() == SyntaxKind::MATH_OPERATOR {
                    if let Some(tok) = elems[i].as_token() {
                        run.push_str(tok.text());
                    }
                    i += 1;
                }
                let mut char_off = 0usize;
                for atom in operators::split_operator_atoms(&run) {
                    let class = operators::coerce(operators::classify_operator(atom), prev);
                    if depth == 0 && operators::is_spaced(class) {
                        out.push(Break {
                            index: run_start + char_off,
                            class,
                        });
                    }
                    prev = Some(class);
                    // Each operator char is exactly one token.
                    char_off += atom.chars().count();
                }
            }
            // Anything else (e.g. an equation label) is an ordinary operand.
            _ => {
                prev = Some(AtomClass::Ord);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::SyntaxNode;
    use panache_parser::parser::math::{MathParseOptions, parse_math_content};

    /// Top-level elements of a parsed math content string.
    fn elems(content: &str) -> Vec<SyntaxElement> {
        let node = SyntaxNode::new_root(parse_math_content(content, MathParseOptions::default()));
        node.children_with_tokens().collect()
    }

    fn lines(content: &str, width: usize) -> Vec<String> {
        break_free_row(&elems(content), width)
    }

    fn rel_indices(content: &str) -> Vec<usize> {
        spaced_operator_breaks(&elems(content))
            .iter()
            .filter(|b| b.class == AtomClass::Rel)
            .map(|b| b.index)
            .collect()
    }

    #[test]
    fn short_row_stays_one_line() {
        assert_eq!(lines("a = b = c", 80), vec!["a = b = c"]);
    }

    #[test]
    fn overwidth_relation_chain_breaks_and_aligns() {
        // Wide enough that each relation segment fits ⇒ relations only.
        assert_eq!(
            lines("A = bbbbbbbbbb = cccccccccc", 20),
            vec!["A = bbbbbbbbbb", "  = cccccccccc"],
        );
    }

    #[test]
    fn alignment_tracks_the_first_relation_column() {
        // Prefix `\alpha + \beta` is 14 chars wide ⇒ the `=` sits at column 15.
        let out = lines("\\alpha + \\beta = gggggggggg = dddddddddd", 30);
        assert_eq!(out[0], "\\alpha + \\beta = gggggggggg");
        assert_eq!(out[1], "               = dddddddddd");
    }

    #[test]
    fn overwidth_segments_nest_binary_operators() {
        // Each relation segment is itself too wide, so its `+` term breaks one
        // level deeper (under the relation's right-hand side).
        assert_eq!(
            lines("A = aaaaaaaaaa + bbbbbbbbbb = cccccccccc + dddddddddd", 20),
            vec![
                "A = aaaaaaaaaa",
                "    + bbbbbbbbbb",
                "  = cccccccccc",
                "    + dddddddddd",
            ],
        );
    }

    #[test]
    fn command_relations_are_break_points() {
        assert_eq!(
            lines("aaaaaaaa \\to bbbbbbbb \\to cccccccc", 20),
            vec!["aaaaaaaa \\to bbbbbbbb", "         \\to cccccccc"],
        );
    }

    #[test]
    fn relations_inside_parens_are_not_break_points() {
        // The `=` lives inside `(…)`, so there is no depth-0 chain to break.
        let content = "ffffff(xxxxxxxx = yyyyyyyy) zzzzzzzz";
        assert_eq!(rel_indices(content), Vec::<usize>::new());
        assert_eq!(lines(content, 10).len(), 1);
    }

    #[test]
    fn relations_inside_left_right_are_not_break_points() {
        let content = "ffff \\left( xxxx = yyyy \\right) gggg";
        assert_eq!(rel_indices(content), Vec::<usize>::new());
    }

    #[test]
    fn single_relation_does_not_break() {
        let content = "A = bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        assert_eq!(lines(content, 10).len(), 1);
    }

    #[test]
    fn relations_inside_braces_are_opaque() {
        // The `=` is inside a `\frac` argument group (a node we never descend).
        let content = "\\frac{aaaa = bbbb}{cccc} dddd eeee";
        assert_eq!(rel_indices(content), Vec::<usize>::new());
    }

    #[test]
    fn unary_sign_is_not_a_binary_break_point() {
        // `= -ttttt…`: the `-` is unary (after a relation), so the segment has no
        // binary break candidate and stays on one (over-width) line.
        let out = lines("A = -tttttttttt = -uuuuuuuuuu", 12);
        assert_eq!(out, vec!["A = -tttttttttt", "  = -uuuuuuuuuu"]);
    }

    #[test]
    fn single_relation_breaks_binary_terms() {
        // One relation, over-width RHS binary chain: each `+ term` nests under
        // the RHS (`bin_indent = rel_indent + 2 = 4` for the single-char LHS).
        assert_eq!(
            lines("A = aaaaaaaaaa + bbbbbbbbbb + cccccccccc", 20),
            vec!["A = aaaaaaaaaa", "    + bbbbbbbbbb", "    + cccccccccc"],
        );
    }

    #[test]
    fn zero_relation_binary_chain_breaks_flush() {
        // No relation: the first term is the head, each `+ term` flush under it
        // (the same rule the relation cases follow — align under the first term).
        assert_eq!(
            lines("aaaa + bbbb + cccc + dddd", 12),
            vec!["aaaa", "+ bbbb", "+ cccc", "+ dddd"],
        );
    }

    #[test]
    fn zero_relation_no_binary_stays_one_line() {
        // No relation and no top-level binary (the ops live inside the `\frac`
        // group, opaque) ⇒ the over-width row is left untouched.
        assert_eq!(lines("\\frac{aaaaaaaa}{bbbbbbbb}", 12).len(), 1);
    }

    #[test]
    fn zero_relation_leading_unary_sign_is_head() {
        // A leading `-` coerces to unary (list start), so it joins the head
        // rather than starting a break.
        assert_eq!(
            lines("-aaaaaaaa + bbbbbbbb + cccccccc", 12),
            vec!["-aaaaaaaa", "+ bbbbbbbb", "+ cccccccc"],
        );
    }
}
