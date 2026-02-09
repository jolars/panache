use crate::config::Config;
use crate::formatter::inline;
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
    config: &Config,
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

                        // Format display math using the inline formatter
                        let formatted = inline::format_inline_node(&n, config);
                        // Display math gets its own part as formatted string
                        // We'll output it as-is without wrapping
                        parts.push((true, formatted));
                    } else {
                        // Regular inline math - format it using the inline formatter
                        let formatted = inline::format_inline_node(&n, config);
                        current_text.push_str(&formatted);
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
            // Output formatted display math (already formatted with delimiters)
            // on its own line
            if i > 0 {
                output.push('\n');
            }
            output.push_str(content);
            output.push('\n');
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
