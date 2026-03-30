use crate::config::{Config, MathDelimiterStyle};
use crate::formatter::shortcodes::format_shortcode;
use crate::syntax::{DisplayMath, SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;
use rowan::ast::AstNode;

fn expand_tabs_code_span(text: &str, tab_width: usize) -> String {
    let mut out = String::with_capacity(text.len());
    let mut col = 0usize;
    for ch in text.chars() {
        match ch {
            '\t' => {
                let mut spaces = tab_width - (col % tab_width);
                if col == 0 && spaces == tab_width {
                    spaces = 1;
                }
                out.push_str(&" ".repeat(spaces));
                col += spaces;
            }
            '\n' => {
                out.push(' ');
                col += 1;
            }
            _ => {
                out.push(ch);
                col += 1;
            }
        }
    }
    out.trim().to_string()
}

/// Format an inline node to normalized string (e.g., emphasis with asterisks)
pub(super) fn format_inline_node(node: &SyntaxNode, config: &Config) -> String {
    match node.kind() {
        SyntaxKind::AUTO_LINK => {
            let mut result = String::new();
            let mut skip_marker_whitespace = false;
            for child in node.descendants_with_tokens() {
                if let NodeOrToken::Token(tok) = child {
                    match tok.kind() {
                        SyntaxKind::BLOCK_QUOTE_MARKER => {
                            skip_marker_whitespace = true;
                        }
                        SyntaxKind::WHITESPACE if skip_marker_whitespace => {
                            skip_marker_whitespace = false;
                        }
                        SyntaxKind::AUTO_LINK_MARKER | SyntaxKind::TEXT => {
                            skip_marker_whitespace = false;
                            result.push_str(tok.text());
                        }
                        _ => {}
                    }
                }
            }
            result
        }
        SyntaxKind::CODE_SPAN => {
            let mut content = String::new();
            let mut attributes = String::new();
            let mut marker_len = 1usize;
            let mut skip_marker_whitespace = false;

            for child in node.children_with_tokens() {
                match child {
                    NodeOrToken::Node(n) if n.kind() == SyntaxKind::ATTRIBUTE => {
                        attributes = n.text().to_string();
                    }
                    NodeOrToken::Token(t) => {
                        if t.kind() == SyntaxKind::BLOCK_QUOTE_MARKER {
                            skip_marker_whitespace = true;
                        } else if t.kind() == SyntaxKind::WHITESPACE && skip_marker_whitespace {
                            skip_marker_whitespace = false;
                        } else if t.kind() == SyntaxKind::CODE_SPAN_MARKER {
                            skip_marker_whitespace = false;
                            marker_len = marker_len.max(t.text().len());
                        } else if t.kind() != SyntaxKind::ATTRIBUTE {
                            skip_marker_whitespace = false;
                            content.push_str(t.text());
                        }
                    }
                    _ => {}
                }
            }

            // Preserve malformed multi-line triple-backtick code spans as-is so they
            // don't collapse into one line and then reparse differently on pass 2.
            if marker_len >= 3 && content.contains('\n') {
                let trimmed_start = content.trim_start();
                let first_line = trimmed_start.lines().next().unwrap_or_default();
                let looks_quarto_chunk_header =
                    trimmed_start.starts_with('{') && first_line.contains('}');
                if !looks_quarto_chunk_header {
                    return node.text().to_string();
                }
            }

            // Normalize content: replace newlines with spaces and trim
            // Pandoc strips leading/trailing spaces from code spans
            let normalized_content =
                if matches!(config.tab_stops, crate::config::TabStopMode::Preserve) {
                    content.replace('\n', " ").trim().to_string()
                } else {
                    expand_tabs_code_span(&content, config.tab_width)
                };

            let mut backtick_runs = std::collections::HashSet::new();
            let mut current_run = 0;
            for ch in normalized_content.chars() {
                if ch == '`' {
                    current_run += 1;
                } else if current_run > 0 {
                    backtick_runs.insert(current_run);
                    current_run = 0;
                }
            }
            if current_run > 0 {
                backtick_runs.insert(current_run);
            }

            let max_run = backtick_runs.iter().copied().max().unwrap_or(0);

            let needs_padding = normalized_content.starts_with('`')
                || normalized_content.ends_with('`')
                || normalized_content.is_empty();
            let padding = if needs_padding { " " } else { "" };

            let min_needed = (max_run + 1).max(1);
            let final_backtick_count = if normalized_content.is_empty() {
                min_needed.max(marker_len).max(2)
            } else {
                min_needed
            };

            format!(
                "{}{}{}{}",
                "`".repeat(final_backtick_count),
                padding.to_string() + &normalized_content + padding,
                "`".repeat(final_backtick_count),
                attributes
            )
        }
        SyntaxKind::INLINE_EXECUTABLE_CODE => {
            let mut prefix = String::new();
            let mut spacing = String::from(" ");
            let mut code = String::new();

            for child in node.children_with_tokens() {
                if let NodeOrToken::Token(t) = child {
                    match t.kind() {
                        SyntaxKind::TEXT => prefix.push_str(t.text()),
                        SyntaxKind::WHITESPACE => spacing = t.text().to_string(),
                        SyntaxKind::INLINE_EXEC_CODE => code.push_str(t.text()),
                        _ => {}
                    }
                }
            }

            format!("`{}`{{r}}{}{}\\`\\`", prefix.trim_end(), spacing, code)
        }
        SyntaxKind::RAW_INLINE => {
            // Format raw inline span: `content`{=format}
            let mut content = String::new();
            let mut backtick_count = 1;
            let mut format_attr = String::new();

            for child in node.children_with_tokens() {
                match child {
                    NodeOrToken::Node(n) if n.kind() == SyntaxKind::ATTRIBUTE => {
                        format_attr = n.text().to_string();
                    }
                    NodeOrToken::Token(t) => {
                        if t.kind() == SyntaxKind::RAW_INLINE_MARKER {
                            backtick_count = t.text().len();
                        } else if t.kind() == SyntaxKind::RAW_INLINE_CONTENT {
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
        SyntaxKind::EMPHASIS => {
            let mut content = String::new();
            for child in node.children_with_tokens() {
                match child {
                    NodeOrToken::Node(n) => content.push_str(&format_inline_node(&n, config)),
                    NodeOrToken::Token(t) => {
                        if t.kind() != SyntaxKind::EMPHASIS_MARKER {
                            content.push_str(t.text());
                        }
                    }
                }
            }
            // Trim leading and trailing whitespace from emphasis content
            let content = content.trim();
            format!("*{}*", content)
        }
        SyntaxKind::STRONG => {
            let mut content = String::new();
            for child in node.children_with_tokens() {
                match child {
                    NodeOrToken::Node(n) => content.push_str(&format_inline_node(&n, config)),
                    NodeOrToken::Token(t) => {
                        if t.kind() != SyntaxKind::STRONG_MARKER {
                            content.push_str(t.text());
                        }
                    }
                }
            }
            // Trim leading and trailing whitespace from strong emphasis content
            let content = content.trim();
            format!("**{}**", content)
        }
        SyntaxKind::BRACKETED_SPAN => {
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
                        if n.kind() == SyntaxKind::SPAN_CONTENT {
                            for elem in n.children_with_tokens() {
                                match elem {
                                    NodeOrToken::Token(t) => result.push_str(t.text()),
                                    NodeOrToken::Node(nested) => {
                                        result.push_str(&format_inline_node(&nested, config));
                                    }
                                }
                            }
                        } else if n.kind() == SyntaxKind::SPAN_ATTRIBUTES {
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
        SyntaxKind::INLINE_MATH => {
            // Check if this is display math (has DisplayMathMarker)
            let is_display_math = node.children_with_tokens().any(|t| {
                matches!(t, NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::DISPLAY_MATH_MARKER)
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
                        if tok.kind() == SyntaxKind::INLINE_MATH_MARKER
                            || tok.kind() == SyntaxKind::DISPLAY_MATH_MARKER =>
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
                            "$`" => ("$`", "`$"),
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
        SyntaxKind::DISPLAY_MATH => {
            // Display math: $$content$$ or \[content\] or \\[content\\]
            // Format on separate lines with proper normalization
            let Some(display_math) = DisplayMath::cast(node.clone()) else {
                return node.text().to_string();
            };
            let content = display_math.content();

            // Preserve malformed display math that contains unescaped single-dollar
            // delimiters inside content; normalizing it can cause cross-pass drift.
            if display_math.has_unescaped_single_dollar_in_content() {
                return node.text().to_string();
            }

            let opening_value = display_math
                .opening_marker()
                .unwrap_or_else(|| "$$".to_string());
            let closing_value = display_math
                .closing_marker()
                .unwrap_or_else(|| "$$".to_string());
            let opening = opening_value.as_str();
            let closing = closing_value.as_str();
            let is_environment = display_math.is_environment_form();

            // Apply delimiter style preference
            let (open, close) = if is_environment {
                (opening, closing)
            } else {
                match config.math_delimiter_style {
                    MathDelimiterStyle::Preserve => (opening, closing),
                    MathDelimiterStyle::Dollars => ("$$", "$$"),
                    MathDelimiterStyle::Backslash => (r"\[", r"\]"),
                }
            };

            let mut result = String::new();
            if is_environment {
                result.push_str(open);
                result.push_str(&content);
                if !content.ends_with('\n') {
                    result.push('\n');
                }
                result.push_str(close);
                return result;
            }

            // Normalize content:
            // 1. Trim leading/trailing whitespace (including newlines)
            // 2. Ensure content is on separate lines from delimiters
            // 3. Strip common leading whitespace from all lines (preserve relative indentation)
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
        SyntaxKind::HARD_LINE_BREAK => {
            // Normalize hard line breaks to backslash-newline when escaped_line_breaks is enabled
            // Otherwise preserve original format (trailing spaces)
            if config.extensions.escaped_line_breaks {
                "\\\n".to_string()
            } else {
                node.text().to_string()
            }
        }
        SyntaxKind::SHORTCODE => {
            // Format Quarto shortcodes with normalized spacing
            format_shortcode(node)
        }
        SyntaxKind::CROSSREF => {
            let mut result = String::new();
            let mut skip_marker_whitespace = false;
            for child in node.children_with_tokens() {
                match child {
                    NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::BLOCK_QUOTE_MARKER => {
                        skip_marker_whitespace = true;
                    }
                    NodeOrToken::Token(tok)
                        if tok.kind() == SyntaxKind::WHITESPACE && skip_marker_whitespace =>
                    {
                        skip_marker_whitespace = false;
                    }
                    NodeOrToken::Token(tok) => {
                        skip_marker_whitespace = false;
                        result.push_str(tok.text());
                    }
                    NodeOrToken::Node(n) => {
                        skip_marker_whitespace = false;
                        result.push_str(&n.text().to_string());
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
