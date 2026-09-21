use crate::config::{Config, MathDelimiterStyle};
use crate::formatter::Formatter;
use crate::formatter::core::{normalize_attribute_text, normalize_span_attributes};
use crate::formatter::math::{self, MathContext, MathFormatOptions};
use crate::formatter::shortcodes::format_shortcode;
use crate::formatter::smart::normalize_smart_punctuation;
use crate::syntax::{
    DisplayMath, ImageAlt, InlineMath, InlineNode, LinkText, SyntaxKind, SyntaxNode, SyntaxToken,
    UnresolvedReference, code_span_payload, text_without_line_prefixes,
};
use rowan::NodeOrToken;
use rowan::ast::AstNode;
use std::borrow::Cow;

impl Formatter {
    pub(super) fn format_delimited_inline(
        &mut self,
        node: &SyntaxNode,
        indent: usize,
        delimiter: &str,
        marker: SyntaxKind,
    ) {
        self.output.push_str(delimiter);
        for child in node.children_with_tokens() {
            match child {
                NodeOrToken::Node(child) => self.format_node_sync(&child, indent),
                NodeOrToken::Token(token) if token.kind() != marker => {
                    self.output.push_str(token.text());
                }
                NodeOrToken::Token(_) => {}
            }
        }
        self.output.push_str(delimiter);
    }
}

/// Expand a code span's tabs and join its lines, the way a reader sees it.
///
/// Tabs are expanded before the reader runs, so each one is worth however many
/// columns it takes to reach the next stop *from its column in the source
/// line* — `` a`x\ty`b `` is one space, `` `x\ty` `` is two. Measuring from
/// column 0 of the span's own content instead would rewrite the span to a
/// different number of spaces, changing what the document means. The column
/// bookkeeping (container prefixes, list gobble) lives in the parser crate so
/// the pandoc-native projector and the formatter agree; only the line join is
/// ours, since the reader turns each internal newline into a single space.
fn expand_tabs_code_span(node: &SyntaxNode, tab_width: usize) -> String {
    code_span_payload(node, tab_width).replace('\n', " ")
}

fn format_citation_like(node: &SyntaxNode, config: &Config) -> String {
    let mut result = String::new();
    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::LINE_PREFIX => {}
            NodeOrToken::Token(tok) => {
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
                result.push_str(&n.text().to_string());
            }
        }
    }
    collapse_internal_newlines(&result).into_owned()
}

fn collapse_internal_newlines(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains('\n') {
        return std::borrow::Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch.is_whitespace() {
            let mut has_newline = ch == '\n';
            let mut run = String::from(ch);
            while let Some(&next) = chars.peek() {
                if next.is_whitespace() {
                    has_newline |= next == '\n';
                    run.push(next);
                    chars.next();
                } else {
                    break;
                }
            }
            if has_newline {
                out.push(' ');
            } else {
                out.push_str(&run);
            }
        } else {
            out.push(ch);
        }
    }
    std::borrow::Cow::Owned(out)
}

/// Whether `node` lives inside a table cell whose row must stay on a single
/// line. Pipe and simple tables encode one row per source line, so any newline
/// emitted inside a cell breaks the table; grid and multiline tables can hold
/// multi-line block content and are excluded.
fn in_single_line_table_cell(node: &SyntaxNode) -> bool {
    let mut in_cell = false;
    for ancestor in node.ancestors() {
        match ancestor.kind() {
            SyntaxKind::TABLE_CELL => in_cell = true,
            SyntaxKind::PIPE_TABLE | SyntaxKind::SIMPLE_TABLE => return in_cell,
            SyntaxKind::GRID_TABLE | SyntaxKind::MULTILINE_TABLE => return false,
            _ => {}
        }
    }
    false
}

pub(super) fn collapse_spaces(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut in_space = false;
    for ch in text.chars() {
        if matches!(ch, ' ' | '\t') {
            if !in_space {
                result.push(' ');
            }
            in_space = true;
        } else {
            result.push(ch);
            in_space = false;
        }
    }
    result
}

fn inline_token_text(token: &SyntaxToken, collapse_ws: bool) -> Cow<'_, str> {
    if collapse_ws && token.kind() == SyntaxKind::TEXT {
        Cow::Owned(collapse_spaces(token.text()))
    } else {
        Cow::Borrowed(token.text())
    }
}

fn contains_inline_prose(node: &SyntaxNode) -> bool {
    matches!(
        InlineNode::cast(node.clone().into()),
        InlineNode::Link(_)
            | InlineNode::Image(_)
            | InlineNode::Strikeout(_)
            | InlineNode::Mark(_)
            | InlineNode::Superscript(_)
            | InlineNode::Subscript(_)
    ) || LinkText::can_cast(node.kind())
        || ImageAlt::can_cast(node.kind())
}

/// Format an inline node to normalized string (e.g., emphasis with asterisks).
pub(super) fn format_inline_node(node: &SyntaxNode, config: &Config) -> String {
    format_inline_node_with_spacing(node, config, false)
}

/// Only prose tokens may lose spaces: literal payloads and link destinations
/// can contain identical-looking runs with different meanings.
pub(super) fn format_inline_node_with_spacing(
    node: &SyntaxNode,
    config: &Config,
    collapse_ws: bool,
) -> String {
    match node.kind() {
        SyntaxKind::AUTO_LINK => {
            let mut result = String::new();
            for child in node.descendants_with_tokens() {
                if let NodeOrToken::Token(tok) = child {
                    match tok.kind() {
                        SyntaxKind::AUTO_LINK_MARKER | SyntaxKind::TEXT => {
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

            for child in node.children_with_tokens() {
                match child {
                    NodeOrToken::Node(n) if n.kind() == SyntaxKind::ATTRIBUTE => {
                        attributes = normalize_attribute_text(&n.text().to_string());
                    }
                    NodeOrToken::Token(t) => {
                        if t.kind() == SyntaxKind::INLINE_CODE_MARKER {
                            marker_len = marker_len.max(t.text().len());
                        } else if t.kind() == SyntaxKind::INLINE_CODE_CONTENT {
                            content.push_str(t.text());
                        }
                    }
                    _ => {}
                }
            }

            // Reducing the delimiter would turn documented examples into execution.
            if marker_len > 1
                && crate::syntax::inline_execution_parts(content.trim(), &config.parser_extensions)
                    .is_some()
            {
                return node
                    .descendants_with_tokens()
                    .filter_map(|el| el.into_token())
                    .filter(|token| token.kind() != SyntaxKind::LINE_PREFIX)
                    .map(|token| token.text().to_string())
                    .collect();
            }

            let mut collapse_block_chunk = false;
            if marker_len >= 3 && content.contains('\n') {
                let trimmed_start = content.trim_start();
                let first_line = trimmed_start.lines().next().unwrap_or_default();
                let looks_quarto_chunk_header =
                    trimmed_start.starts_with('{') && first_line.contains('}');
                if looks_quarto_chunk_header {
                    collapse_block_chunk = true;
                }
            }

            let mut normalized_content =
                if matches!(config.tab_stops, crate::config::TabStopMode::Preserve) {
                    content.replace('\n', " ")
                } else {
                    expand_tabs_code_span(node, config.tab_width)
                };
            if collapse_block_chunk {
                normalized_content = normalized_content.trim().to_string();
            }

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
        SyntaxKind::INLINE_EXEC_SPAN => node
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|token| token.kind() != SyntaxKind::LINE_PREFIX)
            .map(|token| token.text().to_string())
            .collect(),
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
            for child in node.children_with_tokens() {
                match child {
                    NodeOrToken::Node(n) => {
                        if n.kind() == SyntaxKind::DISPLAY_MATH {
                            content.push_str(&n.text().to_string());
                        } else {
                            content.push_str(&format_inline_node_with_spacing(
                                &n,
                                config,
                                collapse_ws,
                            ));
                        }
                    }
                    NodeOrToken::Token(t) => {
                        if t.kind() == SyntaxKind::LINE_PREFIX {
                            continue;
                        }
                        if t.kind() != SyntaxKind::EMPHASIS_MARKER {
                            content.push_str(
                                normalize_smart_punctuation(
                                    inline_token_text(&t, collapse_ws).as_ref(),
                                    config.formatter_extensions.smart,
                                    config.formatter_extensions.smart_quotes,
                                )
                                .as_ref(),
                            );
                        }
                    }
                }
            }
            let content = content.trim();
            format!("*{}*", content)
        }
        SyntaxKind::STRONG => {
            let mut content = String::new();
            for child in node.children_with_tokens() {
                match child {
                    NodeOrToken::Node(n) => {
                        if n.kind() == SyntaxKind::DISPLAY_MATH {
                            content.push_str(&n.text().to_string());
                        } else {
                            content.push_str(&format_inline_node_with_spacing(
                                &n,
                                config,
                                collapse_ws,
                            ));
                        }
                    }
                    NodeOrToken::Token(t) => {
                        if t.kind() == SyntaxKind::LINE_PREFIX {
                            continue;
                        }
                        if t.kind() != SyntaxKind::STRONG_MARKER {
                            content.push_str(&inline_token_text(&t, collapse_ws));
                        }
                    }
                }
            }
            let content = content.trim();
            format!("**{}**", content)
        }
        SyntaxKind::INLINE_HTML_SPAN => {
            let mut result = String::new();
            for child in node.children_with_tokens() {
                match child {
                    NodeOrToken::Token(t) => {
                        if t.kind() == SyntaxKind::LINE_PREFIX {
                            continue;
                        }
                        result.push_str(&inline_token_text(&t, collapse_ws));
                    }
                    NodeOrToken::Node(n) => {
                        if n.kind() == SyntaxKind::SPAN_CONTENT {
                            for elem in n.children_with_tokens() {
                                match elem {
                                    NodeOrToken::Token(t) => {
                                        if t.kind() == SyntaxKind::LINE_PREFIX {
                                            continue;
                                        }
                                        result.push_str(&inline_token_text(&t, collapse_ws))
                                    }
                                    NodeOrToken::Node(nested) => {
                                        result.push_str(&format_inline_node_with_spacing(
                                            &nested,
                                            config,
                                            collapse_ws,
                                        ));
                                    }
                                }
                            }
                        } else {
                            // The enclosing container emits its own continuation prefixes.
                            result.push_str(&text_without_line_prefixes(&n));
                        }
                    }
                }
            }
            result
        }
        SyntaxKind::BRACKETED_SPAN => {
            let mut result = String::new();
            for child in node.children_with_tokens() {
                match child {
                    NodeOrToken::Token(t) => {
                        if t.kind() == SyntaxKind::LINE_PREFIX {
                            continue;
                        }
                        result.push_str(
                            normalize_smart_punctuation(
                                inline_token_text(&t, collapse_ws).as_ref(),
                                config.formatter_extensions.smart,
                                config.formatter_extensions.smart_quotes,
                            )
                            .as_ref(),
                        );
                    }
                    NodeOrToken::Node(n) => {
                        if n.kind() == SyntaxKind::SPAN_CONTENT {
                            for elem in n.children_with_tokens() {
                                match elem {
                                    NodeOrToken::Token(t) => {
                                        if t.kind() == SyntaxKind::LINE_PREFIX {
                                            continue;
                                        }
                                        result.push_str(
                                            normalize_smart_punctuation(
                                                inline_token_text(&t, collapse_ws).as_ref(),
                                                config.formatter_extensions.smart,
                                                config.formatter_extensions.smart_quotes,
                                            )
                                            .as_ref(),
                                        );
                                    }
                                    NodeOrToken::Node(nested) => {
                                        result.push_str(&format_inline_node_with_spacing(
                                            &nested,
                                            config,
                                            collapse_ws,
                                        ));
                                    }
                                }
                            }
                        } else if n.kind() == SyntaxKind::SPAN_ATTRIBUTES {
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
            let is_display_math = node.children_with_tokens().any(|t| {
                matches!(t, NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::DISPLAY_MATH_MARKER)
            });

            let content = InlineMath::cast(node.clone())
                .map(|math| math.content())
                .unwrap_or_default();

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

            let (open, close) = match config.math_delimiter_style {
                MathDelimiterStyle::Preserve => {
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
                    if is_display_math {
                        ("$$", "$$")
                    } else {
                        ("$", "$")
                    }
                }
                MathDelimiterStyle::Backslash => {
                    if is_display_math {
                        (r"\[", r"\]")
                    } else {
                        (r"\(", r"\)")
                    }
                }
            };

            if is_display_math {
                let opts = MathFormatOptions::from_config(config, MathContext::Display);
                let clean = content.clone();
                match math::format_math(&clean, &opts) {
                    Some(body) => format!("{}\n{}\n{}", open, body, close),
                    None => format!(
                        "{}\n{}\n{}",
                        open,
                        math::trim_preserved_math_body(&content),
                        close,
                    ),
                }
            } else {
                let opts = MathFormatOptions::from_config(config, MathContext::Inline);
                let clean = content.clone();
                match math::format_math(&clean, &opts) {
                    Some(body) => format!("{}{}{}", open, body, close),
                    None => format!("{}{}{}", open, collapse_internal_newlines(&content), close),
                }
            }
        }
        SyntaxKind::DISPLAY_MATH => {
            let Some(display_math) = DisplayMath::cast(node.clone()) else {
                return node.text().to_string();
            };
            let content = display_math.content();

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

            if in_single_line_table_cell(node) {
                let inline_content = content.split_whitespace().collect::<Vec<_>>().join(" ");
                return format!("{opening}{inline_content}{closing}");
            }

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
                match math::format_math(&content, &opts) {
                    Some(body) => {
                        result.push('\n');
                        math::push_body_with_trailing_newline(&mut result, &body);
                    }
                    None => {
                        result.push_str(&math::trim_trailing_whitespace_per_line(&content));
                        if !content.ends_with('\n') {
                            result.push('\n');
                        }
                    }
                }
                result.push_str(close);
                return result;
            }

            result.push_str(open);
            result.push('\n');

            let opts = MathFormatOptions::from_config(config, MathContext::Display);
            match math::format_math(&content, &opts) {
                Some(body) => {
                    math::push_body_with_trailing_newline(&mut result, &body);
                }
                None => {
                    let cleaned_content = math::trim_trailing_whitespace_per_line(&content);
                    let mut trimmed_content = cleaned_content.trim_end_matches(['\r', '\n']);
                    while let Some((first, rest)) = trimmed_content.split_once('\n') {
                        if first.trim().is_empty() {
                            trimmed_content = rest;
                        } else {
                            break;
                        }
                    }
                    if !trimmed_content.is_empty() {
                        let min_indent = trimmed_content
                            .lines()
                            .filter(|line| !line.trim().is_empty())
                            .map(|line| line.len() - line.trim_start().len())
                            .min()
                            .unwrap_or(0);

                        let pad = " ".repeat(config.math_indent);
                        for line in trimmed_content.lines() {
                            let stripped = if line.len() >= min_indent {
                                &line[min_indent..]
                            } else {
                                line
                            };
                            if !stripped.is_empty() {
                                result.push_str(&pad);
                                result.push_str(stripped);
                            }
                            result.push('\n');
                        }
                    }
                }
            }

            result.push_str(close);
            result
        }
        SyntaxKind::HARD_LINE_BREAK => {
            if config.formatter_extensions.escaped_line_breaks {
                "\\\n".to_string()
            } else {
                let text = node.text().to_string();
                let ending = text.find(['\r', '\n']).map_or("", |at| &text[at..]);
                format!("{}{ending}", crate::utils::hard_break_marker(&text))
            }
        }
        SyntaxKind::NONBREAKING_SPACE => "\\ ".to_string(),
        SyntaxKind::SHORTCODE => format_shortcode(node),
        SyntaxKind::INLINE_FOOTNOTE => {
            let mut content = String::new();
            for child in node.children_with_tokens() {
                match child {
                    NodeOrToken::Node(n) => {
                        content.push_str(&format_inline_node_with_spacing(&n, config, collapse_ws))
                    }
                    NodeOrToken::Token(t) => {
                        if !matches!(
                            t.kind(),
                            SyntaxKind::INLINE_FOOTNOTE_START | SyntaxKind::INLINE_FOOTNOTE_END
                        ) {
                            content.push_str(
                                normalize_smart_punctuation(
                                    inline_token_text(&t, collapse_ws).as_ref(),
                                    config.formatter_extensions.smart,
                                    config.formatter_extensions.smart_quotes,
                                )
                                .as_ref(),
                            );
                        }
                    }
                }
            }
            let normalized = if collapse_ws {
                content.trim().to_string()
            } else {
                content
                    .split_ascii_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            format!("^[{}]", normalized)
        }
        SyntaxKind::CITATION | SyntaxKind::CROSSREF => format_citation_like(node, config),
        SyntaxKind::LATEX_COMMAND => math::format_latex_math_environment(node, config)
            .unwrap_or_else(|| node.text().to_string()),
        _ if UnresolvedReference::can_cast(node.kind()) => {
            // Containers add their own prefixes to every physical output line.
            text_without_line_prefixes(node)
        }
        _ if collapse_ws && contains_inline_prose(node) => {
            let mut result = String::new();
            for child in node.children_with_tokens() {
                match child {
                    NodeOrToken::Token(token) if token.kind() == SyntaxKind::LINE_PREFIX => {}
                    NodeOrToken::Token(token) => {
                        result.push_str(&inline_token_text(&token, true));
                    }
                    NodeOrToken::Node(child) => {
                        result.push_str(&format_inline_node_with_spacing(&child, config, true));
                    }
                }
            }
            result
        }
        _ => node.text().to_string(),
    }
}
