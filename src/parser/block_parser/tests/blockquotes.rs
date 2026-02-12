use crate::parser::block_parser::tests::helpers::parse_blocks;
use crate::syntax::SyntaxKind;

fn count_nodes_of_type(root: &crate::syntax::SyntaxNode, kind: SyntaxKind) -> usize {
    let mut count = 0;

    fn walk_tree(node: &crate::syntax::SyntaxNode, target_kind: SyntaxKind, count: &mut usize) {
        if node.kind() == target_kind {
            *count += 1;
        }
        for child in node.children() {
            walk_tree(&child, target_kind, count);
        }
    }

    walk_tree(root, kind, &mut count);
    count
}

fn find_nodes_of_type(
    root: &crate::syntax::SyntaxNode,
    kind: SyntaxKind,
) -> Vec<crate::syntax::SyntaxNode> {
    let mut nodes = Vec::new();

    fn walk_tree(
        node: &crate::syntax::SyntaxNode,
        target_kind: SyntaxKind,
        nodes: &mut Vec<crate::syntax::SyntaxNode>,
    ) {
        if node.kind() == target_kind {
            nodes.push(node.clone());
        }
        for child in node.children() {
            walk_tree(&child, target_kind, nodes);
        }
    }

    walk_tree(root, kind, &mut nodes);
    nodes
}

#[test]
fn single_blockquote_paragraph() {
    let input = "> This is a simple blockquote.";
    let tree = parse_blocks(input);

    // Should have 1 BlockQuote node and 1 Paragraph node
    assert_eq!(count_nodes_of_type(&tree, SyntaxKind::BlockQuote), 1);
    assert_eq!(count_nodes_of_type(&tree, SyntaxKind::PARAGRAPH), 1);

    let blockquotes = find_nodes_of_type(&tree, SyntaxKind::BlockQuote);
    let blockquote = &blockquotes[0];

    // The paragraph should be inside the blockquote
    assert_eq!(count_nodes_of_type(blockquote, SyntaxKind::PARAGRAPH), 1);
}

#[test]
fn multi_line_blockquote() {
    let input = "> This is line one.\n> This is line two.";
    let tree = parse_blocks(input);

    // Should have 1 BlockQuote node and 1 Paragraph node (multi-line paragraph)
    assert_eq!(count_nodes_of_type(&tree, SyntaxKind::BlockQuote), 1);
    assert_eq!(count_nodes_of_type(&tree, SyntaxKind::PARAGRAPH), 1);
}

#[test]
fn nested_blockquotes() {
    let input = "> Outer quote\n>\n> > Inner quote\n>\n> Back to outer";
    let tree = parse_blocks(input);

    // Should have 2 BlockQuote nodes (outer and inner)
    assert_eq!(count_nodes_of_type(&tree, SyntaxKind::BlockQuote), 2);

    let blockquotes = find_nodes_of_type(&tree, SyntaxKind::BlockQuote);

    // Outer blockquote should contain the inner blockquote
    let outer = &blockquotes[0]; // First one should be the outer

    // Check that inner blockquote is actually inside the outer one
    let inner_found_in_outer = !find_nodes_of_type(outer, SyntaxKind::BlockQuote).is_empty();
    assert!(
        inner_found_in_outer,
        "Inner blockquote should be nested inside outer"
    );
}

#[test]
fn triple_nested_blockquotes() {
    let input = "> Level 1\n>\n> > Level 2\n> >\n> > > Level 3";
    let tree = parse_blocks(input);

    // Should have 3 BlockQuote nodes
    assert_eq!(count_nodes_of_type(&tree, SyntaxKind::BlockQuote), 3);
}

#[test]
fn blockquote_with_blank_lines() {
    let input = "> First paragraph\n>\n> Second paragraph";
    let tree = parse_blocks(input);

    // Should have 1 BlockQuote node
    assert_eq!(count_nodes_of_type(&tree, SyntaxKind::BlockQuote), 1);

    // Should have 2 Paragraph nodes inside the blockquote
    let blockquotes = find_nodes_of_type(&tree, SyntaxKind::BlockQuote);
    let blockquote = &blockquotes[0];
    assert_eq!(count_nodes_of_type(blockquote, SyntaxKind::PARAGRAPH), 2);
}

#[test]
fn blockquote_with_heading() {
    let input = "> # This is a heading in a blockquote\n>\n> And this is a paragraph.";
    let tree = parse_blocks(input);

    // Should have 1 BlockQuote node
    assert_eq!(count_nodes_of_type(&tree, SyntaxKind::BlockQuote), 1);

    let blockquotes = find_nodes_of_type(&tree, SyntaxKind::BlockQuote);
    let blockquote = &blockquotes[0];

    // Should have 1 Heading and 1 Paragraph inside the blockquote
    assert_eq!(count_nodes_of_type(blockquote, SyntaxKind::Heading), 1);
    assert_eq!(count_nodes_of_type(blockquote, SyntaxKind::PARAGRAPH), 1);
}

#[test]
fn blockquote_requires_blank_line_before() {
    let input = "Regular paragraph\n> This should not be a blockquote";
    let tree = parse_blocks(input);

    // Should have 0 BlockQuote nodes (no blank line before)
    assert_eq!(count_nodes_of_type(&tree, SyntaxKind::BlockQuote), 0);
    // Should have 1 paragraph (no blank line means they merge in Markdown)
    assert_eq!(count_nodes_of_type(&tree, SyntaxKind::PARAGRAPH), 1);
}

#[test]
fn blockquote_at_start_of_document() {
    let input = "> This is at the start of the document";
    let tree = parse_blocks(input);

    // Should have 1 BlockQuote node (no blank line needed at start)
    assert_eq!(count_nodes_of_type(&tree, SyntaxKind::BlockQuote), 1);
}

#[test]
fn blockquote_after_blank_line() {
    let input = "Regular paragraph\n\n> This should be a blockquote";
    let tree = parse_blocks(input);

    // Should have 1 BlockQuote node (has blank line before)
    assert_eq!(count_nodes_of_type(&tree, SyntaxKind::BlockQuote), 1);
    // Should have 1 regular paragraph + 1 paragraph inside blockquote
    assert_eq!(count_nodes_of_type(&tree, SyntaxKind::PARAGRAPH), 2);
}

#[test]
fn complex_nested_structure() {
    let input = "> Outer quote with paragraph\n>\n> > Inner quote\n> >\n> > > Triple nested\n> >\n> > Back to double nested\n>\n> Back to outer";
    let tree = parse_blocks(input);

    // Should have multiple BlockQuote nodes (at least 3 levels)
    let blockquote_count = count_nodes_of_type(&tree, SyntaxKind::BlockQuote);
    assert!(
        blockquote_count >= 3,
        "Should have at least 3 blockquote levels, found {}",
        blockquote_count
    );

    // Should have multiple paragraphs
    let paragraph_count = count_nodes_of_type(&tree, SyntaxKind::PARAGRAPH);
    assert!(
        paragraph_count >= 3,
        "Should have multiple paragraphs, found {}",
        paragraph_count
    );
}

// Tests based on Pandoc spec examples

#[test]
fn spec_basic_blockquote() {
    let input = "> This is a block quote. This\n> paragraph has two lines.\n>\n> 1. This is a list inside a block quote.\n> 2. Second item.";
    let tree = parse_blocks(input);

    // Should have 1 BlockQuote node
    assert_eq!(count_nodes_of_type(&tree, SyntaxKind::BlockQuote), 1);

    // Should contain paragraphs (lists not yet parsed, but treated as paragraphs)
    let blockquotes = find_nodes_of_type(&tree, SyntaxKind::BlockQuote);
    let blockquote = &blockquotes[0];
    assert!(count_nodes_of_type(blockquote, SyntaxKind::PARAGRAPH) >= 1);
}

#[test]
fn spec_nested_blockquote() {
    let input = "> This is a block quote.\n>\n> > A block quote within a block quote.";
    let tree = parse_blocks(input);

    // Should have 2 BlockQuote nodes (outer and inner)
    assert_eq!(count_nodes_of_type(&tree, SyntaxKind::BlockQuote), 2);

    // Verify nesting structure
    let blockquotes = find_nodes_of_type(&tree, SyntaxKind::BlockQuote);
    let outer = &blockquotes[0];

    // Inner blockquote should be nested inside outer
    assert!(!find_nodes_of_type(outer, SyntaxKind::BlockQuote).is_empty());
}

#[test]
fn spec_blank_before_blockquote_required() {
    // This should NOT create a nested blockquote due to blank_before_blockquote
    let input = "> This is a block quote.\n>> Not nested, since blank_before_blockquote is enabled by default";
    let tree = parse_blocks(input);

    // Should have only 1 BlockQuote node (the >> line becomes part of the paragraph)
    assert_eq!(count_nodes_of_type(&tree, SyntaxKind::BlockQuote), 1);

    // Should have 1 paragraph containing both lines
    assert_eq!(count_nodes_of_type(&tree, SyntaxKind::PARAGRAPH), 1);
}

#[test]
fn spec_blockquote_with_indented_code() {
    let input = ">     code";
    let tree = parse_blocks(input);

    // Should have 1 BlockQuote node
    assert_eq!(count_nodes_of_type(&tree, SyntaxKind::BlockQuote), 1);

    // The content should preserve the indentation
    let blockquotes = find_nodes_of_type(&tree, SyntaxKind::BlockQuote);
    let blockquote = &blockquotes[0];
    let text = blockquote.text().to_string();
    assert!(
        text.contains("    code"),
        "Should preserve 4-space indentation for code"
    );
}

#[test]
fn spec_blockquote_optional_space_after_marker() {
    // Test both "> " and ">" forms
    let input1 = "> With space";
    let input2 = ">Without space";

    let tree1 = parse_blocks(input1);

    let tree2 = parse_blocks(input2);

    // Both should create blockquotes
    assert_eq!(count_nodes_of_type(&tree1, SyntaxKind::BlockQuote), 1);
    assert_eq!(count_nodes_of_type(&tree2, SyntaxKind::BlockQuote), 1);
}

#[test]
fn spec_blockquote_max_three_space_indent() {
    // Up to 3 spaces before > should be allowed
    let input1 = "   > Three spaces should work";
    let input2 = "    > Four spaces should not work"; // This should be treated as code block

    let tree1 = parse_blocks(input1);

    let tree2 = parse_blocks(input2);

    // First should create blockquote
    assert_eq!(count_nodes_of_type(&tree1, SyntaxKind::BlockQuote), 1);

    // Second should NOT create blockquote (should be treated as code block)
    assert_eq!(count_nodes_of_type(&tree2, SyntaxKind::BlockQuote), 0);
    assert_eq!(count_nodes_of_type(&tree2, SyntaxKind::CodeBlock), 1);
}

// Test lazy blockquote form
#[test]
fn spec_lazy_blockquote_form() {
    let input = "> This is a block quote. This\nparagraph has two lines.";
    let tree = parse_blocks(input);

    // Should have 1 BlockQuote node containing the lazy continuation
    assert_eq!(count_nodes_of_type(&tree, SyntaxKind::BlockQuote), 1);

    // The blockquote should contain both lines as a single paragraph
    let blockquotes = find_nodes_of_type(&tree, SyntaxKind::BlockQuote);
    let blockquote = &blockquotes[0];
    let text = blockquote.text().to_string();

    // Should contain both the first line and the lazy continuation
    assert!(
        text.contains("This is a block quote"),
        "Should contain first line"
    );
    assert!(
        text.contains("paragraph has two lines"),
        "Should contain lazy continuation"
    );
}

#[test]
fn blockquote_with_code_block() {
    let input = "> ```python\n> print(\"hello\")\n> ```\n";
    let tree = parse_blocks(input);

    // Should have 1 BlockQuote with 1 CodeBlock inside
    assert_eq!(count_nodes_of_type(&tree, SyntaxKind::BlockQuote), 1);
    assert_eq!(count_nodes_of_type(&tree, SyntaxKind::CodeBlock), 1);

    let blockquotes = find_nodes_of_type(&tree, SyntaxKind::BlockQuote);
    let blockquote = &blockquotes[0];

    // Code block should be inside the blockquote
    assert_eq!(count_nodes_of_type(blockquote, SyntaxKind::CodeBlock), 1);
}
