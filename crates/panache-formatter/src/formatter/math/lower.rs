//! Typed CST lowering for the Badness-parity math formatter.

use panache_parser::semantic::math::{MathBreakPriority, semantic_math_atoms_in};
use rowan::TextRange;
use rowan::ast::AstNode;

use crate::syntax::{MathContent, MathGroup, SyntaxElement, SyntaxKind, SyntaxToken};

use super::ir::Ir;

/// Lower inline words, trivia, and recursively nested ordinary brace groups.
///
/// Returning `None` keeps every unsupported shape on the legacy renderer until
/// its own parity slice lands.
pub(super) fn try_lower_inline(content: &MathContent) -> Option<Ir> {
    lower_elements(content.elements().collect())
}

fn lower_elements(elements: Vec<SyntaxElement>) -> Option<Ir> {
    if !elements.iter().all(is_supported_element) {
        return None;
    }

    let mut pieces = Vec::new();
    let mut previous_end = None;

    for atom in semantic_math_atoms_in(elements.iter().cloned()) {
        let (document, slash) = atom_document(atom.range, &elements)?;
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

fn is_supported_element(element: &SyntaxElement) -> bool {
    match element {
        SyntaxElement::Token(token) => matches!(
            token.kind(),
            SyntaxKind::MATH_WORD | SyntaxKind::MATH_SPACE | SyntaxKind::MATH_NEWLINE
        ),
        SyntaxElement::Node(node) => MathGroup::cast(node.clone()).is_some_and(|group| {
            group.is_closed()
                && group
                    .body_elements()
                    .all(|element| is_supported_element(&element))
        }),
    }
}

fn atom_document(range: TextRange, elements: &[SyntaxElement]) -> Option<(Ir, bool)> {
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
            let group = MathGroup::cast(node.clone())?;
            let open = group.open_token()?;
            let close = group.close_token()?;
            let body = lower_elements(group.body_elements().collect())?;
            Some((
                Ir::concat([Ir::verbatim(open.text()), body, Ir::verbatim(close.text())]),
                false,
            ))
        }
        _ => None,
    }
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
        try_lower_inline(&content(input)).map(|document| Printer::new(80, 2).print_flat(&document))
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
