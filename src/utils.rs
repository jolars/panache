use crate::syntax::SyntaxKind;

/// Check if a syntax kind represents a block-level element for formatting purposes.
/// This determines when to add blank lines between elements.
pub fn is_block_element(kind: SyntaxKind) -> bool {
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

/// Check if a syntax kind represents a block-level element for range filtering.
/// This is more comprehensive than is_block_element and includes all structural blocks.
pub fn is_structural_block(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PARAGRAPH
            | SyntaxKind::Heading
            | SyntaxKind::CodeBlock
            | SyntaxKind::BlockQuote
            | SyntaxKind::List
            | SyntaxKind::ListItem
            | SyntaxKind::DefinitionList
            | SyntaxKind::DefinitionItem
            | SyntaxKind::LineBlock
            | SyntaxKind::SimpleTable
            | SyntaxKind::MultilineTable
            | SyntaxKind::PipeTable
            | SyntaxKind::GridTable
            | SyntaxKind::FencedDiv
            | SyntaxKind::HorizontalRule
            | SyntaxKind::YamlMetadata
            | SyntaxKind::PandocTitleBlock
            | SyntaxKind::HtmlBlock
            | SyntaxKind::MathBlock
            | SyntaxKind::BlankLine
            | SyntaxKind::ReferenceDefinition
            | SyntaxKind::FootnoteDefinition
    )
}
