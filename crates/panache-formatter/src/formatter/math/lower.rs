//! Typed CST lowering for the Badness-parity math formatter.

use panache_parser::semantic::math::{
    ArgKind, ArgumentDomain, DelimiterRole, MathBreakPriority, MathClass, SemanticMathAtom,
    SignatureScope, match_arg_slot, math_atoms, math_command_info, semantic_math_atoms_in,
};
use rowan::TextRange;
use rowan::ast::AstNode;

use crate::syntax::{
    MathArgument, MathBegin, MathCommand, MathContent, MathDelimited, MathEnvironment, MathGroup,
    MathLineBreak, MathNameGroup, MathScript, MathScripted, SyntaxElement, SyntaxKind, SyntaxNode,
    SyntaxToken,
};

use super::ir::Ir;
use super::printer::Printer;
use super::{MathFormatOptions, render};
use crate::config::MathMode;

const INDENT_WIDTH: usize = 2;

/// Lower a supported math content body into the shared document IR.
///
/// Returning `None` keeps an unsupported shape on the formatter's verbatim
/// preservation boundary.
pub(super) fn try_lower_content(content: &MathContent, scope: &SignatureScope) -> Option<Ir> {
    let elements = content.elements().collect::<Vec<_>>();
    if elements
        .iter()
        .any(|element| element.kind() == SyntaxKind::MATH_EQUATION_LABEL)
    {
        let (body, label) = split_trailing_equation_label(elements)?;
        return Some(Ir::concat([
            lower_body(body, scope, Spacing::Normal, true, false)?,
            Ir::text(" "),
            Ir::verbatim(label),
        ]));
    }
    lower_body(elements, scope, Spacing::Normal, true, false)
}

/// Lower a free display body, including Panache's implicit alignment for
/// relation chains separated by authored `\\` row markers.
#[cfg(test)]
pub(super) fn try_lower_display_content(
    content: &MathContent,
    scope: &SignatureScope,
    line_width: usize,
) -> Option<Ir> {
    try_lower_display_content_with_mode(content, scope, line_width, MathMode::Reflow)
}

pub(super) fn try_lower_display_content_with_mode(
    content: &MathContent,
    scope: &SignatureScope,
    line_width: usize,
    mode: MathMode,
) -> Option<Ir> {
    let elements = content.elements().collect::<Vec<_>>();
    try_lower_display_elements_with_mode(elements, scope, line_width, mode)
}

fn try_lower_display_elements_with_mode(
    elements: Vec<SyntaxElement>,
    scope: &SignatureScope,
    line_width: usize,
    mode: MathMode,
) -> Option<Ir> {
    if elements
        .iter()
        .any(|element| element.kind() == SyntaxKind::MATH_EQUATION_LABEL)
    {
        return lower_display_equation_label(elements, scope, line_width, mode);
    }
    if mode == MathMode::Preserve {
        return lower_preserved_display_lines(elements, scope);
    }
    let semantic_atoms = semantic_math_atoms_in(elements.iter().cloned()).collect::<Vec<_>>();
    if elements
        .iter()
        .any(|element| element.kind() == SyntaxKind::MATH_LINE_BREAK)
    {
        lower_authored_breaks(
            elements,
            semantic_atoms,
            scope,
            Spacing::Normal,
            AuthoredBreakOptions {
                preserve_comment_context: true,
                environment_rows: false,
                align_authored_relations: true,
                display_width: (mode == MathMode::Reflow).then_some(line_width),
            },
        )
    } else if elements
        .iter()
        .any(|element| element.kind() == SyntaxKind::MATH_COMMENT)
    {
        lower_body_with_atoms(
            elements,
            semantic_atoms,
            scope,
            Spacing::Normal,
            true,
            false,
        )
    } else {
        let pieces = lower_pieces_with_atoms(
            &elements,
            &semantic_atoms,
            scope,
            Spacing::Normal,
            true,
            false,
        )?;
        Some(if mode == MathMode::Reflow {
            layout_display_pieces(&pieces, line_width, Spacing::Normal)
        } else {
            document_from_pieces(&pieces, Spacing::Normal)
        })
    }
}

fn lower_display_equation_label(
    elements: Vec<SyntaxElement>,
    scope: &SignatureScope,
    line_width: usize,
    mode: MathMode,
) -> Option<Ir> {
    let (body_elements, label) = split_trailing_equation_label(elements)?;
    let body = try_lower_display_elements_with_mode(
        body_elements,
        scope,
        line_width.saturating_sub(label.chars().count() + 1),
        mode,
    )?;
    Some(Ir::concat([body, Ir::text(" "), Ir::verbatim(label)]))
}

/// Normalize each authored display line independently while retaining the
/// source's top-level soft line boundaries. Operator roles still come from the
/// full semantic stream, so a binary operator that begins a continuation line
/// does not become unary merely because the line is preserved.
fn lower_preserved_display_lines(
    elements: Vec<SyntaxElement>,
    scope: &SignatureScope,
) -> Option<Ir> {
    let semantic_atoms = semantic_math_atoms_in(elements.iter().cloned()).collect::<Vec<_>>();
    let mut rows = Vec::new();
    let mut row = Vec::new();
    for element in elements {
        if element.kind() == SyntaxKind::MATH_NEWLINE {
            if row.iter().any(|element| !is_layout_trivia(element)) {
                rows.push(std::mem::take(&mut row));
            } else {
                row.clear();
            }
        } else {
            row.push(element);
        }
    }
    if row.iter().any(|element| !is_layout_trivia(element)) {
        rows.push(row);
    }

    let documents = rows
        .into_iter()
        .map(|row| {
            let row_atoms = semantic_atoms_for(&row, &semantic_atoms);
            if row
                .iter()
                .any(|element| element.kind() == SyntaxKind::MATH_LINE_BREAK)
            {
                lower_authored_breaks(
                    row,
                    row_atoms,
                    scope,
                    Spacing::Normal,
                    AuthoredBreakOptions {
                        preserve_comment_context: true,
                        environment_rows: false,
                        align_authored_relations: true,
                        display_width: None,
                    },
                )
            } else {
                lower_body_with_atoms(row, row_atoms, scope, Spacing::Normal, true, false)
            }
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Ir::join(Ir::HardLine, documents))
}

fn split_trailing_equation_label(
    elements: Vec<SyntaxElement>,
) -> Option<(Vec<SyntaxElement>, String)> {
    let mut labels = elements
        .iter()
        .enumerate()
        .filter(|(_, element)| element.kind() == SyntaxKind::MATH_EQUATION_LABEL);
    let (label_index, label) = labels.next()?;
    if labels.next().is_some()
        || elements[label_index + 1..]
            .iter()
            .any(|element| !is_layout_trivia(element))
        || elements[..label_index].iter().all(is_layout_trivia)
    {
        return None;
    }

    Some((elements[..label_index].to_vec(), label.to_string()))
}

/// Lower inline content containing one top-level environment.
pub(super) fn try_lower_inline_environment(
    elements: Vec<SyntaxElement>,
    opts: &MathFormatOptions,
) -> Option<Ir> {
    try_lower_environment_composition(elements, opts, 1)
}

pub(super) fn try_lower_environment_composition(
    elements: Vec<SyntaxElement>,
    opts: &MathFormatOptions,
    hanging_offset: usize,
) -> Option<Ir> {
    lower_environment_composition(elements, opts, hanging_offset, Spacing::Normal)
}

fn lower_environment_composition(
    elements: Vec<SyntaxElement>,
    opts: &MathFormatOptions,
    hanging_offset: usize,
    spacing: Spacing,
) -> Option<Ir> {
    let mut pieces = try_lower_environment_pieces(elements, opts)?;
    for piece in &mut pieces {
        if piece.multiline_tail_width.is_some() {
            piece.document = Ir::align(hanging_offset, piece.document.clone());
        }
    }
    Some(document_from_pieces(&pieces, spacing))
}

/// Lower a free display containing one top-level environment.
///
/// The environment is a semantic operand whose forced lines participate in
/// the same binary- and relation-led layout as every other typed atom. Its
/// closing-line width lets following atoms compose without flattening the
/// environment document.
pub(super) fn try_lower_display_environment(
    elements: Vec<SyntaxElement>,
    opts: &MathFormatOptions,
    line_width: usize,
) -> Option<Ir> {
    let pieces = try_lower_environment_pieces(elements, opts)?;
    if let Some(document) = layout_enclosed_environment(&pieces) {
        return Some(document);
    }
    if opts.mode == MathMode::Reflow
        && let Some(boundary) = expression_after_environment(&pieces)
    {
        // An environment's suffix stays attached, but the next expression
        // starts at the display indent with its own relation alignment.
        return Some(Ir::join(
            Ir::HardLine,
            [&pieces[..boundary], &pieces[boundary..]]
                .map(|chain| layout_display_pieces(chain, line_width, Spacing::Normal)),
        ));
    }
    Some(layout_display_pieces(&pieces, line_width, Spacing::Normal))
}

fn expression_after_environment(pieces: &[Piece]) -> Option<usize> {
    let environment = pieces.iter().position(|piece| {
        piece.multiline_tail_width.is_some() && piece.document.contains_forced_break()
    })?;
    pieces[environment + 1..]
        .iter()
        .position(|piece| {
            !piece.punctuation
                && piece.delimiter != Some(DelimiterRole::Close)
                && !matches!(
                    &piece.document,
                    Ir::Text(text) | Ir::Verbatim(text) if text.as_ref() == "."
                )
        })
        .map(|offset| environment + 1 + offset)
}

/// Make ordinary delimiters participate in the forced layout of an enclosed
/// environment. The semantic atom stream supplies delimiter and punctuation
/// boundaries even when several characters share one CST word token.
fn layout_enclosed_environment(pieces: &[Piece]) -> Option<Ir> {
    let environment = pieces
        .iter()
        .position(|piece| piece.multiline_tail_width.is_some())?;
    let mut openings = Vec::new();
    let mut enclosure = None;
    for (index, piece) in pieces.iter().enumerate() {
        match piece.delimiter {
            Some(DelimiterRole::Open) => openings.push(index),
            Some(DelimiterRole::Close) => {
                let open = openings.pop()?;
                if open < environment && environment < index {
                    enclosure = Some((open, index));
                }
            }
            Some(DelimiterRole::Fence) | None => {}
        }
    }
    let (open, close) = enclosure?;
    let mut segments = Vec::new();
    let mut start = open + 1;
    let mut depth = 0usize;
    for index in open + 1..close {
        match pieces[index].delimiter {
            Some(DelimiterRole::Open) => depth += 1,
            Some(DelimiterRole::Close) => depth = depth.saturating_sub(1),
            Some(DelimiterRole::Fence) | None => {}
        }
        if depth == 0 && pieces[index].punctuation {
            segments.push(document_from_pieces(
                &pieces[start..=index],
                Spacing::Normal,
            ));
            start = index + 1;
        }
    }
    if start < close {
        segments.push(document_from_pieces(&pieces[start..close], Spacing::Normal));
    }
    let body = Ir::join(Ir::HardLine, segments);
    Some(Ir::group(Ir::concat([
        document_from_pieces(&pieces[..=open], Spacing::Normal),
        Ir::indent(Ir::concat([Ir::SoftLine, body])),
        Ir::SoftLine,
        document_from_pieces(&pieces[close..], Spacing::Normal),
    ])))
}

fn try_lower_environment_pieces(
    elements: Vec<SyntaxElement>,
    opts: &MathFormatOptions,
) -> Option<Vec<Piece>> {
    let environment_indices = elements
        .iter()
        .enumerate()
        .filter_map(|(index, element)| display_environment(element).map(|_| index))
        .collect::<Vec<_>>();
    let [environment_index] = environment_indices.as_slice() else {
        return None;
    };
    let environment_element = &elements[*environment_index];
    let (environment, scripted) = display_environment(environment_element)?;
    let end = environment.end()?;
    let script_document = match scripted.as_ref() {
        Some(scripted) => {
            if !environment_scripted_is_supported(scripted, &environment, &opts.signature_scope) {
                return None;
            }
            Some(lower_script_suffix(
                scripted,
                &opts.signature_scope,
                true,
                false,
            )?)
        }
        None => None,
    };
    let script_width = script_document.as_ref().map_or(Some(0), Ir::flat_width)?;
    let end_width = end.syntax().text().to_string().chars().count() + script_width;
    let environment_document = Ir::concat([
        try_lower_environment_document(&environment, opts)?,
        script_document.unwrap_or(Ir::Nil),
    ]);
    let semantic_atoms = semantic_math_atoms_in(elements.iter().cloned()).collect::<Vec<_>>();
    let environment_atom = semantic_atoms.iter().copied().find(|atom| {
        atom.range.start() == environment_element.text_range().start()
            && atom.range.end() == environment_element.text_range().end()
    })?;

    let before = &elements[..*environment_index];
    let after = &elements[*environment_index + 1..];
    let before_atoms = semantic_atoms_for(before, &semantic_atoms);
    let mut pieces = lower_pieces_with_atoms(
        before,
        &before_atoms,
        &opts.signature_scope,
        Spacing::Normal,
        true,
        false,
    )?;
    pieces.push(Piece {
        role: Role::from(environment_atom.break_priority),
        delimiter: environment_atom.delimiter,
        assignment: false,
        definition: false,
        conditioning_relation: false,
        punctuation: false,
        display_separator: false,
        unary: environment_atom.coerced_unary || environment_atom.coerced_postfix,
        dimension_sign: environment_atom.attached_dimension_sign,
        authored_space_before: before_atoms
            .last()
            .is_some_and(|atom| atom.range.end() < environment_atom.range.start()),
        slash: false,
        control_word_operator: false,
        starts_control_word_letter: false,
        ends_control_word: false,
        multiline_tail_width: Some(end_width),
        document: environment_document,
    });
    let after_atoms = semantic_atoms_for(after, &semantic_atoms);
    let mut after_pieces = lower_pieces_with_atoms(
        after,
        &after_atoms,
        &opts.signature_scope,
        Spacing::Normal,
        true,
        false,
    )?;
    if let (Some(piece), Some(atom)) = (after_pieces.first_mut(), after_atoms.first()) {
        piece.authored_space_before = environment_atom.range.end() < atom.range.start();
    }
    pieces.extend(after_pieces);

    Some(pieces)
}

fn display_environment(element: &SyntaxElement) -> Option<(MathEnvironment, Option<MathScripted>)> {
    let node = element.as_node()?;
    if let Some(environment) = MathEnvironment::cast(node.clone()) {
        return Some((environment, None));
    }
    let scripted = MathScripted::cast(node.clone())?;
    let environment = scripted
        .base()?
        .into_node()
        .and_then(MathEnvironment::cast)?;
    Some((environment, Some(scripted)))
}

fn environment_scripted_is_supported(
    scripted: &MathScripted,
    environment: &MathEnvironment,
    scope: &SignatureScope,
) -> bool {
    let base_range = environment.syntax().text_range();
    scripted.syntax().children_with_tokens().all(|element| {
        element.text_range() == base_range
            || is_layout_trivia(&element)
            || element
                .into_node()
                .and_then(MathScript::cast)
                .is_some_and(|script| script_is_supported(&script, scope))
    })
}

/// Lower a closed paired delimiter whose body contains one well-formed environment.
///
/// This stays separate from ordinary atom lowering until environments become
/// first-class typed atom documents. The narrow shape lets the existing
/// environment-grid document compose without admitting unpunctuated multiple
/// environments or malformed delimiter bodies.
pub(super) fn try_lower_delimited_environment(
    content: &MathContent,
    opts: &MathFormatOptions,
) -> Option<Ir> {
    let mut top = content
        .elements()
        .filter(|element| !is_layout_trivia(element));
    let delimited = top.next()?.into_node().and_then(MathDelimited::cast)?;
    if top.next().is_some() {
        return None;
    }

    let left = delimited.left_token()?;
    let open = delimited.opening_delimiter()?;
    let body = delimited.body()?;
    let right = delimited.right_token()?;
    let close = delimited.closing_delimiter()?;
    let body_elements = body
        .elements()
        .filter(|element| !is_layout_trivia(element))
        .collect::<Vec<_>>();
    let body_document = match body_elements.as_slice() {
        [element] => {
            let environment = element.as_node().cloned().and_then(MathEnvironment::cast)?;
            try_lower_environment_document(&environment, opts)?
        }
        _ => render::mixed_delimited_environment_document(&body, opts)?,
    };
    let opening_width = left.text().chars().count() + open.text().chars().count();
    // Badness lays comment-pinned inline continuations out after the host `$`
    // opener, which the delimiter-free math entry point does not retain.
    let host_inline_offset = usize::from(opts.context == super::MathContext::Inline);
    Some(Ir::concat([
        Ir::verbatim(left.text()),
        Ir::verbatim(open.text()),
        Ir::text(" "),
        Ir::align(opening_width + 1 + host_inline_offset, body_document),
        Ir::text(" "),
        Ir::verbatim(right.text()),
        Ir::verbatim(close.text()),
    ]))
}

/// Lower an environment body, where Badness canonicalizes a space before each
/// authored row marker.
pub(super) fn try_lower_environment_content(
    content: &MathContent,
    scope: &SignatureScope,
) -> Option<Ir> {
    lower_body(
        content.elements().collect(),
        scope,
        Spacing::EnvironmentCell,
        true,
        true,
    )
}

/// Lower the bare body of a host raw environment through the same row and cell
/// path used by nested TeX environments.
pub(super) fn try_lower_environment_body(
    elements: &[SyntaxElement],
    opts: &MathFormatOptions,
) -> Option<Ir> {
    lower_environment_rows(elements, opts)
}

/// Lower a well-formed TeX environment, including its derived row and cell
/// layout, into the shared document IR.
pub(super) fn try_lower_environment_document(
    environment: &MathEnvironment,
    opts: &MathFormatOptions,
) -> Option<Ir> {
    let begin = environment.begin()?;
    let end = environment.end()?;
    let begin_name = begin.name()?;
    let end_name = end.name()?;
    if begin_name != end_name || end.syntax().text().to_string() != format!(r"\end{{{end_name}}}") {
        return None;
    }
    let begin = lower_environment_begin(&begin, &opts.signature_scope)?;

    let body = environment.body()?;
    let body = lower_environment_rows(
        &body.syntax().children_with_tokens().collect::<Vec<_>>(),
        opts,
    )?;
    Some(Ir::concat([
        begin,
        Ir::indent(Ir::concat([Ir::HardLine, body])),
        Ir::HardLine,
        Ir::verbatim(end.syntax().text().to_string()),
    ]))
}

fn lower_environment_begin(begin: &MathBegin, scope: &SignatureScope) -> Option<Ir> {
    let command = begin.command_token()?;
    let name_group = begin.name_group()?;
    let name = name_group.name()?;
    let shell = format!(r"\begin{{{name}}}");
    if command.text() != r"\begin"
        || name_group.syntax().text().to_string() != format!("{{{name}}}")
    {
        return None;
    }

    if !begin
        .syntax()
        .children_with_tokens()
        .all(|element| match element {
            SyntaxElement::Token(token) => matches!(
                token.kind(),
                SyntaxKind::MATH_CONTROL_WORD | SyntaxKind::MATH_SPACE | SyntaxKind::MATH_NEWLINE
            ),
            SyntaxElement::Node(node) => {
                MathNameGroup::cast(node.clone()).is_some() || MathArgument::cast(node).is_some()
            }
        })
    {
        return None;
    }

    let arguments = begin.attached_arguments().collect::<Vec<_>>();
    let Some(signature) = scope.environment_signature(&name) else {
        return arguments.is_empty().then(|| Ir::verbatim(shell));
    };
    let mut slot = 0;
    let mut header = shell;
    for argument in arguments {
        if !argument.is_closed() {
            return None;
        }
        let kind = match argument {
            MathArgument::Brace(_) => ArgKind::Brace,
            MathArgument::Bracket(_) => ArgKind::Bracket,
        };
        match_arg_slot(&signature.arguments, &mut slot, kind)?;
        header.push_str(&argument.syntax().text().to_string());
    }
    if signature.arguments[slot..]
        .iter()
        .any(|argument| argument.required)
    {
        return None;
    }
    Some(Ir::verbatim(header))
}

pub(super) fn contains_malformed_environment(tree: &SyntaxNode) -> bool {
    tree.descendants()
        .filter_map(MathEnvironment::cast)
        .any(|environment| {
            let (Some(begin), Some(end)) = (environment.begin(), environment.end()) else {
                return true;
            };
            let (Some(begin_name), Some(end_name)) = (begin.name(), end.name()) else {
                return true;
            };
            let begin_shell_is_valid = begin
                .command_token()
                .is_some_and(|token| token.text() == r"\begin")
                && begin.name_group().is_some_and(|group| {
                    group.syntax().text().to_string() == format!("{{{begin_name}}}")
                });
            begin_name != end_name
                || !begin_shell_is_valid
                || end.syntax().text().to_string() != format!(r"\end{{{end_name}}}")
        })
}

#[derive(Debug)]
struct EnvironmentRow {
    elements: Vec<SyntaxElement>,
    break_text: Option<String>,
}

impl EnvironmentRow {
    fn is_blank(&self) -> bool {
        self.break_text.is_none() && self.elements.iter().all(is_layout_trivia)
    }

    fn single_environment(&self) -> Option<MathEnvironment> {
        if self.break_text.is_some() {
            return None;
        }
        let mut content = self
            .elements
            .iter()
            .filter(|element| !is_layout_trivia(element));
        let environment = content
            .next()?
            .as_node()
            .cloned()
            .and_then(MathEnvironment::cast)?;
        content.next().is_none().then_some(environment)
    }
}

#[derive(Debug)]
struct EnvironmentCell {
    document: Ir,
    atom_offset: usize,
}

impl EnvironmentCell {
    fn first_line_width(&self, printer: &Printer) -> usize {
        printer
            .print(&self.document, 0)
            .lines()
            .next()
            .unwrap_or_default()
            .chars()
            .count()
    }

    fn starts_relation(&self) -> bool {
        let rendered = Printer::new(usize::MAX / 2, INDENT_WIDTH).print(&self.document, 0);
        let text = rendered.trim_start();
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
        math_command_info(&name).class == MathClass::Rel
    }
}

enum EnvironmentBodyItem {
    Block(Ir),
    Row {
        cells: Vec<EnvironmentCell>,
        break_text: Option<String>,
    },
}

fn lower_environment_rows(elements: &[SyntaxElement], opts: &MathFormatOptions) -> Option<Ir> {
    let printer = Printer::new(opts.line_width, INDENT_WIDTH);
    let rows = split_environment_rows(elements);
    let has_authored_break = rows.iter().any(|row| row.break_text.is_some());
    let mut items = Vec::new();

    for row in rows {
        if row.is_blank() {
            continue;
        }
        if let Some(environment) = row.single_environment() {
            items.push(EnvironmentBodyItem::Block(try_lower_environment_document(
                &environment,
                opts,
            )?));
            continue;
        }

        let cells = split_environment_cells(&row.elements)
            .into_iter()
            .map(|elements| lower_environment_cell(elements, opts))
            .collect::<Option<Vec<_>>>()?;
        items.push(EnvironmentBodyItem::Row {
            cells,
            break_text: row.break_text,
        });
    }

    let tight_grid = items.iter().any(|item| match item {
        EnvironmentBodyItem::Row { cells, .. } => cells
            .iter()
            .take(cells.len().saturating_sub(1))
            .any(|cell| cell.document.contains_forced_break()),
        EnvironmentBodyItem::Block(_) => false,
    });
    let widths = environment_column_widths(&items, &printer);
    let mut rows = Vec::new();
    for item in items {
        let document = match item {
            EnvironmentBodyItem::Block(document) => document,
            EnvironmentBodyItem::Row { cells, break_text } if tight_grid => {
                join_tight_environment_cells(&cells, break_text.as_deref())
            }
            EnvironmentBodyItem::Row { cells, break_text } => {
                join_environment_cells(&cells, &widths, break_text.as_deref())
            }
        };
        rows.push(document);
    }

    if rows.is_empty() && !has_authored_break {
        Some(Ir::Nil)
    } else {
        Some(Ir::join(Ir::HardLine, rows))
    }
}

fn lower_environment_cell(
    elements: Vec<SyntaxElement>,
    opts: &MathFormatOptions,
) -> Option<EnvironmentCell> {
    let comment_elements = elements
        .iter()
        .filter(|element| contains_nested_comment(element))
        .cloned()
        .collect::<Vec<_>>();
    let document = lower_environment_cell_elements(elements, opts)?;
    let atom_offset = match comment_elements.as_slice() {
        [] => 0,
        [comment_element] => {
            let comment_document =
                try_lower_elements(vec![comment_element.clone()], &opts.signature_scope)?;
            let printer = Printer::new(usize::MAX / 2, INDENT_WIDTH);
            let first_line = printer.print(&document, 0).lines().next()?.to_string();
            let comment_first_line = printer
                .print(&comment_document, 0)
                .lines()
                .next()?
                .to_string();
            first_line[..first_line.rfind(&comment_first_line)?]
                .chars()
                .count()
        }
        _ => return None,
    };
    Some(EnvironmentCell {
        document,
        atom_offset,
    })
}

fn lower_environment_cell_elements(
    elements: Vec<SyntaxElement>,
    opts: &MathFormatOptions,
) -> Option<Ir> {
    if elements
        .iter()
        .any(|element| display_environment(element).is_some())
    {
        return lower_environment_composition(elements, opts, 0, Spacing::EnvironmentCell);
    }
    let lower = |elements| {
        lower_body(
            elements,
            &opts.signature_scope,
            Spacing::EnvironmentCell,
            true,
            false,
        )
    };
    if !elements
        .iter()
        .any(|element| element.kind() == SyntaxKind::MATH_EQUATION_LABEL)
    {
        return lower(elements);
    }

    let mut documents = Vec::new();
    let mut segment = Vec::new();
    for element in elements {
        if element.kind() == SyntaxKind::MATH_EQUATION_LABEL {
            let trailing_space = segment.iter().any(|element| !is_layout_trivia(element))
                && segment.last().is_some_and(|element: &SyntaxElement| {
                    element.kind() == SyntaxKind::MATH_SPACE
                });
            documents.push(lower(std::mem::take(&mut segment))?);
            if trailing_space {
                documents.push(Ir::text(" "));
            }
            documents.push(Ir::verbatim(element.to_string()));
        } else {
            segment.push(element);
        }
    }
    documents.push(lower(segment)?);
    Some(Ir::concat(documents))
}

fn contains_nested_comment(element: &SyntaxElement) -> bool {
    element.as_node().is_some_and(|node| {
        node.descendants_with_tokens()
            .any(|element| element.kind() == SyntaxKind::MATH_COMMENT)
    })
}

fn split_environment_rows(elements: &[SyntaxElement]) -> Vec<EnvironmentRow> {
    let mut rows = Vec::new();
    let mut current = Vec::new();
    for element in elements {
        match element.kind() {
            SyntaxKind::MATH_LINE_BREAK => rows.push(EnvironmentRow {
                elements: std::mem::take(&mut current),
                break_text: Some(element.to_string()),
            }),
            SyntaxKind::MATH_NEWLINE => rows.push(EnvironmentRow {
                elements: std::mem::take(&mut current),
                break_text: None,
            }),
            _ => current.push(element.clone()),
        }
    }
    if !current.is_empty() {
        rows.push(EnvironmentRow {
            elements: current,
            break_text: None,
        });
    }
    rows
}

fn split_environment_cells(elements: &[SyntaxElement]) -> Vec<Vec<SyntaxElement>> {
    let mut cells = vec![Vec::new()];
    for element in elements {
        if element.kind() == SyntaxKind::MATH_ALIGN {
            cells.push(Vec::new());
        } else {
            cells.last_mut().expect("seeded cell").push(element.clone());
        }
    }
    cells
}

fn environment_column_widths(items: &[EnvironmentBodyItem], printer: &Printer) -> Vec<usize> {
    let mut widths = Vec::new();
    for item in items {
        let EnvironmentBodyItem::Row { cells, .. } = item else {
            continue;
        };
        if cells.len() < 2 {
            continue;
        }
        for (column, cell) in cells.iter().enumerate() {
            if column >= widths.len() {
                widths.resize(column + 1, 0);
            }
            widths[column] = widths[column].max(cell.first_line_width(printer));
        }
    }
    widths
}

fn join_tight_environment_cells(cells: &[EnvironmentCell], break_text: Option<&str>) -> Ir {
    let mut documents = Vec::new();
    for (column, cell) in cells.iter().enumerate() {
        if column > 0 {
            documents.push(Ir::text("&"));
            if cell.starts_relation() {
                documents.push(Ir::text(" "));
            }
        }
        documents.push(cell.document.clone());
    }
    if let Some(marker) = break_text {
        documents.push(Ir::verbatim(marker));
    }
    Ir::concat(documents)
}

fn join_environment_cells(
    cells: &[EnvironmentCell],
    widths: &[usize],
    break_text: Option<&str>,
) -> Ir {
    let printer = Printer::new(usize::MAX / 2, INDENT_WIDTH);
    let last = cells.len().saturating_sub(1);
    let mut documents = Vec::new();
    let mut prefix_width = 0;
    for (column, cell) in cells.iter().enumerate() {
        if column > 0 {
            documents.push(Ir::text(" & "));
            prefix_width += 3;
        }
        let document = Ir::align(
            prefix_width,
            Ir::align(cell.atom_offset, cell.document.clone()),
        );
        documents.push(document);
        let width = cell.first_line_width(&printer);
        let padding = if column == last && break_text.is_none() {
            0
        } else {
            widths
                .get(column)
                .copied()
                .unwrap_or(0)
                .saturating_sub(width)
        };
        if padding > 0 {
            documents.push(Ir::text(" ".repeat(padding)));
        }
        prefix_width += width + padding;
    }
    if let Some(marker) = break_text {
        if !cells.is_empty() {
            documents.push(Ir::text(" "));
        }
        documents.push(Ir::verbatim(marker));
    }
    Ir::concat(documents)
}

/// Lower a formatter-derived row or cell without inventing a CST wrapper.
pub(super) fn try_lower_elements(
    elements: Vec<SyntaxElement>,
    scope: &SignatureScope,
) -> Option<Ir> {
    lower_body(elements, scope, Spacing::Normal, true, false)
}

/// Lower a bracketed body, routing comment-bearing bodies through hard lines.
fn lower_body(
    elements: Vec<SyntaxElement>,
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    environment_rows: bool,
) -> Option<Ir> {
    lower_body_configured(
        elements,
        scope,
        spacing,
        preserve_comment_context,
        environment_rows,
        false,
    )
}

fn lower_body_configured(
    elements: Vec<SyntaxElement>,
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    environment_rows: bool,
    align_authored_relations: bool,
) -> Option<Ir> {
    // A row break changes layout, but Badness retains the preceding atom when
    // assigning the following operator's contextual role.
    let semantic_atoms = semantic_math_atoms_in(elements.iter().cloned()).collect::<Vec<_>>();
    if elements
        .iter()
        .any(|element| element.kind() == SyntaxKind::MATH_LINE_BREAK)
    {
        lower_authored_breaks(
            elements,
            semantic_atoms,
            scope,
            spacing,
            AuthoredBreakOptions {
                preserve_comment_context,
                environment_rows,
                align_authored_relations,
                display_width: None,
            },
        )
    } else {
        lower_body_with_atoms(
            elements,
            semantic_atoms,
            scope,
            spacing,
            preserve_comment_context,
            environment_rows,
        )
    }
}

fn lower_body_with_atoms(
    elements: Vec<SyntaxElement>,
    semantic_atoms: Vec<SemanticMathAtom>,
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    environment_rows: bool,
) -> Option<Ir> {
    if elements
        .iter()
        .any(|element| element.kind() == SyntaxKind::MATH_COMMENT)
    {
        lower_edge_comments(
            elements,
            semantic_atoms,
            scope,
            spacing,
            preserve_comment_context,
            environment_rows,
        )
    } else {
        lower_elements_with_atoms(
            elements,
            semantic_atoms,
            scope,
            spacing,
            preserve_comment_context,
            environment_rows,
        )
    }
}

#[derive(Clone, Copy)]
struct RelationLayout {
    column: usize,
    rhs_start: usize,
    assignment: bool,
    definition: bool,
}

#[derive(Clone, Copy)]
struct AuthoredBreakOptions {
    preserve_comment_context: bool,
    environment_rows: bool,
    align_authored_relations: bool,
    display_width: Option<usize>,
}

fn lower_authored_breaks(
    elements: Vec<SyntaxElement>,
    semantic_atoms: Vec<SemanticMathAtom>,
    scope: &SignatureScope,
    spacing: Spacing,
    options: AuthoredBreakOptions,
) -> Option<Ir> {
    let AuthoredBreakOptions {
        preserve_comment_context,
        environment_rows,
        align_authored_relations,
        display_width,
    } = options;
    struct AuthoredRow {
        document: Ir,
        marker: Option<String>,
        authored_space: bool,
        adjacent_comment: Option<String>,
        first_relation: Option<RelationLayout>,
        starts_with_relation: bool,
        has_align: bool,
        breakable_pieces: Option<Vec<Piece>>,
    }

    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut elements = elements.into_iter().peekable();

    while let Some(element) = elements.next() {
        if element.kind() != SyntaxKind::MATH_LINE_BREAK {
            row.push(element);
            continue;
        }

        let line_break = element.into_node().and_then(MathLineBreak::cast)?;
        if line_break.marker_token().as_ref().map(SyntaxToken::text) != Some(r"\\")
            || line_break
                .modifier()
                .is_some_and(|modifier| !modifier.is_closed())
        {
            return None;
        }

        let authored_space = row.iter().any(|element| !is_layout_trivia(element))
            && row.last().is_some_and(is_layout_trivia);
        let row_atoms = if environment_rows {
            semantic_math_atoms_in(row.iter().cloned()).collect()
        } else {
            semantic_atoms_for(&row, &semantic_atoms)
        };
        let has_align = row
            .iter()
            .any(|element| element.kind() == SyntaxKind::MATH_ALIGN);
        let has_comment = row
            .iter()
            .any(|element| element.kind() == SyntaxKind::MATH_COMMENT);
        let (first_relation, starts_with_relation) =
            if align_authored_relations && !has_align && !has_comment {
                relation_layout(
                    &row,
                    &row_atoms,
                    scope,
                    spacing,
                    preserve_comment_context,
                    environment_rows,
                )?
            } else {
                (None, false)
            };
        let row_elements = std::mem::take(&mut row);
        let breakable_pieces = (!has_align && !has_comment)
            .then(|| {
                lower_pieces_with_atoms(
                    &row_elements,
                    &row_atoms,
                    scope,
                    spacing,
                    preserve_comment_context,
                    environment_rows,
                )
            })
            .flatten();
        let document = lower_authored_row(
            row_elements,
            &row_atoms,
            scope,
            spacing,
            preserve_comment_context,
            environment_rows,
        )?;
        // Look past the trivia after the marker for a comment on the same
        // logical row, buffering it rather than cloning the whole iterator.
        let mut skipped = Vec::new();
        while environment_rows && elements.peek().is_some_and(is_layout_trivia) {
            skipped.push(elements.next().expect("peeked element"));
        }
        let adjacent_comment = if environment_rows
            && elements
                .peek()
                .is_some_and(|element| element.kind() == SyntaxKind::MATH_COMMENT)
        {
            let comment = elements.next().expect("peeked comment");
            if elements
                .peek()
                .is_some_and(|element| element.kind() == SyntaxKind::MATH_NEWLINE)
            {
                elements.next();
            }
            Some(comment.to_string())
        } else {
            // No comment followed, so the trivia opens the next row.
            row.extend(skipped);
            None
        };
        rows.push(AuthoredRow {
            document,
            marker: Some(line_break.syntax().to_string()),
            authored_space,
            adjacent_comment,
            first_relation,
            starts_with_relation,
            has_align,
            breakable_pieces,
        });
    }

    let row_atoms = if environment_rows {
        semantic_math_atoms_in(row.iter().cloned()).collect()
    } else {
        semantic_atoms_for(&row, &semantic_atoms)
    };
    let has_align = row
        .iter()
        .any(|element| element.kind() == SyntaxKind::MATH_ALIGN);
    let has_comment = row
        .iter()
        .any(|element| element.kind() == SyntaxKind::MATH_COMMENT);
    let (first_relation, starts_with_relation) =
        if align_authored_relations && !has_align && !has_comment {
            relation_layout(
                &row,
                &row_atoms,
                scope,
                spacing,
                preserve_comment_context,
                environment_rows,
            )?
        } else {
            (None, false)
        };
    let breakable_pieces = (!has_align && !has_comment)
        .then(|| {
            lower_pieces_with_atoms(
                &row,
                &row_atoms,
                scope,
                spacing,
                preserve_comment_context,
                environment_rows,
            )
        })
        .flatten();
    let document = lower_authored_row(
        row,
        &row_atoms,
        scope,
        spacing,
        preserve_comment_context,
        environment_rows,
    )?;
    rows.push(AuthoredRow {
        document,
        marker: None,
        authored_space: false,
        adjacent_comment: None,
        first_relation,
        starts_with_relation,
        has_align,
        breakable_pieces,
    });

    // A final row with no content of its own would otherwise leave a trailing
    // hard break, printing as a blank line before the closing delimiter.
    if rows.len() > 1
        && rows
            .last()
            .is_some_and(|row| row.marker.is_none() && matches!(row.document, Ir::Nil))
    {
        rows.pop();
    }

    let max_row_width = if environment_rows {
        rows.iter()
            .filter_map(|row| row.document.flat_width())
            .max()
            .unwrap_or(0)
    } else {
        0
    };
    let mut relation_indents = vec![0usize; rows.len()];
    if align_authored_relations {
        let mut index = 0;
        while index < rows.len() {
            if rows[index].marker.is_some() {
                let mut end = index;
                while rows[end].marker.is_some()
                    && end + 1 < rows.len()
                    && rows[end + 1].starts_with_relation
                {
                    end += 1;
                }
                if end > index && !rows[index..=end].iter().any(|row| row.has_align) {
                    let head_relation = rows[index].first_relation;
                    let head_width = rows[index].document.flat_width()?;
                    for continuation in index + 1..=end {
                        let continuation_relation = rows[continuation].first_relation?;
                        relation_indents[continuation] = match head_relation {
                            Some(head) if head.definition => 0,
                            Some(head) if !head.assignment => head.column,
                            Some(head) if continuation_relation.assignment => head.column,
                            Some(head) => head.rhs_start,
                            None => head_width.checked_add(1)?,
                        };
                    }
                    index = end + 1;
                    continue;
                }
            }
            index += 1;
        }
    }
    let mut documents = Vec::new();
    for (index, row) in rows.into_iter().enumerate() {
        let row_width = row.document.flat_width();
        let document = match (display_width, row.breakable_pieces) {
            (Some(width), Some(pieces)) => layout_display_pieces(
                &pieces,
                width.saturating_sub(relation_indents[index]),
                spacing,
            ),
            _ => row.document,
        };
        let mut row_documents = vec![document];
        if let Some(marker) = row.marker {
            if environment_rows {
                let padding = row_width.map_or(1, |width| max_row_width.saturating_sub(width) + 1);
                row_documents.push(Ir::text(" ".repeat(padding)));
            } else if row.authored_space {
                row_documents.push(Ir::text(" "));
            }
            row_documents.push(Ir::verbatim(marker));
            if let Some(comment) = row.adjacent_comment {
                row_documents.extend([Ir::text(" "), Ir::verbatim(comment)]);
            }
        }
        let row_document = Ir::concat(row_documents);
        if index == 0 {
            documents.push(row_document);
        } else if relation_indents[index] > 0 {
            documents.push(Ir::align(
                relation_indents[index],
                Ir::concat([Ir::HardLine, row_document]),
            ));
        } else {
            documents.extend([Ir::HardLine, row_document]);
        }
    }
    Some(Ir::concat(documents))
}

fn lower_authored_row(
    elements: Vec<SyntaxElement>,
    semantic_atoms: &[SemanticMathAtom],
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    environment_rows: bool,
) -> Option<Ir> {
    if !elements
        .iter()
        .any(|element| element.kind() == SyntaxKind::MATH_ALIGN)
    {
        return lower_body_with_atoms(
            elements,
            semantic_atoms.to_vec(),
            scope,
            spacing,
            preserve_comment_context,
            environment_rows,
        );
    }

    let mut documents = Vec::new();
    let mut cell = Vec::new();
    for (index, element) in elements.iter().enumerate() {
        if element.kind() != SyntaxKind::MATH_ALIGN {
            cell.push(element.clone());
            continue;
        }

        let spaced_before = cell.last().is_some_and(|element| {
            element.kind() == SyntaxKind::MATH_SPACE
                && cell.iter().any(|element| !is_layout_trivia(element))
        });
        let cell_atoms = semantic_atoms_for(&cell, semantic_atoms);
        let spaced_before = spaced_before
            || cell_atoms
                .last()
                .is_some_and(|atom| atom.break_priority != MathBreakPriority::None);
        documents.push(lower_body_with_atoms(
            std::mem::take(&mut cell),
            cell_atoms,
            scope,
            spacing,
            preserve_comment_context,
            environment_rows,
        )?);

        let next_separator = elements[index + 1..]
            .iter()
            .find(|element| element.kind() == SyntaxKind::MATH_ALIGN)
            .map(SyntaxElement::text_range);
        let spaced_after = elements[index + 1..]
            .first()
            .is_some_and(|element| element.kind() == SyntaxKind::MATH_SPACE)
            || semantic_atoms
                .iter()
                .find(|atom| {
                    atom.range.start() >= element.text_range().end()
                        && next_separator.is_none_or(|range| atom.range.end() <= range.start())
                })
                .is_some_and(|atom| atom.break_priority != MathBreakPriority::None);
        if spaced_before {
            documents.push(Ir::text(" "));
        }
        documents.push(Ir::verbatim(element.to_string()));
        if spaced_after {
            documents.push(Ir::text(" "));
        }
    }

    let cell_atoms = semantic_atoms_for(&cell, semantic_atoms);
    documents.push(lower_body_with_atoms(
        cell,
        cell_atoms,
        scope,
        spacing,
        preserve_comment_context,
        environment_rows,
    )?);
    Some(Ir::concat(documents))
}

/// Badness indents a comment-broken body by one column per bracket level,
/// including the closing delimiter after a trailing comment. Applying the
/// hanging indent only to broken bodies keeps every flat body byte-identical.
fn hanging(width: usize, body: Ir) -> Ir {
    if body.contains_forced_break() {
        Ir::align(width, body)
    } else {
        body
    }
}

fn lower_edge_comments(
    elements: Vec<SyntaxElement>,
    semantic_atoms: Vec<SemanticMathAtom>,
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    environment_rows: bool,
) -> Option<Ir> {
    let mut documents = Vec::new();
    let mut segment_start = 0;
    for (index, comment) in elements.iter().enumerate() {
        if comment.kind() != SyntaxKind::MATH_COMMENT {
            continue;
        }
        let segment = &elements[segment_start..index];
        let segment_atoms = if preserve_comment_context {
            semantic_atoms_for(segment, &semantic_atoms)
        } else {
            semantic_math_atoms_in(segment.iter().cloned()).collect()
        };
        let has_content = !segment_atoms.is_empty();
        documents.push(lower_elements_with_atoms(
            segment.to_vec(),
            segment_atoms,
            scope,
            spacing,
            preserve_comment_context,
            environment_rows,
        )?);
        if has_content {
            let trailing_trivia = segment
                .iter()
                .rev()
                .take_while(|element| is_layout_trivia(element));
            let mut has_space = false;
            let mut has_newline = false;
            for trivia in trailing_trivia {
                has_space = true;
                has_newline |= trivia.kind() == SyntaxKind::MATH_NEWLINE;
            }
            if has_newline {
                return None;
            } else if has_space {
                documents.push(Ir::text(" "));
            }
        }
        documents.push(Ir::verbatim(comment.to_string()));
        // A comment runs to end of line, so anything the caller emits after
        // this body -- a closing brace, `\end`, the next segment -- has to
        // start on a new line.
        if index + 1 < elements.len() {
            documents.push(Ir::HardLine);
        }
        segment_start = index + 1;
    }
    let segment = &elements[segment_start..];
    let trailing_atoms = if preserve_comment_context {
        semantic_atoms_for(segment, &semantic_atoms)
    } else {
        semantic_math_atoms_in(segment.iter().cloned()).collect()
    };
    documents.push(lower_elements_with_atoms(
        segment.to_vec(),
        trailing_atoms,
        scope,
        spacing,
        preserve_comment_context,
        environment_rows,
    )?);
    Some(Ir::concat(documents))
}

fn semantic_atoms_for(
    elements: &[SyntaxElement],
    semantic_atoms: &[SemanticMathAtom],
) -> Vec<SemanticMathAtom> {
    semantic_atoms
        .iter()
        .copied()
        .filter(|atom| {
            elements.iter().any(|element| {
                element.text_range().start() <= atom.range.start()
                    && element.text_range().end() >= atom.range.end()
            })
        })
        .collect()
}

fn lower_elements(
    elements: Vec<SyntaxElement>,
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    environment_rows: bool,
) -> Option<Ir> {
    let semantic_atoms = semantic_math_atoms_in(elements.iter().cloned()).collect();
    lower_elements_with_atoms(
        elements,
        semantic_atoms,
        scope,
        spacing,
        preserve_comment_context,
        environment_rows,
    )
}

fn lower_elements_with_atoms(
    elements: Vec<SyntaxElement>,
    semantic_atoms: Vec<SemanticMathAtom>,
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    environment_rows: bool,
) -> Option<Ir> {
    let pieces = lower_pieces_with_atoms(
        &elements,
        &semantic_atoms,
        scope,
        spacing,
        preserve_comment_context,
        environment_rows,
    )?;
    Some(document_from_pieces(&pieces, spacing))
}

fn lower_pieces_with_atoms(
    elements: &[SyntaxElement],
    semantic_atoms: &[SemanticMathAtom],
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    environment_rows: bool,
) -> Option<Vec<Piece>> {
    if !elements
        .iter()
        .all(|element| is_supported_element(element, scope))
    {
        return None;
    }

    let semantic_atoms = coalesce_scripted_relations(elements, semantic_atoms);
    let mut pieces = Vec::new();
    let mut previous_end = None;
    let nested_spacing = match spacing {
        Spacing::EnvironmentCell => Spacing::Normal,
        _ => spacing,
    };

    for atom in semantic_atoms {
        let atom_document = scripted_relation_document(
            atom,
            elements,
            scope,
            nested_spacing,
            preserve_comment_context,
            environment_rows,
        )
        .or_else(|| {
            atom_document(
                atom,
                elements,
                scope,
                nested_spacing,
                preserve_comment_context,
                environment_rows,
            )
        })?;
        pieces.push(Piece {
            role: Role::from(atom.break_priority),
            delimiter: atom.delimiter,
            assignment: atom.break_priority == MathBreakPriority::Relation
                && relation_is_assignment(atom, elements),
            definition: atom.break_priority == MathBreakPriority::Relation
                && is_definition_relation(atom, elements),
            conditioning_relation: atom.break_priority == MathBreakPriority::Relation
                && atom_source_text(atom, elements).as_deref() == Some(r"\mid"),
            punctuation: atom.class == MathClass::Punct,
            display_separator: usize::from(atom.range.len()) == r"\qquad".len()
                && atom_source_text(atom, elements).as_deref() == Some(r"\qquad"),
            unary: atom.coerced_unary || atom.coerced_postfix,
            dimension_sign: atom.attached_dimension_sign,
            authored_space_before: previous_end.is_some_and(|end| end < atom.range.start()),
            slash: atom_document.slash,
            control_word_operator: atom_document.control_word_operator,
            starts_control_word_letter: atom_document.starts_control_word_letter,
            ends_control_word: atom_document.ends_control_word,
            multiline_tail_width: None,
            document: atom_document.document,
        });
        previous_end = Some(atom.range.end());
    }

    Some(pieces)
}

/// Coalesce a composite relation split between a word token and a scripted
/// tail. Definition-colon runs remain punctuation in the parser stream, while
/// the following relation scalar can own the script; non-colon composite
/// relations have the same CST seam. Authored whitespace still prevents
/// coalescing.
fn coalesce_scripted_relations(
    elements: &[SyntaxElement],
    semantic_atoms: &[SemanticMathAtom],
) -> Vec<SemanticMathAtom> {
    let mut coalesced = Vec::with_capacity(semantic_atoms.len());
    let mut index = 0;
    while let Some(&first) = semantic_atoms.get(index) {
        let Some(word) = elements.iter().find_map(|element| {
            element.as_token().filter(|token| {
                token.kind() == SyntaxKind::MATH_WORD
                    && token.text_range().start() <= first.range.start()
                    && token.text_range().end() >= first.range.end()
            })
        }) else {
            coalesced.push(first);
            index += 1;
            continue;
        };
        let definition = first.class == MathClass::Punct
            && token_slice(first.range, word).as_deref() == Some(":");
        if first.class != MathClass::Rel && !definition {
            coalesced.push(first);
            index += 1;
            continue;
        }

        let mut relation_index = index + 1;
        while definition
            && semantic_atoms.get(relation_index).is_some_and(|atom| {
                atom.class == MathClass::Punct
                    && semantic_atoms[relation_index - 1].range.end() == atom.range.start()
                    && token_slice(atom.range, word).as_deref() == Some(":")
            })
        {
            relation_index += 1;
        }
        let Some(&relation) = semantic_atoms.get(relation_index) else {
            coalesced.push(first);
            index += 1;
            continue;
        };
        let combined = TextRange::new(first.range.start(), relation.range.end());
        if relation.class != MathClass::Rel
            || semantic_atoms[relation_index - 1].range.end() != relation.range.start()
            || definition && !atom_starts_with_equals(relation, elements)
            || !definition && scripted_relation_parts(combined, elements).is_none()
        {
            coalesced.push(first);
            index += 1;
            continue;
        }

        coalesced.push(SemanticMathAtom {
            range: combined,
            class: MathClass::Rel,
            delimiter: None,
            break_priority: MathBreakPriority::Relation,
            coerced_unary: false,
            coerced_postfix: false,
            attached_dimension_sign: false,
        });
        index = relation_index + 1;
    }
    coalesced
}

fn atom_starts_with_equals(atom: SemanticMathAtom, elements: &[SyntaxElement]) -> bool {
    elements.iter().any(|element| {
        if element.text_range().start() > atom.range.start()
            || element.text_range().end() < atom.range.end()
        {
            return false;
        }
        match element {
            SyntaxElement::Token(token) if token.kind() == SyntaxKind::MATH_WORD => {
                token_slice(atom.range, token).is_some_and(|text| text.starts_with('='))
            }
            SyntaxElement::Node(node) => MathScripted::cast(node.clone())
                .and_then(|scripted| scripted.base())
                .and_then(SyntaxElement::into_token)
                .is_some_and(|base| {
                    base.kind() == SyntaxKind::MATH_WORD && base.text().starts_with('=')
                }),
            _ => false,
        }
    })
}

fn atom_source_text(atom: SemanticMathAtom, elements: &[SyntaxElement]) -> Option<String> {
    let element = elements.iter().find(|element| {
        element.text_range().start() <= atom.range.start()
            && element.text_range().end() >= atom.range.end()
    })?;
    let start = usize::from(atom.range.start() - element.text_range().start());
    let end = usize::from(atom.range.end() - element.text_range().start());
    element.to_string().get(start..end).map(str::to_owned)
}

fn document_from_pieces(pieces: &[Piece], spacing: Spacing) -> Ir {
    let mut documents = Vec::new();
    let mut column = 0usize;
    for (index, piece) in pieces.iter().enumerate() {
        if piece_gap_before(pieces, index, spacing) {
            documents.push(Ir::text(" "));
            column += 1;
        }
        if let Some(tail_width) = piece.multiline_tail_width {
            documents.push(Ir::align(column, piece.document.clone()));
            column += tail_width;
        } else {
            column += piece.document.flat_width().unwrap_or(0);
            documents.push(piece.document.clone());
        }
    }

    Ir::concat(documents)
}

fn piece_gap_before(pieces: &[Piece], index: usize, spacing: Spacing) -> bool {
    if index == 0 {
        return false;
    }
    let slash_is_spaced = |slash_index: usize| {
        pieces[slash_index].slash
            && (pieces[slash_index].authored_space_before
                || pieces
                    .get(slash_index + 1)
                    .is_some_and(|piece| piece.authored_space_before)
                || adjacent_operator(pieces, slash_index, spacing))
    };
    gap_before(pieces, index, spacing) || slash_is_spaced(index - 1) || slash_is_spaced(index)
}

fn piece_columns(pieces: &[Piece], spacing: Spacing) -> Option<Vec<(usize, usize)>> {
    let mut column = 0usize;
    let mut columns = Vec::with_capacity(pieces.len());
    for (index, piece) in pieces.iter().enumerate() {
        if piece_gap_before(pieces, index, spacing) {
            column = column.checked_add(1)?;
        }
        let start = column;
        column = column.checked_add(
            piece
                .multiline_tail_width
                .or_else(|| piece.document.flat_width())?,
        )?;
        columns.push((start, column));
    }
    Some(columns)
}

fn top_level_pieces(pieces: &[Piece]) -> impl Iterator<Item = (usize, &Piece)> {
    let mut depth = 0usize;
    pieces.iter().enumerate().filter(move |&(_, piece)| {
        let top_level = depth == 0;
        match piece.delimiter {
            Some(DelimiterRole::Open) => depth += 1,
            Some(DelimiterRole::Close) => depth = depth.saturating_sub(1),
            Some(DelimiterRole::Fence) | None => {}
        }
        top_level
    })
}

fn top_level_operators(pieces: &[Piece]) -> Vec<(usize, Role)> {
    top_level_pieces(pieces)
        .filter_map(|(index, piece)| (piece.role != Role::Operand).then_some((index, piece.role)))
        .collect()
}

/// Lay out a typed free-display row with a relation-first hierarchy: relation
/// continuations align semantically, and an over-width relation segment breaks
/// again at each binary operator.
fn layout_display_pieces(pieces: &[Piece], line_width: usize, spacing: Spacing) -> Ir {
    let flat = document_from_pieces(pieces, spacing);
    if flat.flat_width().is_some_and(|width| width <= line_width) {
        return flat;
    }
    if pieces
        .iter()
        .all(|piece| piece.multiline_tail_width.is_none() && piece.document.flat_width().is_some())
    {
        return layout_flat_display_pieces(pieces, line_width, spacing);
    }
    if !pieces
        .iter()
        .any(|piece| piece.multiline_tail_width.is_some())
        && flat.flat_width().is_none()
    {
        return flat;
    }

    let relations = top_level_operators(pieces)
        .into_iter()
        .filter_map(|(index, role)| (role == Role::Relation).then_some(index))
        .collect::<Vec<_>>();
    let mut lines = Vec::<(usize, Ir)>::new();

    if relations.len() < 2 {
        lines.extend(layout_display_segment(pieces, 0, line_width, spacing));
    } else {
        let Some(columns) = piece_columns(pieces, spacing) else {
            return flat;
        };
        let first = relations[0];
        let relation_column = columns.get(first).map_or(0, |&(start, _)| start);
        let rhs_start = columns
            .get(first)
            .map_or(relation_column, |&(_, end)| end.saturating_add(1));
        let first_assignment = pieces[first].assignment;
        let bounds = std::iter::once(0)
            .chain(relations.iter().skip(1).copied())
            .chain(std::iter::once(pieces.len()))
            .collect::<Vec<_>>();
        for segment in 0..bounds.len() - 1 {
            let start = bounds[segment];
            let end = bounds[segment + 1];
            let crosses_multiline_atom = pieces[..start]
                .iter()
                .any(|piece| piece.multiline_tail_width.is_some());
            let indent = if segment == 0 {
                0
            } else if !first_assignment || pieces[start].assignment || crosses_multiline_atom {
                relation_column
            } else {
                rhs_start
            };
            lines.extend(layout_display_segment(
                &pieces[start..end],
                indent,
                line_width,
                spacing,
            ));
        }
    }

    lines_document(lines)
}

/// Choose only the expression and operator breaks that improve a flat display.
///
/// Overflow is more expensive than an ordinary continuation. Conditioning
/// relations are the exception: their stronger break penalty can keep a tiny
/// overflow intact instead of turning every predicate into its own line. Once
/// a break is worthwhile, the secondary score favors balanced segments.
fn layout_flat_display_pieces(pieces: &[Piece], line_width: usize, spacing: Spacing) -> Ir {
    // These weights make any ordinary break preferable to one column of
    // overflow. A conditioning row alone may keep that single column because
    // splitting its predicates is more disruptive than the cosmetic excess.
    const OVERFLOW_COST: u128 = 16;
    const EXPRESSION_BREAK_COST: u128 = 1;
    const RELATION_BREAK_COST: u128 = 1;
    const FIRST_RELATION_BREAK_COST: u128 = 8;
    const BINARY_BREAK_COST: u128 = 4;
    const CONDITIONING_RELATION_BREAK_COST: u128 = 64;

    #[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
    struct Score {
        penalty: u128,
        max_width: usize,
        squared_widths: u128,
        lines: usize,
    }

    impl Score {
        fn with_line(self, width: usize, line_width: usize, break_cost: u128) -> Self {
            let overflow = width.saturating_sub(line_width) as u128;
            let width = width as u128;
            Self {
                penalty: self
                    .penalty
                    .saturating_add(
                        overflow
                            .saturating_mul(overflow)
                            .saturating_mul(OVERFLOW_COST),
                    )
                    .saturating_add(break_cost),
                max_width: self.max_width.max(width as usize),
                squared_widths: self
                    .squared_widths
                    .saturating_add(width.saturating_mul(width)),
                lines: self.lines + 1,
            }
        }
    }

    #[derive(Clone, Copy)]
    struct State {
        score: Score,
        anchor: Option<usize>,
        indent: usize,
        previous: (usize, usize),
    }

    let bounds = display_breaks(pieces);
    if bounds.len() == 2 {
        return document_from_pieces(pieces, spacing);
    }

    let Some(columns) = piece_columns(pieces, spacing) else {
        return document_from_pieces(pieces, spacing);
    };
    let has_conditioning_relation = pieces.iter().any(|piece| piece.conditioning_relation);

    // A break can move the first relation or start a new independent expression.
    // Keep alternatives with distinct printed anchors: their future line widths
    // differ, so the best prefix alone does not determine the best full layout.
    let mut best = vec![Vec::<State>::new(); bounds.len()];
    best[0].push(State {
        score: Score::default(),
        anchor: None,
        indent: 0,
        previous: (0, 0),
    });
    for end in 1..bounds.len() {
        let (prefixes, suffixes) = best.split_at_mut(end);
        let current = &mut suffixes[0];
        for (start, states) in prefixes.iter().enumerate() {
            let boundary = bounds[start];
            let segment_end = bounds[end].index;
            let segment_start = boundary.content_start(pieces, segment_end);
            let break_cost = if start == 0 {
                0
            } else if boundary.expression_start {
                EXPRESSION_BREAK_COST
            } else if has_conditioning_relation && pieces[segment_start].role == Role::Relation {
                CONDITIONING_RELATION_BREAK_COST
            } else if pieces[segment_start].role == Role::Relation {
                if boundary.first_relation == Some(segment_start) {
                    FIRST_RELATION_BREAK_COST
                } else {
                    RELATION_BREAK_COST
                }
            } else {
                BINARY_BREAK_COST
            };
            for (state_index, state) in states.iter().enumerate() {
                let indent = boundary.indent(pieces, &columns, state.anchor);
                let score = if segment_start > boundary.index {
                    state.score.with_line(
                        columns[boundary.index].1 - columns[boundary.index].0,
                        line_width,
                        EXPRESSION_BREAK_COST,
                    )
                } else {
                    state.score
                };
                let width = indent.saturating_add(
                    columns[segment_end - 1]
                        .1
                        .saturating_sub(columns[segment_start].0),
                );
                let anchor = bounds[end]
                    .first_relation
                    .filter(|&relation| relation < segment_end)
                    .and_then(|relation| {
                        if relation >= segment_start {
                            Some(
                                indent
                                    .saturating_add(columns[relation].0 - columns[segment_start].0),
                            )
                        } else {
                            state.anchor
                        }
                    });
                let candidate = State {
                    score: score.with_line(width, line_width, break_cost),
                    anchor,
                    indent,
                    previous: (start, state_index),
                };
                if let Some(existing) = current.iter_mut().find(|state| state.anchor == anchor) {
                    if candidate.score < existing.score {
                        *existing = candidate;
                    }
                } else {
                    current.push(candidate);
                }
            }
        }
    }

    let mut lines = Vec::new();
    let mut end = bounds.len() - 1;
    let Some((mut state_index, _)) = best[end]
        .iter()
        .enumerate()
        .min_by_key(|(_, state)| state.score)
    else {
        return document_from_pieces(pieces, spacing);
    };
    while end > 0 {
        let state = best[end][state_index];
        let (start, previous_state) = state.previous;
        let boundary = bounds[start];
        let content_start = boundary.content_start(pieces, bounds[end].index);
        lines.push((
            state.indent,
            document_from_pieces(&pieces[content_start..bounds[end].index], spacing),
        ));
        if content_start > boundary.index {
            lines.push((
                0,
                document_from_pieces(&pieces[boundary.index..content_start], spacing),
            ));
        }
        end = start;
        state_index = previous_state;
    }
    lines.reverse();
    lines_document(lines)
}

#[derive(Clone, Copy, Default)]
struct DisplayBreak {
    index: usize,
    first_relation: Option<usize>,
    relation: Option<usize>,
    expression_start: bool,
}

impl DisplayBreak {
    fn content_start(self, pieces: &[Piece], end: usize) -> usize {
        // A selected spacing separator owns a source line, so the following
        // expression's width and relation anchor exclude the command's width.
        if self.expression_start && pieces[self.index].display_separator && self.index + 1 < end {
            self.index + 1
        } else {
            self.index
        }
    }

    fn indent(self, pieces: &[Piece], columns: &[(usize, usize)], anchor: Option<usize>) -> usize {
        if self.index == 0 || self.expression_start {
            return 0;
        }
        let (Some(first), Some(relation), Some(anchor)) =
            (self.first_relation, self.relation, anchor)
        else {
            return 0;
        };
        let rhs = |index: usize| columns[index].1 - columns[index].0 + 1;
        let relation_indent = if !pieces[first].assignment || pieces[relation].assignment {
            anchor
        } else {
            anchor.saturating_add(rhs(first))
        };
        match pieces[self.index].role {
            Role::Operand => 0,
            Role::Relation if self.index == first => 0,
            Role::Relation => relation_indent,
            Role::Binary if relation == first => anchor.saturating_add(rhs(first)),
            Role::Binary => relation_indent.saturating_add(rhs(relation)),
        }
    }
}

fn display_breaks(pieces: &[Piece]) -> Vec<DisplayBreak> {
    let mut bounds = vec![DisplayBreak::default()];
    let mut first_relation = None;
    let mut relation = None;
    for (index, piece) in top_level_pieces(pieces) {
        if piece.display_separator && index > 0 && index + 1 < pieces.len() {
            first_relation = None;
            relation = None;
            if let Some(previous) = bounds.last_mut().filter(|bound| bound.index == index) {
                previous.expression_start = true;
            } else {
                bounds.push(DisplayBreak {
                    index,
                    expression_start: true,
                    ..Default::default()
                });
            }
        }
        if piece.role == Role::Relation {
            first_relation.get_or_insert(index);
            relation = Some(index);
        }
        if piece.role != Role::Operand {
            if let Some(previous) = bounds.last_mut().filter(|bound| bound.index == index) {
                previous.first_relation = first_relation;
                previous.relation = relation;
            } else {
                bounds.push(DisplayBreak {
                    index,
                    first_relation,
                    relation,
                    expression_start: false,
                });
            }
        }
        if piece.punctuation {
            first_relation = None;
            relation = None;
            if index + 1 < pieces.len() && !pieces[index + 1].punctuation {
                bounds.push(DisplayBreak {
                    index: index + 1,
                    expression_start: true,
                    ..Default::default()
                });
            }
        }
    }
    bounds.push(DisplayBreak {
        index: pieces.len(),
        first_relation,
        relation,
        expression_start: false,
    });
    bounds
}

fn layout_display_segment(
    pieces: &[Piece],
    base_indent: usize,
    line_width: usize,
    spacing: Spacing,
) -> Vec<(usize, Ir)> {
    let flat = document_from_pieces(pieces, spacing);
    if flat
        .flat_width()
        .is_some_and(|width| base_indent.saturating_add(width) <= line_width)
    {
        return vec![(base_indent, flat)];
    }
    if !pieces
        .iter()
        .any(|piece| piece.multiline_tail_width.is_some())
        && flat.flat_width().is_none()
    {
        return vec![(base_indent, flat)];
    }

    let operators = top_level_operators(pieces);
    let binaries = operators
        .iter()
        .filter_map(|&(index, role)| (role == Role::Binary).then_some(index))
        .collect::<Vec<_>>();
    let Some(&first_binary) = binaries.first() else {
        return vec![(base_indent, flat)];
    };
    let Some(columns) = piece_columns(pieces, spacing) else {
        return vec![(base_indent, flat)];
    };
    let rhs_offset = operators
        .iter()
        .find(|&&(index, role)| role == Role::Relation && index < first_binary)
        .and_then(|&(index, _)| columns.get(index))
        .map_or(0, |&(_, end)| end.saturating_add(1));

    let mut lines = Vec::new();
    let head = document_from_pieces(&pieces[..first_binary], spacing);
    if !matches!(head, Ir::Nil) {
        lines.push((base_indent, head));
    }
    for (position, &start) in binaries.iter().enumerate() {
        let end = binaries.get(position + 1).copied().unwrap_or(pieces.len());
        lines.push((
            base_indent.saturating_add(rhs_offset),
            document_from_pieces(&pieces[start..end], spacing),
        ));
    }
    lines
}

fn lines_document(mut lines: Vec<(usize, Ir)>) -> Ir {
    let Some((_, first)) = lines.first().cloned() else {
        return Ir::Nil;
    };
    let mut documents = vec![first];
    for (indent, document) in lines.drain(1..) {
        documents.push(Ir::align(indent, Ir::concat([Ir::HardLine, document])));
    }
    Ir::concat(documents)
}

fn relation_layout(
    elements: &[SyntaxElement],
    semantic_atoms: &[SemanticMathAtom],
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    environment_rows: bool,
) -> Option<(Option<RelationLayout>, bool)> {
    let pieces = lower_pieces_with_atoms(
        elements,
        semantic_atoms,
        scope,
        spacing,
        preserve_comment_context,
        environment_rows,
    )?;
    let base_gaps = (0..pieces.len())
        .map(|index| gap_before(&pieces, index, spacing))
        .collect::<Vec<_>>();
    let spaced_slashes = (0..pieces.len())
        .map(|index| {
            pieces[index].slash
                && (pieces[index].authored_space_before
                    || pieces
                        .get(index + 1)
                        .is_some_and(|piece| piece.authored_space_before)
                    || adjacent_operator(&pieces, index, spacing))
        })
        .collect::<Vec<_>>();

    let starts_with_relation = pieces
        .first()
        .is_some_and(|piece| piece.role == Role::Relation);
    let mut column = 0;
    let mut delimiter_depth = 0usize;
    for (index, piece) in pieces.iter().enumerate() {
        if index > 0 && (base_gaps[index] || spaced_slashes[index - 1] || spaced_slashes[index]) {
            column += 1;
        }
        let width = piece.document.flat_width()?;
        if delimiter_depth == 0 && piece.role == Role::Relation {
            return Some((
                Some(RelationLayout {
                    column,
                    rhs_start: column + width + 1,
                    assignment: piece.assignment,
                    definition: piece.definition,
                }),
                starts_with_relation,
            ));
        }
        column += width;
        match piece.delimiter {
            Some(DelimiterRole::Open) => delimiter_depth += 1,
            Some(DelimiterRole::Close) => delimiter_depth = delimiter_depth.saturating_sub(1),
            Some(DelimiterRole::Fence) | None => {}
        }
    }
    Some((None, starts_with_relation))
}

fn relation_is_assignment(atom: SemanticMathAtom, elements: &[SyntaxElement]) -> bool {
    elements
        .iter()
        .find(|element| {
            element.text_range().start() <= atom.range.start()
                && element.text_range().end() >= atom.range.end()
        })
        .is_some_and(assignment_element)
}

fn is_definition_relation(atom: SemanticMathAtom, elements: &[SyntaxElement]) -> bool {
    elements.iter().any(|element| match element {
        SyntaxElement::Token(token)
            if token.kind() == SyntaxKind::MATH_WORD
                && token.text_range().start() <= atom.range.start()
                && token.text_range().end() >= atom.range.end() =>
        {
            token_slice(atom.range, token)
                .is_some_and(|text| text.starts_with(':') && text.ends_with('='))
        }
        _ => false,
    }) || scripted_relation_parts(atom.range, elements)
        .is_some_and(|(prefix, _)| prefix.starts_with(':'))
}

fn assignment_element(element: &SyntaxElement) -> bool {
    match element {
        // Badness aligns definition relations with the equality/comparison
        // chain they introduce. Only assignment-arrow commands use the
        // right-hand-side continuation anchor in typed layout.
        SyntaxElement::Token(_) => false,
        SyntaxElement::Node(node) => {
            if let Some(command) = MathCommand::cast(node.clone()) {
                return command.name_token().is_some_and(|name| {
                    matches!(
                        name.text().strip_prefix('\\').unwrap_or(name.text()),
                        "gets" | "leftarrow" | "mapsto" | "coloneqq"
                    )
                });
            }
            MathScripted::cast(node.clone())
                .and_then(|scripted| scripted.base())
                .is_some_and(|base| assignment_element(&base))
        }
    }
}

pub(super) fn has_unproven_argument_domain(
    elements: &[SyntaxElement],
    scope: &SignatureScope,
) -> bool {
    elements.iter().any(|element| {
        let Some(node) = element.as_node() else {
            return false;
        };
        std::iter::once(node.clone())
            .chain(node.descendants())
            .filter_map(MathCommand::cast)
            .any(|command| command_has_unproven_argument_domain(&command, scope))
    })
}

fn command_has_unproven_argument_domain(command: &MathCommand, scope: &SignatureScope) -> bool {
    let arguments = command.attached_arguments().collect::<Vec<_>>();
    if arguments.is_empty() {
        return false;
    }
    let Some(name) = command.name() else {
        return true;
    };
    if scope.is_redefined(&name) {
        return true;
    }
    let Some(signature) = scope.command_signature(&name) else {
        return true;
    };
    let mut slot = 0;
    arguments.into_iter().any(|argument| {
        let kind = match argument {
            MathArgument::Brace(_) => ArgKind::Brace,
            MathArgument::Bracket(_) => ArgKind::Bracket,
        };
        match_arg_slot(&signature.arguments, &mut slot, kind)
            .is_none_or(|argument| argument.domain != ArgumentDomain::Math)
    })
}

fn is_supported_element(element: &SyntaxElement, scope: &SignatureScope) -> bool {
    match element {
        SyntaxElement::Token(token) => matches!(
            token.kind(),
            SyntaxKind::MATH_WORD
                | SyntaxKind::MATH_SPACE
                | SyntaxKind::MATH_NEWLINE
                | SyntaxKind::MATH_BRACKET_OPEN
                | SyntaxKind::MATH_BRACKET_CLOSE
                | SyntaxKind::MATH_CONTROL_SYMBOL
        ),
        SyntaxElement::Node(node) => {
            MathGroup::cast(node.clone()).is_some_and(|group| {
                group.is_closed()
                    && group.body_elements().all(|element| {
                        // A comment is not a semantic atom; `lower_body` decides
                        // whether this body's comments are safe to break at.
                        element.kind() == SyntaxKind::MATH_COMMENT
                            || is_supported_element(&element, scope)
                    })
            }) || MathCommand::cast(node.clone())
                .is_some_and(|command| command_is_supported(&command, scope))
                || MathDelimited::cast(node.clone())
                    .is_some_and(|delimited| delimited_is_supported(&delimited, scope))
                || MathScripted::cast(node.clone())
                    .is_some_and(|scripted| scripted_is_supported(&scripted, scope))
                || MathLineBreak::cast(node.clone()).is_some_and(|line_break| {
                    line_break.marker_token().as_ref().map(SyntaxToken::text) == Some(r"\\")
                        && line_break
                            .modifier()
                            .is_none_or(|modifier| modifier.is_closed())
                })
        }
    }
}

fn atom_document(
    atom: SemanticMathAtom,
    elements: &[SyntaxElement],
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    environment_rows: bool,
) -> Option<AtomDocument> {
    let range = atom.range;
    let element = elements.iter().find(|element| {
        element.text_range().start() <= range.start() && element.text_range().end() >= range.end()
    })?;
    let (document, slash) = match element {
        SyntaxElement::Token(token)
            if matches!(
                token.kind(),
                SyntaxKind::MATH_WORD
                    | SyntaxKind::MATH_BRACKET_OPEN
                    | SyntaxKind::MATH_BRACKET_CLOSE
                    | SyntaxKind::MATH_CONTROL_SYMBOL
            ) =>
        {
            let text = token_slice(range, token)?;
            let slash = text == "/";
            let document = if text == r"\ " {
                Ir::control_space()
            } else {
                Ir::verbatim(text)
            };
            (document, slash)
        }
        SyntaxElement::Node(node) => {
            if let Some(group) = MathGroup::cast(node.clone()) {
                let open = group.open_token()?;
                let close = group.close_token()?;
                let body = lower_body(
                    group.body_elements().collect(),
                    scope,
                    spacing,
                    preserve_comment_context,
                    false,
                )?;
                (
                    Ir::concat([
                        Ir::verbatim(open.text()),
                        hanging(1, body),
                        Ir::verbatim(close.text()),
                    ]),
                    false,
                )
            } else if let Some(command) = MathCommand::cast(node.clone()) {
                (
                    lower_command(
                        &command,
                        scope,
                        spacing,
                        preserve_comment_context,
                        environment_rows,
                    )?,
                    false,
                )
            } else if let Some(delimited) = MathDelimited::cast(node.clone()) {
                (
                    lower_delimited(
                        &delimited,
                        scope,
                        spacing,
                        preserve_comment_context,
                        environment_rows,
                    )?,
                    false,
                )
            } else {
                let scripted = MathScripted::cast(node.clone())?;
                (
                    lower_scripted(
                        &scripted,
                        scope,
                        spacing,
                        preserve_comment_context,
                        environment_rows,
                    )?,
                    false,
                )
            }
        }
        _ => return None,
    };
    let raw_class = math_atoms(element)
        .find(|raw| raw.range.start() == range.start())
        .map(|raw| raw.class)?;
    Some(AtomDocument {
        document,
        slash,
        control_word_operator: element_ends_control_word(element)
            && matches!(raw_class, MathClass::Bin | MathClass::Rel),
        starts_control_word_letter: element_starts_control_word_letter(element),
        ends_control_word: element_ends_control_word(element),
    })
}

fn scripted_relation_document(
    atom: SemanticMathAtom,
    elements: &[SyntaxElement],
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    environment_rows: bool,
) -> Option<AtomDocument> {
    let (prefix, scripted) = scripted_relation_parts(atom.range, elements)?;
    let scripted = lower_scripted(
        &scripted,
        scope,
        spacing,
        preserve_comment_context,
        environment_rows,
    )?;
    Some(AtomDocument {
        document: Ir::concat([Ir::verbatim(prefix), scripted]),
        slash: false,
        control_word_operator: false,
        starts_control_word_letter: false,
        ends_control_word: false,
    })
}

fn scripted_relation_parts(
    range: TextRange,
    elements: &[SyntaxElement],
) -> Option<(String, MathScripted)> {
    elements.windows(2).find_map(|pair| {
        let [SyntaxElement::Token(head), SyntaxElement::Node(node)] = pair else {
            return None;
        };
        if head.kind() != SyntaxKind::MATH_WORD
            || head.text_range().start() > range.start()
            || head.text_range().end() != node.text_range().start()
            || node.text_range().end() != range.end()
        {
            return None;
        }
        let scripted = MathScripted::cast(node.clone())?;
        let base = scripted.base()?.into_token()?;
        if base.kind() != SyntaxKind::MATH_WORD || !base.text().starts_with('=') {
            return None;
        }
        let prefix_range = TextRange::new(range.start(), head.text_range().end());
        let prefix = token_slice(prefix_range, head)?;
        let base_starts_relation = base.text().starts_with(['=', '<', '>']);
        let definition =
            prefix.chars().all(|character| character == ':') && base.text().starts_with('=');
        let composite = prefix
            .chars()
            .all(|character| matches!(character, '=' | '<' | '>'))
            && base_starts_relation;
        (!prefix.is_empty() && (definition || composite)).then_some((prefix, scripted))
    })
}

fn lower_delimited(
    delimited: &MathDelimited,
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    _environment_rows: bool,
) -> Option<Ir> {
    if !delimited_is_supported(delimited, scope) {
        return None;
    }

    let left = delimited.left_token()?;
    let open = delimited.opening_delimiter()?;
    let body = delimited.body()?;
    let right = delimited.right_token()?;
    let close = delimited.closing_delimiter()?;
    let mut documents = vec![Ir::verbatim(left.text()), Ir::verbatim(open.text())];
    if !body.text().trim().is_empty() {
        let inner = lower_body(
            body.elements().collect(),
            scope,
            spacing,
            preserve_comment_context,
            false,
        )?;
        let opening_width = left.text().chars().count() + open.text().chars().count();
        documents.push(Ir::align(
            opening_width + 1,
            Ir::concat([Ir::text(" "), inner, Ir::text(" ")]),
        ));
    }
    documents.extend([Ir::verbatim(right.text()), Ir::verbatim(close.text())]);
    Some(Ir::concat(documents))
}

fn delimited_is_supported(delimited: &MathDelimited, scope: &SignatureScope) -> bool {
    let (Some(left), Some(open), Some(body), Some(right), Some(close)) = (
        delimited.left_token(),
        delimited.opening_delimiter(),
        delimited.body(),
        delimited.right_token(),
        delimited.closing_delimiter(),
    ) else {
        return false;
    };
    let structural_ranges = [
        left.text_range(),
        open.text_range(),
        body.syntax().text_range(),
        right.text_range(),
        close.text_range(),
    ];
    delimited.syntax().children_with_tokens().all(|element| {
        structural_ranges.contains(&element.text_range()) || is_layout_trivia(&element)
    }) && body.elements().all(|element| {
        // The body's own comments break through `lower_body`; a comment outside
        // it, such as one between `\left` and its delimiter, stays unsupported.
        element.kind() == SyntaxKind::MATH_COMMENT || is_supported_element(&element, scope)
    })
}

fn lower_scripted(
    scripted: &MathScripted,
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    environment_rows: bool,
) -> Option<Ir> {
    let base = scripted.base()?;
    if scripted.scripts().next().is_none() || !scripted_is_supported(scripted, scope) {
        return None;
    }

    Some(Ir::concat([
        lower_elements(
            vec![base],
            scope,
            spacing,
            preserve_comment_context,
            environment_rows,
        )?,
        lower_script_suffix(scripted, scope, preserve_comment_context, environment_rows)?,
    ]))
}

fn lower_script_suffix(
    scripted: &MathScripted,
    scope: &SignatureScope,
    preserve_comment_context: bool,
    environment_rows: bool,
) -> Option<Ir> {
    let mut documents = Vec::new();
    for script in scripted.scripts() {
        if !script_is_supported(&script, scope) {
            return None;
        }
        let marker = script.marker_token()?;
        let argument = script.argument()?;
        documents.push(Ir::verbatim(marker.text()));
        documents.push(lower_elements(
            vec![argument],
            scope,
            Spacing::Script,
            preserve_comment_context,
            environment_rows,
        )?);
    }
    (!documents.is_empty()).then(|| Ir::concat(documents))
}

fn scripted_is_supported(scripted: &MathScripted, scope: &SignatureScope) -> bool {
    let Some(base) = scripted.base() else {
        return false;
    };
    if !is_supported_element(&base, scope) {
        return false;
    }

    let base_range = base.text_range();
    scripted.syntax().children_with_tokens().all(|element| {
        element.text_range() == base_range
            || is_layout_trivia(&element)
            || element
                .into_node()
                .and_then(MathScript::cast)
                .is_some_and(|script| script_is_supported(&script, scope))
    })
}

fn script_is_supported(script: &MathScript, scope: &SignatureScope) -> bool {
    let (Some(marker), Some(argument)) = (script.marker_token(), script.argument()) else {
        return false;
    };
    if !is_supported_element(&argument, scope) {
        return false;
    }

    let argument_range = argument.text_range();
    script.syntax().children_with_tokens().all(|element| {
        element.text_range() == marker.text_range()
            || element.text_range() == argument_range
            || is_layout_trivia(&element)
    })
}

fn is_layout_trivia(element: &SyntaxElement) -> bool {
    matches!(
        element.kind(),
        SyntaxKind::MATH_SPACE | SyntaxKind::MATH_NEWLINE
    )
}

fn lower_command(
    command: &MathCommand,
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    _environment_rows: bool,
) -> Option<Ir> {
    let name = command.name_token()?;
    if is_supported_bare_command(command, scope) {
        return Some(Ir::verbatim(name.text()));
    }

    let Some(arguments) = matched_command_arguments(command, scope) else {
        let source = opaque_command_source(command, scope)?;
        return Some(if source.contains('\n') {
            Ir::verbatim(source)
        } else {
            Ir::text(source)
        });
    };
    let mut previous_end = name.text_range().end();
    let mut documents = vec![Ir::verbatim(name.text())];
    if let Some(star) = command.star_token() {
        documents.push(Ir::verbatim(star.text()));
        previous_end = star.text_range().end();
    }
    for (argument, domain) in arguments {
        let open = argument.open_token()?;
        let close = argument.close_token()?;
        let body = if domain == ArgumentDomain::Math {
            hanging(
                1,
                lower_body(
                    argument.body_elements().collect(),
                    scope,
                    spacing,
                    preserve_comment_context,
                    false,
                )?,
            )
        } else {
            let source = argument.syntax().text().to_string();
            let body = source
                .strip_prefix(open.text())?
                .strip_suffix(close.text())?;
            if body.contains('\n') {
                Ir::verbatim(body.to_owned())
            } else {
                Ir::text(body.to_owned())
            }
        };
        if previous_end < argument.syntax().text_range().start() {
            documents.push(Ir::text(" "));
        }
        documents.extend([Ir::verbatim(open.text()), body, Ir::verbatim(close.text())]);
        previous_end = argument.syntax().text_range().end();
    }
    Some(Ir::concat(documents))
}

fn command_is_supported(command: &MathCommand, scope: &SignatureScope) -> bool {
    is_supported_bare_command(command, scope)
        || matched_command_arguments(command, scope).is_some()
        || opaque_command_source(command, scope).is_some()
}

fn opaque_command_source(command: &MathCommand, scope: &SignatureScope) -> Option<String> {
    let name = command.name()?;
    if scope.command_signature(&name).is_some() && !scope.is_redefined(&name) {
        return None;
    }
    let arguments = command.attached_arguments().collect::<Vec<_>>();
    if arguments.is_empty() || arguments.iter().any(|argument| !argument.is_closed()) {
        return None;
    }
    command
        .syntax()
        .children_with_tokens()
        .all(|element| match element {
            SyntaxElement::Token(token) => {
                matches!(
                    token.kind(),
                    SyntaxKind::MATH_CONTROL_WORD
                        | SyntaxKind::MATH_SPACE
                        | SyntaxKind::MATH_NEWLINE
                ) || token.kind() == SyntaxKind::MATH_WORD && token.text() == "*"
            }
            SyntaxElement::Node(node) => MathArgument::cast(node).is_some(),
        })
        .then(|| command.syntax().text().to_string())
}

fn is_supported_bare_command(command: &MathCommand, scope: &SignatureScope) -> bool {
    let Some(name) = command.name() else {
        return false;
    };
    if matches!(name.as_str(), "left" | "right")
        || scope.is_redefined(&name)
        || command
            .syntax()
            .children_with_tokens()
            .any(|element| element.kind() != SyntaxKind::MATH_CONTROL_WORD)
    {
        return false;
    }

    true
}

fn matched_command_arguments(
    command: &MathCommand,
    scope: &SignatureScope,
) -> Option<Vec<(MathArgument, ArgumentDomain)>> {
    if !command
        .syntax()
        .children_with_tokens()
        .all(|element| match element {
            SyntaxElement::Token(token) => {
                matches!(
                    token.kind(),
                    SyntaxKind::MATH_CONTROL_WORD
                        | SyntaxKind::MATH_SPACE
                        | SyntaxKind::MATH_NEWLINE
                ) || token.kind() == SyntaxKind::MATH_WORD && token.text() == "*"
            }
            SyntaxElement::Node(node) => MathArgument::cast(node).is_some(),
        })
    {
        return None;
    }
    let signature = scope.command_signature(&command.name()?)?;
    let arguments = command.attached_arguments().collect::<Vec<_>>();
    let mut slot = 0;
    let mut matched = Vec::with_capacity(arguments.len());

    for argument in arguments {
        if !argument.is_closed() {
            return None;
        }
        let kind = match argument {
            MathArgument::Brace(_) => ArgKind::Brace,
            MathArgument::Bracket(_) => ArgKind::Bracket,
        };
        let domain = match_arg_slot(&signature.arguments, &mut slot, kind)
            .map_or(ArgumentDomain::Unknown, |argument| argument.domain);
        matched.push((argument, domain));
    }

    if signature.arguments[slot..]
        .iter()
        .any(|argument| argument.required)
    {
        return None;
    }
    Some(matched)
}

struct Piece {
    role: Role,
    delimiter: Option<DelimiterRole>,
    assignment: bool,
    definition: bool,
    /// A conditioning bar such as `\mid` makes neighboring relations separate
    /// predicates rather than one relation chain.
    conditioning_relation: bool,
    punctuation: bool,
    display_separator: bool,
    /// A `+`/`-` that TeX coerced to a unary sign. It binds to the operand
    /// beside it, so it strips the authored space on either side.
    unary: bool,
    /// A sign scanned as part of an unbraced TeX dimension. It binds to the
    /// dimension on its right without stripping the command-to-argument gap.
    dimension_sign: bool,
    authored_space_before: bool,
    slash: bool,
    control_word_operator: bool,
    starts_control_word_letter: bool,
    ends_control_word: bool,
    /// Width of the atom's final forced line, measured from its own start.
    multiline_tail_width: Option<usize>,
    document: Ir,
}

struct AtomDocument {
    document: Ir,
    slash: bool,
    control_word_operator: bool,
    starts_control_word_letter: bool,
    ends_control_word: bool,
}

fn gap_before(pieces: &[Piece], index: usize, spacing: Spacing) -> bool {
    let Some(previous) = index.checked_sub(1).and_then(|index| pieces.get(index)) else {
        return false;
    };
    let current = &pieces[index];
    if previous.ends_control_word && current.starts_control_word_letter {
        return true;
    }

    // An environment cell can continue an earlier expression. Keep the author's
    // gap after its leading sign because cell-local coercion cannot prove unary
    // intent. A neighboring binary operator or relation still wins its space.
    let preserve_leading_sign = spacing == Spacing::EnvironmentCell && index == 1;
    let tight =
        previous.unary && !preserve_leading_sign || current.unary || previous.dimension_sign;

    match spacing {
        Spacing::Normal | Spacing::EnvironmentCell => {
            if current.role != Role::Operand || previous.role != Role::Operand {
                true
            } else {
                !tight && current.authored_space_before
            }
        }
        Spacing::Script => {
            current.control_word_operator
                || previous.control_word_operator
                || current.role == Role::Operand
                    && previous.role == Role::Operand
                    && !tight
                    && current.authored_space_before
                    && !touches_delimiter(previous, current)
        }
    }
}

fn adjacent_operator(pieces: &[Piece], index: usize, spacing: Spacing) -> bool {
    let previous = index.checked_sub(1).and_then(|index| pieces.get(index));
    let next = pieces.get(index + 1);
    match spacing {
        Spacing::Normal | Spacing::EnvironmentCell => previous
            .into_iter()
            .chain(next)
            .any(|piece| piece.role != Role::Operand),
        Spacing::Script => previous
            .into_iter()
            .chain(next)
            .any(|piece| piece.control_word_operator),
    }
}

fn touches_delimiter(previous: &Piece, current: &Piece) -> bool {
    [previous.delimiter, current.delimiter]
        .into_iter()
        .any(|role| matches!(role, Some(DelimiterRole::Open | DelimiterRole::Close)))
}

fn element_starts_control_word_letter(element: &SyntaxElement) -> bool {
    element_boundary_token(element, true)
        .and_then(|token| token.text().chars().next())
        .is_some_and(is_control_word_letter)
}

fn element_ends_control_word(element: &SyntaxElement) -> bool {
    element_boundary_token(element, false)
        .is_some_and(|token| token.kind() == SyntaxKind::MATH_CONTROL_WORD)
}

fn element_boundary_token(element: &SyntaxElement, first: bool) -> Option<SyntaxToken> {
    match element {
        SyntaxElement::Token(token) => Some(token.clone()),
        SyntaxElement::Node(node) => {
            let mut tokens = node
                .descendants_with_tokens()
                .filter_map(|element| element.into_token())
                .filter(|token| {
                    !matches!(
                        token.kind(),
                        SyntaxKind::MATH_SPACE | SyntaxKind::MATH_NEWLINE
                    )
                });
            if first { tokens.next() } else { tokens.last() }
        }
    }
}

/// Whether `character` could extend a preceding control word, forcing a space
/// between them. The parser's control-word alphabet is `[A-Za-z@]`; non-ASCII
/// letters stay included here because gluing a command to a following Greek
/// letter reads as one word even though the parser would still split it.
/// Catcode-12 characters such as `:` and `_` never extend a control word.
fn is_control_word_letter(character: char) -> bool {
    character.is_alphabetic() || character == '@'
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Spacing {
    Normal,
    EnvironmentCell,
    Script,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Operand,
    Binary,
    Relation,
}

impl From<MathBreakPriority> for Role {
    fn from(priority: MathBreakPriority) -> Self {
        match priority {
            MathBreakPriority::None => Self::Operand,
            MathBreakPriority::Binary => Self::Binary,
            MathBreakPriority::Relation => Self::Relation,
        }
    }
}

fn token_slice(range: TextRange, word: &SyntaxToken) -> Option<String> {
    let word_range = word.text_range();
    if range.start() < word_range.start() || range.end() > word_range.end() {
        return None;
    }
    let start = usize::from(range.start() - word_range.start());
    let end = usize::from(range.end() - word_range.start());
    Some(word.text()[start..end].to_owned())
}

#[cfg(test)]
mod tests {
    use panache_parser::parser::math::{MathParseOptions, parse_math_content};
    use panache_parser::parser::parse;
    use panache_parser::semantic::math::SignatureScope;
    use rowan::ast::AstNode;

    use super::*;
    use crate::formatter::math::printer::Printer;
    use crate::syntax::SyntaxNode;

    fn content(input: &str) -> MathContent {
        MathContent::cast(SyntaxNode::new_root(parse_math_content(
            input,
            MathParseOptions::default(),
        )))
        .expect("math content root")
    }

    /// Print with forced breaks intact — these tests assert the lowered layout,
    /// including the hard lines that comments and `\\` rows introduce. Inline
    /// math flattens them instead; see `Printer::print_flat`.
    fn lower(input: &str) -> Option<String> {
        try_lower_content(&content(input), &SignatureScope::default())
            .map(|document| Printer::new(80, 2).print(&document, 0))
    }

    fn lower_display(input: &str) -> Option<String> {
        lower_display_width(input, 80)
    }

    fn lower_display_width(input: &str, line_width: usize) -> Option<String> {
        try_lower_display_content(&content(input), &SignatureScope::default(), line_width)
            .map(|document| Printer::new(line_width, 2).print(&document, 0))
    }

    #[test]
    fn lowers_flat_words_and_trivia() {
        let cases = [
            ("  a  b\nc  ", "a b c"),
            ("α+β", "α + β"),
            ("x=-y", "x = -y"),
            ("a--b", "a - -b"),
            ("- x", "-x"),
            ("a<=b", "a <= b"),
            ("a/ b", "a / b"),
            ("a /b", "a / b"),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_the_last_flattened_formatter_shapes() {
        for (input, expected) in [
            (r"\Big[ x \Big]", r"\Big[ x \Big]"),
            (r"\sqrt [3]{x}", r"\sqrt [3]{x}"),
            (r"\frac\alpha\beta", r"\frac\alpha\beta"),
            (r"\frac12", r"\frac12"),
            (r"\{ a \}", r"\{ a \}"),
            (r"a = \$ 5", r"a = \$ 5"),
            (r"\int_0^1 x \, dx", r"\int_0^1 x \, dx"),
        ] {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_definition_relations_through_the_semantic_stream() {
        let cases = [
            ("x:=y", "x := y"),
            ("a::=b", "a ::= b"),
            ("a:=_ib", "a :=_i b"),
            ("a::=_ib", "a ::=_i b"),
            (r"\mu:=\nu", r"\mu := \nu"),
            (r"x\coloneqq y", r"x \coloneqq y"),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
        assert_eq!(lower_display("x:=y").as_deref(), Some("x := y"));
        assert_eq!(
            lower_display_width("A := bbbbbbbbbb = cccccccccc", 20).as_deref(),
            Some("A := bbbbbbbbbb\n  = cccccccccc"),
        );
        let authored = concat!(r"A :=_i a \\", "\n", r":=_j b \\", "\n", "= c");
        assert_eq!(lower_display(authored).as_deref(), Some(authored));
    }

    #[test]
    fn lowers_ordinary_groups_recursively() {
        let cases = [
            ("{ a+b }", "{a + b}"),
            ("a+{b-c}", "a + {b - c}"),
            ("a {- b}", "a {-b}"),
            ("{{ α<=β }}", "{{α <= β}}"),
            ("{   }", "{}"),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_signature_proven_math_arguments_and_command_shells() {
        let cases = [
            (r"\frac{ a+b }{ c-d }", r"\frac{a + b}{c - d}"),
            (r"\sqrt{ a+b }", r"\sqrt{a + b}"),
            (r"\sqrt[ a+b ]{ c-d }", r"\sqrt[a + b]{c - d}"),
            (r"\frac { a+b } { c-d }", r"\frac {a + b} {c - d}"),
            (r"x+\frac{{ a+b }}{c}", r"x + \frac{{a + b}}{c}"),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_edge_comments_in_signature_proven_math_arguments() {
        let cases = [
            (
                "\\frac{% numerator\n a+b}{c}",
                "\\frac{% numerator\n a + b}{c}",
            ),
            (
                "\\frac{a+b % numerator\n}{c}",
                "\\frac{a + b % numerator\n }{c}",
            ),
            ("\\sqrt[% index\n n+1]{x}", "\\sqrt[% index\n n + 1]{x}"),
            (
                "\\frac{a % keep this comment\n+b}{c}",
                "\\frac{a % keep this comment\n + b}{c}",
            ),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_bare_commands_through_the_semantic_stream() {
        let cases = [
            (r"\alpha+\beta", r"\alpha + \beta"),
            (r"a\cdot b", r"a \cdot b"),
            (r"x\leq-y", r"x \leq -y"),
            (r"\sin x", r"\sin x"),
            (r"\unknown x", r"\unknown x"),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_supported_scripts_through_the_semantic_stream() {
        let cases = [
            ("x^2", "x^2"),
            ("x _ i ^ { a+b }", "x_i^{a+b}"),
            (r"\alpha_i+\beta^2", r"\alpha_i + \beta^2"),
            (r"\frac{ a+b }{c}^2", r"\frac{a + b}{c}^2"),
            ("{ a+b }^2", "{a + b}^2"),
            ("e^{x_i^2}", "e^{x_i^2}"),
            (r"\sum_{i=1}^{n} i", r"\sum_{i=1}^{n} i"),
            (r"x^\alpha+y_\beta", r"x^\alpha + y_\beta"),
            (r"x^{a\in A}", r"x^{a \in A}"),
            (r"x^{\alpha b}", r"x^{\alpha b}"),
            ("x^{( a )}", "x^{(a)}"),
            ("x^{a/ b}", "x^{a / b}"),
            (r"x^{\frac{a+b}{c-d}}", r"x^{\frac{a+b}{c-d}}"),
            (r"a\leq_i-b", r"a \leq_i -b"),
            (r"e^{- t}", r"e^{-t}"),
            (r"a: =_ib", r"a: =_i b"),
            (r"x< =_iy", r"x < =_i y"),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_closed_paired_delimiters_with_supported_plain_bodies() {
        let cases = [
            (r"\left (  a+b  \right )", r"\left( a + b \right)"),
            (
                r"x+\left[ \frac{ a+b }{c} \right]",
                r"x + \left[ \frac{a + b}{c} \right]",
            ),
            (r"\left.   \alpha   \right|", r"\left. \alpha \right|"),
            (
                r"\left\langle x \right\rangle",
                r"\left\langle x \right\rangle",
            ),
            (r"\left(   \right)", r"\left(\right)"),
            (r"\left x \right)", r"\leftx\right)"),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_supported_scripts_inside_paired_delimiters() {
        let cases = [
            (
                r"\left( x _ i + y ^ { a+b } \right)",
                r"\left( x_i + y^{a+b} \right)",
            ),
            (
                r"\left[ \frac{ a+b }{c}^2 \right]",
                r"\left[ \frac{a + b}{c}^2 \right]",
            ),
            (r"x ^ { \left( a+b \right) }", r"x^{\left( a+b \right)}"),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_scripted_paired_delimiter_bases() {
        let cases = [
            (r"\left( x \right) ^ 2", r"\left( x \right)^2"),
            (
                r"a + \left[ b+c \right] _ { i+j }",
                r"a + \left[ b + c \right]_{i+j}",
            ),
            (r"\left. x_i \right| _ 0 ^ 1", r"\left. x_i \right|_0^1"),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_nested_paired_delimiters_recursively() {
        let cases = [
            (
                r"\left[ \left( a+b \right) + c \right]",
                r"\left[ \left( a + b \right) + c \right]",
            ),
            (
                r"x ^ { \left[ \left( a+b \right) \right] }",
                r"x^{\left[ \left( a+b \right) \right]}",
            ),
            (
                r"\left( \left[ x \right] ^ 2 \right) _ i",
                r"\left( \left[ x \right]^2 \right)_i",
            ),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_leading_and_trailing_top_level_comments() {
        let cases = [
            ("% leading comment\nx = 1\n", "% leading comment\nx = 1"),
            ("% base comment\nx^2", "% base comment\nx^2"),
            ("a + b % this is a comment\n", "a + b % this is a comment\n"),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_edge_comments_in_ordinary_groups() {
        let cases = [
            ("{a+b % inner\n}", "{a + b % inner\n }"),
            ("{% inner\n a+b}", "{% inner\n a + b}"),
            ("{a % inner\n+b}", "{a % inner\n + b}"),
            ("{ % only\n }", "{% only\n }"),
            ("{{a % inner\n}}", "{{a % inner\n  }}"),
            ("{a+b % inner\n} + c", "{a + b % inner\n } + c"),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_edge_comments_in_bracketed_bodies_one_column_per_level() {
        let cases = [
            ("\\frac{{a % inner\n}}{c}", "\\frac{{a % inner\n  }}{c}"),
            ("\\sqrt[{a % inner\n}]{b}", "\\sqrt[{a % inner\n  }]{b}"),
            ("{\\frac{a % inner\n}{b}}", "{\\frac{a % inner\n  }{b}}"),
            (
                "\\left( {a % inner\n} \\right)",
                "\\left( {a % inner\n        } \\right)",
            ),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_edge_comments_in_paired_delimiter_bodies() {
        let cases = [
            (
                "\\left( a % inner\n+ b \\right)",
                "\\left( a % inner\n       + b \\right)",
            ),
            (
                "\\left( % lead\n a+b \\right)",
                "\\left( % lead\n       a + b \\right)",
            ),
            (
                "\\left(a % inner\n\\right)",
                "\\left( a % inner\n        \\right)",
            ),
            (
                "\\left( % only\n \\right)",
                "\\left( % only\n        \\right)",
            ),
            (
                "\\left\\langle a % inner\n+b \\right\\rangle",
                "\\left\\langle a % inner\n             + b \\right\\rangle",
            ),
            (
                "\\left( \\left[ a % inner\n \\right] \\right)",
                "\\left( \\left[ a % inner\n               \\right] \\right)",
            ),
            (
                "\\left( a % inner\n \\right)^2",
                "\\left( a % inner\n        \\right)^2",
            ),
            (
                "\\frac{\\left( a % inner\n \\right)}{b}",
                "\\frac{\\left( a % inner\n         \\right)}{b}",
            ),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_edge_comments_in_supported_script_arguments() {
        let cases = [
            ("x^{a % inner\n+b}", "x^{a % inner\n +b}"),
            ("x^{% inner\n a}", "x^{% inner\n a}"),
            ("x^{a+b % inner\n}", "x^{a+b % inner\n }"),
            ("x_{a % inner\n}^2", "x_{a % inner\n }^2"),
            ("x^{{a % inner\n}}", "x^{{a % inner\n  }}"),
            (
                "x^{a % inner\n}_{b % other\n}",
                "x^{a % inner\n }_{b % other\n }",
            ),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn carries_operator_context_across_mid_expression_comments() {
        let cases = [
            (
                "a% operand before comment\n+b",
                "a% operand before comment\n+ b",
            ),
            (
                "a+% binary before comment\n-b",
                "a +% binary before comment\n-b",
            ),
            (
                "a=% relation before comment\n-b",
                "a =% relation before comment\n-b",
            ),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_authored_line_breaks() {
        let cases = [
            ("a\\\\b", "a\\\\\nb"),
            ("a \\\\*[2ex]\n-b", "a \\\\*[2ex]\n- b"),
            ("a+b\\\\c-d", "a + b\\\\\nc - d"),
            ("{a+b\\\\c-d}", "{a + b\\\\\n c - d}"),
            ("\\frac{a+b\\\\c-d}{e}", "\\frac{a + b\\\\\n c - d}{e}"),
            (
                "\\left( a+b\\\\c-d \\right)",
                "\\left( a + b\\\\\n       c - d \\right)",
            ),
        ];

        for (input, expected) in cases {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn lowers_comments_after_authored_line_breaks() {
        let input = "a \\\\ % first row\nb";
        assert_eq!(lower(input).as_deref(), Some("a \\\\\n% first row\nb"));
    }

    #[test]
    fn authored_line_break_lowering_is_idempotent() {
        for input in [
            "a\\\\b",
            "a \\\\*[2ex]\n-b",
            "a+b\\\\c-d",
            "{a+b\\\\c-d}",
            "\\frac{a+b\\\\c-d}{e}",
            "\\left( a+b\\\\c-d \\right)",
            "a \\\\ % first row\nb",
        ] {
            let once = lower(input).expect("supported authored line break");
            let twice = lower(&once).expect("formatted authored line break");
            assert_eq!(once, twice, "not idempotent: {input:?}");
        }
    }

    #[test]
    fn aligns_supported_relation_chains_across_authored_breaks() {
        let cases = [
            ("x = a \\\\\n= b \\\\\n= c", "x = a \\\\\n  = b \\\\\n  = c"),
            (
                "x \\gets a \\\\\n= b \\\\\n= c",
                "x \\gets a \\\\\n        = b \\\\\n        = c",
            ),
            ("x =_i a \\\\\n=_j b", "x =_i a \\\\\n  =_j b"),
            (
                "x \\gets a \\\\\n\\leftarrow b \\\\\n= c",
                "x \\gets a \\\\\n  \\leftarrow b \\\\\n        = c",
            ),
            ("x \\\\\n= b", "x \\\\\n  = b"),
        ];

        for (input, expected) in cases {
            let once = lower_display(input).expect("supported authored relation chain");
            assert_eq!(once, expected, "{input:?}");
            assert_eq!(lower_display(&once).as_deref(), Some(once.as_str()));
        }

        assert_eq!(
            lower_display("{x = a \\\\\n= b}").as_deref(),
            Some("{x = a \\\\\n = b}"),
            "nested authored rows must not acquire top-level relation alignment",
        );
    }

    #[test]
    fn lowers_width_driven_relation_and_binary_breaks() {
        let cases = [
            (
                "A = aaaaaaaaaa + bbbbbbbbbb = cccccccccc + dddddddddd",
                "A = aaaaaaaaaa\n    + bbbbbbbbbb\n  = cccccccccc\n    + dddddddddd",
            ),
            (
                concat!(r"x = a \\", "\n", "= bbbbbbbb + cccccccc + dddddddd"),
                concat!(
                    r"x = a \\",
                    "\n  = bbbbbbbb\n    + cccccccc\n    + dddddddd"
                ),
            ),
        ];

        for (input, expected) in cases {
            let once = lower_display_width(input, 20).expect("supported display wrapping");
            assert_eq!(once, expected, "{input:?}");
            assert_eq!(
                lower_display_width(&once, 20).as_deref(),
                Some(once.as_str()),
            );
        }
    }

    #[test]
    fn rejects_malformed_scripts() {
        for input in ["x^", "x^% argument comment\n2"] {
            assert_eq!(lower(input), None, "{input:?}");
        }
    }

    #[test]
    fn lowers_scripted_composite_relations_as_one_atom() {
        for (input, expected) in [
            (r"x<=_iy", r"x <=_i y"),
            (r"x>=_iy", r"x >=_i y"),
            (r"a==_kb", r"a ==_k b"),
        ] {
            assert_eq!(lower(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn defers_paired_delimiter_recovery_and_shell_comments() {
        for input in [
            r"\left( x",
            r"\left( x \right",
            "\\left % keep\n( x \\right)",
        ] {
            assert_eq!(lower(input), None, "{input:?}");
        }
    }

    #[test]
    fn lowers_unattached_required_commands_but_rejects_redefined_semantics() {
        for input in [r"\frac", r"\sqrt", r"\text"] {
            assert_eq!(lower(input).as_deref(), Some(input), "{input:?}");
        }

        let document = parse("\\newcommand{\\leq}{x}\n\n$\\leq$\n", None);
        let scope = SignatureScope::from_root(&document);
        assert!(scope.is_redefined("leq"));
        assert!(try_lower_content(&content(r"\leq"), &scope).is_none());
    }

    #[test]
    fn rejects_commands_without_a_complete_math_signature_match() {
        for input in [r"\frac{a}", r"\frac{a}{b", "\\frac% keep\n{a}{b}"] {
            assert_eq!(lower(input), None, "{input:?}");
        }
    }

    #[test]
    fn preserves_nonmath_and_unproven_argument_domains_opaquely() {
        for input in [
            r"\text{ a+b }",
            r"\text{ a+b }^2",
            r"\unknown{ a+b }",
            r"\frac{a}{b}{c}",
        ] {
            assert_eq!(lower(input).as_deref(), Some(input), "{input:?}");
        }
    }

    #[test]
    fn rejects_every_unsupported_shape_category() {
        let cases = [
            "a+b\n% own line",
            r"a\\[1ex",
            "a&b",
            r"\begin{matrix}x\end{matrix}",
        ];

        for input in cases {
            assert_eq!(lower(input), None, "{input:?}");
        }
    }
}
