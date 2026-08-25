//! Typed CST lowering for the Badness-parity math formatter.

use panache_parser::semantic::math::{
    ArgKind, ArgumentDomain, DelimiterRole, MathBreakPriority, MathClass, SemanticMathAtom,
    SignatureScope, match_arg_slot, math_atoms, semantic_math_atoms_in,
};
use rowan::TextRange;
use rowan::ast::AstNode;

use crate::syntax::{
    MathArgument, MathCommand, MathContent, MathDelimited, MathGroup, MathScript, MathScripted,
    SyntaxElement, SyntaxKind, SyntaxToken,
};

use super::ir::Ir;

/// Lower inline words, trivia, ordinary groups, supported commands, and scripts.
///
/// Returning `None` keeps every unsupported shape on the legacy renderer until
/// its own parity slice lands.
pub(super) fn try_lower_inline(content: &MathContent, scope: &SignatureScope) -> Option<Ir> {
    lower_elements(content.elements().collect(), scope, Spacing::Normal)
}

fn lower_elements(
    elements: Vec<SyntaxElement>,
    scope: &SignatureScope,
    spacing: Spacing,
) -> Option<Ir> {
    if has_split_definition_relation(&elements)
        || has_scripted_composite_relation(&elements)
        || !elements
            .iter()
            .all(|element| is_supported_element(element, scope))
    {
        return None;
    }

    let mut pieces = Vec::new();
    let mut previous_end = None;

    for atom in semantic_math_atoms_in(elements.iter().cloned()) {
        let atom_document = atom_document(atom, &elements, scope, spacing)?;
        pieces.push(Piece {
            role: Role::from(atom.break_priority),
            delimiter: atom.delimiter,
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
    // owns the script (`a:` + `=_i`, or `x<` + `=_i`). Keep those seams on the
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
            ':' => base == '=',
            '=' | '<' | '>' => matches!(base, '=' | '<' | '>'),
            _ => false,
        }
    })
}

fn has_split_definition_relation(elements: &[SyntaxElement]) -> bool {
    // The legacy renderer repairs this CST boundary into one `:=` atom; keep
    // the whole list there until the shared semantic stream owns that repair.
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
                    && group
                        .body_elements()
                        .all(|element| is_supported_element(&element, scope))
            }) || MathCommand::cast(node.clone())
                .is_some_and(|command| command_is_supported(&command, scope))
                || MathDelimited::cast(node.clone())
                    .is_some_and(|delimited| delimited_is_supported(&delimited, scope))
                || MathScripted::cast(node.clone())
                    .is_some_and(|scripted| scripted_is_supported(&scripted, scope))
        }
    }
}

fn atom_document(
    atom: SemanticMathAtom,
    elements: &[SyntaxElement],
    scope: &SignatureScope,
    spacing: Spacing,
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
                let body = lower_elements(group.body_elements().collect(), scope, spacing)?;
                (
                    Ir::concat([Ir::verbatim(open.text()), body, Ir::verbatim(close.text())]),
                    false,
                )
            } else if let Some(command) = MathCommand::cast(node.clone()) {
                (lower_command(&command, scope, spacing)?, false)
            } else if let Some(delimited) = MathDelimited::cast(node.clone()) {
                (lower_delimited(&delimited, scope, spacing)?, false)
            } else {
                let scripted = MathScripted::cast(node.clone())?;
                (lower_scripted(&scripted, scope, spacing)?, false)
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
        let inner = lower_elements(body.elements().collect(), scope, spacing)?;
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
    }) && body
        .elements()
        .all(|element| is_supported_element(&element, scope))
}

fn lower_scripted(scripted: &MathScripted, scope: &SignatureScope, spacing: Spacing) -> Option<Ir> {
    let base = scripted.base()?;
    let scripts = scripted.scripts().collect::<Vec<_>>();
    if scripts.is_empty() || !scripted_is_supported(scripted, scope) {
        return None;
    }

    let mut documents = vec![lower_elements(vec![base], scope, spacing)?];
    for script in scripts {
        let marker = script.marker_token()?;
        let argument = script.argument()?;
        documents.push(Ir::verbatim(marker.text()));
        documents.push(lower_elements(vec![argument], scope, Spacing::Script)?);
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

fn lower_command(command: &MathCommand, scope: &SignatureScope, spacing: Spacing) -> Option<Ir> {
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
        let body = lower_elements(argument.body_elements().collect(), scope, spacing)?;
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

    match spacing {
        Spacing::Normal => {
            current.role != Role::Operand
                || previous.role != Role::Operand
                || current.authored_space_before
        }
        Spacing::Script => {
            current.control_word_operator
                || previous.control_word_operator
                || current.role == Role::Operand
                    && previous.role == Role::Operand
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

fn is_control_word_letter(character: char) -> bool {
    character.is_alphabetic() || matches!(character, '@' | '_' | ':')
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

    fn lower(input: &str) -> Option<String> {
        try_lower_inline(&content(input), &SignatureScope::default())
            .map(|document| Printer::new(80, 2).print_flat(&document))
    }

    #[test]
    fn lowers_flat_words_and_trivia() {
        let cases = [
            ("  a  b\nc  ", "a b c"),
            ("α+β", "α + β"),
            ("x=-y", "x = -y"),
            ("a--b", "a - -b"),
            ("- x", "- x"),
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
            ("a {- b}", "a {- b}"),
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
            (r"e^{- t}", r"e^{- t}"),
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
    fn rejects_malformed_and_unsupported_scripts() {
        for input in [
            "x^",
            "% base comment\nx^2",
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
        assert!(try_lower_inline(&content(r"\leq"), &scope).is_none());
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
            "% comment\nx",
            r"a\\b",
            "a&b",
            r"\begin{matrix}x\end{matrix}",
        ];

        for input in cases {
            assert_eq!(lower(input), None, "{input:?}");
        }
    }
}
