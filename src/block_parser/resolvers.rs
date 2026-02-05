use crate::block_parser::blockquotes::{build_blockquote_node, try_identify_blockquote};
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::GreenNodeBuilder;

pub(crate) fn resolve_containers(root: SyntaxNode) -> SyntaxNode {
    let mut builder = GreenNodeBuilder::new();

    // Copy the root node type
    builder.start_node(root.kind().into());

    // Process the document children
    if let Some(doc) = root.children().find(|n| n.kind() == SyntaxKind::DOCUMENT) {
        builder.start_node(SyntaxKind::DOCUMENT.into());
        resolve_container_children(&mut builder, &doc.children().collect::<Vec<_>>());
        builder.finish_node();
    }

    builder.finish_node();
    SyntaxNode::new_root(builder.finish())
}

fn resolve_container_children(builder: &mut GreenNodeBuilder<'static>, children: &[SyntaxNode]) {
    let mut i = 0;

    while i < children.len() {
        if let Some(blockquote_end) = try_identify_blockquote(children, i) {
            // Found blockquote pattern from i..blockquote_end
            build_blockquote_node(builder, &children[i..blockquote_end]);
            i = blockquote_end;
        } else {
            // Regular node, copy as-is (list nesting is handled in the parser)
            copy_node_recursively(builder, &children[i]);
            i += 1;
        }
    }
}

fn copy_node_recursively(builder: &mut GreenNodeBuilder<'static>, node: &SyntaxNode) {
    builder.start_node(node.kind().into());

    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Node(n) => copy_node_recursively(builder, &n),
            rowan::NodeOrToken::Token(t) => {
                builder.token(t.kind().into(), t.text());
            }
        }
    }

    builder.finish_node();
}
