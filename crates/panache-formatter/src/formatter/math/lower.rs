//! Typed CST lowering for the Badness-parity math formatter.

use panache_parser::semantic::math::{
    ArgKind, ArgumentDomain, DelimiterRole, MathBreakPriority, MathClass, SemanticMathAtom,
    SignatureScope, match_arg_slot, math_atoms, semantic_math_atoms_in,
};
use rowan::TextRange;
use rowan::ast::AstNode;

use crate::syntax::{
    MathArgument, MathCommand, MathContent, MathDelimited, MathEnvironment, MathGroup,
    MathLineBreak, MathScript, MathScripted, SyntaxElement, SyntaxKind, SyntaxToken,
};

use super::ir::Ir;
use super::{MathFormatOptions, render};

/// Lower a supported math content body into the shared document IR.
///
/// Returning `None` keeps every unsupported shape on the legacy renderer until
/// its own parity slice lands.
pub(super) fn try_lower_content(content: &MathContent, scope: &SignatureScope) -> Option<Ir> {
    lower_body(
        content.elements().collect(),
        scope,
        Spacing::Normal,
        true,
        false,
    )
}

/// Lower a closed paired delimiter whose body is one well-formed environment.
///
/// This stays separate from ordinary atom lowering until environments become
/// first-class typed atom documents. The narrow shape lets the existing
/// environment-grid document compose without admitting mixed or malformed
/// delimiter bodies.
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
    let mut body_elements = body.elements().filter(|element| !is_layout_trivia(element));
    let environment = body_elements
        .next()?
        .into_node()
        .and_then(MathEnvironment::cast)?;
    if body_elements.next().is_some() {
        return None;
    }

    let environment = render::environment_document(environment.syntax(), opts)?;
    let opening_width = left.text().chars().count() + open.text().chars().count();
    Some(Ir::concat([
        Ir::verbatim(left.text()),
        Ir::verbatim(open.text()),
        Ir::text(" "),
        Ir::align(opening_width + 1, environment),
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
        Spacing::Normal,
        true,
        true,
    )
}

/// Lower a formatter-derived row or cell without inventing a CST wrapper.
pub(super) fn try_lower_elements(
    elements: Vec<SyntaxElement>,
    scope: &SignatureScope,
) -> Option<Ir> {
    lower_body(elements, scope, Spacing::Normal, true, false)
}

/// Lower a first alignment cell using Badness's line-local comment context.
pub(super) fn try_lower_first_grid_cell(
    elements: Vec<SyntaxElement>,
    scope: &SignatureScope,
) -> Option<Ir> {
    lower_body(elements, scope, Spacing::Normal, false, false)
}

/// Lower a bracketed body, routing comment-bearing bodies through hard lines.
fn lower_body(
    elements: Vec<SyntaxElement>,
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    environment_rows: bool,
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
            preserve_comment_context,
            environment_rows,
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

fn lower_authored_breaks(
    elements: Vec<SyntaxElement>,
    semantic_atoms: Vec<SemanticMathAtom>,
    scope: &SignatureScope,
    spacing: Spacing,
    preserve_comment_context: bool,
    environment_rows: bool,
) -> Option<Ir> {
    struct AuthoredRow {
        document: Ir,
        marker: Option<String>,
        authored_space: bool,
        adjacent_comment: Option<String>,
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
        let row_elements = std::mem::take(&mut row);
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
        });
    }

    let row_atoms = if environment_rows {
        semantic_math_atoms_in(row.iter().cloned()).collect()
    } else {
        semantic_atoms_for(&row, &semantic_atoms)
    };
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
    let mut documents = Vec::new();
    let row_count = rows.len();
    for (index, row) in rows.into_iter().enumerate() {
        let row_width = row.document.flat_width();
        documents.push(row.document);
        let Some(marker) = row.marker else {
            continue;
        };
        if environment_rows {
            let padding = row_width.map_or(1, |width| max_row_width.saturating_sub(width) + 1);
            documents.push(Ir::text(" ".repeat(padding)));
        } else if row.authored_space {
            documents.push(Ir::text(" "));
        }
        documents.push(Ir::verbatim(marker));
        if let Some(comment) = row.adjacent_comment {
            documents.extend([Ir::text(" "), Ir::verbatim(comment)]);
        }
        if index + 1 < row_count {
            documents.push(Ir::HardLine);
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
    if has_definition_relation(&elements)
        || has_scripted_composite_relation(&elements)
        || !elements
            .iter()
            .all(|element| is_supported_element(element, scope))
    {
        return None;
    }

    let mut pieces = Vec::new();
    let mut previous_end = None;

    for atom in semantic_atoms {
        let atom_document = atom_document(
            atom,
            &elements,
            scope,
            spacing,
            preserve_comment_context,
            environment_rows,
        )?;
        pieces.push(Piece {
            role: Role::from(atom.break_priority),
            delimiter: atom.delimiter,
            unary: atom.coerced_unary,
            authored_space_before: previous_end.is_some_and(|end| end < atom.range.start()),
            slash: atom_document.slash,
            control_word_operator: atom_document.control_word_operator,
            starts_control_word_letter: atom_document.starts_control_word_letter,
            ends_control_word: atom_document.ends_control_word,
            document: atom_document.document,
        });
        previous_end = Some(atom.range.end());
    }

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

    let mut documents = Vec::new();
    for (index, piece) in pieces.iter().enumerate() {
        if index > 0 && (base_gaps[index] || spaced_slashes[index - 1] || spaced_slashes[index]) {
            documents.push(Ir::text(" "));
        }
        documents.push(piece.document.clone());
    }

    Some(Ir::concat(documents))
}

fn has_scripted_composite_relation(elements: &[SyntaxElement]) -> bool {
    // The legacy renderer fuses an adjacent relation head with the scalar that
    // owns the script (`a:` + `:=_i`, or `x<` + `=_i`). Keep those seams on the
    // fallback until typed lowering can preserve the established spelling.
    elements.windows(2).any(|pair| {
        let [SyntaxElement::Token(head), SyntaxElement::Node(node)] = pair else {
            return false;
        };
        if head.kind() != SyntaxKind::MATH_WORD
            || head.text_range().end() != node.text_range().start()
        {
            return false;
        }
        let Some(base) = MathScripted::cast(node.clone())
            .and_then(|scripted| scripted.base())
            .and_then(SyntaxElement::into_token)
            .filter(|base| base.kind() == SyntaxKind::MATH_WORD)
        else {
            return false;
        };
        let (Some(head), Some(base)) = (head.text().chars().last(), base.text().chars().next())
        else {
            return false;
        };

        match head {
            ':' => base == ':' || base == '=',
            '=' | '<' | '>' => matches!(base, '=' | '<' | '>'),
            _ => false,
        }
    })
}

fn has_definition_relation(elements: &[SyntaxElement]) -> bool {
    // Keep authored definition-relation spelling on the compatibility path,
    // including the CST boundary created when a command precedes it.
    if elements.iter().any(|element| match element {
        SyntaxElement::Token(token) => {
            token.kind() == SyntaxKind::MATH_WORD && token.text().contains(":=")
        }
        SyntaxElement::Node(node) => node.descendants_with_tokens().any(|descendant| {
            descendant.into_token().is_some_and(|token| {
                token.kind() == SyntaxKind::MATH_WORD && token.text().contains(":=")
            })
        }),
    }) {
        return true;
    }
    elements.iter().enumerate().any(|(index, element)| {
        if !matches!(element, SyntaxElement::Node(node) if MathCommand::cast(node.clone()).is_some()) {
            return false;
        }
        elements[index + 1..]
            .iter()
            .find(|candidate| {
                !matches!(
                    candidate.kind(),
                    SyntaxKind::MATH_SPACE | SyntaxKind::MATH_NEWLINE
                )
            })
            .is_some_and(|candidate| {
                matches!(candidate, SyntaxElement::Token(token) if token.kind() == SyntaxKind::MATH_WORD && token.text().starts_with(":="))
            })
    })
}

fn is_supported_element(element: &SyntaxElement, scope: &SignatureScope) -> bool {
    match element {
        SyntaxElement::Token(token) => matches!(
            token.kind(),
            SyntaxKind::MATH_WORD | SyntaxKind::MATH_SPACE | SyntaxKind::MATH_NEWLINE
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
        SyntaxElement::Token(token) if token.kind() == SyntaxKind::MATH_WORD => {
            let text = token_slice(range, token)?;
            let slash = text == "/";
            (Ir::verbatim(text), slash)
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
    let scripts = scripted.scripts().collect::<Vec<_>>();
    if scripts.is_empty() || !scripted_is_supported(scripted, scope) {
        return None;
    }

    let mut documents = vec![lower_elements(
        vec![base],
        scope,
        spacing,
        preserve_comment_context,
        environment_rows,
    )?];
    for script in scripts {
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
    Some(Ir::concat(documents))
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

    let arguments = matched_math_arguments(command, scope)?;
    let mut previous_end = name.text_range().end();
    let mut documents = vec![Ir::verbatim(name.text())];
    if let Some(star) = command.star_token() {
        documents.push(Ir::verbatim(star.text()));
        previous_end = star.text_range().end();
    }
    for argument in arguments {
        let open = argument.open_token()?;
        let close = argument.close_token()?;
        let elements = argument.body_elements().collect::<Vec<_>>();
        let body = hanging(
            1,
            lower_body(elements, scope, spacing, preserve_comment_context, false)?,
        );
        if previous_end < argument.syntax().text_range().start() {
            documents.push(Ir::text(" "));
        }
        documents.extend([Ir::verbatim(open.text()), body, Ir::verbatim(close.text())]);
        previous_end = argument.syntax().text_range().end();
    }
    Some(Ir::concat(documents))
}

fn command_is_supported(command: &MathCommand, scope: &SignatureScope) -> bool {
    is_supported_bare_command(command, scope) || matched_math_arguments(command, scope).is_some()
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

    scope.command_signature(&name).is_none_or(|signature| {
        signature
            .arguments
            .iter()
            .all(|argument| !argument.required)
    })
}

fn matched_math_arguments(
    command: &MathCommand,
    scope: &SignatureScope,
) -> Option<Vec<MathArgument>> {
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
        let spec = match_arg_slot(&signature.arguments, &mut slot, kind)?;
        if spec.domain != ArgumentDomain::Math {
            return None;
        }
        matched.push(argument);
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
    /// A `+`/`-` that TeX coerced to a unary sign. It binds to the operand
    /// beside it, so it strips the authored space on either side.
    unary: bool,
    authored_space_before: bool,
    slash: bool,
    control_word_operator: bool,
    starts_control_word_letter: bool,
    ends_control_word: bool,
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

    // A binary operator or relation always wins its space, even next to a
    // unary sign (`a - -b`); otherwise a unary sign strips the authored space
    // it would have kept as an ordinary atom (`f( - x)` -> `f(-x)`).
    let tight = previous.unary || current.unary;

    match spacing {
        Spacing::Normal => {
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
        Spacing::Normal => previous
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
    fn rejects_malformed_and_unsupported_scripts() {
        for input in [
            "x^",
            "x^% argument comment\n2",
            r"\text{ a+b }^2",
            r"a:=_ib",
            r"a::=_ib",
            r"x<=_iy",
            r"x>=_iy",
            r"a==_kb",
        ] {
            assert_eq!(lower(input), None, "{input:?}");
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
    fn rejects_redefined_and_incomplete_bare_commands() {
        for input in [r"\frac", r"\sqrt", r"\text", r"\mu:=\nu"] {
            assert_eq!(lower(input), None, "{input:?}");
        }

        let document = parse("\\newcommand{\\leq}{x}\n\n$\\leq$\n", None);
        let scope = SignatureScope::from_root(&document);
        assert!(scope.is_redefined("leq"));
        assert!(try_lower_content(&content(r"\leq"), &scope).is_none());
    }

    #[test]
    fn rejects_commands_without_a_complete_math_signature_match() {
        for input in [
            r"\text{ a+b }",
            r"\unknown{ a+b }",
            r"\frac{a}",
            r"\frac{a}{b}{c}",
            r"\frac{a}{b",
            "\\frac% keep\n{a}{b}",
        ] {
            assert_eq!(lower(input), None, "{input:?}");
        }
    }

    #[test]
    fn rejects_every_unsupported_shape_category() {
        let cases = [
            "x:=y",
            "a::=b",
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
