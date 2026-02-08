use crate::block_parser::tests::helpers::{
    assert_block_kinds, count_children, find_all, find_first, parse_blocks,
};
use crate::syntax::{SyntaxKind, SyntaxToken};

fn get_code_content(node: &crate::syntax::SyntaxNode) -> Option<String> {
    find_first(node, SyntaxKind::CodeContent).map(|n| n.text().to_string())
}

fn get_code_info_token(node: &crate::syntax::SyntaxNode) -> Option<SyntaxToken> {
    for element in node.descendants_with_tokens() {
        if let Some(token) = element.as_token()
            && token.kind() == SyntaxKind::CodeInfo
        {
            return Some(token.clone());
        }
    }
    None
}

fn get_code_info(node: &crate::syntax::SyntaxNode) -> Option<String> {
    get_code_info_token(node).map(|t| t.text().to_string())
}

#[test]
fn parses_simple_backtick_code_block() {
    let input = "```\nprint(\"hello\")\n```\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CodeBlock]);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "print(\"hello\")\n");
}

#[test]
fn parses_simple_tilde_code_block() {
    let input = "~~~\nprint(\"hello\")\n~~~\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CodeBlock]);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "print(\"hello\")\n");
}

#[test]
fn parses_code_block_with_language() {
    let input = "```python\nprint(\"hello\")\n```\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CodeBlock]);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "print(\"hello\")\n");

    let info = get_code_info(&node).unwrap();
    assert_eq!(info, "python");
}

#[test]
fn parses_code_block_with_attributes() {
    let input = "```{python}\nprint(\"hello\")\n```\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CodeBlock]);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "print(\"hello\")\n");

    let info = get_code_info(&node).unwrap();
    assert_eq!(info, "{python}");
}

#[test]
fn parses_code_block_with_complex_attributes() {
    let input = "```{python #mycode .numberLines startFrom=\"100\"}\nprint(\"hello\")\n```\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CodeBlock]);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "print(\"hello\")\n");

    let info = get_code_info(&node).unwrap();
    assert_eq!(info, "{python #mycode .numberLines startFrom=\"100\"}");
}

#[test]
fn parses_multiline_code_block() {
    let input = "```python\nfor i in range(10):\n    print(i)\n```\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CodeBlock]);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "for i in range(10):\n    print(i)\n");
}

#[test]
fn requires_blank_line_before_code_block() {
    let input = "text\n```\ncode\n```\n";
    let node = parse_blocks(input);

    // Should parse as paragraph, not code block
    assert!(find_first(&node, SyntaxKind::CodeBlock).is_none());
}

#[test]
fn parses_code_block_at_start_of_document() {
    let input = "```\ncode\n```\n";

    assert_block_kinds(input, &[SyntaxKind::CodeBlock]);
}

#[test]
fn parses_code_block_after_blank_line() {
    let input = "text\n\n```\ncode\n```\n";
    let node = parse_blocks(input);

    let blocks: Vec<_> = node
        .descendants()
        .filter(|n| matches!(n.kind(), SyntaxKind::PARAGRAPH | SyntaxKind::CodeBlock))
        .collect();

    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].kind(), SyntaxKind::PARAGRAPH);
    assert_eq!(blocks[1].kind(), SyntaxKind::CodeBlock);
}

#[test]
fn requires_at_least_three_fence_chars() {
    let input = "``\ncode\n``\n";
    let node = parse_blocks(input);

    // Should not parse as code block
    assert!(find_first(&node, SyntaxKind::CodeBlock).is_none());
}

#[test]
fn closing_fence_must_have_at_least_same_length() {
    let input = "````\ncode\n```\n";
    let node = parse_blocks(input);

    // Code block should be parsed, but without proper closing
    assert!(find_first(&node, SyntaxKind::CodeBlock).is_some());

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "code\n```\n"); // The ``` becomes part of content
}

#[test]
fn closing_fence_can_be_longer() {
    let input = "```\ncode\n`````\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CodeBlock]);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "code\n");
}

#[test]
fn mixed_fence_chars_dont_close() {
    let input = "```\ncode\n~~~\n";
    let node = parse_blocks(input);

    // Should parse code block but ~~~ becomes content
    assert!(find_first(&node, SyntaxKind::CodeBlock).is_some());

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "code\n~~~\n");
}

#[test]
fn empty_code_block() {
    let input = "```\n```\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CodeBlock]);

    // Should have no content node for empty blocks
    assert!(get_code_content(&node).is_none());
}

#[test]
fn code_block_with_leading_spaces() {
    let input = "  ```python\n  print(\"hello\")\n  ```\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CodeBlock]);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "  print(\"hello\")\n");
}

// Indented code block tests

#[test]
fn parses_indented_code_block() {
    let input = "
    code line 1
    code line 2";
    let tree = parse_blocks(input);

    assert_eq!(find_all(&tree, SyntaxKind::CodeBlock).len(), 1);
    let code_blocks = find_all(&tree, SyntaxKind::CodeBlock);
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

    assert_eq!(find_all(&tree, SyntaxKind::CodeBlock).len(), 1);
}

#[test]
fn indented_code_requires_blank_line_before() {
    let input = "paragraph
    not code";
    let tree = parse_blocks(input);

    // Should be a single paragraph, not a code block
    assert_eq!(find_all(&tree, SyntaxKind::CodeBlock).len(), 0);
    assert_eq!(find_all(&tree, SyntaxKind::PARAGRAPH).len(), 1);
}

#[test]
fn indented_code_with_tab() {
    let input = "
\tcode with tab";
    let tree = parse_blocks(input);

    assert_eq!(find_all(&tree, SyntaxKind::CodeBlock).len(), 1);
}

#[test]
fn indented_code_in_blockquote() {
    let input = ">
>     code in blockquote";
    let tree = parse_blocks(input);

    assert_eq!(find_all(&tree, SyntaxKind::BlockQuote).len(), 1);
    assert_eq!(find_all(&tree, SyntaxKind::CodeBlock).len(), 1);
}
