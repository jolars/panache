//! Rendering pipeline for the math content formatter.
//!
//! Operates on a freshly re-parsed `MATH_CONTENT` tree (see the parent module).
//! The transforms are structural and each is independently idempotent — see
//! `STYLE.md` for the rules and the alignment idempotency argument. The short
//! version: every cell is *trimmed before its width is measured* and padding is
//! *trailing only*, so a second pass measures the same content widths and emits
//! identical bytes.

use rowan::NodeOrToken;

use super::operators::{self, AtomClass};
use super::{MathContext, MathFormatOptions, linebreak};
use crate::syntax::{SyntaxElement, SyntaxKind, SyntaxNode};

const INDENT: &str = "  ";

/// Entry point: dispatch on context. Returns delimiter-free content.
pub(super) fn render(tree: &SyntaxNode, opts: &MathFormatOptions) -> String {
    let top: Vec<SyntaxElement> = tree.children_with_tokens().collect();
    match opts.context {
        MathContext::Inline => render_inline(&top).trim().to_string(),
        MathContext::Display => render_display(&top, opts),
        MathContext::EnvironmentBody => render_body_lines(&top, 1, opts).join("\n"),
    }
}

// ---------------------------------------------------------------------------
// Display: block environments interleaved with free (non-aligned) rows.
// ---------------------------------------------------------------------------

fn render_display(top: &[SyntaxElement], opts: &MathFormatOptions) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut pending: Vec<SyntaxElement> = Vec::new();
    let flat_indent = " ".repeat(opts.math_indent);

    for el in top {
        if el.kind() == SyntaxKind::MATH_ENVIRONMENT {
            flush_free_rows(&pending, &flat_indent, opts.line_width, &mut lines);
            pending.clear();
            if let Some(node) = el.as_node() {
                lines.extend(render_environment_lines(node, 0, opts));
            }
        } else {
            pending.push(el.clone());
        }
    }
    flush_free_rows(&pending, &flat_indent, opts.line_width, &mut lines);
    lines.join("\n")
}

/// Free (non-environment) display content: one *logical* row per equation,
/// whitespace collapsed, never column-aligned (a bare `&` outside an
/// environment is not a column separator). A logical row is split only on a
/// top-level hard break (`\\`); a soft newline is insignificant whitespace
/// (math ignores it), so it is *not* a row boundary — this lets the line-breaker
/// re-join its own continuations on a later pass and recompute the same layout
/// (idempotency). Each logical row is then handed to [`linebreak::break_free_row`],
/// which keeps it on one line unless it exceeds `line_width`.
fn flush_free_rows(
    elems: &[SyntaxElement],
    indent: &str,
    line_width: usize,
    lines: &mut Vec<String>,
) {
    let rows = split_logical_rows(elems);
    // A relation chain spread across `\\` hard breaks is aligned like an implicit
    // `aligned`: continuation rows hang under the head row's RHS column (rule
    // "b"). `extra` is that per-row alignment offset (0 for heads / non-chains).
    let extra = relation_chain_alignment(&rows);
    for (idx, row) in rows.iter().enumerate() {
        if row.is_blank() {
            continue;
        }
        let ei = extra[idx];
        let pad = " ".repeat(ei);
        // Charge the flat math-indent *and* the alignment offset against the
        // budget so packed (and single) lines genuinely stay within `line_width`
        // once the indent and pad are prepended.
        let budget = line_width.saturating_sub(indent.chars().count() + ei);
        let physical = linebreak::break_free_row(&row.elems, budget);
        let last = physical.len() - 1;
        for (i, content) in physical.into_iter().enumerate() {
            // The trailing `\\` (if any) rides the final physical line.
            let content = if i == last {
                with_break(content, row.has_break)
            } else {
                content
            };
            lines.push(format!("{indent}{pad}{content}"));
        }
    }
}

/// Per-row alignment offset for relation chains split across `\\` hard breaks.
///
/// A *group* is a maximal run of `\\`-joined rows whose head ends in a hard break
/// and whose every following row begins with a top-level relation operator (a
/// continuation like `= b`). For a group of ≥ 2 rows, each continuation row is
/// offset to the head row's [`linebreak::rhs_start_column`] so the continuation
/// relations hang under the head's right-hand side. Heads, non-chain rows, and
/// any group containing a top-level `&` (left to the existing free-row path) get
/// offset 0. Recomputed from the (whitespace-trimmed) logical rows each pass, so
/// the alignment is a deterministic fixed point.
fn relation_chain_alignment(rows: &[Row]) -> Vec<usize> {
    let mut extra = vec![0usize; rows.len()];
    let mut i = 0;
    while i < rows.len() {
        if rows[i].has_break && !rows[i].is_blank() {
            let mut k = i;
            while rows[k].has_break
                && k + 1 < rows.len()
                && !rows[k + 1].is_blank()
                && linebreak::begins_with_top_level_relation(&rows[k + 1].elems)
            {
                k += 1;
            }
            if k > i && !rows[i..=k].iter().any(|r| has_top_level_align(&r.elems)) {
                let col = linebreak::continuation_anchor(&rows[i].elems);
                for offset in extra.iter_mut().take(k + 1).skip(i + 1) {
                    *offset = col;
                }
                i = k + 1;
                continue;
            }
        }
        i += 1;
    }
    extra
}

/// True if any direct element of the row is a top-level `&` alignment tab.
fn has_top_level_align(elems: &[SyntaxElement]) -> bool {
    elems.iter().any(|el| el.kind() == SyntaxKind::MATH_ALIGN)
}

// ---------------------------------------------------------------------------
// Environments.
// ---------------------------------------------------------------------------

fn render_environment_lines(
    env: &SyntaxNode,
    depth: usize,
    opts: &MathFormatOptions,
) -> Vec<String> {
    let Some(parts) = EnvParts::of(env) else {
        // Defensive: a shape we don't recognize (only reachable if the parser
        // contract drifts) — emit collapsed-but-verbatim so we stay lossless-ish.
        return vec![render_inline(
            &env.children_with_tokens().collect::<Vec<_>>(),
        )];
    };
    let indent = INDENT.repeat(depth);
    let mut lines = vec![format!("{indent}{}", parts.begin_line)];
    lines.extend(render_body_lines(&parts.body, depth + 1, opts));
    lines.push(format!("{indent}{}", parts.end_line));
    lines
}

/// An environment's reconstructed `\begin{name}` / `\end{name}` lines and the
/// body elements between them.
struct EnvParts {
    begin_line: String,
    end_line: String,
    body: Vec<SyntaxElement>,
}

impl EnvParts {
    fn of(env: &SyntaxNode) -> Option<Self> {
        let children: Vec<SyntaxElement> = env.children_with_tokens().collect();
        let is_cmd = |el: &SyntaxElement, text: &str| {
            el.as_token()
                .is_some_and(|t| t.kind() == SyntaxKind::MATH_COMMAND && t.text() == text)
        };
        let begin_idx = children.iter().position(|c| is_cmd(c, r"\begin"))?;
        let end_idx = children.iter().position(|c| is_cmd(c, r"\end"))?;
        let begin_name = first_group_after(&children, begin_idx);
        let end_name = first_group_after(&children, end_idx);

        let begin_line = format!(r"\begin{}", group_text(&children, begin_name));
        let end_line = format!(r"\end{}", group_text(&children, end_name));
        // Body = everything strictly between the begin name group and `\end`.
        let body_start = begin_name.map(|i| i + 1).unwrap_or(begin_idx + 1);
        let body = children[body_start..end_idx].to_vec();
        Some(Self {
            begin_line,
            end_line,
            body,
        })
    }
}

fn first_group_after(children: &[SyntaxElement], idx: usize) -> Option<usize> {
    children[idx + 1..]
        .iter()
        .position(|c| c.kind() == SyntaxKind::MATH_GROUP)
        .map(|p| p + idx + 1)
}

/// `{name}` text of the group at `idx`, or empty (an unnamed environment).
fn group_text(children: &[SyntaxElement], idx: Option<usize>) -> String {
    idx.and_then(|i| children[i].as_node())
        .map(|n| n.text().to_string())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Body layout: alignable rows + nested-environment blocks.
// ---------------------------------------------------------------------------

enum BodyItem {
    /// A nested environment rendered on its own line(s), already indented.
    Block(Vec<String>),
    /// A normal table row: trimmed cells split on top-level `&`.
    Row { cells: Vec<String>, has_break: bool },
}

fn render_body_lines(
    body: &[SyntaxElement],
    depth: usize,
    opts: &MathFormatOptions,
) -> Vec<String> {
    let indent = INDENT.repeat(depth);
    let mut items: Vec<BodyItem> = Vec::new();

    for row in split_rows(body) {
        if row.is_blank() {
            continue;
        }
        if let Some(env) = row.single_environment() {
            items.push(BodyItem::Block(render_environment_lines(&env, depth, opts)));
        } else {
            let cells = split_cells(&row.elems)
                .iter()
                .map(|cell| render_inline(cell).trim().to_string())
                .collect();
            items.push(BodyItem::Row {
                cells,
                has_break: row.has_break,
            });
        }
    }

    let widths = column_widths(&items);
    let mut out: Vec<String> = Vec::new();
    for item in items {
        match item {
            BodyItem::Block(lines) => out.extend(lines),
            BodyItem::Row { cells, has_break } => {
                let line = join_cells(&cells, &widths, has_break);
                out.push(format!("{indent}{}", with_break(line, has_break)));
            }
        }
    }
    out
}

/// Per-column max width over **every** cell of multi-cell rows (including the
/// last column, so trailing `\\` can be aligned too). Single-cell rows have no
/// separator and don't participate. Computed on already-trimmed cells — this is
/// the idempotency engine.
fn column_widths(items: &[BodyItem]) -> Vec<usize> {
    let mut widths: Vec<usize> = Vec::new();
    for item in items {
        if let BodyItem::Row { cells, .. } = item {
            if cells.len() < 2 {
                continue; // single cell ⇒ no column separator ⇒ nothing to pad
            }
            for (col, cell) in cells.iter().enumerate() {
                let w = cell.chars().count();
                if col >= widths.len() {
                    widths.resize(col + 1, 0);
                }
                widths[col] = widths[col].max(w);
            }
        }
    }
    widths
}

/// Pad cells to their column width and join with the canonical ` & ` separator
/// (matches latexindent: one space on each side of `&`). The last cell is padded
/// only when the row has a trailing `\\` — so the `\\` line up — and never on a
/// final/soft-break row, which would leave trailing whitespace. Single-cell rows
/// are never padded.
fn join_cells(cells: &[String], widths: &[usize], has_break: bool) -> String {
    if cells.is_empty() {
        return String::new();
    }
    if cells.len() == 1 {
        return cells[0].clone();
    }
    let last = cells.len() - 1;
    let mut parts: Vec<String> = Vec::with_capacity(cells.len());
    for (col, cell) in cells.iter().enumerate() {
        if col == last && !has_break {
            parts.push(cell.clone());
        } else {
            let width = widths.get(col).copied().unwrap_or(0);
            parts.push(pad_right(cell, width));
        }
    }
    parts.join(" & ")
}

fn pad_right(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - len))
    }
}

/// Append a normalized ` \\` line break (or a bare `\\` if the row is empty).
fn with_break(line: String, has_break: bool) -> String {
    if !has_break {
        return line;
    }
    if line.is_empty() {
        r"\\".to_string()
    } else {
        format!(r"{line} \\")
    }
}

// ---------------------------------------------------------------------------
// Rows & cells.
// ---------------------------------------------------------------------------

struct Row {
    elems: Vec<SyntaxElement>,
    has_break: bool,
}

impl Row {
    /// No content tokens and no hard break ⇒ a blank/whitespace-only line.
    fn is_blank(&self) -> bool {
        !self.has_break && self.elems.iter().all(is_layout_whitespace)
    }

    /// If the row's only content is a single nested environment (no `&`, no
    /// `\\`), return it so it can be block-laid-out instead of inlined.
    fn single_environment(&self) -> Option<SyntaxNode> {
        if self.has_break {
            return None;
        }
        let mut content = self.elems.iter().filter(|el| !is_layout_whitespace(el));
        let first = content.next()?;
        if content.next().is_some() {
            return None;
        }
        first
            .as_node()
            .filter(|n| n.kind() == SyntaxKind::MATH_ENVIRONMENT)
            .cloned()
    }
}

/// Split a flat element run into *logical* rows for free display content: only a
/// top-level hard break (`\\`) ends a row. A soft newline stays *inside* the row
/// as insignificant whitespace (the rendered equation is identical with or
/// without it), so a multi-line author equation — or one the line-breaker split
/// itself on a prior pass — collapses back to a single logical unit and is
/// re-laid-out identically. Contrast [`split_rows`], which also breaks on soft
/// newlines and is used for environment-body layout.
///
/// **Exception: a soft newline that terminates a `%` comment IS significant** —
/// a comment runs to end-of-line, so joining past it would absorb the next
/// line's content into the comment (and silently delete it from the rendered
/// math). Such a newline ends the logical row. A `MATH_COMMENT` always runs up
/// to the next newline, so it is the last content token before this newline;
/// keeping the boundary leaves the comment alone on its line, matching the
/// pre-line-breaking behavior.
fn split_logical_rows(elems: &[SyntaxElement]) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    let mut cur: Vec<SyntaxElement> = Vec::new();
    let mut cur_has_comment = false;
    for el in elems {
        match el.kind() {
            SyntaxKind::MATH_LINE_BREAK => {
                rows.push(Row {
                    elems: std::mem::take(&mut cur),
                    has_break: true,
                });
                cur_has_comment = false;
            }
            // A comment-terminating newline closes the row (drop the newline, as
            // a soft break); any other soft newline is kept as in-row whitespace.
            SyntaxKind::MATH_NEWLINE if cur_has_comment => {
                rows.push(Row {
                    elems: std::mem::take(&mut cur),
                    has_break: false,
                });
                cur_has_comment = false;
            }
            kind => {
                if kind == SyntaxKind::MATH_COMMENT {
                    cur_has_comment = true;
                }
                cur.push(el.clone());
            }
        }
    }
    if !cur.is_empty() {
        rows.push(Row {
            elems: cur,
            has_break: false,
        });
    }
    rows
}

/// Split a flat element run into rows. A row ends at a top-level `\\` (hard
/// break, recorded) or a top-level newline (soft break, dropped). Trailing
/// content with no terminator is the final row.
fn split_rows(elems: &[SyntaxElement]) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    let mut cur: Vec<SyntaxElement> = Vec::new();
    for el in elems {
        match el.kind() {
            SyntaxKind::MATH_LINE_BREAK => {
                rows.push(Row {
                    elems: std::mem::take(&mut cur),
                    has_break: true,
                });
            }
            SyntaxKind::MATH_NEWLINE => {
                rows.push(Row {
                    elems: std::mem::take(&mut cur),
                    has_break: false,
                });
            }
            _ => cur.push(el.clone()),
        }
    }
    if !cur.is_empty() {
        rows.push(Row {
            elems: cur,
            has_break: false,
        });
    }
    rows
}

/// Split a row into cells on top-level `&` tokens. A `&` nested inside a group
/// or sub-environment is not a separator (it isn't a direct child here).
fn split_cells(elems: &[SyntaxElement]) -> Vec<Vec<SyntaxElement>> {
    let mut cells: Vec<Vec<SyntaxElement>> = vec![Vec::new()];
    for el in elems {
        if el.kind() == SyntaxKind::MATH_ALIGN {
            cells.push(Vec::new());
        } else {
            cells.last_mut().expect("seeded").push(el.clone());
        }
    }
    cells
}

pub(super) fn is_layout_whitespace(el: &SyntaxElement) -> bool {
    matches!(el.kind(), SyntaxKind::MATH_SPACE | SyntaxKind::MATH_NEWLINE)
        && el.as_token().is_some()
}

// ---------------------------------------------------------------------------
// Inline rendering: flatten tokens, then re-space around operators.
// ---------------------------------------------------------------------------

/// Render a run of elements onto a single line. Groups and nested environments
/// are flattened in document order, whitespace runs collapse to one space, and
/// operators are re-spaced precedence-aware (`a+b` → `a + b`, unary `-x` stays
/// tight) per [`super::operators`]. Not trimmed — callers trim at the cell/row
/// level so that group interiors (`\text{ a }`) keep their spacing.
///
/// `pub(super)` so the line-breaker ([`linebreak`]) can render each broken
/// segment through the same single-line path, guaranteeing the segments re-space
/// exactly as the unbroken row would.
pub(super) fn render_inline(elems: &[SyntaxElement]) -> String {
    render_inline_seeded(elems, None)
}

/// Like [`render_inline`] but seeds the preceding-atom class. The line-breaker
/// uses this for a continuation that *starts* with a binary operator: rendered
/// in isolation the `+`/`-` would coerce to a unary sign (`+b`), but seeding a
/// closing-operand class keeps it binary (`+ b`). `None` reproduces
/// [`render_inline`] exactly.
pub(super) fn render_inline_seeded(elems: &[SyntaxElement], seed: Option<AtomClass>) -> String {
    let toks = flatten_tokens(elems);
    collapse_spaces(&space_operators(&toks, seed))
}

/// Flatten elements into `(kind, text)` tokens in document order, descending
/// into group/environment nodes so `{`/`}` and nested-environment tokens appear
/// in the stream (the spacing pass needs them for atom-class context).
fn flatten_tokens(elems: &[SyntaxElement]) -> Vec<(SyntaxKind, String)> {
    let mut out: Vec<(SyntaxKind, String)> = Vec::new();
    for el in elems {
        match el {
            NodeOrToken::Token(tok) => out.push((tok.kind(), tok.text().to_string())),
            NodeOrToken::Node(node) => {
                for tok in node
                    .descendants_with_tokens()
                    .filter_map(|e| e.into_token())
                {
                    out.push((tok.kind(), tok.text().to_string()));
                }
            }
        }
    }
    out
}

/// The spacing demand an emitted atom places on the gaps beside it.
#[derive(Clone, Copy, PartialEq)]
enum Demand {
    /// Nothing emitted yet — no leading space before the first atom.
    Start,
    /// An ordinary atom: keep author whitespace, add nothing.
    Plain,
    /// A binary/relation operator run: one space on each side.
    SpacedOp,
    /// A unary (coerced) operator run: tight; strips adjacent author space.
    TightOp,
}

/// Walk the flat token stream, grouping consecutive `MATH_OPERATOR` tokens into
/// runs and emitting one space on each side of binary/relation runs while
/// keeping unary runs tight. Author whitespace between non-operator atoms is
/// preserved (a command-terminating space in `\alpha x`, a `\text{ a }`
/// interior); whitespace adjacent to a tight unary operator is stripped, but a
/// space demanded by a neighboring spaced operator still wins.
fn space_operators(toks: &[(SyntaxKind, String)], seed: Option<AtomClass>) -> String {
    let mut out = String::new();
    let mut prev_class: Option<AtomClass> = seed;
    let mut prev_demand = Demand::Start;
    let mut pending_space = false;

    let mut i = 0;
    while i < toks.len() {
        let (kind, text) = &toks[i];
        match *kind {
            SyntaxKind::MATH_SPACE | SyntaxKind::MATH_NEWLINE => {
                pending_space = true;
                i += 1;
            }
            SyntaxKind::MATH_OPERATOR => {
                // Collect the maximal run of *adjacent* operator tokens (a space
                // between two operators breaks the run, so `- -` stays two),
                // then split it into atoms: relation-char runs merge (`<=`),
                // each sign char stands alone so it can be unary (`=-` → `= -`).
                let mut run = String::new();
                while i < toks.len() && toks[i].0 == SyntaxKind::MATH_OPERATOR {
                    run.push_str(&toks[i].1);
                    i += 1;
                }
                for atom in operators::split_operator_atoms(&run) {
                    let class = operators::coerce(operators::classify_operator(atom), prev_class);
                    let demand = if operators::is_spaced(class) {
                        Demand::SpacedOp
                    } else {
                        Demand::TightOp
                    };
                    emit_atom(&mut out, prev_demand, demand, pending_space, atom);
                    pending_space = false; // only the first atom sees the run's leading space
                    prev_demand = demand;
                    prev_class = Some(class);
                }
            }
            SyntaxKind::MATH_COMMAND => {
                let name = text.strip_prefix('\\').unwrap_or(text);
                let demand = match operators::command_class(name) {
                    // A binary/relation command operator (`\cdot`, `\leq`) gets
                    // one space on each side. A coerced (unary-position) command
                    // op, a large operator (`\sum`), a delimiter (`\left`), or an
                    // ordinary command (`\alpha`, `\frac`) stays Plain — never
                    // TightOp — so the mandatory command-terminating space is
                    // preserved, never stripped into a wrong control word.
                    Some(raw) => {
                        let class = operators::coerce(raw, prev_class);
                        prev_class = Some(class);
                        if operators::is_spaced(class) {
                            Demand::SpacedOp
                        } else {
                            Demand::Plain
                        }
                    }
                    None => {
                        prev_class = Some(AtomClass::Ord);
                        Demand::Plain
                    }
                };
                emit_atom(&mut out, prev_demand, demand, pending_space, text);
                pending_space = false;
                prev_demand = demand;
                i += 1;
            }
            SyntaxKind::MATH_COMMENT => {
                // Transparent for class purposes (an operator looks back past a
                // comment), but emitted verbatim.
                emit_atom(&mut out, prev_demand, Demand::Plain, pending_space, text);
                pending_space = false;
                prev_demand = Demand::Plain;
                i += 1;
            }
            _ => {
                emit_atom(&mut out, prev_demand, Demand::Plain, pending_space, text);
                pending_space = false;
                prev_demand = Demand::Plain;
                prev_class = atom_prev_class(*kind, text);
                i += 1;
            }
        }
    }
    out
}

/// Append `text`, inserting the resolved gap before it. The first atom
/// (`prev == Start`) never gets a leading space.
fn emit_atom(out: &mut String, prev: Demand, cur: Demand, pending_space: bool, text: &str) {
    if prev != Demand::Start && gap_space(prev, cur, pending_space) {
        out.push(' ');
    }
    out.push_str(text);
}

/// Resolve the gap between two adjacent atoms: a spaced operator always wins
/// (one space); a tight operator otherwise strips the gap; plain atoms preserve
/// author whitespace.
fn gap_space(prev: Demand, cur: Demand, pending_space: bool) -> bool {
    if prev == Demand::SpacedOp || cur == Demand::SpacedOp {
        true
    } else if prev == Demand::TightOp || cur == Demand::TightOp {
        false
    } else {
        pending_space
    }
}

/// The atom class a non-operator token presents to a *following* operator run.
/// `MATH_COMMAND` is handled inline in [`space_operators`] (it sets `prev_class`
/// from the coerced class), so it never reaches here. `None` resets context (a
/// `\\` starts a fresh line, so a following `+`/`-` is unary).
fn atom_prev_class(kind: SyntaxKind, _text: &str) -> Option<AtomClass> {
    // Delimiters/punctuation (`( ) [ ] , ;`) carry their class on the token
    // kind now — the parser tokenizes them, so the formatter no longer re-lexes
    // a `MATH_TEXT` tail to recover it.
    if let Some(class) = operators::delimiter_class(kind) {
        return Some(class);
    }
    let class = match kind {
        SyntaxKind::MATH_TEXT => AtomClass::Ord,
        SyntaxKind::MATH_GROUP_OPEN => AtomClass::Open,
        SyntaxKind::MATH_GROUP_CLOSE => AtomClass::Close,
        // `^`/`_` bind tightly; a `&` opens a fresh cell — both make a directly
        // following `+`/`-` unary.
        SyntaxKind::MATH_SCRIPT | SyntaxKind::MATH_ALIGN => AtomClass::Open,
        SyntaxKind::MATH_LINE_BREAK => return None,
        // MATH_EQUATION_LABEL and anything unforeseen: ordinary.
        _ => AtomClass::Ord,
    };
    Some(class)
}

/// Collapse runs of spaces to a single space (tabs already became spaces). Safe
/// everywhere: math mode ignores spaces; text mode collapses runs anyway.
fn collapse_spaces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        if ch == ' ' {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out
}
