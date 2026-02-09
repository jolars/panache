use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;

/// Format an inline node to normalized string (e.g., emphasis with asterisks)
#[allow(clippy::only_used_in_recursion)]
pub(super) fn format_inline_node(node: &SyntaxNode) -> String {
    match node.kind() {
        SyntaxKind::CodeSpan => {
            let mut content = String::new();
            let mut backtick_count = 1;
            let mut attributes = String::new();

            for child in node.children_with_tokens() {
                match child {
                    NodeOrToken::Node(n) if n.kind() == SyntaxKind::Attribute => {
                        attributes = n.text().to_string();
                    }
                    NodeOrToken::Token(t) => {
                        if t.kind() == SyntaxKind::CodeSpanMarker {
                            backtick_count = t.text().len();
                        } else if t.kind() != SyntaxKind::Attribute {
                            content.push_str(t.text());
                        }
                    }
                    _ => {}
                }
            }

            format!(
                "{}{}{}{}",
                "`".repeat(backtick_count),
                content,
                "`".repeat(backtick_count),
                attributes
            )
        }
        SyntaxKind::Emphasis => {
            let mut content = String::new();
            for child in node.children_with_tokens() {
                match child {
                    NodeOrToken::Node(n) => content.push_str(&format_inline_node(&n)),
                    NodeOrToken::Token(t) => {
                        if t.kind() != SyntaxKind::EmphasisMarker {
                            content.push_str(t.text());
                        }
                    }
                }
            }
            format!("*{}*", content)
        }
        SyntaxKind::Strong => {
            let mut content = String::new();
            for child in node.children_with_tokens() {
                match child {
                    NodeOrToken::Node(n) => content.push_str(&format_inline_node(&n)),
                    NodeOrToken::Token(t) => {
                        if t.kind() != SyntaxKind::StrongMarker {
                            content.push_str(t.text());
                        }
                    }
                }
            }
            format!("**{}**", content)
        }
        SyntaxKind::BracketedSpan => {
            // Format bracketed span: [content]{.attributes}
            // Need to traverse children to avoid extra spaces
            let mut result = String::new();
            for child in node.children_with_tokens() {
                match child {
                    NodeOrToken::Token(t) => {
                        result.push_str(t.text());
                    }
                    NodeOrToken::Node(n) => {
                        // Recursively format nested content
                        if n.kind() == SyntaxKind::SpanContent {
                            for elem in n.children_with_tokens() {
                                match elem {
                                    NodeOrToken::Token(t) => result.push_str(t.text()),
                                    NodeOrToken::Node(nested) => {
                                        result.push_str(&format_inline_node(&nested));
                                    }
                                }
                            }
                        } else if n.kind() == SyntaxKind::SpanAttributes {
                            // Output attributes token by token to avoid spaces
                            for elem in n.children_with_tokens() {
                                match elem {
                                    NodeOrToken::Token(t) => result.push_str(t.text()),
                                    NodeOrToken::Node(_) => {} // Shouldn't happen
                                }
                            }
                        } else {
                            result.push_str(&n.text().to_string());
                        }
                    }
                }
            }
            result
        }
        _ => {
            // For other inline nodes, just return their text
            node.text().to_string()
        }
    }
}
