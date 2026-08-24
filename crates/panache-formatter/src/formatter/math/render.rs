//! Rendering pipeline for the math content formatter.
//!
//! Operates on a freshly re-parsed `MATH_CONTENT` tree (see the parent module).
//! The transforms are structural and each is independently idempotent — see
//! `STYLE.md` for the rules and the alignment idempotency argument. The short
//! version: every cell is *trimmed before its width is measured* and padding is
//! *trailing only*, so a second pass measures the same content widths and emits
//! identical bytes.

use rowan::NodeOrToken;

use super::layout::{Doc, Printer};
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

fn render_display(top: &[SyntaxElement], opts: &MathFormatOptions) -> String {
    if has_mixed_environment_content(top) {
        return render_mixed_delimited_display(top, opts)
            .or_else(|| render_top_level_mixed_environment(top, opts))
            .unwrap_or_else(|| {
                let content: String = top.iter().map(ToString::to_string).collect();
                content.trim_matches(['\r', '\n']).to_string()
            });
    }

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

fn has_mixed_environment_content(elems: &[SyntaxElement]) -> bool {
    let has_environment = elems.iter().any(contains_environment);
    let has_free_content = elems.iter().any(|element| {
        element.kind() != SyntaxKind::MATH_ENVIRONMENT && !is_layout_whitespace(element)
    });
    has_environment && has_free_content
}

fn contains_environment(element: &SyntaxElement) -> bool {
    element.kind() == SyntaxKind::MATH_ENVIRONMENT
        || element.as_node().is_some_and(|node| {
            node.descendants()
                .any(|descendant| descendant.kind() == SyntaxKind::MATH_ENVIRONMENT)
        })
}

fn render_mixed_delimited_display(
    elems: &[SyntaxElement],
    opts: &MathFormatOptions,
) -> Option<String> {
    if elems.iter().any(contains_comment) {
        return None;
    }
    let environments: Vec<usize> = elems
        .iter()
        .enumerate()
        .filter_map(|(index, element)| {
            (element.kind() == SyntaxKind::MATH_ENVIRONMENT).then_some(index)
        })
        .collect();
    if environments.is_empty() {
        return None;
    }

    let (open, close) = enclosing_delimiters(elems, &environments)?;
    let prefix = render_inline(&elems[..=open]).trim().to_string();
    let suffix = render_inline_seeded(&elems[close..], Some(AtomClass::Close))
        .trim()
        .to_string();
    let body = delimited_body_doc(&elems[open + 1..close], opts)?;
    let doc = Doc::group(Doc::concat([
        Doc::text(prefix),
        Doc::indent(Doc::concat([Doc::SoftLine, body])),
        Doc::SoftLine,
        Doc::text(suffix),
    ]));

    Some(Printer::new(opts.line_width, INDENT.len()).print(&doc, opts.math_indent))
}

fn contains_comment(element: &SyntaxElement) -> bool {
    element.kind() == SyntaxKind::MATH_COMMENT
        || element.as_node().is_some_and(|node| {
            node.descendants_with_tokens()
                .filter_map(|descendant| descendant.into_token())
                .any(|token| token.kind() == SyntaxKind::MATH_COMMENT)
        })
}

fn enclosing_delimiters(elems: &[SyntaxElement], environments: &[usize]) -> Option<(usize, usize)> {
    for open in 0..elems.len() {
        if elems[open].kind() != SyntaxKind::MATH_OPEN {
            continue;
        }
        let mut delimiters = vec![element_text(&elems[open])?];
        for (close, element) in elems.iter().enumerate().skip(open + 1) {
            match element.kind() {
                SyntaxKind::MATH_OPEN => delimiters.push(element_text(element)?),
                SyntaxKind::MATH_CLOSE => {
                    let opening = delimiters.pop()?;
                    if !delimiters_match(opening, element_text(element)?) {
                        break;
                    }
                    if delimiters.is_empty() {
                        if environments
                            .iter()
                            .all(|environment| open < *environment && *environment < close)
                        {
                            return Some((open, close));
                        }
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn element_text(element: &SyntaxElement) -> Option<&str> {
    element.as_token().map(|token| token.text())
}

fn delimiters_match(open: &str, close: &str) -> bool {
    matches!((open, close), ("(", ")") | ("[", "]"))
}

fn render_top_level_mixed_environment(
    elems: &[SyntaxElement],
    opts: &MathFormatOptions,
) -> Option<String> {
    if elems.iter().any(contains_comment) || !ordinary_delimiters_balanced(elems) {
        return None;
    }
    let doc = mixed_segment_doc(elems, opts)?;
    Some(Printer::new(opts.line_width, INDENT.len()).print(&doc, opts.math_indent))
}

fn ordinary_delimiters_balanced(elems: &[SyntaxElement]) -> bool {
    let mut openings = Vec::new();
    for element in elems {
        match element.kind() {
            SyntaxKind::MATH_OPEN => {
                let Some(text) = element_text(element) else {
                    return false;
                };
                openings.push(text);
            }
            SyntaxKind::MATH_CLOSE => {
                let Some(opening) = openings.pop() else {
                    return false;
                };
                let Some(closing) = element_text(element) else {
                    return false;
                };
                if !delimiters_match(opening, closing) {
                    return false;
                }
            }
            _ => {}
        }
    }
    openings.is_empty()
}

fn delimited_body_doc(body: &[SyntaxElement], opts: &MathFormatOptions) -> Option<Doc> {
    let mut segments = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;

    for (index, element) in body.iter().enumerate() {
        match element.kind() {
            SyntaxKind::MATH_OPEN => depth += 1,
            SyntaxKind::MATH_CLOSE => depth = depth.saturating_sub(1),
            SyntaxKind::MATH_PUNCT if depth == 0 => {
                segments.push(Doc::concat([
                    mixed_segment_doc(&body[start..index], opts)?,
                    Doc::text(element.to_string()),
                ]));
                start = index + 1;
            }
            _ => {}
        }
    }
    segments.push(mixed_segment_doc(&body[start..], opts)?);
    Some(Doc::join(Doc::Line, segments))
}

fn mixed_segment_doc(segment: &[SyntaxElement], opts: &MathFormatOptions) -> Option<Doc> {
    if segment.iter().any(contains_unsafe_mixed_trivia) {
        return None;
    }
    if segment.iter().any(|element| {
        element.kind() != SyntaxKind::MATH_ENVIRONMENT && contains_environment(element)
    }) {
        return None;
    }
    let environment_indices: Vec<usize> = segment
        .iter()
        .enumerate()
        .filter_map(|(index, element)| {
            (element.kind() == SyntaxKind::MATH_ENVIRONMENT).then_some(index)
        })
        .collect();
    match environment_indices.as_slice() {
        [] => Some(Doc::text(render_inline(segment).trim().to_string())),
        [environment_index] => {
            let before = &segment[..*environment_index];
            let after = &segment[*environment_index + 1..];
            let prefix = render_before_operand(before);
            let prefix_width = prefix.chars().count();
            let environment = segment[*environment_index].as_node()?;
            let environment_doc = Doc::join(
                Doc::HardLine,
                render_environment_lines(environment, 0, opts)
                    .into_iter()
                    .map(Doc::text),
            );
            let mut suffix = render_inline_seeded(after, Some(AtomClass::Close))
                .trim()
                .to_string();
            if !suffix.is_empty() && needs_space_after_environment(after) {
                suffix.insert(0, ' ');
            }
            Some(Doc::concat([
                Doc::text(prefix),
                Doc::align(prefix_width, environment_doc),
                Doc::text(suffix),
            ]))
        }
        _ => None,
    }
}

fn contains_unsafe_mixed_trivia(element: &SyntaxElement) -> bool {
    if element.kind() == SyntaxKind::MATH_ENVIRONMENT {
        return false;
    }
    if matches!(
        element.kind(),
        SyntaxKind::MATH_COMMENT | SyntaxKind::MATH_LINE_BREAK
    ) {
        return true;
    }
    element.as_node().is_some_and(|node| {
        node.descendants_with_tokens()
            .filter_map(|descendant| descendant.into_token())
            .any(|token| {
                matches!(
                    token.kind(),
                    SyntaxKind::MATH_COMMENT | SyntaxKind::MATH_LINE_BREAK
                )
            })
    })
}

fn render_before_operand(before: &[SyntaxElement]) -> String {
    let mut tokens = flatten_tokens(before);
    tokens.push(FlatToken::Token(SyntaxKind::MATH_TEXT, "X".to_string()));
    let rendered = collapse_spaces(&space_operators(&tokens, None));
    rendered
        .strip_suffix('X')
        .expect("synthetic trailing operand must survive spacing")
        .trim_start()
        .to_string()
}

fn needs_space_after_environment(after: &[SyntaxElement]) -> bool {
    let had_space = after.first().is_some_and(is_layout_whitespace);
    let Some(element) = after.iter().find(|element| !is_layout_whitespace(element)) else {
        return false;
    };
    let Some(token) = element.as_token() else {
        return had_space;
    };
    if token.kind() == SyntaxKind::MATH_OPERATOR {
        return operators::is_spaced(operators::coerce(
            operators::classify_operator(token.text()),
            Some(AtomClass::Close),
        ));
    }
    if token.kind() == SyntaxKind::MATH_COMMAND {
        let name = token.text().strip_prefix('\\').unwrap_or(token.text());
        return operators::command_class(name)
            .map(|class| operators::is_spaced(operators::coerce(class, Some(AtomClass::Close))))
            .unwrap_or(had_space);
    }
    had_space
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
    let extra = relation_chain_alignment(&rows);
    for (idx, row) in rows.iter().enumerate() {
        if row.is_blank() {
            continue;
        }
        let ei = extra[idx];
        let pad = " ".repeat(ei);
        let budget = line_width.saturating_sub(indent.chars().count() + ei);
        let physical = linebreak::break_free_row(&row.elems, budget);
        let last = physical.len() - 1;
        for (i, content) in physical.into_iter().enumerate() {
            let content = if i == last {
                with_break(content, row.has_break)
            } else {
                content
            };
            lines.push(format!("{indent}{pad}{content}"));
        }
    }
}

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

fn has_top_level_align(elems: &[SyntaxElement]) -> bool {
    elems.iter().any(|el| el.kind() == SyntaxKind::MATH_ALIGN)
}

fn render_environment_lines(
    env: &SyntaxNode,
    depth: usize,
    opts: &MathFormatOptions,
) -> Vec<String> {
    let Some(parts) = EnvParts::of(env) else {
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

fn group_text(children: &[SyntaxElement], idx: Option<usize>) -> String {
    idx.and_then(|i| children[i].as_node())
        .map(|n| n.text().to_string())
        .unwrap_or_default()
}

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

struct Row {
    elems: Vec<SyntaxElement>,
    has_break: bool,
}

impl Row {
    fn is_blank(&self) -> bool {
        !self.has_break && self.elems.iter().all(is_layout_whitespace)
    }

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

enum FlatToken {
    Token(SyntaxKind, String),
    ScriptStart,
    ScriptEnd,
}

impl FlatToken {
    fn token(&self) -> Option<(SyntaxKind, &str)> {
        match self {
            Self::Token(kind, text) => Some((*kind, text)),
            Self::ScriptStart | Self::ScriptEnd => None,
        }
    }
}

fn flatten_tokens(elems: &[SyntaxElement]) -> Vec<FlatToken> {
    let mut out = Vec::new();
    for el in elems {
        flatten_element(el, &mut out);
    }
    out
}

fn flatten_element(element: &SyntaxElement, out: &mut Vec<FlatToken>) {
    match element {
        NodeOrToken::Token(token) => {
            out.push(FlatToken::Token(token.kind(), token.text().to_string()))
        }
        NodeOrToken::Node(node) => {
            let is_script = matches!(
                node.kind(),
                SyntaxKind::MATH_SUBSCRIPT | SyntaxKind::MATH_SUPERSCRIPT
            );
            if is_script {
                out.push(FlatToken::ScriptStart);
            }
            for child in node.children_with_tokens() {
                flatten_element(&child, out);
            }
            if is_script {
                out.push(FlatToken::ScriptEnd);
            }
        }
    }
}

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

fn space_operators(toks: &[FlatToken], seed: Option<AtomClass>) -> String {
    let mut out = String::new();
    let mut prev_class: Option<AtomClass> = seed;
    let mut prev_demand = Demand::Start;
    let mut pending_space = false;
    let mut group_stack: Vec<bool> = Vec::new();
    let mut prev_sig_is_text_cmd = false;
    let mut star_modifier_pending = false;
    let mut colon_head = false;
    let mut script_stack = Vec::new();

    let mut i = 0;
    while i < toks.len() {
        match &toks[i] {
            FlatToken::ScriptStart => {
                script_stack.push((prev_class, prev_demand, group_stack.len()));
                pending_space = false;
                prev_class = Some(AtomClass::Open);
                prev_demand = Demand::TightOp;
                prev_sig_is_text_cmd = false;
                star_modifier_pending = false;
                colon_head = false;
                i += 1;
                continue;
            }
            FlatToken::ScriptEnd => {
                if let Some((class, demand, group_depth)) = script_stack.pop() {
                    prev_class = class;
                    prev_demand = demand;
                    group_stack.truncate(group_depth);
                }
                pending_space = false;
                prev_sig_is_text_cmd = false;
                star_modifier_pending = false;
                colon_head = false;
                i += 1;
                continue;
            }
            FlatToken::Token(_, _) => {}
        }
        let (kind, text) = toks[i]
            .token()
            .expect("script boundaries are handled before math tokens");
        match kind {
            SyntaxKind::MATH_SPACE | SyntaxKind::MATH_NEWLINE => {
                pending_space = true;
                i += 1;
            }
            SyntaxKind::MATH_TEXT
                if operators::is_definition_colon(
                    text,
                    toks.get(i + 1).and_then(FlatToken::token),
                ) =>
            {
                colon_head = true;
                prev_sig_is_text_cmd = false;
                star_modifier_pending = false;
                i += 1;
            }
            SyntaxKind::MATH_OPERATOR => {
                let mut run = String::new();
                while let Some((SyntaxKind::MATH_OPERATOR, text)) =
                    toks.get(i).and_then(FlatToken::token)
                {
                    run.push_str(text);
                    i += 1;
                }
                for (n, atom) in operators::split_operator_atoms(&run)
                    .into_iter()
                    .enumerate()
                {
                    let atom = if n == 0 && colon_head {
                        format!(":{atom}")
                    } else {
                        atom.to_string()
                    };
                    let is_modifier = n == 0 && atom == "*" && star_modifier_pending;
                    let class = if is_modifier {
                        AtomClass::Ord
                    } else {
                        operators::coerce(operators::classify_operator(&atom), prev_class)
                    };
                    let demand = if is_modifier {
                        Demand::TightOp
                    } else if operators::is_spaced(class) {
                        Demand::SpacedOp
                    } else {
                        Demand::TightOp
                    };
                    emit_atom(&mut out, prev_demand, demand, pending_space, &atom);
                    pending_space = false; // only the first atom sees the run's leading space
                    prev_demand = demand;
                    prev_class = Some(class);
                }
                colon_head = false;
                prev_sig_is_text_cmd = false;
                star_modifier_pending = false;
            }
            SyntaxKind::MATH_COMMAND => {
                let name = text.strip_prefix('\\').unwrap_or(text);
                let demand = match operators::command_class(name) {
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
                prev_sig_is_text_cmd = operators::is_text_mode_command(name);
                star_modifier_pending = operators::takes_star_modifier(name);
                i += 1;
            }
            SyntaxKind::MATH_COMMENT => {
                emit_atom(&mut out, prev_demand, Demand::Plain, pending_space, text);
                pending_space = false;
                prev_demand = Demand::Plain;
                prev_sig_is_text_cmd = false;
                star_modifier_pending = false;
                i += 1;
            }
            SyntaxKind::MATH_CARET | SyntaxKind::MATH_UNDERSCORE => {
                emit_atom(&mut out, prev_demand, Demand::TightOp, pending_space, text);
                pending_space = false;
                prev_demand = Demand::TightOp;
                prev_class = Some(AtomClass::Open);
                prev_sig_is_text_cmd = false;
                star_modifier_pending = false;
                i += 1;
            }
            SyntaxKind::MATH_GROUP_OPEN => {
                let parent_text = group_stack.last().copied().unwrap_or(false);
                let is_text = prev_sig_is_text_cmd || parent_text;
                group_stack.push(is_text);
                emit_atom(&mut out, prev_demand, Demand::Plain, pending_space, text);
                pending_space = false;
                prev_demand = if is_text {
                    Demand::Plain
                } else {
                    Demand::TightOp
                };
                prev_class = Some(AtomClass::Open);
                prev_sig_is_text_cmd = false;
                star_modifier_pending = false;
                i += 1;
            }
            SyntaxKind::MATH_GROUP_CLOSE => {
                let is_text = group_stack.pop().unwrap_or(false);
                let cur = if is_text {
                    Demand::Plain
                } else {
                    Demand::TightOp
                };
                emit_atom(&mut out, prev_demand, cur, pending_space, text);
                pending_space = false;
                prev_demand = Demand::Plain;
                prev_class = Some(AtomClass::Close);
                prev_sig_is_text_cmd = false;
                star_modifier_pending = false;
                i += 1;
            }
            _ => {
                emit_atom(&mut out, prev_demand, Demand::Plain, pending_space, text);
                pending_space = false;
                prev_demand = Demand::Plain;
                prev_class = atom_prev_class(kind, text);
                prev_sig_is_text_cmd = false;
                star_modifier_pending = false;
                i += 1;
            }
        }
    }
    if colon_head {
        emit_atom(&mut out, prev_demand, Demand::Plain, pending_space, ":");
    }
    out
}

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

fn atom_prev_class(kind: SyntaxKind, _text: &str) -> Option<AtomClass> {
    if let Some(class) = operators::delimiter_class(kind) {
        return Some(class);
    }
    let class = match kind {
        SyntaxKind::MATH_TEXT => AtomClass::Ord,
        SyntaxKind::MATH_GROUP_OPEN => AtomClass::Open,
        SyntaxKind::MATH_GROUP_CLOSE => AtomClass::Close,
        SyntaxKind::MATH_CARET | SyntaxKind::MATH_UNDERSCORE | SyntaxKind::MATH_ALIGN => {
            AtomClass::Open
        }
        SyntaxKind::MATH_LINE_BREAK => return None,
        _ => AtomClass::Ord,
    };
    Some(class)
}

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
