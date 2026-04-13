pub use panache_parser::syntax::*;
use panache_parser::syntax::{
    SyntaxElement as ParserSyntaxElement, SyntaxNode as ParserSyntaxNode,
    SyntaxToken as ParserSyntaxToken,
};

#[derive(Debug, serde::Serialize)]
struct JsonRange {
    start: u32,
    end: u32,
}

#[derive(Debug, serde::Serialize)]
pub struct JsonToken {
    kind: String,
    range: JsonRange,
    text: String,
}

#[derive(Debug, serde::Serialize)]
pub struct JsonNode {
    kind: String,
    range: JsonRange,
    children: Vec<JsonElement>,
}

#[derive(Debug, serde::Serialize)]
#[serde(untagged)]
pub enum JsonElement {
    Node(JsonNode),
    Token(JsonToken),
}

fn range_to_json(range: rowan::TextRange) -> JsonRange {
    JsonRange {
        start: u32::from(range.start()),
        end: u32::from(range.end()),
    }
}

fn token_to_json(token: ParserSyntaxToken) -> JsonToken {
    JsonToken {
        kind: format!("{:?}", token.kind()),
        range: range_to_json(token.text_range()),
        text: token.text().to_string(),
    }
}

fn element_to_json(element: ParserSyntaxElement) -> JsonElement {
    match element {
        rowan::NodeOrToken::Node(node) => JsonElement::Node(cst_to_json(&node)),
        rowan::NodeOrToken::Token(token) => JsonElement::Token(token_to_json(token)),
    }
}

pub fn cst_to_json(node: &ParserSyntaxNode) -> JsonNode {
    let children: Vec<JsonElement> = node.children_with_tokens().map(element_to_json).collect();
    JsonNode {
        kind: format!("{:?}", node.kind()),
        range: range_to_json(node.text_range()),
        children,
    }
}
