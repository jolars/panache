use crate::config::{Extensions, Flavor, ParserOptions};
use crate::parser::Parser;
use crate::syntax::{SyntaxKind, SyntaxNode};

pub fn parse_blocks(input: &str) -> SyntaxNode {
    let config = ParserOptions::default();
    Parser::new(input, &config).parse()
}

pub fn parse_blocks_with_config(input: &str, config: &ParserOptions) -> SyntaxNode {
    Parser::new(input, config).parse()
}

pub fn parse_blocks_quarto(input: &str) -> SyntaxNode {
    let config = ParserOptions {
        flavor: Flavor::Quarto,
        extensions: Extensions::for_flavor(Flavor::Quarto),
        ..Default::default()
    };
    Parser::new(input, &config).parse()
}

pub fn parse_blocks_rmarkdown(input: &str) -> SyntaxNode {
    let config = ParserOptions {
        flavor: Flavor::RMarkdown,
        extensions: Extensions::for_flavor(Flavor::RMarkdown),
        ..Default::default()
    };
    Parser::new(input, &config).parse()
}

pub fn parse_blocks_gfm(input: &str) -> SyntaxNode {
    let config = ParserOptions {
        flavor: Flavor::Gfm,
        extensions: Extensions::for_flavor(Flavor::Gfm),
        ..Default::default()
    };
    Parser::new(input, &config).parse()
}

pub fn find_first(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxNode> {
    node.descendants().find(|n| n.kind() == kind)
}

pub fn find_all(node: &SyntaxNode, kind: SyntaxKind) -> Vec<SyntaxNode> {
    node.descendants().filter(|n| n.kind() == kind).collect()
}

pub fn get_blocks(node: &SyntaxNode) -> Vec<SyntaxNode> {
    if node.kind() == SyntaxKind::DOCUMENT {
        node.children().collect()
    } else {
        Vec::new()
    }
}

pub fn assert_block_kinds(input: &str, expected: &[SyntaxKind]) {
    let node = parse_blocks(input);
    assert_block_kinds_for_node(&node, expected, input);
}

pub fn assert_block_kinds_for_node(node: &SyntaxNode, expected: &[SyntaxKind], input: &str) {
    let blocks = get_blocks(node);
    let actual: Vec<_> = blocks.iter().map(|n| n.kind()).collect();
    assert_eq!(
        actual, expected,
        "Block kinds did not match for input:\n{}",
        input
    );
}

/// Count direct children of a specific kind
pub fn count_children(node: &SyntaxNode, kind: SyntaxKind) -> usize {
    node.children().filter(|n| n.kind() == kind).count()
}
