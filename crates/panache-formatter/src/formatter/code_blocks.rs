use crate::config::{Config, Flavor};
use crate::syntax::{AstNode, SyntaxKind, SyntaxNode};
use panache_parser::parser::blocks::code_blocks::{CodeBlockType, InfoString};
use rowan::NodeOrToken;
use std::collections::HashMap;

use super::hashpipe;

pub type FormattedCodeMap = HashMap<(String, String), String>;

#[derive(Debug, Clone)]
pub struct ExternalCodeBlock {
    pub language: String,
    pub original: String,
    pub formatter_input: String,
    pub hashpipe_prefix: Option<String>,
}

/// Format a code block, normalizing fence markers and attributes based on config
pub(super) fn format_code_block(
    node: &SyntaxNode,
    config: &Config,
    formatted_code: &FormattedCodeMap,
    output: &mut String,
) {
    if is_unclosed_fenced_code_block(node) {
        output.push_str(&node.text().to_string());
        return;
    }

    let (info_node, language, extracted_content) = extract_code_block_parts(node);
    let mut content = extracted_content;
    let language_key = language.unwrap_or_default();

    if let Some(formatted) = formatted_code.get(&(language_key.clone(), content.clone())) {
        content = expand_tabs_with_width(formatted, config.tab_width);
    } else if let Some(raw_content) = extract_raw_code_block_content(node)
        && let Some(formatted) = formatted_code.get(&(language_key, raw_content))
    {
        content = expand_tabs_with_width(formatted, config.tab_width);
    }

    let info_node = match info_node {
        Some(node) => node,
        None => {
            // No info string, just output basic fence
            let mut final_content = content;
            if !matches!(config.tab_stops, crate::config::TabStopMode::Preserve) {
                final_content = expand_tabs_with_width(&final_content, config.tab_width);
            }
            let fence_char = '`';
            let fence_length = determine_fence_length(&final_content, fence_char);
            output.push_str(&fence_char.to_string().repeat(fence_length));
            output.push('\n');
            output.push_str(&final_content);
            output.push_str(&fence_char.to_string().repeat(fence_length));
            output.push('\n');
            return;
        }
    };

    // Parse the info string to get block type
    let info_string_raw = info_node.text().to_string();
    let info = InfoString::parse(&info_string_raw);

    // Check if we have formatted version from external formatter
    let mut final_content = content;
    if !matches!(config.tab_stops, crate::config::TabStopMode::Preserve) {
        final_content = expand_tabs_with_width(&final_content, config.tab_width);
    }

    // Determine fence character based on config
    let fence_char = '`';

    // Determine fence length (check for nested fences in content)
    let fence_length = determine_fence_length(&final_content, fence_char);

    // Check if we should use hashpipe format for Quarto executable chunks
    let use_hashpipe = matches!(config.flavor, Flavor::Quarto | Flavor::RMarkdown)
        && matches!(&info.block_type, CodeBlockType::Executable { .. });

    if use_hashpipe {
        // Try to format as hashpipe with YAML-style options
        // Falls back to inline format if language comment syntax is unknown
        if format_code_block_hashpipe(
            node,
            &info_node,
            &final_content,
            fence_char,
            fence_length,
            config,
            output,
        ) {
            return; // Successfully formatted as hashpipe
        }
        // Fall through to traditional inline format for unknown languages
    }

    // Format the info string based on config and block type (traditional inline)
    let formatted_info = format_info_string(&info_node, &info);

    log::trace!("formatted_info = '{}'", formatted_info);

    // Output normalized code block
    for _ in 0..fence_length {
        output.push(fence_char);
    }
    if !formatted_info.is_empty() {
        output.push_str(&formatted_info);
    }
    output.push('\n');
    output.push_str(&final_content);
    for _ in 0..fence_length {
        output.push(fence_char);
    }
    output.push('\n');
}

fn is_unclosed_fenced_code_block(node: &SyntaxNode) -> bool {
    let has_open = node
        .children()
        .any(|child| child.kind() == SyntaxKind::CODE_FENCE_OPEN);
    let has_close = node
        .children()
        .any(|child| child.kind() == SyntaxKind::CODE_FENCE_CLOSE);

    has_open && !has_close
}

fn extract_raw_code_block_content(node: &SyntaxNode) -> Option<String> {
    node.children()
        .find(|child| child.kind() == SyntaxKind::CODE_CONTENT)
        .map(|child| child.text().to_string())
}

fn expand_tabs_with_width(text: &str, tab_width: usize) -> String {
    let mut out = String::with_capacity(text.len());
    let mut col = 0usize;
    for ch in text.chars() {
        match ch {
            '\t' => {
                let spaces = tab_width - (col % tab_width);
                out.push_str(&" ".repeat(spaces));
                col += spaces;
            }
            '\n' => {
                out.push('\n');
                col = 0;
            }
            _ => {
                out.push(ch);
                col += 1;
            }
        }
    }
    out
}

fn strip_indent_columns(indent: &str, columns: usize) -> String {
    let mut remaining = columns;
    let mut idx = 0;
    for (i, ch) in indent.char_indices() {
        if remaining == 0 {
            break;
        }
        match ch {
            ' ' => {
                remaining = remaining.saturating_sub(1);
                idx = i + 1;
            }
            '\t' => {
                remaining = remaining.saturating_sub(4);
                idx = i + 1;
            }
            _ => break,
        }
    }
    indent[idx..].to_string()
}

fn indent_columns(indent: &str) -> usize {
    let mut cols = 0usize;
    for ch in indent.chars() {
        match ch {
            ' ' => cols += 1,
            '\t' => cols += 4 - (cols % 4),
            _ => break,
        }
    }
    cols
}

fn extract_code_block_parts(node: &SyntaxNode) -> (Option<SyntaxNode>, Option<String>, String) {
    let mut info_node: Option<SyntaxNode> = None;
    let mut language: Option<String> = None;
    let mut content = String::new();
    let mut has_fence = false;
    let mut fence_indent = String::new();
    let mut fence_indent_cols = 0usize;

    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Token(t) => {
                if t.kind() == SyntaxKind::WHITESPACE && !has_fence {
                    fence_indent = t.text().to_string();
                }
            }
            NodeOrToken::Node(n) => match n.kind() {
                SyntaxKind::CODE_FENCE_OPEN => {
                    has_fence = true;
                    fence_indent_cols = indent_columns(&fence_indent);
                    for child_token in n.children_with_tokens() {
                        if let NodeOrToken::Node(node) = child_token
                            && node.kind() == SyntaxKind::CODE_INFO
                        {
                            for info_token in node.children_with_tokens() {
                                if let NodeOrToken::Token(t) = info_token
                                    && t.kind() == SyntaxKind::CODE_LANGUAGE
                                {
                                    language = Some(t.text().to_string());
                                }
                            }
                            info_node = Some(node);
                        }
                    }
                }
                SyntaxKind::CODE_CONTENT => {
                    let base_indent_cols = if has_fence { fence_indent_cols } else { 4 };
                    let mut line_content = String::new();
                    let mut line_indent = String::new();
                    let mut at_line_start = true;
                    let mut saw_blockquote_marker = false;

                    for token in n.children_with_tokens() {
                        match token {
                            NodeOrToken::Token(t) => match t.kind() {
                                SyntaxKind::BLOCK_QUOTE_MARKER if at_line_start => {
                                    // Parser may preserve blockquote continuation markers inside
                                    // indented code content for losslessness. These are container
                                    // syntax, not code bytes, so ignore them for formatter output.
                                    saw_blockquote_marker = true;
                                }
                                SyntaxKind::WHITESPACE if at_line_start => {
                                    if saw_blockquote_marker {
                                        let ws = t.text();
                                        if let Some(stripped) = ws.strip_prefix(' ') {
                                            line_indent.push_str(stripped);
                                        } else {
                                            line_indent.push_str(ws);
                                        }
                                        saw_blockquote_marker = false;
                                    } else {
                                        line_indent.push_str(t.text());
                                    }
                                }
                                SyntaxKind::TEXT => {
                                    saw_blockquote_marker = false;
                                    if at_line_start && t.text().is_empty() {
                                        continue;
                                    }
                                    if at_line_start {
                                        line_content.push_str(&strip_indent_columns(
                                            &line_indent,
                                            base_indent_cols,
                                        ));
                                        line_indent.clear();
                                        at_line_start = false;
                                    }
                                    line_content.push_str(t.text());
                                }
                                SyntaxKind::NEWLINE => {
                                    saw_blockquote_marker = false;
                                    if !at_line_start {
                                        content.push_str(&line_content);
                                    }
                                    content.push('\n');
                                    line_content.clear();
                                    line_indent.clear();
                                    at_line_start = true;
                                }
                                _ => {}
                            },
                            NodeOrToken::Node(inner_node) => {
                                let node_text = inner_node.text().to_string();
                                if node_text.is_empty() {
                                    continue;
                                }
                                if at_line_start {
                                    line_content.push_str(&strip_indent_columns(
                                        &line_indent,
                                        base_indent_cols,
                                    ));
                                    line_indent.clear();
                                    at_line_start = false;
                                }
                                line_content.push_str(&node_text);
                            }
                        }
                    }

                    if !at_line_start {
                        content.push_str(&line_content);
                    }
                }
                _ => {}
            },
        }
    }

    (info_node, language, content)
}

/// Split container-stripped code-block `content` into its leading hashpipe
/// preamble (the `#|` header lines) and the remaining body, using the parser's
/// embedded `HASHPIPE_YAML_CONTENT` extent. The preamble is the first
/// `line_count` physical lines of `content` (container prefixes are already
/// stripped from both, so line counts line up). Returns `None` when the block
/// has no hashpipe preamble.
fn split_hashpipe_header(content: &str, code_block_node: &SyntaxNode) -> Option<(String, String)> {
    let line_count = hashpipe::hashpipe_preamble_line_count(code_block_node)?;
    let mut header_end = 0usize;
    for _ in 0..line_count {
        match content[header_end..].find('\n') {
            Some(rel) => header_end += rel + 1,
            None => {
                header_end = content.len();
                break;
            }
        }
    }
    Some((
        content[..header_end].to_string(),
        content[header_end..].to_string(),
    ))
}

/// Determine the minimum fence length needed to avoid conflicts with content
fn determine_fence_length(content: &str, fence_char: char) -> usize {
    let mut max_sequence = 0;
    let mut current_sequence = 0;

    for ch in content.chars() {
        if ch == fence_char {
            current_sequence += 1;
            max_sequence = max_sequence.max(current_sequence);
        } else if ch == '\n' || ch == '\r' {
            // Only count fence sequences at start of line as potential conflicts
            current_sequence = 0;
        } else if current_sequence > 0 {
            // Non-fence char, reset
            current_sequence = 0;
        }
    }

    // Use at least one more than the longest sequence in content, minimum 3 per spec
    (max_sequence + 1).max(3)
}

/// One entry in an executable fence's `{language ...}` info string, in the
/// order it appears. Class/Id sit on the fence as bare `.foo`/`#foo`
/// attributes; Label and KeyValue are the comma-list members.
#[derive(Debug, Clone)]
pub(super) enum ChunkOptionRepr {
    /// `.foo` attribute (text includes the leading `.`).
    Class(String),
    /// `#foo` attribute (text includes the leading `#`).
    Id(String),
    /// Bareword label, e.g. `mylabel` in `{r mylabel}`.
    Label(String),
    /// `key=value` (with or without quotes).
    KeyValue {
        key: String,
        value: String,
        is_quoted: bool,
    },
}

/// Extract chunk options from CST CHUNK_OPTIONS node, preserving order.
fn extract_chunk_options_from_cst(info_node: &SyntaxNode) -> Vec<ChunkOptionRepr> {
    use crate::syntax::{ChunkInfoItem, CodeInfo};

    let Some(info) = CodeInfo::cast(info_node.clone()) else {
        return Vec::new();
    };

    let mut options = Vec::new();
    let mut pending_label_parts = Vec::new();
    for item in info.chunk_items() {
        match item {
            ChunkInfoItem::Label(label) => {
                let value = label.text();
                if !value.is_empty() {
                    pending_label_parts.push(value);
                }
            }
            ChunkInfoItem::Class(class) => {
                let value = class.text();
                if !value.is_empty() {
                    options.push(ChunkOptionRepr::Class(value));
                }
            }
            ChunkInfoItem::Id(id) => {
                let value = id.text();
                if !value.is_empty() {
                    options.push(ChunkOptionRepr::Id(value));
                }
            }
            ChunkInfoItem::Option(option) => {
                if !pending_label_parts.is_empty() {
                    options.push(ChunkOptionRepr::Label(pending_label_parts.join(" ")));
                    pending_label_parts.clear();
                }
                if let (Some(key), Some(value)) = (option.key(), option.value()) {
                    options.push(ChunkOptionRepr::KeyValue {
                        key,
                        value,
                        is_quoted: option.is_quoted(),
                    });
                }
            }
        }
    }

    if !pending_label_parts.is_empty() {
        options.push(ChunkOptionRepr::Label(pending_label_parts.join(" ")));
    }

    options
}

/// Render the contents of an executable fence's `{...}` info string for the
/// given language and (possibly empty) option list. Class/id attributes are
/// emitted space-separated immediately after the language (pandoc canonical
/// shape — `{python .marimo .cell-code}`). Labels and `key=value` pairs are
/// emitted comma-separated after a `, ` separator (Quarto chunk-option
/// style — `{r, label="x", echo=false}`).
pub(super) fn render_executable_info(language: &str, options: &[ChunkOptionRepr]) -> String {
    let mut attr_parts = Vec::new();
    let mut option_parts = Vec::new();
    for option in options {
        match option {
            ChunkOptionRepr::Class(text) | ChunkOptionRepr::Id(text) => {
                attr_parts.push(text.clone());
            }
            ChunkOptionRepr::Label(text) => option_parts.push(text.clone()),
            ChunkOptionRepr::KeyValue {
                key,
                value,
                is_quoted,
            } => {
                if *is_quoted {
                    // Re-add quotes. Pick a quote char that won't collide with
                    // the value contents so we don't produce broken syntax like
                    // `key="class="cover""` for an original `key='class="cover"'`.
                    let quote = if value.contains('"') && !value.contains('\'') {
                        '\''
                    } else {
                        '"'
                    };
                    option_parts.push(format!("{}={}{}{}", key, quote, value, quote));
                } else {
                    option_parts.push(format!("{}={}", key, value));
                }
            }
        }
    }

    let mut out = String::from("{");
    out.push_str(language);
    if !attr_parts.is_empty() {
        out.push(' ');
        out.push_str(&attr_parts.join(" "));
    }
    if !option_parts.is_empty() {
        out.push_str(", ");
        out.push_str(&option_parts.join(", "));
    }
    out.push('}');
    out
}

/// Format the info string based on block type and config preferences
fn format_info_string(info_node: &SyntaxNode, info: &InfoString) -> String {
    log::trace!(
        "format_info_string: block_type={:?}, raw='{}'",
        info.block_type,
        info.raw
    );
    match &info.block_type {
        CodeBlockType::Plain => {
            // No language, just attributes (if any)
            if info.attributes.is_empty() {
                String::new()
            } else {
                format!("{{{}}}", format_attributes(&info.attributes, false))
            }
        }
        CodeBlockType::DisplayShortcut { language } => {
            // Display block with shortcut syntax
            if info.attributes.is_empty() {
                // Preserve the full info string, not just the first word.
                // Only the first word is the language class, but the rest is
                // meaningful, opaque metadata (e.g. Documenter.jl's
                // `@example foo`, `jldoctest; setup = :(...)`, `@repl bar`)
                // that must survive formatting. This bare multi-word form only
                // reaches the formatter under CommonMark/GFM; the Pandoc
                // dialect parses it as an inline code span upstream.
                info.raw.trim().to_string()
            } else {
                format!(
                    "{} {{{}}}",
                    language,
                    format_attributes(&info.attributes, false)
                )
            }
        }
        CodeBlockType::DisplayExplicit { classes } => {
            // Display block with explicit Pandoc syntax
            // Convert to shortcut form: ```{.python} -> ```python
            if let Some(first_class) = classes.first() {
                if info.attributes.is_empty() && classes.len() == 1 {
                    first_class.clone()
                } else {
                    // Mix shortcut + attributes
                    let mut attrs: Vec<String> =
                        classes.iter().skip(1).map(|c| format!(".{}", c)).collect();
                    attrs.extend(info.attributes.iter().map(|(k, v)| {
                        if let Some(val) = v {
                            format!("{}=\"{}\"", k, val)
                        } else {
                            k.clone()
                        }
                    }));
                    if attrs.is_empty() {
                        first_class.clone()
                    } else {
                        format!("{} {{{}}}", first_class, attrs.join(" "))
                    }
                }
            } else {
                // No classes, just attributes
                if info.attributes.is_empty() {
                    String::new()
                } else {
                    format!("{{{}}}", format_attributes(&info.attributes, false))
                }
            }
        }
        CodeBlockType::Executable { language } => {
            // Executable chunk: extract options from CST nodes
            // Always keep as {language} with attributes
            let options = extract_chunk_options_from_cst(info_node);
            render_executable_info(language, &options)
        }
        CodeBlockType::Raw { format } => {
            // Raw block: always preserve exactly as {=format}
            // No attributes allowed per Pandoc spec
            format!("{{={}}}", format)
        }
    }
}

/// Format a code block using Quarto hashpipe style for executable chunks.
///
/// Converts simple inline options to hashpipe format with YAML syntax,
/// while keeping complex expressions in the inline position.
/// If the language's comment syntax is unknown, returns false to fall back to inline format.
fn format_code_block_hashpipe(
    code_block_node: &SyntaxNode,
    info_node: &SyntaxNode,
    content: &str,
    fence_char: char,
    fence_length: usize,
    config: &Config,
    output: &mut String,
) -> bool {
    let info = InfoString::parse(&info_node.text().to_string());
    let language = match &info.block_type {
        CodeBlockType::Executable { language } => language,
        _ => unreachable!("hashpipe only for executable chunks"),
    };

    // Classify options into simple (hashpipe) vs complex (inline)
    // Extract from CST nodes
    let Some(comment_prefix) = hashpipe::get_comment_prefix(language) else {
        return false; // Unknown language - fall back to inline format
    };
    let ((simple, complex), had_content_hashpipe) =
        hashpipe::split_options_from_cst_with_content(info_node, code_block_node);

    // Try to get hashpipe lines - returns None for unknown languages
    let hashpipe_lines = match hashpipe::format_as_hashpipe(
        language,
        &simple,
        config.line_width,
        config.wrap.as_ref(),
    ) {
        Some(lines) => lines,
        None => return false, // Unknown language - fall back to inline format
    };

    // Open fence with language and any complex options (classes/ids stay on
    // the fence verbatim; un-hashpipeable key=value pairs ride along as the
    // comma group).
    for _ in 0..fence_length {
        output.push(fence_char);
    }
    output.push_str(&render_executable_info(language, &complex));
    output.push('\n');

    // Add hashpipe options
    for line in &hashpipe_lines {
        output.push_str(line);
        output.push('\n');
    }

    // Add content, dropping already-parsed leading hashpipe header lines to avoid duplication.
    let body = if had_content_hashpipe {
        split_hashpipe_header(content, code_block_node)
            .map(|(_header, body)| body)
            .unwrap_or_else(|| content.to_string())
    } else {
        content.to_string()
    };

    if !hashpipe_lines.is_empty() {
        let body_without_leading_blanks = strip_leading_blank_lines(&body);
        let (body_without_marker_separators, had_marker_separator) =
            strip_leading_hashpipe_blank_markers(body_without_leading_blanks, comment_prefix);
        if !body_without_marker_separators.trim().is_empty()
            && (had_marker_separator || !body_without_marker_separators.starts_with(comment_prefix))
        {
            output.push('\n');
        }
        output.push_str(body_without_marker_separators);
    } else {
        output.push_str(&body);
    }

    // Close fence
    for _ in 0..fence_length {
        output.push(fence_char);
    }
    output.push('\n');

    true // Successfully formatted as hashpipe
}

fn strip_leading_blank_lines(content: &str) -> &str {
    let mut idx = 0usize;

    while idx < content.len() {
        let rest = &content[idx..];
        let Some(line_end) = rest.find('\n') else {
            if rest.trim().is_empty() {
                return "";
            }
            break;
        };

        let line = &rest[..=line_end];
        let line_without_newline = line.trim_end_matches(['\r', '\n']);
        if line_without_newline.trim().is_empty() {
            idx += line_end + 1;
            continue;
        }

        break;
    }

    &content[idx..]
}

fn strip_leading_hashpipe_blank_markers<'a>(content: &'a str, prefix: &str) -> (&'a str, bool) {
    let mut idx = 0usize;
    let mut consumed = false;

    while idx < content.len() {
        let rest = &content[idx..];
        let Some(line_end) = rest.find('\n') else {
            let trimmed = rest.trim_start_matches([' ', '\t']).trim_end_matches('\r');
            if trimmed == prefix {
                consumed = true;
                idx = content.len();
            }
            break;
        };

        let line = &rest[..line_end];
        let trimmed = line.trim_start_matches([' ', '\t']).trim_end_matches('\r');
        if trimmed == prefix {
            consumed = true;
            idx += line_end + 1;
            continue;
        }
        break;
    }

    (&content[idx..], consumed)
}

/// Format attribute key-value pairs
///
/// For executable chunks, preserve unquoted values when they're safe identifiers
/// (no spaces, no special chars). This preserves R/Julia/Python chunk semantics.
fn format_attributes(attrs: &[(String, Option<String>)], preserve_unquoted: bool) -> String {
    let separator = if preserve_unquoted {
        ", " // Executable chunks use commas
    } else {
        " " // Display blocks use spaces
    };

    attrs
        .iter()
        .map(|(k, v)| {
            if let Some(val) = v {
                if preserve_unquoted {
                    // For executable chunks, we need to preserve R syntax
                    // Add quotes if the value contains spaces or commas (needs quoting)
                    // but don't quote if it already looks like an R expression
                    let needs_quotes = (val.contains(' ') || val.contains(','))
                        && !val.contains('(')
                        && !val.contains('[')
                        && !val.contains('{');

                    if needs_quotes {
                        // Quote and escape
                        let escaped_val = val.replace('\\', "\\\\").replace('"', "\\\"");
                        format!("{}=\"{}\"", k, escaped_val)
                    } else {
                        // Keep as-is (R expression or simple identifier)
                        format!("{}={}", k, val)
                    }
                } else {
                    // For display blocks, always quote
                    // Escape internal quotes and backslashes
                    let escaped_val = val.replace('\\', "\\\\").replace('"', "\\\"");
                    format!("{}=\"{}\"", k, escaped_val)
                }
            } else {
                k.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(separator)
}

/// Collect all code blocks and their info strings from the syntax tree.
/// Collect all code blocks from the syntax tree for external formatting.
/// Returns a flat list of (language, content) pairs.
pub fn collect_code_blocks(
    tree: &SyntaxNode,
    _input: &str,
    config: &Config,
) -> Vec<ExternalCodeBlock> {
    let mut result = Vec::new();
    for node in tree.descendants() {
        if node.kind() != SyntaxKind::CODE_BLOCK {
            continue;
        }

        let (info_node, language, content) = extract_code_block_parts(&node);
        if content.is_empty() {
            continue;
        }

        let info = info_node
            .as_ref()
            .map(|n| InfoString::parse(&n.text().to_string()))
            .unwrap_or_else(|| InfoString::parse(""));

        let language = language.unwrap_or_else(|| match info.block_type {
            CodeBlockType::DisplayShortcut { language }
            | CodeBlockType::Executable { language } => language,
            CodeBlockType::DisplayExplicit { classes } => {
                classes.first().cloned().unwrap_or_default()
            }
            _ => String::new(),
        });

        if language.is_empty() && !config.formatters.contains_key("") {
            continue;
        }

        result.push(ExternalCodeBlock {
            language,
            original: content.clone(),
            formatter_input: content,
            hashpipe_prefix: None,
        });
    }

    if !matches!(config.flavor, Flavor::Quarto | Flavor::RMarkdown) {
        return result;
    }

    let mut updated = Vec::with_capacity(result.len());
    for block in result {
        let mut formatter_input = block.formatter_input.clone();
        let mut prefix = None;

        for node in tree.descendants() {
            if node.kind() != SyntaxKind::CODE_BLOCK {
                continue;
            }

            let (info_node, language, content) = extract_code_block_parts(&node);
            if content != block.original {
                continue;
            }

            let info_node = match info_node {
                Some(node) => node,
                None => break,
            };

            let info_raw = info_node.text().to_string();
            let info = InfoString::parse(&info_raw);
            let is_executable = matches!(info.block_type, CodeBlockType::Executable { .. });
            if !is_executable {
                break;
            }

            let language = language.unwrap_or_else(|| match info.block_type {
                CodeBlockType::Executable { language } => language,
                _ => String::new(),
            });

            if hashpipe::get_comment_prefix(&language).is_some()
                && let Some((header, body)) = split_hashpipe_header(&content, &node)
            {
                formatter_input = body;
                prefix = Some(header);
            }
            break;
        }

        updated.push(ExternalCodeBlock {
            language: block.language,
            original: block.original,
            formatter_input,
            hashpipe_prefix: prefix,
        });
    }

    updated
}

#[cfg(test)]
mod tests {
    use super::split_hashpipe_header;
    use crate::config::{Extensions, Flavor, ParserOptions};
    use crate::syntax::SyntaxKind;

    #[test]
    fn split_hashpipe_header_handles_empty_value_with_indented_list() {
        // Parse a real Quarto chunk so the embedded HASHPIPE_YAML preamble exists;
        // the split is driven by that node's line count.
        let input = "```{r}\n#| fig-cap:\n#|   - A\n#|   - B\n```\n";
        let options = ParserOptions {
            flavor: Flavor::Quarto,
            extensions: Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        };
        let tree = crate::parser::parse(input, Some(options));
        let code_block = tree
            .descendants()
            .find(|node| node.kind() == SyntaxKind::CODE_BLOCK)
            .expect("code block");

        let content = "#| fig-cap:\n#|   - A\n#|   - B\n";
        let split = split_hashpipe_header(content, &code_block);
        assert!(split.is_some(), "expected hashpipe header split");
        let (header, body) = split.unwrap();
        assert_eq!(header, content);
        assert_eq!(body, "");
    }
}
