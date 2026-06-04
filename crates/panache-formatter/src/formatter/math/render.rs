//! Rendering pipeline for the math content formatter.
//!
//! Operates on a freshly re-parsed `MATH_CONTENT` tree (see the parent module).
//! The transforms are structural and each is independently idempotent — see
//! `STYLE.md` for the rules and the alignment idempotency argument. The short
//! version: every cell is *trimmed before its width is measured* and padding is
//! *trailing only*, so a second pass measures the same content widths and emits
//! identical bytes.

use rowan::NodeOrToken;

use super::{MathContext, MathFormatOptions};
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
            flush_free_rows(&pending, &flat_indent, &mut lines);
            pending.clear();
            if let Some(node) = el.as_node() {
                lines.extend(render_environment_lines(node, 0, opts));
            }
        } else {
            pending.push(el.clone());
        }
    }
    flush_free_rows(&pending, &flat_indent, &mut lines);
    lines.join("\n")
}

/// Free (non-environment) display content: one line per row, whitespace
/// collapsed, never column-aligned (a bare `&` outside an environment is not a
/// column separator).
fn flush_free_rows(elems: &[SyntaxElement], indent: &str, lines: &mut Vec<String>) {
    for row in split_rows(elems) {
        if row.is_blank() {
            continue;
        }
        let body = render_inline(&row.elems).trim().to_string();
        lines.push(format!("{indent}{}", with_break(body, row.has_break)));
    }
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

fn is_layout_whitespace(el: &SyntaxElement) -> bool {
    matches!(el.kind(), SyntaxKind::MATH_SPACE | SyntaxKind::MATH_NEWLINE)
        && el.as_token().is_some()
}

// ---------------------------------------------------------------------------
// Inline rendering: flatten tokens, whitespace runs → single space.
// ---------------------------------------------------------------------------

/// Render a run of elements onto a single line, collapsing every whitespace run
/// (including newlines) to one space. Groups and nested environments are
/// flattened in document order. Not trimmed — callers trim at the cell/row
/// level so that group interiors (`\text{ a }`) keep their spacing.
fn render_inline(elems: &[SyntaxElement]) -> String {
    let mut s = String::new();
    for el in elems {
        push_inline(&mut s, el);
    }
    collapse_spaces(&s)
}

fn push_inline(s: &mut String, el: &SyntaxElement) {
    match el {
        NodeOrToken::Token(tok) => push_token(s, tok.kind(), tok.text()),
        NodeOrToken::Node(node) => {
            for tok in node
                .descendants_with_tokens()
                .filter_map(|e| e.into_token())
            {
                push_token(s, tok.kind(), tok.text());
            }
        }
    }
}

fn push_token(s: &mut String, kind: SyntaxKind, text: &str) {
    match kind {
        SyntaxKind::MATH_SPACE | SyntaxKind::MATH_NEWLINE => s.push(' '),
        _ => s.push_str(text),
    }
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
