use crate::config::Config;
use crate::formatter::inline;
use crate::formatter::math_delimiters::{
    count_unescaped_single_dollars, has_ambiguous_dollar_delimiters,
};
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;

const INLINE_MATH_SPACE_SENTINEL: char = '\u{E000}';

fn protect_inline_math_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_inline_math = false;
    let mut backslash_run = 0usize;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        let escaped_dollar = ch == '$' && backslash_run % 2 == 1;
        if ch == '$' && !escaped_dollar {
            if matches!(chars.peek(), Some('$')) {
                out.push('$');
                out.push(chars.next().unwrap_or('$'));
                backslash_run = 0;
                continue;
            }
            in_inline_math = !in_inline_math;
            out.push(ch);
            backslash_run = 0;
            continue;
        }

        if in_inline_math && ch.is_whitespace() {
            out.push(INLINE_MATH_SPACE_SENTINEL);
        } else {
            out.push(ch);
        }

        if ch == '\\' {
            backslash_run += 1;
        } else {
            backslash_run = 0;
        }
    }

    out
}

fn normalize_inline_math_line_breaks(lines: &mut [String]) {
    if lines.len() < 2 {
        return;
    }
    for i in 0..(lines.len() - 1) {
        let current = lines[i].trim_end().to_string();
        if !current.ends_with('$') || current.ends_with("$$") {
            continue;
        }
        if count_unescaped_single_dollars(&current).is_multiple_of(2) {
            continue;
        }
        let without_marker = current[..current.len() - 1].trim_end().to_string();
        let next = lines[i + 1].trim_start().to_string();
        lines[i] = without_marker;
        lines[i + 1] = if next.starts_with('$') {
            next
        } else if next.is_empty() {
            "$".to_string()
        } else {
            format!("$ {next}")
        };
    }
}

/// Check if a paragraph contains inline display math ($$...$$ within paragraph)
pub(super) fn contains_inline_display_math(node: &SyntaxNode) -> bool {
    if has_ambiguous_dollar_delimiters(&node.text().to_string()) {
        return false;
    }
    node.descendants().any(|child| {
        if child.kind() != SyntaxKind::DISPLAY_MATH {
            return false;
        }
        let content = child
            .children_with_tokens()
            .filter_map(|el| match el {
                NodeOrToken::Token(t) if t.kind() == SyntaxKind::TEXT => Some(t.text().to_string()),
                _ => None,
            })
            .collect::<String>();
        count_unescaped_single_dollars(&content) == 0
    })
}

/// Check if a paragraph contains raw LaTeX commands.
pub(super) fn contains_latex_command(node: &SyntaxNode) -> bool {
    node.descendants()
        .any(|child| child.kind() == SyntaxKind::LATEX_COMMAND)
}

pub(super) fn is_bookdown_text_reference(node: &SyntaxNode) -> bool {
    let text = node.text().to_string();
    let trimmed = text.trim_end_matches(['\r', '\n']);
    if !trimmed.starts_with("(ref:") || !trimmed.contains(") ") {
        return false;
    }
    !trimmed[trimmed.find(") ").unwrap() + 2..].contains('\n')
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
    let mut skip_marker_whitespace = false;
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

                        // Attributes may be emitted as plain text right after display math in CST.
                        let mut attr_text = None;
                        if next_idx < children.len() {
                            match &children[next_idx] {
                                NodeOrToken::Node(attr_node)
                                    if attr_node.kind() == SyntaxKind::ATTRIBUTE
                                        && config.extensions.quarto_crossrefs =>
                                {
                                    attr_text = Some(attr_node.text().to_string());
                                    i = next_idx;
                                }
                                NodeOrToken::Token(t)
                                    if t.kind() == SyntaxKind::TEXT
                                        && config.extensions.quarto_crossrefs
                                        && t.text().trim_start().starts_with('{') =>
                                {
                                    attr_text = Some(t.text().to_string());
                                    i = next_idx;
                                }
                                _ => {}
                            }
                        }

                        if let Some(attrs) = attr_text
                            && config.extensions.quarto_crossrefs
                        {
                            // Append attribute on same line as closing $$
                            if formatted.ends_with('\n') {
                                formatted.pop();
                            }
                            formatted.push(' ');
                            formatted.push_str(attrs.trim());
                            formatted.push('\n');
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
                if t.kind() == SyntaxKind::BLOCKQUOTE_MARKER {
                    skip_marker_whitespace = true;
                } else if t.kind() == SyntaxKind::WHITESPACE && skip_marker_whitespace {
                    skip_marker_whitespace = false;
                } else if t.kind() == SyntaxKind::NEWLINE {
                    skip_marker_whitespace = false;
                    current_text.push(' '); // Replace newlines with spaces for wrapping
                } else if t.kind() != SyntaxKind::DISPLAY_MATH_MARKER {
                    skip_marker_whitespace = false;
                    current_text.push_str(t.text());
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

            // Format as paragraph text with wrapping.
            // Normalize internal whitespace to keep wrapping stable across passes.
            let text = content.split_whitespace().collect::<Vec<_>>().join(" ");
            if !text.is_empty() {
                let protected = protect_inline_math_spaces(&text);
                let mut lines = super::wrapping::wrap_text_first_fit(&protected, line_width);
                normalize_inline_math_line_breaks(&mut lines);
                for (j, line) in lines.iter().enumerate() {
                    if j > 0 {
                        output.push('\n');
                    }
                    output.push_str(&line.replace(INLINE_MATH_SPACE_SENTINEL, " "));
                }
            }
        }
    }

    // End with newline
    if !output.ends_with('\n') {
        output.push('\n');
    }
}
