use crate::config::Config;
use crate::formatter::inline;
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;

/// Check if a paragraph contains inline display math ($$...$$ within paragraph)
pub(super) fn contains_inline_display_math(node: &SyntaxNode) -> bool {
    for child in node.descendants() {
        if child.kind() == SyntaxKind::DISPLAY_MATH {
            return true;
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

    let children: Vec<_> = node.children_with_tokens().collect();
    let mut i = 0;
    while i < children.len() {
        match &children[i] {
            NodeOrToken::Node(n) => {
                if n.kind() == SyntaxKind::DISPLAY_MATH {
                    // Save current text as paragraph part
                    if !current_text.trim().is_empty() {
                        parts.push((false, current_text.clone()));
                        current_text.clear();
                    }

                    // Format display math using the inline formatter
                    let mut formatted = inline::format_inline_node(n, config);

                    // Check if there are attributes following this display math
                    // Pattern: DisplayMath, [WHITESPACE], Attribute
                    if i + 1 < children.len() {
                        // Check for optional whitespace
                        let mut next_idx = i + 1;
                        if let NodeOrToken::Token(t) = &children[next_idx]
                            && t.kind() == SyntaxKind::WHITESPACE
                        {
                            next_idx += 1;
                        }

                        // Check for Attribute node
                        if next_idx < children.len()
                            && let NodeOrToken::Node(attr_node) = &children[next_idx]
                            && attr_node.kind() == SyntaxKind::ATTRIBUTE
                        {
                            // Append attribute on same line as closing $$
                            // Remove trailing newline from formatted math
                            if formatted.ends_with('\n') {
                                formatted.pop();
                            }
                            formatted.push(' ');
                            formatted.push_str(&attr_node.text().to_string());
                            formatted.push('\n');

                            // Skip the whitespace and attribute nodes
                            i = next_idx;
                        }
                    }

                    // Display math gets its own part as formatted string
                    // We'll output it as-is without wrapping
                    parts.push((true, formatted));
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
        i += 1;
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
            // Note: content already ends with '\n' if it had attributes, or we add one here
            if !content.ends_with('\n') {
                output.push('\n');
            }
        } else {
            // Text part - ensure it's on a new line if it comes after display math
            // Check if previous part was display math
            let prev_was_display_math =
                i > 0 && parts.get(i - 1).map(|(is_dm, _)| *is_dm).unwrap_or(false);

            if prev_was_display_math {
                // Text after display math should be on its own line
                // (output already ends with '\n' from display math, so we're good)
            } else if i > 0 && !output.ends_with('\n') {
                // Text after non-display content needs newline if output doesn't have one
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
