use crate::config::{Config, MathDelimiterStyle};
use crate::formatter::core::{normalize_attribute_text, normalize_span_attributes};
use crate::formatter::math::{self, MathContext, MathFormatOptions};
use crate::formatter::shortcodes::format_shortcode;
use crate::formatter::smart::normalize_smart_punctuation;
use crate::syntax::{DisplayMath, InlineMath, SyntaxKind, SyntaxNode};
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
                            // Autolinks are literal URLs/emails: emit verbatim,
                            // never smart-normalize (pandoc keeps `—`/`…` here).
                            result.push_str(tok.text());
                        }
                        _ => {}
                    }
                }
            }
            result
        }
        SyntaxKind::INLINE_CODE => {
            let mut content = String::new();
            let mut attributes = String::new();
            let mut marker_len = 1usize;
            let mut skip_marker_whitespace = false;

            for child in node.children_with_tokens() {
                match child {
                    NodeOrToken::Node(n) if n.kind() == SyntaxKind::ATTRIBUTE => {
                        // The parser now preserves attribute bytes verbatim;
                        // normalization (id-first, quoted values) is a formatter
                        // concern, applied here as for headings.
                        attributes = normalize_attribute_text(&n.text().to_string());
                    }
                    NodeOrToken::Token(t) => {
                        if t.kind() == SyntaxKind::BLOCK_QUOTE_MARKER {
                            skip_marker_whitespace = true;
                        } else if t.kind() == SyntaxKind::WHITESPACE && skip_marker_whitespace {
                            skip_marker_whitespace = false;
                        } else if t.kind() == SyntaxKind::INLINE_CODE_MARKER {
                            skip_marker_whitespace = false;
                            marker_len = marker_len.max(t.text().len());
                        } else if t.kind() == SyntaxKind::INLINE_CODE_CONTENT {
                            skip_marker_whitespace = false;
                            // Code spans are literal: never apply smart
                            // punctuation normalization to their contents
                            // (pandoc keeps `—`/`…`/curly quotes verbatim here).
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
        SyntaxKind::INLINE_EXEC => {
            let mut prefix = String::new();
            let mut spacing = String::from(" ");
            let mut code = String::new();

            for child in node.children_with_tokens() {
                if let NodeOrToken::Token(t) = child {
                    match t.kind() {
                        SyntaxKind::TEXT => prefix.push_str(t.text()),
                        SyntaxKind::WHITESPACE => spacing = t.text().to_string(),
                        SyntaxKind::INLINE_EXEC_CONTENT => code.push_str(t.text()),
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
                            content.push_str(
                                normalize_smart_punctuation(
                                    t.text(),
                                    config.formatter_extensions.smart,
                                    config.formatter_extensions.smart_quotes,
                                )
                                .as_ref(),
                            );
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
            let mut skip_marker_whitespace = false;
            for child in node.children_with_tokens() {
                match child {
                    NodeOrToken::Node(n) => {
                        skip_marker_whitespace = false;
                        if n.kind() == SyntaxKind::DISPLAY_MATH {
                            content.push_str(&n.text().to_string());
                        } else {
                            content.push_str(&format_inline_node(&n, config));
                        }
                    }
                    NodeOrToken::Token(t) => {
                        if t.kind() == SyntaxKind::BLOCK_QUOTE_MARKER {
                            skip_marker_whitespace = true;
                            continue;
                        }
                        if t.kind() == SyntaxKind::WHITESPACE && skip_marker_whitespace {
                            skip_marker_whitespace = false;
                            continue;
                        }
                        skip_marker_whitespace = false;
                        if t.kind() != SyntaxKind::EMPHASIS_MARKER {
                            content.push_str(
                                normalize_smart_punctuation(
                                    t.text(),
                                    config.formatter_extensions.smart,
                                    config.formatter_extensions.smart_quotes,
                                )
                                .as_ref(),
                            );
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
            let mut skip_marker_whitespace = false;
            for child in node.children_with_tokens() {
                match child {
                    NodeOrToken::Node(n) => {
                        skip_marker_whitespace = false;
                        if n.kind() == SyntaxKind::DISPLAY_MATH {
                            content.push_str(&n.text().to_string());
                        } else {
                            content.push_str(&format_inline_node(&n, config));
                        }
                    }
                    NodeOrToken::Token(t) => {
                        if t.kind() == SyntaxKind::BLOCK_QUOTE_MARKER {
                            skip_marker_whitespace = true;
                            continue;
                        }
                        if t.kind() == SyntaxKind::WHITESPACE && skip_marker_whitespace {
                            skip_marker_whitespace = false;
                            continue;
                        }
                        skip_marker_whitespace = false;
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
        SyntaxKind::INLINE_HTML_SPAN => {
            // Inline `<span ...>...</span>` lift (Pandoc dialect). The open
            // tag's bytes are tokenized at finer granularity (TEXT, WHITESPACE,
            // HTML_ATTRS) — emit them verbatim. SPAN_CONTENT recurses through
            // the inline formatter for nested markdown.
            let mut result = String::new();
            for child in node.children_with_tokens() {
                match child {
                    NodeOrToken::Token(t) => {
                        result.push_str(t.text());
                    }
                    NodeOrToken::Node(n) => {
                        if n.kind() == SyntaxKind::SPAN_CONTENT {
                            for elem in n.children_with_tokens() {
                                match elem {
                                    NodeOrToken::Token(t) => result.push_str(t.text()),
                                    NodeOrToken::Node(nested) => {
                                        result.push_str(&format_inline_node(&nested, config));
                                    }
                                }
                            }
                        } else {
                            // HTML_ATTRS and any other open-tag region nodes —
                            // emit their bytes verbatim to stay lossless.
                            result.push_str(&n.text().to_string());
                        }
                    }
                }
            }
            result
        }
        SyntaxKind::BRACKETED_SPAN => {
            // Format bracketed span: [content]{.attributes}
            // Need to traverse children to avoid extra spaces
            let mut result = String::new();
            let mut skip_marker_whitespace = false;
            for child in node.children_with_tokens() {
                match child {
                    NodeOrToken::Token(t) => {
                        if t.kind() == SyntaxKind::BLOCK_QUOTE_MARKER {
                            skip_marker_whitespace = true;
                            continue;
                        }
                        if t.kind() == SyntaxKind::WHITESPACE && skip_marker_whitespace {
                            skip_marker_whitespace = false;
                            continue;
                        }
                        skip_marker_whitespace = false;
                        result.push_str(
                            normalize_smart_punctuation(
                                t.text(),
                                config.formatter_extensions.smart,
                                config.formatter_extensions.smart_quotes,
                            )
                            .as_ref(),
                        );
                    }
                    NodeOrToken::Node(n) => {
                        // Recursively format nested content
                        if n.kind() == SyntaxKind::SPAN_CONTENT {
                            let mut skip_marker_whitespace = false;
                            for elem in n.children_with_tokens() {
                                match elem {
                                    NodeOrToken::Token(t) => {
                                        if t.kind() == SyntaxKind::BLOCK_QUOTE_MARKER {
                                            skip_marker_whitespace = true;
                                            continue;
                                        }
                                        if t.kind() == SyntaxKind::WHITESPACE
                                            && skip_marker_whitespace
                                        {
                                            skip_marker_whitespace = false;
                                            continue;
                                        }
                                        skip_marker_whitespace = false;
                                        result.push_str(
                                            normalize_smart_punctuation(
                                                t.text(),
                                                config.formatter_extensions.smart,
                                                config.formatter_extensions.smart_quotes,
                                            )
                                            .as_ref(),
                                        );
                                    }
                                    NodeOrToken::Node(nested) => {
                                        skip_marker_whitespace = false;
                                        result.push_str(&format_inline_node(&nested, config));
                                    }
                                }
                            }
                        } else if n.kind() == SyntaxKind::SPAN_ATTRIBUTES {
                            // Normalize attributes: collapse interior whitespace
                            // runs to a single space (structure-independent).
                            result.push_str(&normalize_span_attributes(&n));
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

            // Content now lives in a `MATH_CONTENT` subtree; its text is the
            // raw bytes between the delimiters (verbatim path).
            let content = node
                .children()
                .find(|n| n.kind() == SyntaxKind::MATH_CONTENT)
                .map(|n| n.text().to_string())
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

            // Experimental gate: reformat content structurally (clean content
            // read via the prefix-stripping `InlineMath::content()`). Off ⇒
            // verbatim (byte-identical to before).
            if is_display_math {
                let opts = MathFormatOptions::from_config(config, MathContext::Display);
                if opts.enabled {
                    let clean = InlineMath::cast(node.clone())
                        .map(|m| m.content())
                        .unwrap_or_else(|| content.clone());
                    format!("{}\n{}\n{}", open, math::format_math(&clean, &opts), close)
                } else {
                    format!("{}\n{}\n{}", open, content.trim(), close)
                }
            } else {
                let opts = MathFormatOptions::from_config(config, MathContext::Inline);
                if opts.enabled {
                    let clean = InlineMath::cast(node.clone())
                        .map(|m| m.content())
                        .unwrap_or_else(|| content.clone());
                    format!("{}{}{}", open, math::format_math(&clean, &opts), close)
                } else {
                    format!("{}{}{}", open, content, close)
                }
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
                let opts = MathFormatOptions::from_config(config, MathContext::EnvironmentBody);
                result.push_str(open);
                if opts.enabled {
                    result.push('\n');
                    result.push_str(&math::format_math(&content, &opts));
                    result.push('\n');
                } else {
                    result.push_str(&content);
                    if !content.ends_with('\n') {
                        result.push('\n');
                    }
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

            let opts = MathFormatOptions::from_config(config, MathContext::Display);
            if opts.enabled {
                result.push_str(&math::format_math(&content, &opts));
                result.push('\n');
            } else {
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
            }

            result.push_str(close);
            result
        }
        SyntaxKind::HARD_LINE_BREAK => {
            // Normalize hard line breaks to backslash-newline when escaped_line_breaks is enabled
            // Otherwise preserve original format (trailing spaces)
            if config.formatter_extensions.escaped_line_breaks {
                "\\\n".to_string()
            } else {
                node.text().to_string()
            }
        }
        SyntaxKind::NONBREAKING_SPACE => "\\ ".to_string(),
        SyntaxKind::SHORTCODE => {
            // Format Quarto shortcodes with normalized spacing
            format_shortcode(node)
        }
        SyntaxKind::INLINE_FOOTNOTE => {
            let mut content = String::new();
            for child in node.children_with_tokens() {
                match child {
                    NodeOrToken::Node(n) => content.push_str(&format_inline_node(&n, config)),
                    NodeOrToken::Token(t) => {
                        if !matches!(
                            t.kind(),
                            SyntaxKind::INLINE_FOOTNOTE_START | SyntaxKind::INLINE_FOOTNOTE_END
                        ) {
                            content.push_str(
                                normalize_smart_punctuation(
                                    t.text(),
                                    config.formatter_extensions.smart,
                                    config.formatter_extensions.smart_quotes,
                                )
                                .as_ref(),
                            );
                        }
                    }
                }
            }
            let normalized = content
                .split_ascii_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            format!("^[{}]", normalized)
        }
        SyntaxKind::CITATION => {
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
                        result.push_str(
                            normalize_smart_punctuation(
                                tok.text(),
                                config.formatter_extensions.smart,
                                config.formatter_extensions.smart_quotes,
                            )
                            .as_ref(),
                        );
                    }
                    NodeOrToken::Node(n) => {
                        skip_marker_whitespace = false;
                        result.push_str(&n.text().to_string());
                    }
                }
            }
            result
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
                        result.push_str(
                            normalize_smart_punctuation(
                                tok.text(),
                                config.formatter_extensions.smart,
                                config.formatter_extensions.smart_quotes,
                            )
                            .as_ref(),
                        );
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
