use crate::syntax::SyntaxKind;

pub(super) fn is_block_element(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PARAGRAPH
            | SyntaxKind::List
            | SyntaxKind::DefinitionList
            | SyntaxKind::BlockQuote
            | SyntaxKind::MathBlock
            | SyntaxKind::CodeBlock
            | SyntaxKind::SimpleTable
            | SyntaxKind::MultilineTable
            | SyntaxKind::PipeTable
            | SyntaxKind::LineBlock
    )
}
