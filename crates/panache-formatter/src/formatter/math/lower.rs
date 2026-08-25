//! Typed CST lowering for the Badness-parity math formatter.

use panache_parser::semantic::math::{MathBreakPriority, semantic_math_atoms};
use rowan::TextRange;
use rowan::ast::AstNode;

use crate::syntax::{MathContent, SyntaxKind, SyntaxToken};

use super::ir::Ir;

/// Lower the first migration slice: a flat inline list of words and trivia.
///
/// Returning `None` keeps every unsupported shape on the legacy renderer until
/// its own parity slice lands.
pub(super) fn try_lower_flat_inline(content: &MathContent) -> Option<Ir> {
    if !content.elements().all(|element| {
        element.as_token().is_some()
            && matches!(
                element.kind(),
                SyntaxKind::MATH_WORD | SyntaxKind::MATH_SPACE | SyntaxKind::MATH_NEWLINE
            )
    }) {
        return None;
    }

    let words = content
        .elements()
        .filter_map(|element| element.into_token())
        .filter(|token| token.kind() == SyntaxKind::MATH_WORD)
        .collect::<Vec<_>>();
    let mut pieces = Vec::new();
    let mut previous_end = None;
    let mut word_index = 0;

    for atom in semantic_math_atoms(content.syntax()) {
        let text = atom_text(atom.range, &words, &mut word_index)?;
        pieces.push(Piece {
            role: Role::from(atom.break_priority),
            authored_space_before: previous_end.is_some_and(|end| end < atom.range.start()),
            slash: text == "/",
            text,
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
        documents.push(Ir::verbatim(piece.text.clone()));
    }

    Some(Ir::concat(documents))
}

struct Piece {
    role: Role,
    authored_space_before: bool,
    slash: bool,
    text: String,
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

fn atom_text(range: TextRange, words: &[SyntaxToken], word_index: &mut usize) -> Option<String> {
    while words
        .get(*word_index)
        .is_some_and(|word| word.text_range().end() <= range.start())
    {
        *word_index += 1;
    }
    let word = words.get(*word_index)?;
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
        try_lower_flat_inline(&content(input))
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
    fn rejects_every_unsupported_shape_category() {
        let cases = [
            r"\alpha",
            "{x}",
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
