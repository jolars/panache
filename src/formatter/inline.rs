use crate::config::{Config, MathDelimiterStyle};
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;

/// Format an inline node to normalized string (e.g., emphasis with asterisks)
#[allow(clippy::only_used_in_recursion)]
pub(super) fn format_inline_node(node: &SyntaxNode, config: &Config) -> String {
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
        SyntaxKind::RawInline => {
            // Format raw inline span: `content`{=format}
            let mut content = String::new();
            let mut backtick_count = 1;
            let mut format_attr = String::new();

            for child in node.children_with_tokens() {
                match child {
                    NodeOrToken::Node(n) if n.kind() == SyntaxKind::Attribute => {
                        format_attr = n.text().to_string();
                    }
                    NodeOrToken::Token(t) => {
                        if t.kind() == SyntaxKind::RawInlineMarker {
                            backtick_count = t.text().len();
                        } else if t.kind() == SyntaxKind::RawInlineContent {
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
                format_attr
            )
        }
        SyntaxKind::Emphasis => {
            let mut content = String::new();
            for child in node.children_with_tokens() {
                match child {
                    NodeOrToken::Node(n) => content.push_str(&format_inline_node(&n, config)),
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
                    NodeOrToken::Node(n) => content.push_str(&format_inline_node(&n, config)),
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
                                        result.push_str(&format_inline_node(&nested, config));
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
        SyntaxKind::InlineMath => {
            // Check if this is display math (has BlockMathMarker)
            let is_display_math = node.children_with_tokens().any(|t| {
                matches!(t, NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::BlockMathMarker)
            });

            // Get the actual content (TEXT token, not node)
            let content = node
                .children_with_tokens()
                .find_map(|c| match c {
                    NodeOrToken::Token(t) if t.kind() == SyntaxKind::TEXT => {
                        Some(t.text().to_string())
                    }
                    _ => None,
                })
                .unwrap_or_default();

            // Get original marker to determine input format
            let original_marker = node
                .children_with_tokens()
                .find_map(|t| match t {
                    NodeOrToken::Token(tok)
                        if tok.kind() == SyntaxKind::InlineMathMarker
                            || tok.kind() == SyntaxKind::BlockMathMarker =>
                    {
                        Some(tok.text().to_string())
                    }
                    _ => None,
                })
                .unwrap_or_else(|| "$".to_string());

            // Determine output format based on config
            let (open, close) = match config.math_delimiter_style {
                MathDelimiterStyle::Preserve => {
                    // Keep original format
                    if is_display_math {
                        match original_marker.as_str() {
                            "\\[" => (r"\[", r"\]"),
                            "\\\\[" => (r"\\[", r"\\]"),
                            _ => ("$$", "$$"), // Default to $$
                        }
                    } else {
                        match original_marker.as_str() {
                            r"\(" => (r"\(", r"\)"),
                            r"\\(" => (r"\\(", r"\\)"),
                            _ => ("$", "$"), // Default to $
                        }
                    }
                }
                MathDelimiterStyle::Dollars => {
                    // Normalize to dollars
                    if is_display_math {
                        ("$$", "$$")
                    } else {
                        ("$", "$")
                    }
                }
                MathDelimiterStyle::Backslash => {
                    // Normalize to single backslash
                    if is_display_math {
                        (r"\[", r"\]")
                    } else {
                        (r"\(", r"\)")
                    }
                }
            };

            // Output formatted math
            if is_display_math {
                // Display math is always block-level with newlines
                format!("{}\n{}\n{}", open, content.trim(), close)
            } else {
                // Inline math stays inline
                format!("{}{}{}", open, content, close)
            }
        }
        _ => {
            // For other inline nodes, just return their text
            node.text().to_string()
        }
    }
}
