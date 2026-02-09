use crate::block_parser::BlockParser;
use crate::config::Config;
use crate::syntax::{SyntaxKind, SyntaxNode};

pub fn parse_blocks(input: &str) -> SyntaxNode {
    let config = Config::default();
    let (tree, _registry) = BlockParser::new(input, &config).parse();
    tree
}

pub fn find_first(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxNode> {
    node.descendants().find(|n| n.kind() == kind)
}

pub fn find_all(node: &SyntaxNode, kind: SyntaxKind) -> Vec<SyntaxNode> {
    node.descendants().filter(|n| n.kind() == kind).collect()
}

pub fn get_blocks(node: &SyntaxNode) -> Vec<SyntaxNode> {
    let document = node
        .children()
        .find(|n| n.kind() == SyntaxKind::DOCUMENT)
        .unwrap();
    let blocks: Vec<SyntaxNode> = document.children().collect();
    blocks
}

pub fn assert_block_kinds(input: &str, expected: &[SyntaxKind]) {
    let node = parse_blocks(input);
    let blocks = get_blocks(&node);
    let actual: Vec<_> = blocks.iter().map(|n| n.kind()).collect();
    assert_eq!(
        actual, expected,
        "Block kinds did not match for input:\n{}",
        input
    );
}

/// Get text content of first node matching the kind
pub fn get_text(node: &SyntaxNode, kind: SyntaxKind) -> Option<String> {
    find_first(node, kind).map(|n| n.text().to_string())
}

/// Print debug tree for inspection
#[allow(dead_code)]
pub fn debug_tree(node: &SyntaxNode) -> String {
    format!("{:#?}", node)
}

/// Count direct children of a specific kind
pub fn count_children(node: &SyntaxNode, kind: SyntaxKind) -> usize {
    node.children().filter(|n| n.kind() == kind).count()
}
