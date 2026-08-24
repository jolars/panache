//! Semantic line-breaking for over-width display **free rows** (`$$…$$`
//! non-environment content).
//!
//! A logical free row wider than the target `line_width` is broken at its
//! top-level operators, with a two-level hierarchy mirroring the amsmath
//! convention: **relations** (`=`, `\leq`, `\to`, …) first, then the **binary**
//! operators (`+`, `\cdot`, …) inside each over-width relation segment. Binary
//! continuations sit **flush** under the relation's right-hand side; the
//! relation/RHS offset alone supplies the visual nesting. Continuation relations
//! align under the first relation — the classic `=` stack for an
//! equality/comparison chain:
//!
//! ```text
//! A = aaaaaaaaaa
//!     + bbbbbbbbbb
//!   = cccccccccc
//!     + dddddddddd
//! ```
//!
//! The exception is an **assignment-led** chain (`\beta \gets X = Y = …`): the
//! arrow *defines* its left-hand side rather than equating it, so the equality
//! continuations anchor under the assignment's *right-hand side* X, not aligned
//! with the arrow. [`continuation_anchor`] picks the column per the leading
//! relation (via [`first_relation_is_assignment`]).
//!
//! A `:=` is such an assignment, and it is *one* relation atom spanning two
//! tokens (the `:` and the `=`; see [`operators::is_definition_colon`]). Its
//! break candidate is anchored on the `:` and spans through the `=`, so a chain
//! is never split between them and the alignment columns measure the whole
//! symbol — the same treatment a composite `<=` gets.
//!
//! ## What "top-level" means, and why groups are opaque
//!
//! Breaks are only ever offered at operators sitting at **delimiter depth 0**.
//! Ordinary `(…)` and `[…]` pairs are flat token runs tracked with an open/close
//! counter. `\left…\right` pairs, brace groups (`{…}`), and environments are
//! structural nodes treated as opaque operands, so the scan never descends into
//! them for break points. This is a *layout policy*, not a hard constraint (math
//! ignores whitespace, so one could break inside `{…}`), chosen because (a) some
//! interiors are whitespace/newline-sensitive (`\text{…}`, trailing `%`
//! comments), (b) one breaks at the outermost structure by convention, and (c)
//! it keeps the walk and its idempotency simple. The consequence we accept: a
//! sub-unit wider than the line with no top-level operator stays over-width,
//! like an unbreakable long word in prose reflow.
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
//! breaks its over-width binary RHS, each `+ term` flush under the RHS. A
//! standalone binary chain (no relation) breaks with the first term as the head
//! and each `+ term` flush under it. The unifying rule across all three: a binary
//! continuation aligns flush under the first term of its operand sequence. A row
//! with no top-level relation or binary operator (e.g. a lone wide `\frac{…}{…}`)
//! stays on one line, like an unbreakable long word in prose reflow. Inline and
//! environment-body math are not line-broken.
//!
//! These offsets are pure functions of the row's content. The block's
//! `math-indent` shifts the whole row right (in the render layer), but never
//! reaches inside to change how operators line up against their operands — so
//! the equation's internal shape is identical at any `math-indent`.
//!
//! The same anchor aligns a relation chain split across `\\` hard breaks in a
//! bare `$$` (an implicit `aligned`); that cross-row pass lives in
//! [`super::render`]'s `relation_chain_alignment`, reusing [`continuation_anchor`]
//! and [`begins_with_top_level_relation`].
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
use crate::syntax::{AstNode, MathScripted, SyntaxElement, SyntaxKind, SyntaxToken};

struct Break {
    /// Element index of the atom's first token (where a break lands before it).
    index: usize,
    /// Element index one past the atom's last token. Usually `index + 1`, but a
    /// composite atom spans several tokens (`<=`, or the `:` + `=` of a `:=`),
    /// and the alignment columns measure up to *here*, not to `index + 1`.
    end: usize,
    /// The atom's coerced class — only `Bin`/`Rel` ever reach here.
    class: AtomClass,
}

fn first_top_level_relation(elems: &[SyntaxElement]) -> Option<Break> {
    spaced_operator_breaks(elems)
        .into_iter()
        .find(|b| b.class == AtomClass::Rel)
}

/// The column (relative to the row's own base, *excluding* the block math-indent)
/// where the first line's right-hand side begins: just past the first top-level
/// relation and its single separating space. This is the alignment anchor for
/// relation continuations — both the over-width within-row breaks and the
/// cross-`\\`-row implicit alignment hang continuations here. If the row has no
/// top-level relation, the anchor is the full rendered width + 1 (where an `&`
/// would sit), used only for a relation-led continuation whose head row lacks a
/// relation; an empty row yields 0.
pub(super) fn rhs_start_column(elems: &[SyntaxElement]) -> usize {
    match first_top_level_relation(elems) {
        Some(b) => {
            render::render_inline(&elems[..b.end])
                .trim()
                .chars()
                .count()
                + 1
        }
        None => {
            let w = render::render_inline(elems).trim().chars().count();
            if w == 0 { 0 } else { w + 1 }
        }
    }
}

/// The column of the first top-level relation itself (its left edge): the
/// rendered width of everything before it, plus one for the separating space.
/// This aligns continuation relations *under the first relation* — the classic
/// chain layout for an equality/comparison chain (`x = a = b` ⇒ the `=` stack).
fn relation_column(elems: &[SyntaxElement]) -> usize {
    match first_top_level_relation(elems) {
        Some(b) => {
            let w = render::render_inline(&elems[..b.index])
                .trim()
                .chars()
                .count();
            if w == 0 { 0 } else { w + 1 }
        }
        None => 0,
    }
}

fn first_relation_is_assignment(elems: &[SyntaxElement]) -> bool {
    let Some(b) = first_top_level_relation(elems) else {
        return false;
    };
    let Some(tok) = semantic_token(&elems[b.index]) else {
        return false;
    };
    match tok.kind() {
        SyntaxKind::MATH_COMMAND => {
            let name = tok.text().strip_prefix('\\').unwrap_or(tok.text());
            matches!(name, "gets" | "leftarrow" | "mapsto" | "coloneqq")
        }
        SyntaxKind::MATH_TEXT => tok.text() == ":",
        _ => false,
    }
}

/// The column continuation relations hang under, given the leading relation's
/// kind: under the first relation for an equality/comparison chain
/// ([`relation_column`]), but under the assignment's right-hand side for an
/// assignment-led chain (or a relationless head) ([`rhs_start_column`]).
pub(super) fn continuation_anchor(elems: &[SyntaxElement]) -> usize {
    match first_top_level_relation(elems) {
        Some(_) if !first_relation_is_assignment(elems) => relation_column(elems),
        _ => rhs_start_column(elems),
    }
}

/// True when the row's first non-layout-whitespace element is a top-level
/// relation operator (e.g. a continuation line `= b`). Used to detect a
/// relation chain spread across `\\` hard breaks.
pub(super) fn begins_with_top_level_relation(elems: &[SyntaxElement]) -> bool {
    match spaced_operator_breaks(elems).first() {
        Some(b) => {
            b.class == AtomClass::Rel && elems[..b.index].iter().all(render::is_layout_whitespace)
        }
        None => false,
    }
}

/// Break one logical free-display row into physical content lines (no base
/// math-indent, no trailing `\\` — the caller adds those). A row that fits, or
/// that has no usable relation chain, is returned on one line unchanged.
///
/// Every *binary* continuation line sits **flush** under the first term of its
/// operand sequence — under the segment's right-hand side, or under the chain's
/// head term for a relationless chain. Relation continuations hang at
/// [`continuation_anchor`] (under the first relation, or the assignment's RHS).
/// Both offsets are pure functions of the row's content; the block's
/// `math-indent` shifts the whole row right (applied by the render layer) but
/// never changes these internal alignment columns.
pub(super) fn break_free_row(elems: &[SyntaxElement], line_width: usize) -> Vec<String> {
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

    if rels.is_empty() {
        return break_binary_segment(elems, 0, line_width);
    }

    let rel_indent = continuation_anchor(elems);

    if rels.len() == 1 {
        return break_binary_segment(elems, 0, line_width);
    }

    let bounds: Vec<usize> = std::iter::once(0)
        .chain(rels[1..].iter().copied())
        .chain(std::iter::once(elems.len()))
        .collect();

    let mut out: Vec<String> = Vec::new();
    for w in 0..bounds.len() - 1 {
        let seg = &elems[bounds[w]..bounds[w + 1]];
        let seg_indent = if w == 0 { 0 } else { rel_indent };
        out.extend(break_binary_segment(seg, seg_indent, line_width));
    }
    out
}

/// Lay out one relation segment: keep it on a single line at `base_indent` if it
/// fits, otherwise split it before each top-level binary operator. Each `+ term`
/// hangs flush under the first term of this segment's operand sequence — its own
/// right-hand side (`base_indent + rhs_start_column(seg)`), or the chain start
/// when the segment has no relation.
fn break_binary_segment(
    seg: &[SyntaxElement],
    base_indent: usize,
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
        return vec![format!("{base_pad}{single}")];
    }

    let rhs_offset = match first_top_level_relation(seg) {
        Some(b) if bins[0] > b.index => {
            render::render_inline(&seg[..b.end]).trim().chars().count() + 1
        }
        _ => 0,
    };
    let bin_pad = " ".repeat(base_indent + rhs_offset);
    let mut out: Vec<String> = Vec::new();
    let head = render::render_inline(&seg[..bins[0]]).trim().to_string();
    if !head.is_empty() {
        out.push(format!("{base_pad}{head}"));
    }
    for w in 0..bins.len() {
        let start = bins[w];
        let end = bins.get(w + 1).copied().unwrap_or(seg.len());
        let cont = render::render_inline_seeded(&seg[start..end], Some(AtomClass::Close))
            .trim()
            .to_string();
        out.push(format!("{bin_pad}{cont}"));
    }
    out
}

fn spaced_operator_breaks(elems: &[SyntaxElement]) -> Vec<Break> {
    let mut out: Vec<Break> = Vec::new();
    let mut depth: i32 = 0;
    let mut prev: Option<AtomClass> = None;
    let mut star_modifier_pending = false;
    let mut i = 0;
    while i < elems.len() {
        let el = &elems[i];
        match el.kind() {
            SyntaxKind::MATH_OPEN => {
                depth += 1;
                prev = Some(AtomClass::Open);
                star_modifier_pending = false;
                i += 1;
            }
            SyntaxKind::MATH_CLOSE => {
                depth -= 1;
                prev = Some(AtomClass::Close);
                star_modifier_pending = false;
                i += 1;
            }
            SyntaxKind::MATH_PUNCT => {
                prev = Some(AtomClass::Punct);
                star_modifier_pending = false;
                i += 1;
            }
            SyntaxKind::MATH_TEXT
                if is_definition_colon_element(
                    el.as_token().map(|t| t.text()).unwrap_or_default(),
                    elems.get(i + 1),
                ) =>
            {
                if elems
                    .get(i + 1)
                    .is_some_and(|next| next.kind() == SyntaxKind::MATH_SCRIPTED)
                {
                    if depth == 0 {
                        out.push(Break {
                            index: i,
                            end: i + 2,
                            class: AtomClass::Rel,
                        });
                    }
                    prev = Some(AtomClass::Rel);
                    star_modifier_pending = false;
                    i += 2;
                    continue;
                }

                let mut run = String::new();
                let mut j = i + 1;
                while j < elems.len() && elems[j].kind() == SyntaxKind::MATH_OPERATOR {
                    if let Some(tok) = elems[j].as_token() {
                        run.push_str(tok.text());
                    }
                    j += 1;
                }
                let head = operators::split_operator_atoms(&run)
                    .first()
                    .map(|a| a.chars().count())
                    .unwrap_or(0);
                let end = i + 1 + head;
                if depth == 0 {
                    out.push(Break {
                        index: i,
                        end,
                        class: AtomClass::Rel,
                    });
                }
                prev = Some(AtomClass::Rel);
                star_modifier_pending = false;
                i = end;
            }
            SyntaxKind::MATH_TEXT => {
                prev = Some(AtomClass::Ord);
                star_modifier_pending = false;
                i += 1;
            }
            SyntaxKind::MATH_CARET | SyntaxKind::MATH_UNDERSCORE | SyntaxKind::MATH_ALIGN => {
                prev = Some(AtomClass::Open);
                star_modifier_pending = false;
                i += 1;
            }
            SyntaxKind::MATH_GROUP | SyntaxKind::MATH_ENVIRONMENT | SyntaxKind::MATH_DELIMITED => {
                prev = Some(AtomClass::Close);
                star_modifier_pending = false;
                i += 1;
            }
            SyntaxKind::MATH_SCRIPTED => {
                let Some(base) = el
                    .as_node()
                    .and_then(|node| MathScripted::cast(node.clone()))
                    .and_then(|scripted| scripted.base())
                else {
                    prev = Some(AtomClass::Ord);
                    star_modifier_pending = false;
                    i += 1;
                    continue;
                };

                let is_star_modifier = star_modifier_pending
                    && base.as_token().is_some_and(|token| {
                        token.kind() == SyntaxKind::MATH_OPERATOR && token.text() == "*"
                    });
                let raw_class = if is_star_modifier {
                    Some(AtomClass::Ord)
                } else {
                    scripted_base_class(&base)
                };
                let class = raw_class.map(|raw| operators::coerce(raw, prev));
                match base.kind() {
                    SyntaxKind::MATH_OPEN => depth += 1,
                    SyntaxKind::MATH_CLOSE => depth -= 1,
                    _ => {}
                }
                if depth == 0 && class.is_some_and(operators::is_spaced) {
                    out.push(Break {
                        index: i,
                        end: i + 1,
                        class: class.expect("spaced scripted atom has a class"),
                    });
                }
                prev = class.or(Some(AtomClass::Ord));
                // A script ends the adjacency required by a following star
                // modifier, even when the scripted base is a command.
                star_modifier_pending = false;
                i += 1;
            }
            SyntaxKind::MATH_SPACE | SyntaxKind::MATH_NEWLINE | SyntaxKind::MATH_COMMENT => {
                i += 1;
            }
            SyntaxKind::MATH_LINE_BREAK => {
                prev = None;
                star_modifier_pending = false;
                i += 1;
            }
            SyntaxKind::MATH_COMMAND => {
                let text = el
                    .as_token()
                    .map(|t| t.text().to_string())
                    .unwrap_or_default();
                let name = text.strip_prefix('\\').unwrap_or(&text);
                if let Some(raw) = operators::command_class(name) {
                    let class = operators::coerce(raw, prev);
                    if depth == 0 && operators::is_spaced(class) {
                        out.push(Break {
                            index: i,
                            end: i + 1,
                            class,
                        });
                    }
                    prev = Some(class);
                } else {
                    prev = Some(AtomClass::Ord);
                }
                star_modifier_pending = operators::takes_star_modifier(name);
                i += 1;
            }
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
                for (n, atom) in operators::split_operator_atoms(&run)
                    .into_iter()
                    .enumerate()
                {
                    let is_modifier = n == 0 && atom == "*" && star_modifier_pending;
                    let class = if is_modifier {
                        AtomClass::Ord
                    } else {
                        operators::coerce(operators::classify_operator(atom), prev)
                    };
                    if depth == 0 && operators::is_spaced(class) {
                        out.push(Break {
                            index: run_start + char_off,
                            end: run_start + char_off + atom.chars().count(),
                            class,
                        });
                    }
                    prev = Some(class);
                    char_off += atom.chars().count();
                }
                star_modifier_pending = false;
            }
            _ => {
                prev = Some(AtomClass::Ord);
                star_modifier_pending = false;
                i += 1;
            }
        }
    }
    out
}

fn semantic_token(element: &SyntaxElement) -> Option<SyntaxToken> {
    if let Some(token) = element.as_token() {
        return Some(token.clone());
    }
    let scripted = element
        .as_node()
        .and_then(|node| MathScripted::cast(node.clone()))?;
    scripted.base()?.into_token()
}

fn is_definition_colon_element(text: &str, next: Option<&SyntaxElement>) -> bool {
    let next = next.and_then(semantic_token);
    operators::is_definition_colon(
        text,
        next.as_ref().map(|token| (token.kind(), token.text())),
    )
}

fn scripted_base_class(base: &SyntaxElement) -> Option<AtomClass> {
    if let Some(class) = operators::delimiter_class(base.kind()) {
        return Some(class);
    }
    match base.kind() {
        SyntaxKind::MATH_OPERATOR => base
            .as_token()
            .map(|token| operators::classify_operator(token.text())),
        SyntaxKind::MATH_COMMAND => base.as_token().map(|token| {
            let name = token.text().strip_prefix('\\').unwrap_or(token.text());
            operators::command_class(name).unwrap_or(AtomClass::Ord)
        }),
        SyntaxKind::MATH_GROUP | SyntaxKind::MATH_ENVIRONMENT | SyntaxKind::MATH_DELIMITED => {
            Some(AtomClass::Close)
        }
        SyntaxKind::MATH_CARET | SyntaxKind::MATH_UNDERSCORE | SyntaxKind::MATH_ALIGN => {
            Some(AtomClass::Open)
        }
        SyntaxKind::MATH_LINE_BREAK => None,
        _ => Some(AtomClass::Ord),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::SyntaxNode;
    use panache_parser::parser::math::{MathParseOptions, parse_math_content};

    fn elems(content: &str) -> Vec<SyntaxElement> {
        let node = SyntaxNode::new_root(parse_math_content(content, MathParseOptions::default()));
        node.children_with_tokens().collect()
    }

    /// Alignment geometry for a logical row (no base block indent — that is
    /// applied by the render layer, not the breaker).
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
    fn starred_command_modifier_is_not_a_break_candidate() {
        assert!(spaced_operator_breaks(&elems("\\operatorname*{minimize} a")).is_empty());
        assert!(spaced_operator_breaks(&elems("\\operatorname*_i{x}")).is_empty());
        assert!(!spaced_operator_breaks(&elems("\\operatorname_i*{x}")).is_empty());
    }

    #[test]
    fn short_row_stays_one_line() {
        assert_eq!(lines("a = b = c", 80), vec!["a = b = c"]);
    }

    #[test]
    fn overwidth_relation_chain_breaks_and_aligns() {
        assert_eq!(
            lines("A = bbbbbbbbbb = cccccccccc", 20),
            vec!["A = bbbbbbbbbb", "  = cccccccccc"],
        );
    }

    #[test]
    fn definition_colon_is_one_relation_atom() {
        assert_eq!(
            lines("A := bbbbbbbbbb := cccccccccc", 20),
            vec!["A := bbbbbbbbbb", "     := cccccccccc"],
        );
    }

    #[test]
    fn definition_colon_fused_into_a_text_run_still_pairs() {
        assert_eq!(
            lines("ab:=bbbbbbbbbb:=cccccccccc", 20),
            vec!["ab := bbbbbbbbbb", "      := cccccccccc"],
        );
    }

    #[test]
    fn reversed_colon_is_not_a_definition() {
        let out = lines("A =: bbbbbbbbbb =: cccccccccc", 20);
        assert_eq!(out.len(), 2);
        assert!(out[0].starts_with("A ="), "{out:?}");
        assert_eq!(&out[1][..3], "  =", "{out:?}");
    }

    #[test]
    fn alignment_tracks_the_first_relation_column() {
        let out = lines("\\alpha + \\beta = gggggggggg = dddddddddd", 30);
        assert_eq!(out[0], "\\alpha + \\beta = gggggggggg");
        assert_eq!(out[1], "               = dddddddddd");
    }

    #[test]
    fn overwidth_segments_nest_binary_operators() {
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
    fn scripted_relations_remain_break_points() {
        assert_eq!(
            lines("aaaa =_i bbbb =_j cccc", 15),
            vec!["aaaa =_i bbbb", "     =_j cccc"],
        );
    }

    #[test]
    fn scripted_assignment_relations_keep_the_rhs_anchor() {
        assert_eq!(
            lines("A \\gets_i bbbb =_j cccc", 18),
            vec!["A \\gets_i bbbb", "          =_j cccc"],
        );
        assert_eq!(
            lines("A :=_i bbbb :=_j cccc", 15),
            vec!["A :=_i bbbb", "       :=_j cccc"],
        );
    }

    #[test]
    fn relations_inside_parens_are_not_break_points() {
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
    fn nested_null_and_asymmetric_delimited_run_is_opaque() {
        let content = "aaaa = \\left. \\left( xxxx = yyyy \\right] \\right| = zzzz";
        assert_eq!(rel_indices(content).len(), 2);
        assert_eq!(
            lines(content, 20),
            vec![
                "aaaa = \\left. \\left( xxxx = yyyy \\right] \\right|",
                "     = zzzz",
            ],
        );
    }

    #[test]
    fn binary_operator_after_delimited_run_stays_binary() {
        assert_eq!(
            lines("\\left[ a = b \\right] + cccccccc + dddddddd", 20),
            vec!["\\left[ a = b \\right]", "+ cccccccc", "+ dddddddd"],
        );
    }

    #[test]
    fn single_relation_does_not_break() {
        let content = "A = bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        assert_eq!(lines(content, 10).len(), 1);
    }

    #[test]
    fn relations_inside_braces_are_opaque() {
        let content = "\\frac{aaaa = bbbb}{cccc} dddd eeee";
        assert_eq!(rel_indices(content), Vec::<usize>::new());
    }

    #[test]
    fn scripted_group_base_does_not_change_delimiter_depth() {
        let content = "{x}_i = bbbbbbbbbb = cccccccccc";
        assert_eq!(rel_indices(content).len(), 2);
    }

    #[test]
    fn unary_sign_is_not_a_binary_break_point() {
        let out = lines("A = -tttttttttt = -uuuuuuuuuu", 12);
        assert_eq!(out, vec!["A = -tttttttttt", "  = -uuuuuuuuuu"]);
    }

    #[test]
    fn single_relation_breaks_binary_terms() {
        assert_eq!(
            lines("A = aaaaaaaaaa + bbbbbbbbbb + cccccccccc", 20),
            vec!["A = aaaaaaaaaa", "    + bbbbbbbbbb", "    + cccccccccc"],
        );
    }

    #[test]
    fn zero_relation_binary_chain_breaks_flush() {
        assert_eq!(
            lines("aaaa + bbbb + cccc + dddd", 12),
            vec!["aaaa", "+ bbbb", "+ cccc", "+ dddd"],
        );
    }

    #[test]
    fn zero_relation_no_binary_stays_one_line() {
        assert_eq!(lines("\\frac{aaaaaaaa}{bbbbbbbb}", 12).len(), 1);
    }

    #[test]
    fn zero_relation_leading_unary_sign_is_head() {
        assert_eq!(
            lines("-aaaaaaaa + bbbbbbbb + cccccccc", 12),
            vec!["-aaaaaaaa", "+ bbbbbbbb", "+ cccccccc"],
        );
    }
}
