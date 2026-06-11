//! Semantic line-breaking for over-width display **free rows** (`$$…$$`
//! non-environment content).
//!
//! A logical free row wider than the target `line_width` is split at its
//! highest-priority *top-level* operators — relations first (`a = b = c`),
//! mirroring the amsmath convention of breaking a chain at its relations. The
//! continuation lines align under the first relation, so the relations stack:
//!
//! ```text
//! A = lots of math lots of math
//!   = other math other math
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
//! consequence we accept: a single sub-unit wider than the line with no
//! top-level operator stays over-width, exactly like an unbreakable long word in
//! prose reflow.
//!
//! ## Scope (first cut)
//!
//! Relations only. A row with no top-level relations (or fewer than two) is left
//! on one line even when over-width; breaking such rows at *binary* operators is
//! deferred — rendering a continuation that starts with a binary operator in
//! isolation would mis-coerce it to a unary sign, which relations (never
//! coerced) sidestep.
//!
//! ## Idempotency
//!
//! The continuation indent is derived from the *logical* row (recomputed every
//! pass), never measured from the source. On a second pass the breaker's own
//! soft newlines and alignment spaces have collapsed back into the single
//! logical row (see [`super::render`]'s `split_logical_rows`), so the identical
//! break points and alignment column are recomputed and the output is a fixed
//! point.

use super::operators::{self, AtomClass};
use super::render;
use crate::syntax::{SyntaxElement, SyntaxKind};

/// Break one logical free-display row into physical content lines (no base
/// indent, no trailing `\\` — the caller adds those). Returns a single line
/// unchanged when the row fits `line_width` or has no usable break points;
/// otherwise the first line carries the leading content up to (and including)
/// the first relation, and each continuation line begins with a relation,
/// indented to align under that first one.
pub(super) fn break_free_row(elems: &[SyntaxElement], line_width: usize) -> Vec<String> {
    // The unbroken, canonical single-line form — also the exact bytes the old
    // code emitted, so a row that fits is byte-identical to before.
    let single = render::render_inline(elems).trim().to_string();
    if single.chars().count() <= line_width {
        return vec![single];
    }

    // Need at least two top-level relations to form a chain worth breaking: the
    // first stays on the opening line, every later one starts a continuation.
    let rels = relation_break_indices(elems);
    if rels.len() < 2 {
        return vec![single];
    }

    // Align continuations under the column of the first relation. A spaced
    // relation after a non-empty prefix has exactly one separating space; a
    // relation at the very start sits at column 0.
    let prefix_width = render::render_inline(&elems[..rels[0]])
        .trim()
        .chars()
        .count();
    let align_col = if prefix_width == 0 {
        0
    } else {
        prefix_width + 1
    };
    let pad = " ".repeat(align_col);

    let mut lines = Vec::with_capacity(rels.len());
    // Opening line: everything up to the second relation (so it keeps the first).
    lines.push(render::render_inline(&elems[..rels[1]]).trim().to_string());
    for w in 1..rels.len() {
        let start = rels[w];
        let end = rels.get(w + 1).copied().unwrap_or(elems.len());
        // A segment starting with a relation re-spaces correctly in isolation:
        // relations never coerce, so they stay spaced regardless of left context.
        let seg = render::render_inline(&elems[start..end]);
        lines.push(format!("{pad}{}", seg.trim()));
    }
    lines
}

/// Element indices of the **depth-0 relation atoms** in document order — the
/// break candidates. Tracks an open/close delimiter depth so operators inside
/// `(…)`/`[…]`/`\left…\right` are excluded; brace groups are whole nodes and
/// never expose their interior operators here at all.
fn relation_break_indices(elems: &[SyntaxElement]) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();
    let mut depth: i32 = 0;
    let mut i = 0;
    while i < elems.len() {
        let el = &elems[i];
        match el.kind() {
            // Bare delimiter tokens open/close a depth level.
            SyntaxKind::MATH_OPEN => {
                depth += 1;
                i += 1;
            }
            SyntaxKind::MATH_CLOSE => {
                depth -= 1;
                i += 1;
            }
            SyntaxKind::MATH_COMMAND => {
                let text = el
                    .as_token()
                    .map(|t| t.text().to_string())
                    .unwrap_or_default();
                let name = text.strip_prefix('\\').unwrap_or(&text);
                // `\left`/`\right` bracket a depth level (the following `(`/`)`
                // adds another, but it always balances, so depth still returns to
                // 0 after the pair — only the depth==0 test matters).
                if name == "left" {
                    depth += 1;
                } else if name == "right" {
                    depth -= 1;
                } else if depth == 0 && operators::command_class(name) == Some(AtomClass::Rel) {
                    out.push(i);
                }
                i += 1;
            }
            // A maximal run of adjacent operator tokens (one char each) splits
            // into atoms; each relation atom is a break candidate at its first
            // token. Done only at depth 0.
            SyntaxKind::MATH_OPERATOR => {
                let run_start = i;
                let mut run = String::new();
                while i < elems.len() && elems[i].kind() == SyntaxKind::MATH_OPERATOR {
                    if let Some(tok) = elems[i].as_token() {
                        run.push_str(tok.text());
                    }
                    i += 1;
                }
                if depth == 0 {
                    let mut char_off = 0usize;
                    for atom in operators::split_operator_atoms(&run) {
                        if operators::classify_operator(atom) == AtomClass::Rel {
                            out.push(run_start + char_off);
                        }
                        // Each operator char is exactly one token, so the atom's
                        // char length is its token count.
                        char_off += atom.chars().count();
                    }
                }
            }
            _ => i += 1,
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

    #[test]
    fn short_row_stays_one_line() {
        assert_eq!(lines("a = b = c", 80), vec!["a = b = c"]);
    }

    #[test]
    fn overwidth_relation_chain_breaks_and_aligns() {
        // First `=` stays on line 0; the second starts an aligned continuation.
        assert_eq!(
            lines("A = bbbbbbbbbb = cccccccccc", 20),
            vec!["A = bbbbbbbbbb", "  = cccccccccc"],
        );
    }

    #[test]
    fn alignment_tracks_the_first_relation_column() {
        // Prefix `\alpha + \beta` is 14 chars wide ⇒ the `=` sits at column 15,
        // so continuations indent 15 spaces.
        let out = lines("\\alpha + \\beta = gggggggggg = dddddddddd", 24);
        assert_eq!(out[0], "\\alpha + \\beta = gggggggggg");
        assert_eq!(out[1], "               = dddddddddd");
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
        // The `=` lives inside `(…)`, so there is no depth-0 chain to break — the
        // row stays one (over-width) line.
        let content = "ffffff(xxxxxxxx = yyyyyyyy) zzzzzzzz";
        assert_eq!(
            lines(content, 10),
            vec![render::render_inline(&elems(content)).trim().to_string()]
        );
    }

    #[test]
    fn relations_inside_left_right_are_not_break_points() {
        let content = "ffff \\left( xxxx = yyyy \\right) gggg";
        let rels = relation_break_indices(&elems(content));
        assert!(rels.is_empty(), "depth-0 rels: {rels:?}");
    }

    #[test]
    fn single_relation_does_not_break() {
        // Over-width but only one relation: nothing to align against, left alone.
        let content = "A = bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        assert_eq!(lines(content, 10).len(), 1);
    }

    #[test]
    fn relations_inside_braces_are_opaque() {
        // The `=` is inside a `\frac` argument group (a node we never descend),
        // so it is not a break candidate.
        let content = "\\frac{aaaa = bbbb}{cccc} dddd eeee";
        let rels = relation_break_indices(&elems(content));
        assert!(rels.is_empty(), "depth-0 rels: {rels:?}");
    }
}
