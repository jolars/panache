use crate::parser::block_parser::tests::helpers::assert_block_kinds;
use crate::syntax::SyntaxKind;

#[test]
fn blankline_between_paragraphs() {
    assert_block_kinds(
        "Paragraph 1\n\nParagraph 2\n",
        &[
            SyntaxKind::PARAGRAPH,
            SyntaxKind::BLANK_LINE,
            SyntaxKind::PARAGRAPH,
        ],
    );
}

#[test]
fn multiple_blanklines_between_paragraphs() {
    assert_block_kinds(
        "Paragraph 1\n\n\n\nParagraph 2\n",
        &[
            SyntaxKind::PARAGRAPH,
            SyntaxKind::BLANK_LINE,
            SyntaxKind::BLANK_LINE,
            SyntaxKind::BLANK_LINE,
            SyntaxKind::PARAGRAPH,
        ],
    );
}

#[test]
fn blankline_before_paragraph() {
    assert_block_kinds(
        "\nParagraph 1\n",
        &[SyntaxKind::BLANK_LINE, SyntaxKind::PARAGRAPH],
    );
}
