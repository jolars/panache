//! Rendering pipeline for the math content formatter.
//!
//! Operates on a freshly re-parsed `MATH_CONTENT` tree (see the parent module).
//! The transforms are structural and each is independently idempotent — see
//! `STYLE.md` for the rules and the alignment idempotency argument. The short
//! version: every cell is *trimmed before its width is measured* and padding is
//! *trailing only*, so a second pass measures the same content widths and emits
//! identical bytes.

use rowan::{GreenNodeBuilder, NodeOrToken};

use super::ir::Ir;
use super::operators::{self, AtomClass};
use super::printer::Printer;
use super::{MathContext, MathFormatOptions, linebreak, lower};
use crate::syntax::{
    AstNode, MathContent, MathEnvironment, MathLineBreak, MathScripted, SyntaxElement, SyntaxKind,
    SyntaxNode, SyntaxToken,
};
use panache_parser::parser::math::MathParseOptions;
use panache_parser::semantic::math::{ArgKind, ArgumentDomain, SignatureScope, match_arg_slot};

const INDENT: &str = "  ";

/// Entry point: dispatch on context. Returns delimiter-free content.
pub(super) fn render(tree: &SyntaxNode, opts: &MathFormatOptions) -> String {
    let top: Vec<SyntaxElement> = tree.children_with_tokens().collect();
    match opts.context {
        MathContext::Inline => render_inline_content(tree, &top, opts),
        MathContext::Display => render_display(tree, &top, opts),
        MathContext::EnvironmentBody => render_environment_body(tree, &top, opts),
    }
}

fn render_environment_body(
    tree: &SyntaxNode,
    elements: &[SyntaxElement],
    opts: &MathFormatOptions,
) -> String {
    if let Some(document) = MathContent::cast(tree.clone())
        .and_then(|content| lower::try_lower_delimited_environment(&content, opts))
    {
        let mut output = Printer::new(opts.line_width, INDENT.len()).print(&document, INDENT.len());
        if ends_in_math_newline(elements) {
            output.push('\n');
        }
        return output;
    }
    let has_comment = tree
        .descendants_with_tokens()
        .any(|element| element.kind() == SyntaxKind::MATH_COMMENT);
    let has_authored_break = tree
        .descendants_with_tokens()
        .any(|element| element.kind() == SyntaxKind::MATH_LINE_BREAK);
    let has_top_level_authored_break = elements.iter().any(|element| {
        element
            .as_node()
            .cloned()
            .and_then(MathLineBreak::cast)
            .and_then(|line_break| line_break.marker_token())
            .is_some_and(|marker| marker.text() == r"\\")
    });
    if !elements.iter().any(contains_environment)
        && ((has_comment && !has_authored_break)
            || (has_authored_break && (!has_comment || has_top_level_authored_break)))
        && !tree
            .descendants_with_tokens()
            .any(|element| element.kind() == SyntaxKind::MATH_ALIGN)
        && let Some(document) = MathContent::cast(tree.clone()).and_then(|content| {
            lower::try_lower_environment_content(&content, &opts.signature_scope)
        })
    {
        return trimmed_body(
            &Printer::new(opts.line_width, INDENT.len()),
            &document,
            INDENT.len(),
        );
    }

    render_body_lines(elements, 1, opts).join("\n")
}

/// Inline math shares its line with the host paragraph or table cell, so a
/// lowered body is printed flat: a newline here would end the paragraph or
/// split the cell across rows. The exception is a body carrying a `%` comment,
/// which runs to end of line and therefore keeps its hard breaks.
fn render_inline_content(
    tree: &SyntaxNode,
    elements: &[SyntaxElement],
    opts: &MathFormatOptions,
) -> String {
    let printer = Printer::new(opts.line_width, INDENT.len());
    let keeps_breaks = tree
        .descendants_with_tokens()
        .any(|element| element.kind() == SyntaxKind::MATH_COMMENT);
    let print = |document: &Ir| {
        if keeps_breaks {
            printer.print(document, 0)
        } else {
            printer.print_flat(document).trim().to_string()
        }
    };

    if let Some(document) = MathContent::cast(tree.clone())
        .and_then(|content| lower::try_lower_delimited_environment(&content, opts))
    {
        return print(&document);
    }
    if elements.iter().any(contains_environment) {
        let semantic = expand_word_elements(elements);
        if let Some(document) = mixed_segment_doc(&semantic, opts, false) {
            return print(&document);
        }
    }
    if let Some(document) = MathContent::cast(tree.clone())
        .and_then(|content| lower::try_lower_content(&content, &opts.signature_scope))
    {
        print(&document)
    } else {
        render_inline(elements, &opts.signature_scope)
            .trim()
            .to_string()
    }
}

/// Print a delimited body without its own trailing break. A body that ends in
/// a comment or a `\\` row lowers to a trailing hard line so nothing follows it
/// on that line; the caller already puts the closing delimiter on a line of its
/// own, so keeping the break would leave a blank line between them.
fn trimmed_body(printer: &Printer, document: &Ir, indent: usize) -> String {
    printer.print(document, indent).trim_end().to_string()
}

fn render_display(tree: &SyntaxNode, top: &[SyntaxElement], opts: &MathFormatOptions) -> String {
    if let Some(document) = MathContent::cast(tree.clone())
        .and_then(|content| lower::try_lower_delimited_environment(&content, opts))
    {
        let mut output =
            Printer::new(opts.line_width, INDENT.len()).print(&document, opts.math_indent);
        if ends_in_math_newline(top) {
            output.push('\n');
        }
        return output;
    }
    let has_comment = tree
        .descendants_with_tokens()
        .any(|element| element.kind() == SyntaxKind::MATH_COMMENT);
    let has_authored_break = tree
        .descendants_with_tokens()
        .any(|element| element.kind() == SyntaxKind::MATH_LINE_BREAK);
    let authored_break_uses_legacy_alignment = has_authored_break && {
        let rows = split_logical_rows(top);
        let parse_opts = MathParseOptions {
            bookdown_equation_labels: opts.bookdown_equation_labels,
        };
        relation_chain_alignment(&rows, parse_opts, &opts.signature_scope)
            .into_iter()
            .any(|indent| indent > 0)
    };
    if !top.iter().any(contains_environment)
        && (has_comment || has_authored_break)
        && !authored_break_uses_legacy_alignment
        && let Some(document) = MathContent::cast(tree.clone())
            .and_then(|content| lower::try_lower_content(&content, &opts.signature_scope))
    {
        return trimmed_body(
            &Printer::new(opts.line_width, INDENT.len()),
            &document,
            opts.math_indent,
        );
    }

    if has_mixed_environment_content(top) {
        let semantic = expand_word_elements(top);
        return render_mixed_delimited_display(&semantic, opts)
            .or_else(|| render_top_level_mixed_environment(&semantic, opts))
            .unwrap_or_else(|| {
                let content: String = top.iter().map(ToString::to_string).collect();
                content.trim_matches(['\r', '\n']).to_string()
            });
    }

    let mut lines: Vec<String> = Vec::new();
    let mut pending: Vec<SyntaxElement> = Vec::new();
    let flat_indent = " ".repeat(opts.math_indent);
    let parse_opts = MathParseOptions {
        bookdown_equation_labels: opts.bookdown_equation_labels,
    };

    for el in top {
        if el.kind() == SyntaxKind::MATH_ENVIRONMENT {
            flush_free_rows(
                &pending,
                &flat_indent,
                opts.line_width,
                parse_opts,
                &opts.signature_scope,
                &mut lines,
            );
            pending.clear();
            if let Some(node) = el.as_node() {
                lines.extend(render_environment_lines(node, 0, opts));
            }
        } else {
            pending.push(el.clone());
        }
    }
    flush_free_rows(
        &pending,
        &flat_indent,
        opts.line_width,
        parse_opts,
        &opts.signature_scope,
        &mut lines,
    );
    lines.join("\n")
}

fn ends_in_math_newline(elements: &[SyntaxElement]) -> bool {
    elements
        .last()
        .is_some_and(|element| element.kind() == SyntaxKind::MATH_NEWLINE)
}

/// Split each top-level multi-atom `MATH_WORD` token into one token per
/// [`operators::word_atoms`] atom, so the delimiter and segment scans below
/// can classify elements one atom at a time. Nodes are opaque operands to
/// those scans and are kept as-is (rendering flattens their original tokens);
/// only the split tokens live in a small synthetic tree of their own.
fn expand_word_elements(elems: &[SyntaxElement]) -> Vec<SyntaxElement> {
    let mut out = Vec::with_capacity(elems.len());
    for element in elems {
        match element {
            NodeOrToken::Token(token)
                if token.kind() == SyntaxKind::MATH_WORD
                    && operators::word_atoms(token.text()).nth(1).is_some() =>
            {
                let mut builder = GreenNodeBuilder::new();
                builder.start_node(SyntaxKind::MATH_CONTENT.into());
                for atom in operators::word_atoms(token.text()) {
                    builder.token(SyntaxKind::MATH_WORD.into(), atom.text);
                }
                builder.finish_node();
                out.extend(SyntaxNode::new_root(builder.finish()).children_with_tokens());
            }
            _ => out.push(element.clone()),
        }
    }
    out
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

/// An element that lays out as an environment block: a bare
/// `MATH_ENVIRONMENT`, or a `MATH_SCRIPTED` node with an environment base,
/// whose script elements glue onto the block's closing line
/// (`\end{pmatrix}^T`).
fn environment_block(element: &SyntaxElement) -> Option<(SyntaxNode, Vec<SyntaxElement>)> {
    let node = element.as_node()?;
    if node.kind() == SyntaxKind::MATH_ENVIRONMENT {
        return Some((node.clone(), Vec::new()));
    }
    let scripted = MathScripted::cast(node.clone())?;
    let base = scripted.base()?.into_node()?;
    if base.kind() != SyntaxKind::MATH_ENVIRONMENT {
        return None;
    }
    let scripts = node
        .children_with_tokens()
        .filter(|child| {
            matches!(
                child.kind(),
                SyntaxKind::MATH_SUBSCRIPT | SyntaxKind::MATH_SUPERSCRIPT
            )
        })
        .collect();
    Some((base, scripts))
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
        .filter_map(|(index, element)| environment_block(element).is_some().then_some(index))
        .collect();
    if environments.is_empty() {
        return None;
    }

    let (open, close) = enclosing_delimiters(elems, &environments)?;
    let prefix = render_inline(&elems[..=open], &opts.signature_scope)
        .trim()
        .to_string();
    let suffix = render_inline_seeded(
        &elems[close..],
        Some(AtomClass::Close),
        &opts.signature_scope,
    )
    .trim()
    .to_string();
    let body = delimited_body_doc(&elems[open + 1..close], opts)?;
    let doc = Ir::group(Ir::concat([
        Ir::text(prefix),
        Ir::indent(Ir::concat([Ir::SoftLine, body])),
        Ir::SoftLine,
        Ir::text(suffix),
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
        if element_word_class(&elems[open]) != Some(AtomClass::Open) {
            continue;
        }
        let mut delimiters = vec![element_text(&elems[open])?];
        for (close, element) in elems.iter().enumerate().skip(open + 1) {
            match element_word_class(element) {
                Some(AtomClass::Open) => delimiters.push(element_text(element)?),
                Some(AtomClass::Close) => {
                    let opening = delimiters.pop()?;
                    if !delimiters_match(&opening, &element_text(element)?) {
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

/// The token that carries an element's atom identity for the delimiter and
/// segment scans: the token itself, or the base token of a `MATH_SCRIPTED`
/// node — so a scripted closing delimiter (`)^2`) still closes its `(`.
/// Structured bases (groups, environments) return `None` and stay opaque.
fn element_atom_token(element: &SyntaxElement) -> Option<SyntaxToken> {
    match element {
        NodeOrToken::Token(token) => Some(token.clone()),
        NodeOrToken::Node(_) => operators::command_name_token(element).or_else(|| {
            let base = operators::scripted_base(element)?;
            match base {
                NodeOrToken::Token(token) => Some(token),
                NodeOrToken::Node(_) => operators::command_name_token(&base),
            }
        }),
    }
}

fn element_text(element: &SyntaxElement) -> Option<String> {
    element_atom_token(element).map(|token| token.text().to_string())
}

fn element_word_class(element: &SyntaxElement) -> Option<AtomClass> {
    let token = element_atom_token(element)?;
    if token.kind() != SyntaxKind::MATH_WORD {
        return None;
    }
    let mut atoms = operators::word_atoms(token.text());
    let atom = atoms.next()?;
    atoms.next().is_none().then_some(atom.class)
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
    let doc = mixed_segment_doc(elems, opts, true)?;
    Some(Printer::new(opts.line_width, INDENT.len()).print(&doc, opts.math_indent))
}

fn ordinary_delimiters_balanced(elems: &[SyntaxElement]) -> bool {
    let mut openings = Vec::new();
    for element in elems {
        match element_word_class(element) {
            Some(AtomClass::Open) => {
                let Some(text) = element_text(element) else {
                    return false;
                };
                openings.push(text);
            }
            Some(AtomClass::Close) => {
                let Some(opening) = openings.pop() else {
                    return false;
                };
                let Some(closing) = element_text(element) else {
                    return false;
                };
                if !delimiters_match(&opening, &closing) {
                    return false;
                }
            }
            _ => {}
        }
    }
    openings.is_empty()
}

fn delimited_body_doc(body: &[SyntaxElement], opts: &MathFormatOptions) -> Option<Ir> {
    let mut segments = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;

    for (index, element) in body.iter().enumerate() {
        match element_word_class(element) {
            Some(AtomClass::Open) => depth += 1,
            Some(AtomClass::Close) => depth = depth.saturating_sub(1),
            Some(AtomClass::Punct) if depth == 0 => {
                segments.push(Ir::concat([
                    mixed_segment_doc(&body[start..index], opts, true)?,
                    Ir::text(element.to_string()),
                ]));
                start = index + 1;
            }
            _ => {}
        }
    }
    segments.push(mixed_segment_doc(&body[start..], opts, true)?);
    Some(Ir::join(Ir::Line, segments))
}

fn mixed_segment_doc(
    segment: &[SyntaxElement],
    opts: &MathFormatOptions,
    hang_environment: bool,
) -> Option<Ir> {
    if segment.iter().any(contains_unsafe_mixed_trivia) {
        return None;
    }
    if segment
        .iter()
        .any(|element| environment_block(element).is_none() && contains_environment(element))
    {
        return None;
    }
    let environment_indices: Vec<usize> = segment
        .iter()
        .enumerate()
        .filter_map(|(index, element)| environment_block(element).is_some().then_some(index))
        .collect();
    match environment_indices.as_slice() {
        [] => Some(Ir::text(
            render_inline(segment, &opts.signature_scope)
                .trim()
                .to_string(),
        )),
        [environment_index] => {
            let before = &segment[..*environment_index];
            let after = &segment[*environment_index + 1..];
            let prefix = render_before_operand(before, &opts.signature_scope);
            let prefix_width = prefix.chars().count();
            let (environment, scripts) = environment_block(&segment[*environment_index])?;
            if !is_well_formed_environment(&environment) {
                return None;
            }
            let environment_doc = Ir::join(
                Ir::HardLine,
                render_environment_lines(&environment, 0, opts)
                    .into_iter()
                    .map(Ir::text),
            );
            let mut suffix =
                render_inline_seeded(after, Some(AtomClass::Close), &opts.signature_scope)
                    .trim()
                    .to_string();
            if !suffix.is_empty() && needs_space_after_environment(after) {
                suffix.insert(0, ' ');
            }
            if !scripts.is_empty() {
                // Scripts attach to the environment itself, so they glue
                // directly onto the `\end{...}` line before any other suffix.
                suffix = format!(
                    "{}{suffix}",
                    render_inline(&scripts, &opts.signature_scope).trim()
                );
            }
            let environment_doc = if hang_environment {
                Ir::align(prefix_width, environment_doc)
            } else {
                environment_doc
            };
            Some(Ir::concat([
                Ir::text(prefix),
                environment_doc,
                Ir::text(suffix),
            ]))
        }
        _ => None,
    }
}

fn is_well_formed_environment(environment: &SyntaxNode) -> bool {
    let Some(environment) = MathEnvironment::cast(environment.clone()) else {
        return false;
    };
    let (Some(begin), Some(end)) = (environment.begin(), environment.end()) else {
        return false;
    };
    let (Some(begin_name), Some(end_name)) = (begin.name(), end.name()) else {
        return false;
    };
    begin_name == end_name
        && begin.syntax().text().to_string() == format!(r"\begin{{{begin_name}}}")
        && end.syntax().text().to_string() == format!(r"\end{{{end_name}}}")
}

fn contains_unsafe_mixed_trivia(element: &SyntaxElement) -> bool {
    match environment_block(element) {
        // The environment interior is laid out row by row, where breaks and
        // comments are safe; only glued scripts must stay trivia-free.
        Some((_, scripts)) => scripts.iter().any(has_comment_or_line_break),
        None => has_comment_or_line_break(element),
    }
}

fn has_comment_or_line_break(element: &SyntaxElement) -> bool {
    fn is_marker(kind: SyntaxKind) -> bool {
        matches!(kind, SyntaxKind::MATH_COMMENT | SyntaxKind::MATH_LINE_BREAK)
    }
    if is_marker(element.kind()) {
        return true;
    }
    element.as_node().is_some_and(|node| {
        node.descendants_with_tokens()
            .any(|descendant| is_marker(descendant.kind()))
    })
}

fn render_before_operand(before: &[SyntaxElement], scope: &SignatureScope) -> String {
    let mut tokens = flatten_tokens(before, scope);
    tokens.push(FlatToken::Token(SyntaxKind::MATH_WORD, "X".to_string()));
    let rendered = space_operators(&tokens, None);
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
    // A scripted atom spaces by its base: `=_i` after `\end{...}` behaves
    // like `=`.
    let Some(token) = element_atom_token(element) else {
        return had_space;
    };
    if token.kind() == SyntaxKind::MATH_WORD {
        let Some(first) = operators::word_atoms(token.text()).next() else {
            return had_space;
        };
        return operators::is_spaced(operators::coerce(first.class, Some(AtomClass::Close)));
    }
    if matches!(
        token.kind(),
        SyntaxKind::MATH_CONTROL_WORD | SyntaxKind::MATH_CONTROL_SYMBOL
    ) {
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
    parse_opts: MathParseOptions,
    scope: &SignatureScope,
    lines: &mut Vec<String>,
) {
    let rows = split_logical_rows(elems);
    let extra = relation_chain_alignment(&rows, parse_opts, scope);
    for (idx, row) in rows.iter().enumerate() {
        if row.is_blank() {
            continue;
        }
        let ei = extra[idx];
        let pad = " ".repeat(ei);
        let budget = line_width.saturating_sub(indent.chars().count() + ei);
        let physical = linebreak::break_free_row(&row.elems, budget, parse_opts, scope);
        let last = physical.len() - 1;
        for (i, content) in physical.into_iter().enumerate() {
            let content = if i == last {
                with_break(content, row.break_text.as_deref())
            } else {
                content
            };
            lines.push(format!("{indent}{pad}{content}"));
        }
    }
}

fn relation_chain_alignment(
    rows: &[Row],
    parse_opts: MathParseOptions,
    scope: &SignatureScope,
) -> Vec<usize> {
    let mut extra = vec![0usize; rows.len()];
    let mut i = 0;
    while i < rows.len() {
        if rows[i].break_text.is_some() && !rows[i].is_blank() {
            let mut k = i;
            while rows[k].break_text.is_some()
                && k + 1 < rows.len()
                && !rows[k + 1].is_blank()
                && linebreak::begins_with_top_level_relation(&rows[k + 1].elems, parse_opts, scope)
            {
                k += 1;
            }
            if k > i && !rows[i..=k].iter().any(|r| has_top_level_align(&r.elems)) {
                for continuation in i + 1..=k {
                    extra[continuation] = linebreak::continuation_anchor_for(
                        &rows[i].elems,
                        &rows[continuation].elems,
                        parse_opts,
                        scope,
                    );
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
            &opts.signature_scope,
        )];
    };
    let indent = INDENT.repeat(depth);
    let mut lines = vec![format!("{indent}{}", parts.begin_line)];
    lines.extend(render_body_lines(&parts.body, depth + 1, opts));
    lines.push(format!("{indent}{}", parts.end_line));
    lines
}

pub(super) fn environment_document(
    environment: &SyntaxNode,
    opts: &MathFormatOptions,
) -> Option<Ir> {
    if !is_well_formed_environment(environment) {
        return None;
    }
    Some(Ir::join(
        Ir::HardLine,
        render_environment_lines(environment, 0, opts)
            .into_iter()
            .map(Ir::text),
    ))
}

struct EnvParts {
    begin_line: String,
    end_line: String,
    body: Vec<SyntaxElement>,
}

impl EnvParts {
    fn of(env: &SyntaxNode) -> Option<Self> {
        let environment = MathEnvironment::cast(env.clone())?;
        let begin_line = environment.begin()?.syntax().text().to_string();
        let end_line = environment.end()?.syntax().text().to_string();
        let body = environment
            .body()?
            .syntax()
            .children_with_tokens()
            .collect();
        Some(Self {
            begin_line,
            end_line,
            body,
        })
    }
}

enum BodyItem {
    /// A nested environment rendered on its own line(s), already indented.
    Block(Vec<String>),
    /// A normal table row: trimmed cells split on top-level `&`.
    Row {
        cells: Vec<BodyCell>,
        break_text: Option<String>,
    },
}

enum BodyCell {
    Flat(String),
    Document(Ir),
}

impl BodyCell {
    fn first_line_width(&self, printer: &Printer) -> usize {
        match self {
            Self::Flat(text) => text.chars().count(),
            Self::Document(document) => printer
                .print(document, 0)
                .lines()
                .next()
                .unwrap_or_default()
                .chars()
                .count(),
        }
    }

    fn starts_relation(&self) -> bool {
        let first_line = match self {
            Self::Flat(text) => text.as_str(),
            Self::Document(document) => {
                let rendered = Printer::new(usize::MAX / 2, INDENT.len()).print(document, 0);
                return rendered.lines().next().is_some_and(starts_relation);
            }
        };
        starts_relation(first_line)
    }
}

fn starts_relation(text: &str) -> bool {
    let text = text.trim_start();
    if text.starts_with(['=', '<', '>']) || text.starts_with(":=") {
        return true;
    }
    let Some(command) = text.strip_prefix('\\') else {
        return false;
    };
    let name = command
        .chars()
        .take_while(|character| character.is_ascii_alphabetic())
        .collect::<String>();
    operators::command_class(&name) == Some(AtomClass::Rel)
}

fn render_body_lines(
    body: &[SyntaxElement],
    depth: usize,
    opts: &MathFormatOptions,
) -> Vec<String> {
    let indent = INDENT.repeat(depth);
    let printer = Printer::new(opts.line_width, INDENT.len());
    let mut items: Vec<BodyItem> = Vec::new();
    let has_authored_break = body
        .iter()
        .any(|element| element.kind() == SyntaxKind::MATH_LINE_BREAK);

    for row in split_rows(body) {
        if row.is_blank() {
            continue;
        }
        if let Some(env) = row.single_environment() {
            items.push(BodyItem::Block(render_environment_lines(&env, depth, opts)));
        } else {
            let split = split_cells(&row.elems);
            let cells = split
                .iter()
                .enumerate()
                .map(|(column, cell)| {
                    if cell.iter().any(contains_nested_comment) {
                        lower_grid_cell(cell, &opts.signature_scope, column == 0)
                            .map(BodyCell::Document)
                            .unwrap_or_else(|| {
                                BodyCell::Flat(
                                    render_inline(cell, &opts.signature_scope)
                                        .trim()
                                        .to_string(),
                                )
                            })
                    } else if has_authored_break {
                        lower::try_lower_elements(cell.clone(), &opts.signature_scope)
                            .map(BodyCell::Document)
                            .unwrap_or_else(|| {
                                BodyCell::Flat(
                                    render_inline(cell, &opts.signature_scope)
                                        .trim()
                                        .to_string(),
                                )
                            })
                    } else {
                        BodyCell::Flat(
                            render_inline(cell, &opts.signature_scope)
                                .trim()
                                .to_string(),
                        )
                    }
                })
                .collect();
            items.push(BodyItem::Row {
                cells,
                break_text: row.break_text,
            });
        }
    }

    let tight_grid = items.iter().any(|item| match item {
        BodyItem::Row { cells, .. } => {
            let last = cells.len().saturating_sub(1);
            cells
                .iter()
                .take(last)
                .any(|cell| matches!(cell, BodyCell::Document(document) if document.contains_forced_break()))
        }
        BodyItem::Block(_) => false,
    });
    let widths = column_widths(&items, &printer);
    let mut out: Vec<String> = Vec::new();
    for item in items {
        match item {
            BodyItem::Block(lines) => out.extend(lines),
            BodyItem::Row { cells, break_text } => {
                if tight_grid {
                    let document = join_tight_cell_documents(&cells, break_text.as_deref());
                    out.push(printer.print(&document, indent.len()));
                } else if cells
                    .iter()
                    .any(|cell| matches!(cell, BodyCell::Document(_)))
                {
                    let document = join_cell_documents(&cells, &widths, break_text.as_deref());
                    out.push(printer.print(&document, indent.len()));
                } else {
                    let cells = cells
                        .iter()
                        .map(|cell| match cell {
                            BodyCell::Flat(text) => text.clone(),
                            BodyCell::Document(_) => unreachable!("handled typed row"),
                        })
                        .collect::<Vec<_>>();
                    let line = join_cells(&cells, &widths, break_text.is_some());
                    out.push(format!(
                        "{indent}{}",
                        with_break(line, break_text.as_deref())
                    ));
                }
            }
        }
    }
    out
}

/// Badness suppresses grid padding and separator spaces for the whole
/// environment when a non-final cell is multiline. An ordinary first-cell
/// group also hangs one column beyond the usual bracket indent.
fn join_tight_cell_documents(cells: &[BodyCell], break_text: Option<&str>) -> Ir {
    let mut documents = Vec::new();
    for (column, cell) in cells.iter().enumerate() {
        if column > 0 {
            documents.push(Ir::text("&"));
            if cell.starts_relation() {
                documents.push(Ir::text(" "));
            }
        }
        let document = match cell {
            BodyCell::Flat(text) => Ir::verbatim(text.clone()),
            BodyCell::Document(document) => document.clone(),
        };
        documents.push(document);
    }
    if let Some(marker) = break_text {
        documents.push(Ir::verbatim(marker));
    }
    Ir::concat(documents)
}

/// Per-column max width over **every** cell of multi-cell rows (including the
/// last column, so trailing `\\` can be aligned too). Single-cell rows have no
/// separator and don't participate. Computed on already-trimmed cells — this is
/// the idempotency engine.
fn column_widths(items: &[BodyItem], printer: &Printer) -> Vec<usize> {
    let mut widths: Vec<usize> = Vec::new();
    for item in items {
        if let BodyItem::Row { cells, .. } = item {
            if cells.len() < 2 {
                continue; // single cell ⇒ no column separator ⇒ nothing to pad
            }
            for (col, cell) in cells.iter().enumerate() {
                let w = cell.first_line_width(printer);
                if col >= widths.len() {
                    widths.resize(col + 1, 0);
                }
                widths[col] = widths[col].max(w);
            }
        }
    }
    widths
}

fn join_cell_documents(cells: &[BodyCell], widths: &[usize], break_text: Option<&str>) -> Ir {
    let last = cells.len().saturating_sub(1);
    let mut documents = Vec::new();
    let mut prefix_width = 0;
    for (column, cell) in cells.iter().enumerate() {
        if column > 0 {
            documents.push(Ir::text(" & "));
            prefix_width += 3;
        }
        let document = match cell {
            BodyCell::Flat(text) => Ir::verbatim(text.clone()),
            BodyCell::Document(document) => document.clone(),
        };
        documents.push(Ir::align(prefix_width, document));

        let cell_width = cell.first_line_width(&Printer::new(usize::MAX / 2, INDENT.len()));
        let pad = if column == last && break_text.is_none() {
            0
        } else {
            widths
                .get(column)
                .copied()
                .unwrap_or(0)
                .saturating_sub(cell_width)
        };
        if pad > 0 {
            documents.push(Ir::text(" ".repeat(pad)));
        }
        prefix_width += cell_width + pad;
    }
    if let Some(marker) = break_text {
        if !cells.is_empty() {
            documents.push(Ir::text(" "));
        }
        documents.push(Ir::verbatim(marker));
    }
    Ir::concat(documents)
}

pub(super) fn can_render_environment_comments(tree: &SyntaxNode, scope: &SignatureScope) -> bool {
    let elements = tree.children_with_tokens().collect::<Vec<_>>();
    can_render_environment_body_comments(&elements, scope)
}

fn can_render_environment_body_comments(
    elements: &[SyntaxElement],
    scope: &SignatureScope,
) -> bool {
    split_rows(elements).into_iter().all(|row| {
        if row.is_blank() {
            return true;
        }
        if let Some(environment) = row.single_environment() {
            let Some(parts) = EnvParts::of(&environment) else {
                return false;
            };
            return can_render_environment_body_comments(&parts.body, scope);
        }

        let cells = split_cells(&row.elems);
        cells.iter().enumerate().all(|(column, cell)| {
            !cell.iter().any(contains_nested_comment)
                || lower_grid_cell(cell, scope, column == 0).is_some()
        })
    })
}

fn lower_grid_cell(
    elements: &[SyntaxElement],
    scope: &SignatureScope,
    first_cell: bool,
) -> Option<Ir> {
    let comment_elements = elements
        .iter()
        .filter(|element| contains_nested_comment(element))
        .collect::<Vec<_>>();
    let [comment_element] = comment_elements.as_slice() else {
        return None;
    };
    let first_group = first_cell
        && comment_element
            .as_node()
            .is_some_and(|node| node.kind() == SyntaxKind::MATH_GROUP);
    let lower = if first_group {
        lower::try_lower_first_grid_cell
    } else {
        lower::try_lower_elements
    };
    let mut document = lower(elements.to_vec(), scope)?;
    let comment_document = lower(vec![(*comment_element).clone()], scope)?;
    let printer = Printer::new(usize::MAX / 2, INDENT.len());
    let first_line = printer.print(&document, 0).lines().next()?.to_string();
    let comment_first_line = printer
        .print(&comment_document, 0)
        .lines()
        .next()?
        .to_string();
    // The nested document's own hanging indent is relative to its containing
    // atom, so carry that atom's formatted cell offset into the row document.
    let offset = first_line[..first_line.rfind(&comment_first_line)?]
        .chars()
        .count();
    document = Ir::align(offset, document);
    if first_group {
        document = Ir::align(1, document);
    }
    Some(document)
}

fn contains_nested_comment(element: &SyntaxElement) -> bool {
    element.as_node().is_some_and(|node| {
        node.descendants_with_tokens()
            .filter_map(|descendant| descendant.into_token())
            .any(|token| token.kind() == SyntaxKind::MATH_COMMENT)
    })
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

fn with_break(line: String, break_text: Option<&str>) -> String {
    let Some(break_text) = break_text else {
        return line;
    };
    if line.is_empty() {
        break_text.to_string()
    } else {
        format!(r"{line} {break_text}")
    }
}

struct Row {
    elems: Vec<SyntaxElement>,
    break_text: Option<String>,
}

impl Row {
    fn is_blank(&self) -> bool {
        self.break_text.is_none() && self.elems.iter().all(is_layout_whitespace)
    }

    fn single_environment(&self) -> Option<SyntaxNode> {
        if self.break_text.is_some() {
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
                    break_text: Some(el.to_string()),
                });
                cur_has_comment = false;
            }
            SyntaxKind::MATH_NEWLINE if cur_has_comment => {
                rows.push(Row {
                    elems: std::mem::take(&mut cur),
                    break_text: None,
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
            break_text: None,
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
                    break_text: Some(el.to_string()),
                });
            }
            SyntaxKind::MATH_NEWLINE => {
                rows.push(Row {
                    elems: std::mem::take(&mut cur),
                    break_text: None,
                });
            }
            _ => cur.push(el.clone()),
        }
    }
    if !cur.is_empty() {
        rows.push(Row {
            elems: cur,
            break_text: None,
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
pub(super) fn render_inline(elems: &[SyntaxElement], scope: &SignatureScope) -> String {
    render_inline_seeded(elems, None, scope)
}

/// Like [`render_inline`] but seeds the preceding-atom class. The line-breaker
/// uses this for a continuation that *starts* with a binary operator: rendered
/// in isolation the `+`/`-` would coerce to a unary sign (`+b`), but seeding a
/// closing-operand class keeps it binary (`+ b`). `None` reproduces
/// [`render_inline`] exactly.
pub(super) fn render_inline_seeded(
    elems: &[SyntaxElement],
    seed: Option<AtomClass>,
    scope: &SignatureScope,
) -> String {
    let toks = flatten_tokens(elems, scope);
    space_operators(&toks, seed)
}

enum FlatToken {
    Token(SyntaxKind, String),
    /// A `\left`/`\right` delimiter whose role comes from its position, not
    /// from the delimiter glyph's ordinary lexical class.
    Delimiter(String, AtomClass),
    /// A starred-variant marker whose command-node ownership proves that it is
    /// a modifier rather than a binary operator.
    CommandStar(String),
    /// An argument whose domain is not proven math; preserve every interior byte.
    Opaque(String),
    ScriptStart,
    ScriptEnd,
}

impl FlatToken {
    fn token(&self) -> Option<(SyntaxKind, &str)> {
        match self {
            Self::Token(kind, text) => Some((*kind, text)),
            Self::CommandStar(text) => Some((SyntaxKind::MATH_WORD, text)),
            Self::Delimiter(_, _) | Self::Opaque(_) | Self::ScriptStart | Self::ScriptEnd => None,
        }
    }
}

fn flatten_tokens(elems: &[SyntaxElement], scope: &SignatureScope) -> Vec<FlatToken> {
    let mut out = Vec::new();
    for el in elems {
        flatten_element(el, scope, &mut out);
    }
    out
}

fn flatten_element(element: &SyntaxElement, scope: &SignatureScope, out: &mut Vec<FlatToken>) {
    match element {
        NodeOrToken::Token(token) => {
            out.push(FlatToken::Token(token.kind(), token.text().to_string()))
        }
        // A line break flattens to one composite token so the spacing pass
        // sees the same `\\` atom it did when the parser emitted it flat.
        NodeOrToken::Node(node) if node.kind() == SyntaxKind::MATH_LINE_BREAK => {
            out.push(FlatToken::Token(
                SyntaxKind::MATH_LINE_BREAK,
                node.text().to_string(),
            ));
        }
        NodeOrToken::Node(node) if node.kind() == SyntaxKind::MATH_COMMAND => {
            let name = operators::command_name_token(element)
                .and_then(|token| token.text().strip_prefix('\\').map(str::to_owned));
            let signature = name
                .as_deref()
                .and_then(|name| scope.command_signature(name));
            let mut slot = 0usize;
            for child in node.children_with_tokens() {
                let group_kind = match child.kind() {
                    SyntaxKind::MATH_GROUP => Some(ArgKind::Brace),
                    SyntaxKind::MATH_OPTIONAL => Some(ArgKind::Bracket),
                    _ => None,
                };
                if let Some(kind) = group_kind {
                    let domain = signature
                        .and_then(|signature| match_arg_slot(&signature.arguments, &mut slot, kind))
                        .map_or(ArgumentDomain::Unknown, |argument| argument.domain);
                    if domain == ArgumentDomain::Math {
                        flatten_element(&child, scope, out);
                    } else {
                        out.push(FlatToken::Opaque(child.to_string()));
                    }
                } else if child.as_token().is_some_and(|token| {
                    token.kind() == SyntaxKind::MATH_WORD && token.text() == "*"
                }) {
                    out.push(FlatToken::CommandStar("*".to_string()));
                } else {
                    flatten_element(&child, scope, out);
                }
            }
        }
        NodeOrToken::Node(node) if node.kind() == SyntaxKind::MATH_DELIMITED => {
            let mut delimiter_role = None;
            for child in node.children_with_tokens() {
                match &child {
                    NodeOrToken::Token(token)
                        if token.kind() == SyntaxKind::MATH_CONTROL_WORD
                            && token.text() == r"\left" =>
                    {
                        flatten_element(&child, scope, out);
                        delimiter_role = Some(AtomClass::Open);
                    }
                    NodeOrToken::Token(token)
                        if token.kind() == SyntaxKind::MATH_CONTROL_WORD
                            && token.text() == r"\right" =>
                    {
                        flatten_element(&child, scope, out);
                        delimiter_role = Some(AtomClass::Close);
                    }
                    NodeOrToken::Token(token)
                        if delimiter_role.is_some()
                            && matches!(
                                token.kind(),
                                SyntaxKind::MATH_SPACE
                                    | SyntaxKind::MATH_NEWLINE
                                    | SyntaxKind::MATH_COMMENT
                            ) =>
                    {
                        flatten_element(&child, scope, out);
                    }
                    NodeOrToken::Token(token) if delimiter_role.is_some() => {
                        out.push(FlatToken::Delimiter(
                            token.text().to_string(),
                            delimiter_role.take().expect("delimiter role is present"),
                        ));
                    }
                    NodeOrToken::Node(_) => {
                        delimiter_role = None;
                        flatten_element(&child, scope, out);
                    }
                    _ => flatten_element(&child, scope, out),
                }
            }
        }
        // Star-modifier handling lives in the `MATH_COMMAND` arm above; only a
        // command node owns a `*` that is a modifier rather than an operator.
        NodeOrToken::Node(node) => {
            let is_script = matches!(
                node.kind(),
                SyntaxKind::MATH_SUBSCRIPT | SyntaxKind::MATH_SUPERSCRIPT
            );
            if is_script {
                out.push(FlatToken::ScriptStart);
            }
            for child in node.children_with_tokens() {
                flatten_element(&child, scope, out);
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

/// Whether `atom` — the final atom of its word token — is a relation head the
/// parser's script split severed from its final scalar (`a:` + scripted `:=`,
/// `x<` + scripted `=`). The severed scalar is always the '='/`<`/`>`-led
/// `MATH_WORD` token that immediately follows.
fn severed_relation_head(atom: &operators::WordAtom, next: Option<&FlatToken>) -> bool {
    let Some((SyntaxKind::MATH_WORD, next_text)) = next.and_then(FlatToken::token) else {
        return false;
    };
    match atom.class {
        // A definition colon fuses only with a following definition relation.
        // A bare colon run is punctuation; `:=` is already a relation.
        AtomClass::Punct => {
            atom.text.chars().all(|character| character == ':')
                && operators::word_atoms(next_text)
                    .next()
                    .is_some_and(|next| next.class == AtomClass::Rel && next.text.ends_with('='))
        }
        AtomClass::Rel => next_text.starts_with(['=', '<', '>']),
        _ => false,
    }
}

fn space_operators(toks: &[FlatToken], seed: Option<AtomClass>) -> String {
    let mut out = String::new();
    let mut prev_class: Option<AtomClass> = seed;
    let mut prev_demand = Demand::Start;
    let mut pending_space = false;
    let mut group_stack: Vec<bool> = Vec::new();
    let mut prev_sig_is_text_cmd = false;
    let mut star_modifier_pending = false;
    // A deferred severed relation head (see [`severed_relation_head`]); it is
    // only ever set when the next token is a relation-led `MATH_WORD`, whose
    // first atom consumes it, so no other arm needs to flush it.
    let mut severed_head: Option<&str> = None;
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
                // Keep `prev_sig_is_text_cmd`: a text-mode command used as an
                // unbraced script argument (`x_\text{ max }`) still owns the
                // brace group that follows the script node.
                star_modifier_pending = false;
                i += 1;
                continue;
            }
            FlatToken::Delimiter(text, class) => {
                emit_atom(&mut out, prev_demand, Demand::Plain, pending_space, text);
                pending_space = false;
                prev_demand = Demand::Plain;
                prev_class = Some(*class);
                prev_sig_is_text_cmd = false;
                star_modifier_pending = false;
                i += 1;
                continue;
            }
            FlatToken::Opaque(text) => {
                emit_atom(&mut out, prev_demand, Demand::Plain, pending_space, text);
                pending_space = false;
                prev_demand = Demand::Plain;
                prev_class = Some(AtomClass::Close);
                prev_sig_is_text_cmd = false;
                star_modifier_pending = false;
                i += 1;
                continue;
            }
            FlatToken::Token(_, _) | FlatToken::CommandStar(_) => {}
        }
        let command_star = matches!(toks[i], FlatToken::CommandStar(_));
        let (kind, text) = toks[i]
            .token()
            .expect("script boundaries are handled before math tokens");
        match kind {
            SyntaxKind::MATH_SPACE | SyntaxKind::MATH_NEWLINE => {
                pending_space = true;
                i += 1;
            }
            SyntaxKind::MATH_WORD => {
                let mut atoms = operators::word_atoms(text).peekable();
                let mut first = true;
                while let Some(atom) = atoms.next() {
                    if atoms.peek().is_none() && severed_relation_head(&atom, toks.get(i + 1)) {
                        severed_head = Some(atom.text);
                        pending_space = false;
                        break;
                    }
                    let fused;
                    let (atom_text, atom_class) = match severed_head.take() {
                        Some(head) => {
                            fused = format!("{head}{}", atom.text);
                            (fused.as_str(), AtomClass::Rel)
                        }
                        None => (atom.text, atom.class),
                    };
                    let is_modifier =
                        first && atom.text == "*" && (star_modifier_pending || command_star);
                    let class = if is_modifier {
                        AtomClass::Ord
                    } else {
                        operators::coerce(atom_class, prev_class)
                    };
                    let demand = if is_modifier {
                        Demand::TightOp
                    } else if operators::is_spaced(class) {
                        Demand::SpacedOp
                    } else if atom_class == AtomClass::Bin {
                        Demand::TightOp
                    } else {
                        Demand::Plain
                    };
                    emit_atom(&mut out, prev_demand, demand, pending_space, atom_text);
                    pending_space = false;
                    prev_demand = demand;
                    prev_class = Some(class);
                    first = false;
                }
                i += 1;
                prev_sig_is_text_cmd = false;
                star_modifier_pending = false;
            }
            SyntaxKind::MATH_CONTROL_WORD | SyntaxKind::MATH_CONTROL_SYMBOL => {
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
            SyntaxKind::MATH_BRACKET_OPEN => {
                emit_atom(&mut out, prev_demand, Demand::TightOp, pending_space, text);
                pending_space = false;
                prev_demand = Demand::TightOp;
                prev_class = Some(AtomClass::Open);
                prev_sig_is_text_cmd = false;
                star_modifier_pending = false;
                i += 1;
            }
            SyntaxKind::MATH_BRACKET_CLOSE => {
                emit_atom(&mut out, prev_demand, Demand::TightOp, pending_space, text);
                pending_space = false;
                prev_demand = Demand::Plain;
                prev_class = Some(AtomClass::Close);
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
    debug_assert!(
        severed_head.is_none(),
        "a severed relation head is always consumed by the next word token"
    );
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
    let class = match kind {
        SyntaxKind::MATH_WORD => AtomClass::Ord,
        SyntaxKind::MATH_GROUP_OPEN => AtomClass::Open,
        SyntaxKind::MATH_GROUP_CLOSE => AtomClass::Close,
        SyntaxKind::MATH_BRACKET_OPEN => AtomClass::Open,
        SyntaxKind::MATH_BRACKET_CLOSE => AtomClass::Close,
        SyntaxKind::MATH_CARET | SyntaxKind::MATH_UNDERSCORE | SyntaxKind::MATH_ALIGN => {
            AtomClass::Open
        }
        SyntaxKind::MATH_LINE_BREAK => return None,
        _ => AtomClass::Ord,
    };
    Some(class)
}
