use super::helpers::{
    assert_block_kinds, assert_block_kinds_for_node, find_all, find_first, parse_blocks,
    parse_blocks_quarto,
};
use crate::syntax::SyntaxKind;

fn get_code_content(node: &crate::syntax::SyntaxNode) -> Option<String> {
    find_first(node, SyntaxKind::CODE_CONTENT).map(|n| n.text().to_string())
}

fn get_code_info_node(node: &crate::syntax::SyntaxNode) -> Option<crate::syntax::SyntaxNode> {
    node.descendants()
        .find(|element| element.kind() == SyntaxKind::CODE_INFO)
}

fn get_code_info(node: &crate::syntax::SyntaxNode) -> Option<String> {
    get_code_info_node(node).map(|n| n.text().to_string())
}

#[test]
fn parses_simple_backtick_code_block() {
    let input = "```\nprint(\"hello\")\n```\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CODE_BLOCK]);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "print(\"hello\")\n");
}

#[test]
fn parses_simple_tilde_code_block() {
    let input = "~~~\nprint(\"hello\")\n~~~\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CODE_BLOCK]);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "print(\"hello\")\n");
}

#[test]
fn parses_code_block_with_language() {
    let input = "```python\nprint(\"hello\")\n```\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CODE_BLOCK]);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "print(\"hello\")\n");

    let info = get_code_info(&node).unwrap();
    assert_eq!(info, "python");
}

#[test]
fn parses_code_block_with_attributes() {
    let input = "```{python}\nprint(\"hello\")\n```\n";
    let node = parse_blocks_quarto(input);

    assert_block_kinds_for_node(&node, &[SyntaxKind::CODE_BLOCK], input);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "print(\"hello\")\n");

    let info = get_code_info(&node).unwrap();
    assert_eq!(info, "{python}");
}

#[test]
fn parses_code_block_with_complex_attributes() {
    let input = "```{python #mycode .numberLines startFrom=\"100\"}\nprint(\"hello\")\n```\n";
    let node = parse_blocks_quarto(input);

    assert_block_kinds_for_node(&node, &[SyntaxKind::CODE_BLOCK], input);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "print(\"hello\")\n");

    let info = get_code_info(&node).unwrap();
    assert_eq!(info, "{python #mycode .numberLines startFrom=\"100\"}");
}

#[test]
fn parses_multiline_code_block() {
    let input = "```python\nfor i in range(10):\n    print(i)\n```\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CODE_BLOCK]);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "for i in range(10):\n    print(i)\n");
}

#[test]
fn code_block_can_interrupt_paragraph() {
    // Fenced code blocks with language identifiers can interrupt paragraphs
    // Bare fences (```) require a blank line to avoid ambiguity with inline code
    let input = "text\n```python\ncode\n```\n";
    let node = parse_blocks(input);

    // Should parse as paragraph followed by code block
    assert_block_kinds_for_node(
        &node,
        &[SyntaxKind::PARAGRAPH, SyntaxKind::CODE_BLOCK],
        input,
    );

    let code_content = get_code_content(&node).unwrap();
    assert_eq!(code_content, "code\n");
}

#[test]
fn bare_fence_requires_blank_line() {
    // Bare ``` without info string requires blank line to avoid confusion with inline code
    let input = "text\n```\ncode\n```\n";
    // Use full parse to get inline parsing too
    let tree = crate::parse(input, None);

    // Should parse as single paragraph
    let paragraphs: Vec<_> = tree
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::PARAGRAPH)
        .collect();
    assert_eq!(paragraphs.len(), 1, "Should have one paragraph");

    // Pandoc treats this as an inline code span spanning lines.
    let code_span = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CODE_SPAN);
    assert!(code_span.is_some(), "Should contain inline code span");
}

#[test]
fn code_block_with_language_can_interrupt_paragraph() {
    // Test with language identifier
    let input = "Some text:\n```r\na <- 1\n```\n";
    let node = parse_blocks(input);

    assert_block_kinds_for_node(
        &node,
        &[SyntaxKind::PARAGRAPH, SyntaxKind::CODE_BLOCK],
        input,
    );

    let code_content = get_code_content(&node).unwrap();
    assert_eq!(code_content, "a <- 1\n");

    let info = get_code_info(&node).unwrap();
    assert_eq!(info, "r");
}

#[test]
fn bare_fence_after_colon_with_closing_fence_can_interrupt_paragraph() {
    let input = "Some text:\n```\ncode\n```\n";
    let node = parse_blocks(input);
    assert_block_kinds_for_node(
        &node,
        &[SyntaxKind::PARAGRAPH, SyntaxKind::CODE_BLOCK],
        input,
    );
}

#[test]
fn parses_code_block_at_start_of_document() {
    let input = "```\ncode\n```\n";

    assert_block_kinds(input, &[SyntaxKind::CODE_BLOCK]);
}

#[test]
fn parses_code_block_after_blank_line() {
    let input = "text\n\n```\ncode\n```\n";
    let node = parse_blocks(input);

    let blocks: Vec<_> = node
        .descendants()
        .filter(|n| matches!(n.kind(), SyntaxKind::PARAGRAPH | SyntaxKind::CODE_BLOCK))
        .collect();

    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].kind(), SyntaxKind::PARAGRAPH);
    assert_eq!(blocks[1].kind(), SyntaxKind::CODE_BLOCK);
}

#[test]
fn requires_at_least_three_fence_chars() {
    let input = "``\ncode\n``\n";
    let node = parse_blocks(input);

    // Should not parse as code block
    assert!(find_first(&node, SyntaxKind::CODE_BLOCK).is_none());
}

#[test]
fn closing_fence_must_have_at_least_same_length() {
    let input = "````\ncode\n```\n";
    let node = parse_blocks(input);

    // Code block should be parsed, but without proper closing
    assert!(find_first(&node, SyntaxKind::CODE_BLOCK).is_some());

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "code\n```\n"); // The ``` becomes part of content
}

#[test]
fn closing_fence_can_be_longer() {
    let input = "```\ncode\n`````\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CODE_BLOCK]);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "code\n");
}

#[test]
fn mixed_fence_chars_dont_close() {
    let input = "```\ncode\n~~~\n";
    let node = parse_blocks(input);

    // Should parse code block but ~~~ becomes content
    assert!(find_first(&node, SyntaxKind::CODE_BLOCK).is_some());

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "code\n~~~\n");
}

#[test]
fn empty_code_block() {
    let input = "```\n```\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CODE_BLOCK]);

    // Should have no content node for empty blocks
    assert!(get_code_content(&node).is_none());
}

#[test]
fn code_block_with_leading_spaces() {
    let input = "  ```python\n  print(\"hello\")\n  ```\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CODE_BLOCK]);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "  print(\"hello\")\n");
}

#[test]
fn definition_list_inline_fence_parses_as_code_block() {
    let input = "Term\n: ```r\n  a <- 1\n  ```\n";
    let node = parse_blocks_quarto(input);

    let code_block = find_first(&node, SyntaxKind::CODE_BLOCK);
    assert!(
        code_block.is_some(),
        "Expected code block inside definition list"
    );

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "  a <- 1\n");

    let info = get_code_info(&node).unwrap();
    assert_eq!(info, "r");
}

// Indented code block tests

#[test]
fn parses_indented_code_block() {
    let input = "
    code line 1
    code line 2";
    let tree = parse_blocks(input);

    assert_eq!(find_all(&tree, SyntaxKind::CODE_BLOCK).len(), 1);
    let code_blocks = find_all(&tree, SyntaxKind::CODE_BLOCK);
    let code = &code_blocks[0];
    let text = code.text().to_string();
    assert!(text.contains("code line 1"));
    assert!(text.contains("code line 2"));
}

#[test]
fn indented_code_block_with_blank_line() {
    let input = "
    code line 1

    code line 2";
    let tree = parse_blocks(input);

    assert_eq!(find_all(&tree, SyntaxKind::CODE_BLOCK).len(), 1);
}

#[test]
fn indented_code_requires_blank_line_before() {
    let input = "paragraph
    not code";
    let tree = parse_blocks(input);

    // Should be a single paragraph, not a code block
    assert_eq!(find_all(&tree, SyntaxKind::CODE_BLOCK).len(), 0);
    assert_eq!(find_all(&tree, SyntaxKind::PARAGRAPH).len(), 1);
}

#[test]
fn indented_code_with_tab() {
    let input = "
\tcode with tab";
    let tree = parse_blocks(input);

    assert_eq!(find_all(&tree, SyntaxKind::CODE_BLOCK).len(), 1);
}

#[test]
fn indented_code_with_list_marker() {
    let input = "
    * one
    * two";
    let tree = parse_blocks(input);

    assert_eq!(find_all(&tree, SyntaxKind::CODE_BLOCK).len(), 1);
}

#[test]
fn indented_code_in_blockquote() {
    let input = ">
>     code in blockquote";
    let tree = parse_blocks(input);

    assert_eq!(find_all(&tree, SyntaxKind::BLOCKQUOTE).len(), 1);
    assert_eq!(find_all(&tree, SyntaxKind::CODE_BLOCK).len(), 1);
}
