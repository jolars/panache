use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;

/// Check if a paragraph contains inline display math ($$...$$ within paragraph)
pub(super) fn contains_inline_display_math(node: &SyntaxNode) -> bool {
    for child in node.descendants() {
        if child.kind() == SyntaxKind::InlineMath {
            // Check if it contains BlockMathMarker ($$)
            for token in child.children_with_tokens() {
                if let NodeOrToken::Token(t) = token
                    && t.kind() == SyntaxKind::BlockMathMarker
                {
                    return true;
                }
            }
        }
    }
    false
}

/// Format a paragraph that contains inline display math by splitting it.
/// Converts: "Some text $$x = y$$ more text" into text with display math formatted.
pub(super) fn format_paragraph_with_display_math(
    node: &SyntaxNode,
    line_width: usize,
    output: &mut String,
) {
    let mut parts: Vec<(bool, String)> = Vec::new(); // (is_display_math, content)
    let mut current_text = String::new();

    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Node(n) => {
                if n.kind() == SyntaxKind::InlineMath {
                    // Check if this is display math
                    let has_block_marker = n.children_with_tokens().any(|t| {
                        matches!(t, NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::BlockMathMarker)
                    });

                    if has_block_marker {
                        // Save current text as paragraph part
                        if !current_text.trim().is_empty() {
                            parts.push((false, current_text.clone()));
                            current_text.clear();
                        }

                        // Extract math content
                        let math_content: String = n
                            .children_with_tokens()
                            .filter_map(|c| match c {
                                NodeOrToken::Token(t) if t.kind() == SyntaxKind::TEXT => {
                                    Some(t.text().to_string())
                                }
                                _ => None,
                            })
                            .collect();

                        parts.push((true, math_content));
                    } else {
                        // Regular inline math - keep in text
                        current_text.push_str(&n.text().to_string());
                    }
                } else {
                    current_text.push_str(&n.text().to_string());
                }
            }
            NodeOrToken::Token(t) => {
                if t.kind() != SyntaxKind::NEWLINE {
                    current_text.push_str(t.text());
                } else {
                    current_text.push(' '); // Replace newlines with spaces for wrapping
                }
            }
        }
    }

    // Save any remaining text
    if !current_text.trim().is_empty() {
        parts.push((false, current_text));
    }

    // Format each part - display math on separate lines within paragraph
    for (i, (is_display_math, content)) in parts.iter().enumerate() {
        if *is_display_math {
            // Format as display math on separate lines
            output.push('\n');
            output.push_str("$$\n");
            output.push_str(content.trim());
            output.push_str("\n$$\n");
        } else {
            // Add space before if not at start
            if i > 0 && !output.ends_with('\n') {
                output.push('\n');
            }

            // Format as paragraph text with wrapping
            let text = content.trim();
            if !text.is_empty() {
                let lines = textwrap::wrap(text, line_width);
                for (j, line) in lines.iter().enumerate() {
                    if j > 0 {
                        output.push('\n');
                    }
                    output.push_str(line);
                }
            }
        }
    }

    // End with newline
    if !output.ends_with('\n') {
        output.push('\n');
    }
}
