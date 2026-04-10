use serde_json::{Value, json};

use super::{SyntaxElement, SyntaxNode, SyntaxToken};

fn range_to_json(range: rowan::TextRange) -> Value {
    json!({
        "start": u32::from(range.start()),
        "end": u32::from(range.end())
    })
}

fn token_to_json(token: SyntaxToken) -> Value {
    json!({
        "kind": format!("{:?}", token.kind()),
        "range": range_to_json(token.text_range()),
        "text": token.text(),
    })
}

fn element_to_json(element: SyntaxElement) -> Value {
    match element {
        rowan::NodeOrToken::Node(node) => cst_to_json(&node),
        rowan::NodeOrToken::Token(token) => token_to_json(token),
    }
}

pub fn cst_to_json(node: &SyntaxNode) -> Value {
    let children: Vec<Value> = node.children_with_tokens().map(element_to_json).collect();

    json!({
        "kind": format!("{:?}", node.kind()),
        "range": range_to_json(node.text_range()),
        "children": children,
    })
}
