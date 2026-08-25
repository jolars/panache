//! Typed CST lowering for the Badness-parity math formatter.

use panache_parser::semantic::math::{
    ArgKind, ArgumentDomain, MathBreakPriority, SignatureScope, match_arg_slot,
    semantic_math_atoms_in,
};
use rowan::TextRange;
use rowan::ast::AstNode;

use crate::syntax::{
    MathArgument, MathCommand, MathContent, MathGroup, SyntaxElement, SyntaxKind, SyntaxToken,
};

use super::ir::Ir;

/// Lower inline words, trivia, ordinary groups, and signature-proven commands.
///
/// Returning `None` keeps every unsupported shape on the legacy renderer until
/// its own parity slice lands.
pub(super) fn try_lower_inline(content: &MathContent, scope: &SignatureScope) -> Option<Ir> {
    lower_elements(content.elements().collect(), scope)
}

fn lower_elements(elements: Vec<SyntaxElement>, scope: &SignatureScope) -> Option<Ir> {
    if !elements
        .iter()
        .all(|element| is_supported_element(element, scope))
    {
        return None;
    }

    let mut pieces = Vec::new();
    let mut previous_end = None;

    for atom in semantic_math_atoms_in(elements.iter().cloned()) {
        let (document, slash) = atom_document(atom.range, &elements, scope)?;
        pieces.push(Piece {
            role: Role::from(atom.break_priority),
            authored_space_before: previous_end.is_some_and(|end| end < atom.range.start()),
            slash,
            document,
        });
        previous_end = Some(atom.range.end());
    }

    let base_gaps = (0..pieces.len())
        .map(|index| gap_before(&pieces, index))
        .collect::<Vec<_>>();
    let spaced_slashes = (0..pieces.len())
        .map(|index| {
            pieces[index].slash
                && (base_gaps[index] || base_gaps.get(index + 1).copied().unwrap_or(false))
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
                .is_some_and(|command| matched_math_arguments(&command, scope).is_some())
        }
    }
}

fn atom_document(
    range: TextRange,
    elements: &[SyntaxElement],
    scope: &SignatureScope,
) -> Option<(Ir, bool)> {
    let element = elements.iter().find(|element| {
        element.text_range().start() <= range.start() && element.text_range().end() >= range.end()
    })?;
    match element {
        SyntaxElement::Token(token) if token.kind() == SyntaxKind::MATH_WORD => {
            let text = token_slice(range, token)?;
            let slash = text == "/";
            Some((Ir::verbatim(text), slash))
        }
        SyntaxElement::Node(node) => {
            if let Some(group) = MathGroup::cast(node.clone()) {
                let open = group.open_token()?;
                let close = group.close_token()?;
                let body = lower_elements(group.body_elements().collect(), scope)?;
                Some((
                    Ir::concat([Ir::verbatim(open.text()), body, Ir::verbatim(close.text())]),
                    false,
                ))
            } else {
                let command = MathCommand::cast(node.clone())?;
                Some((lower_command(&command, scope)?, false))
            }
        }
        _ => None,
    }
}

fn lower_command(command: &MathCommand, scope: &SignatureScope) -> Option<Ir> {
    let arguments = matched_math_arguments(command, scope)?;
    let name = command.name_token()?;
    let mut previous_end = name.text_range().end();
    let mut documents = vec![Ir::verbatim(name.text())];
    if let Some(star) = command.star_token() {
        documents.push(Ir::verbatim(star.text()));
        previous_end = star.text_range().end();
    }
    for argument in arguments {
        let open = argument.open_token()?;
        let close = argument.close_token()?;
        let body = lower_elements(argument.body_elements().collect(), scope)?;
        if previous_end < argument.syntax().text_range().start() {
            documents.push(Ir::text(" "));
        }
        documents.extend([Ir::verbatim(open.text()), body, Ir::verbatim(close.text())]);
        previous_end = argument.syntax().text_range().end();
    }
    Some(Ir::concat(documents))
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
    authored_space_before: bool,
    slash: bool,
    document: Ir,
}

fn gap_before(pieces: &[Piece], index: usize) -> bool {
    let Some(previous) = index.checked_sub(1).and_then(|index| pieces.get(index)) else {
        return false;
    };
    let current = &pieces[index];
    current.role != Role::Operand || previous.role != Role::Operand || current.authored_space_before
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
            r"\alpha",
            "x^2",
            "% comment\nx",
            r"a\\b",
            "a&b",
            r"\left(x\right)",
            r"\begin{matrix}x\end{matrix}",
        ];

        for input in cases {
            assert_eq!(lower(input), None, "{input:?}");
        }
    }
}
