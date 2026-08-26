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
//! arrow *defines* its left-hand side rather than equating it, so equality
//! continuations anchor under the assignment's *right-hand side* X. Repeated
//! assignments still align under the first assignment operator. The
//! continuation's own relation kind therefore selects between those two
//! anchors.
//!
//! A `:=` is such an assignment, and [`operators::word_atoms`] interprets it as
//! one relation atom. When a script splits its lexical word after the colon,
//! the break candidate still spans the colon and scripted equals, so a chain is
//! never split between them.
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
//! The same per-continuation anchor selection aligns a relation chain split
//! across `\\` hard breaks in a bare `$$` (an implicit `aligned`); that
//! cross-row pass lives in [`super::render`]'s `relation_chain_alignment`,
//! reusing [`continuation_anchor_for`] and
//! [`begins_with_top_level_relation`].
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
use crate::syntax::{SyntaxElement, SyntaxKind, SyntaxNode};
use panache_parser::parser::math::{MathParseOptions, parse_math_content};
use panache_parser::semantic::math::SignatureScope;

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
fn rhs_start_column(elems: &[SyntaxElement], scope: &SignatureScope) -> usize {
    match first_top_level_relation(elems) {
        Some(b) => {
            render::render_inline(&elems[..b.end], scope)
                .trim()
                .chars()
                .count()
                + 1
        }
        None => {
            let w = render::render_inline(elems, scope).trim().chars().count();
            if w == 0 { 0 } else { w + 1 }
        }
    }
}

/// The column of the first top-level relation itself (its left edge): the
/// rendered width of everything before it, plus one for the separating space.
/// This aligns continuation relations *under the first relation* — the classic
/// chain layout for an equality/comparison chain (`x = a = b` ⇒ the `=` stack).
fn relation_column_normalized(elems: &[SyntaxElement], scope: &SignatureScope) -> usize {
    match first_top_level_relation(elems) {
        Some(b) => {
            let w = render::render_inline(&elems[..b.index], scope)
                .trim()
                .chars()
                .count();
            if w == 0 { 0 } else { w + 1 }
        }
        None => 0,
    }
}

fn relation_is_assignment(elems: &[SyntaxElement], relation: &Break) -> bool {
    let Some(tok) = semantic_token(&elems[relation.index]) else {
        return false;
    };
    match tok.kind() {
        SyntaxKind::MATH_CONTROL_WORD => {
            let name = tok.text().strip_prefix('\\').unwrap_or(tok.text());
            matches!(name, "gets" | "leftarrow" | "mapsto" | "coloneqq")
        }
        SyntaxKind::MATH_WORD => {
            tok.text() == ":"
                || operators::word_atoms(tok.text())
                    .next()
                    .is_some_and(|atom| atom.text.starts_with(":="))
        }
        _ => false,
    }
}

fn semantic_token(element: &SyntaxElement) -> Option<crate::syntax::SyntaxToken> {
    if let Some(token) = element.as_token() {
        return Some(token.clone());
    }
    if let Some(token) = operators::command_name_token(element) {
        return Some(token);
    }
    let base = operators::scripted_base(element)?;
    match &base {
        rowan::NodeOrToken::Token(token) => Some(token.clone()),
        rowan::NodeOrToken::Node(_) => operators::command_name_token(&base),
    }
}

/// The column one continuation relation hangs under.
///
/// Equality/comparison chains align their relations. For an assignment-led
/// chain, another assignment aligns with the first assignment, while an
/// ordinary relation continues under the assignment's right-hand side.
pub(super) fn continuation_anchor_for(
    head: &[SyntaxElement],
    continuation: &[SyntaxElement],
    parse_opts: MathParseOptions,
    scope: &SignatureScope,
) -> usize {
    let head = normalized_elements(head, parse_opts, scope);
    let continuation = normalized_elements(continuation, parse_opts, scope);
    let continuation_relation = first_top_level_relation(&continuation);
    continuation_anchor_normalized(
        &head,
        continuation_relation
            .as_ref()
            .map(|relation| (continuation.as_slice(), relation)),
        scope,
    )
}

fn continuation_anchor_normalized(
    head: &[SyntaxElement],
    continuation: Option<(&[SyntaxElement], &Break)>,
    scope: &SignatureScope,
) -> usize {
    match first_top_level_relation(head) {
        Some(first) if !relation_is_assignment(head, &first) => {
            relation_column_normalized(head, scope)
        }
        Some(_)
            if continuation
                .is_some_and(|(elements, relation)| relation_is_assignment(elements, relation)) =>
        {
            relation_column_normalized(head, scope)
        }
        _ => rhs_start_column(head, scope),
    }
}

/// True when the row's first non-layout-whitespace element is a top-level
/// relation operator (e.g. a continuation line `= b`). Used to detect a
/// relation chain spread across `\\` hard breaks.
pub(super) fn begins_with_top_level_relation(
    elems: &[SyntaxElement],
    parse_opts: MathParseOptions,
    scope: &SignatureScope,
) -> bool {
    let normalized = normalized_elements(elems, parse_opts, scope);
    match spaced_operator_breaks(&normalized).first() {
        Some(b) => {
            b.class == AtomClass::Rel
                && normalized[..b.index]
                    .iter()
                    .all(render::is_layout_whitespace)
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
/// [`continuation_anchor_for`]: under the first relation, under an assignment's
/// RHS for an ordinary relation, or under its operator for another assignment.
/// These offsets are pure functions of the row's content; the block's
/// `math-indent` shifts the whole row right (applied by the render layer) but
/// never changes these internal alignment columns.
pub(super) fn break_free_row(
    elems: &[SyntaxElement],
    line_width: usize,
    parse_opts: MathParseOptions,
    scope: &SignatureScope,
) -> Vec<String> {
    let single = render::render_inline(elems, scope).trim().to_string();
    if single.chars().count() <= line_width {
        return vec![single];
    }

    let normalized = normalized_elements_from_text(&single, parse_opts);
    let elems = normalized.as_slice();
    let breaks = spaced_operator_breaks(elems);
    let rels: Vec<&Break> = breaks
        .iter()
        .filter(|b| b.class == AtomClass::Rel)
        .collect();

    if rels.is_empty() {
        return break_binary_segment(elems, 0, line_width, scope);
    }

    if rels.len() == 1 {
        return break_binary_segment(elems, 0, line_width, scope);
    }

    let bounds: Vec<usize> = std::iter::once(0)
        .chain(rels[1..].iter().map(|relation| relation.index))
        .chain(std::iter::once(elems.len()))
        .collect();

    let mut out: Vec<String> = Vec::new();
    for w in 0..bounds.len() - 1 {
        let seg = &elems[bounds[w]..bounds[w + 1]];
        let seg_indent = if w == 0 {
            0
        } else {
            continuation_anchor_normalized(elems, Some((elems, rels[w])), scope)
        };
        out.extend(break_binary_segment(seg, seg_indent, line_width, scope));
    }
    out
}

fn normalized_elements(
    elems: &[SyntaxElement],
    parse_opts: MathParseOptions,
    scope: &SignatureScope,
) -> Vec<SyntaxElement> {
    normalized_elements_from_text(render::render_inline(elems, scope).trim(), parse_opts)
}

/// Re-parse rendered text with the *caller's* parse options — the re-parse
/// must reproduce the host's token shape (e.g. `MATH_EQUATION_LABEL` when
/// bookdown labels are on), or label interiors would be re-spaced as
/// operators.
fn normalized_elements_from_text(text: &str, parse_opts: MathParseOptions) -> Vec<SyntaxElement> {
    SyntaxNode::new_root(parse_math_content(text, parse_opts))
        .children_with_tokens()
        .collect()
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
    scope: &SignatureScope,
) -> Vec<String> {
    let single = render::render_inline(seg, scope).trim().to_string();
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
            render::render_inline(&seg[..b.end], scope)
                .trim()
                .chars()
                .count()
                + 1
        }
        _ => 0,
    };
    let bin_pad = " ".repeat(base_indent + rhs_offset);
    let mut out: Vec<String> = Vec::new();
    let head = render::render_inline(&seg[..bins[0]], scope)
        .trim()
        .to_string();
    if !head.is_empty() {
        out.push(format!("{base_pad}{head}"));
    }
    for w in 0..bins.len() {
        let start = bins[w];
        let end = bins.get(w + 1).copied().unwrap_or(seg.len());
        let cont = render::render_inline_seeded(&seg[start..end], Some(AtomClass::Close), scope)
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
            SyntaxKind::MATH_WORD
                if el.as_token().is_some_and(|token| {
                    elems
                        .get(i + 1)
                        .is_some_and(|next| severed_relation_pair(token.text(), next))
                }) =>
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
            }
            SyntaxKind::MATH_WORD => {
                let text = el.as_token().map(|token| token.text()).unwrap_or_default();
                for (atom_index, atom) in operators::word_atoms(text).enumerate() {
                    let is_modifier = atom_index == 0 && atom.text == "*" && star_modifier_pending;
                    let class = if is_modifier {
                        AtomClass::Ord
                    } else {
                        operators::coerce(atom.class, prev)
                    };
                    if depth == 0 && operators::is_spaced(class) {
                        out.push(Break {
                            index: i,
                            end: i + 1,
                            class,
                        });
                    }
                    match class {
                        AtomClass::Open => depth += 1,
                        AtomClass::Close => depth -= 1,
                        _ => {}
                    }
                    prev = Some(class);
                }
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
                let Some(base) = operators::scripted_base(el) else {
                    prev = Some(AtomClass::Ord);
                    star_modifier_pending = false;
                    i += 1;
                    continue;
                };

                let is_star_modifier = star_modifier_pending
                    && base.as_token().is_some_and(|token| {
                        token.kind() == SyntaxKind::MATH_WORD && token.text() == "*"
                    });
                let raw_class = if is_star_modifier {
                    Some(AtomClass::Ord)
                } else {
                    operators::scripted_base_class(&base)
                };
                let class = raw_class.map(|raw| operators::coerce(raw, prev));
                if base.kind() == SyntaxKind::MATH_WORD {
                    match class {
                        Some(AtomClass::Open) => depth += 1,
                        Some(AtomClass::Close) => depth -= 1,
                        _ => {}
                    }
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
            SyntaxKind::MATH_COMMAND | SyntaxKind::MATH_CONTROL_SYMBOL => {
                let text = operators::command_name_token(el)
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
            _ => {
                prev = Some(AtomClass::Ord);
                star_modifier_pending = false;
                i += 1;
            }
        }
    }
    out
}

/// Whether the whole token `head` is a relation head the parser's script
/// split severed from its final scalar, with `next` carrying that scalar as a
/// scripted relation base: a definition colon before a scripted `=`
/// (`: ` + `=_i`), or a relation run before a scripted relation scalar
/// (`<` + `=_i`). The pair is one break candidate, never two.
fn severed_relation_pair(head: &str, next: &SyntaxElement) -> bool {
    let Some(base_text) = operators::scripted_base(next)
        .and_then(|base| base.into_token())
        .filter(|token| token.kind() == SyntaxKind::MATH_WORD)
        .map(|token| token.text().to_string())
    else {
        return false;
    };
    if head == ":" {
        base_text.starts_with('=')
    } else {
        operators::is_relation_run(head) && base_text.starts_with(['=', '<', '>'])
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
        break_free_row(
            &elems(content),
            width,
            MathParseOptions::default(),
            &SignatureScope::default(),
        )
    }

    fn rel_indices(content: &str) -> Vec<usize> {
        spaced_operator_breaks(&elems(content))
            .iter()
            .filter(|b| b.class == AtomClass::Rel)
            .map(|b| b.index)
            .collect()
    }

    /// A star modifier is part of its command, never a break site. Nor is the
    /// operator `*` in the last case: `\operatorname` is a large operator, so
    /// TeX coerces the binary atom after it into a unary sign, and a unary sign
    /// is not a break site either.
    #[test]
    fn starred_command_modifier_is_not_a_break_candidate() {
        assert!(spaced_operator_breaks(&elems("\\operatorname*{minimize} a")).is_empty());
        assert!(spaced_operator_breaks(&elems("\\operatorname*_i{x}")).is_empty());
        assert!(spaced_operator_breaks(&elems("\\operatorname_i*{x}")).is_empty());
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
            vec!["A := bbbbbbbbbb", "  := cccccccccc"],
        );
    }

    #[test]
    fn definition_colon_fused_into_a_text_run_still_pairs() {
        assert_eq!(
            lines("ab:=bbbbbbbbbb:=cccccccccc", 20),
            vec!["ab := bbbbbbbbbb", "   := cccccccccc"],
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
    fn scripted_composite_relation_is_one_break_candidate() {
        // `<=_i` normalizes to a `<` word plus a scripted `=` base; the pair
        // must count as one relation, not two adjacent break sites.
        assert_eq!(rel_indices("aaaa <=_i bbbb <=_j cccc").len(), 2);
        assert_eq!(
            lines("aaaa <=_i bbbb <=_j cccc", 16),
            vec!["aaaa <=_i bbbb", "     <=_j cccc"],
        );
    }

    #[test]
    fn assignment_continuations_choose_the_semantic_anchor() {
        assert_eq!(
            lines("A \\gets_i bbbb =_j cccc", 18),
            vec!["A \\gets_i bbbb", "          =_j cccc"],
        );
        assert_eq!(
            lines("A :=_i bbbb :=_j cccc", 15),
            vec!["A :=_i bbbb", "  :=_j cccc"],
        );
        assert_eq!(
            lines("A :=_i bbbb =_j cccc", 15),
            vec!["A :=_i bbbb", "       =_j cccc"],
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
