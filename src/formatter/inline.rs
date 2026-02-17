use crate::config::{Config, MathDelimiterStyle};
use crate::formatter::shortcodes::format_shortcode;
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
                            // Normalize attributes: skip WHITESPACE, join with single space
                            result.push('{');
                            let mut attr_parts = Vec::new();
                            for elem in n.children_with_tokens() {
                                match elem {
                                    NodeOrToken::Token(t) => {
                                        // Skip braces and whitespace
                                        if t.kind() == SyntaxKind::TEXT {
                                            let text = t.text();
                                            if text != "{" && text != "}" {
                                                attr_parts.push(text.to_string());
                                            }
                                        }
                                    }
                                    NodeOrToken::Node(_) => {} // Shouldn't happen
                                }
                            }
                            result.push_str(&attr_parts.join(" "));
                            result.push('}');
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
        SyntaxKind::DisplayMath => {
            // Display math: $$content$$ or \[content\] or \\[content\\]
            // Format on separate lines with proper normalization
            let mut content = String::new();
            let mut opening_marker = None;
            let mut closing_marker = None;

            for child in node.children_with_tokens() {
                if let NodeOrToken::Token(tok) = child {
                    if tok.kind() == SyntaxKind::BlockMathMarker {
                        if opening_marker.is_none() {
                            opening_marker = Some(tok.text().to_string());
                        } else {
                            closing_marker = Some(tok.text().to_string());
                        }
                    } else if tok.kind() == SyntaxKind::TEXT {
                        content.push_str(tok.text());
                    }
                }
            }

            // Apply delimiter style preference
            let (open, close) = match config.math_delimiter_style {
                MathDelimiterStyle::Preserve => {
                    let opening = opening_marker.as_deref().unwrap_or("$$");
                    let closing = closing_marker.as_deref().unwrap_or("$$");
                    (opening, closing)
                }
                MathDelimiterStyle::Dollars => ("$$", "$$"),
                MathDelimiterStyle::Backslash => (r"\[", r"\]"),
            };

            // Normalize content:
            // 1. Trim leading/trailing whitespace (including newlines)
            // 2. Ensure content is on separate lines from delimiters
            // 3. Strip common leading whitespace from all lines (preserve relative indentation)
            let mut result = String::new();
            result.push_str(open);
            result.push('\n');

            // Process content: trim overall, then strip common leading whitespace
            let trimmed_content = content.trim();
            if !trimmed_content.is_empty() {
                // Find minimum indentation across all non-empty lines
                let min_indent = trimmed_content
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .map(|line| line.len() - line.trim_start().len())
                    .min()
                    .unwrap_or(0);

                // Strip common indentation from each line
                for line in trimmed_content.lines() {
                    if line.len() >= min_indent {
                        result.push_str(&line[min_indent..]);
                    } else {
                        result.push_str(line);
                    }
                    result.push('\n');
                }
            }

            result.push_str(close);
            result
        }
        SyntaxKind::HardLineBreak => {
            // Normalize hard line breaks to backslash-newline when escaped_line_breaks is enabled
            // Otherwise preserve original format (trailing spaces)
            if config.extensions.escaped_line_breaks {
                "\\\n".to_string()
            } else {
                node.text().to_string()
            }
        }
        SyntaxKind::Shortcode => {
            // Format Quarto shortcodes with normalized spacing
            format_shortcode(node)
        }
        _ => {
            // For other inline nodes, just return their text
            node.text().to_string()
        }
    }
}
