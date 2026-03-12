use crate::config::{AttributeStyle, Config, FenceStyle, Flavor};
#[cfg(feature = "lsp")]
use crate::external_formatters::format_code_async;
use crate::parser::blocks::code_blocks::{CodeBlockType, InfoString};
use crate::syntax::{AstNode, SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;
use std::collections::HashMap;
#[cfg(feature = "lsp")]
use std::time::Duration;

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
            let fence_char = match config.code_blocks.fence_style {
                FenceStyle::Backtick => '`',
                FenceStyle::Tilde => '~',
                FenceStyle::Preserve => '`',
            };
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
    let fence_char = match config.code_blocks.fence_style {
        FenceStyle::Backtick => '`',
        FenceStyle::Tilde => '~',
        FenceStyle::Preserve => {
            // Try to detect original fence char from context
            // For now, default to backtick
            '`'
        }
    };

    // Determine fence length (check for nested fences in content)
    let fence_length = determine_fence_length(&final_content, fence_char);

    // Check if we should use hashpipe format for Quarto executable chunks
    let use_hashpipe = matches!(config.flavor, Flavor::Quarto | Flavor::RMarkdown)
        && matches!(&info.block_type, CodeBlockType::Executable { .. });

    if use_hashpipe {
        // Try to format as hashpipe with YAML-style options
        // Falls back to inline format if language comment syntax is unknown
        if format_code_block_hashpipe(
            &info_node,
            &info,
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
    let formatted_info = format_info_string(&info_node, &info, config);

    log::debug!("formatted_info = '{}'", formatted_info);

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
                        if let NodeOrToken::Token(t) = token {
                            match t.kind() {
                                SyntaxKind::BLOCKQUOTE_MARKER if at_line_start => {
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

fn split_hashpipe_header(content: &str, prefix: &str) -> Option<(String, String)> {
    let mut header_end = 0usize;
    let mut saw_prefix = false;

    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with(prefix) {
            saw_prefix = true;
            header_end += line.len();
            continue;
        }
        break;
    }

    if saw_prefix {
        Some((
            content[..header_end].to_string(),
            content[header_end..].to_string(),
        ))
    } else {
        None
    }
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

/// Extract chunk options from CST CHUNK_OPTIONS node.
/// Returns (label, options) where label is the first unlabeled option if any.
fn extract_chunk_options_from_cst(
    info_node: &SyntaxNode,
) -> Vec<(Option<String>, Option<String>, bool)> {
    use crate::syntax::{ChunkLabel, ChunkOption};

    let mut options = Vec::new();
    let mut pending_label_parts = Vec::new();

    // Find CHUNK_OPTIONS node
    for child in info_node.children() {
        if child.kind() == SyntaxKind::CHUNK_OPTIONS {
            // Iterate through options and labels
            for opt_or_label in child.children() {
                if let Some(label) = ChunkLabel::cast(opt_or_label.clone()) {
                    pending_label_parts.push(label.text());
                } else if let Some(opt) = ChunkOption::cast(opt_or_label) {
                    if !pending_label_parts.is_empty() {
                        options.push((None, Some(pending_label_parts.join(" ")), false));
                        pending_label_parts.clear();
                    }
                    // Regular option with key=value
                    if let (Some(key), Some(value)) = (opt.key(), opt.value()) {
                        options.push((Some(key), Some(value), opt.is_quoted()));
                    }
                }
            }
            if !pending_label_parts.is_empty() {
                options.push((None, Some(pending_label_parts.join(" ")), false));
            }
            break;
        }
    }

    options
}

/// Format chunk options for inline display: label, key=value, key="quoted value"
fn format_chunk_options_inline(options: &[(Option<String>, Option<String>, bool)]) -> String {
    let mut parts = Vec::new();

    for (key, value, is_quoted) in options {
        match (key, value) {
            (None, Some(val)) => {
                // Label
                parts.push(val.clone());
            }
            (Some(k), Some(v)) => {
                // Key=value
                if *is_quoted {
                    // Re-add quotes
                    parts.push(format!("{}=\"{}\"", k, v));
                } else {
                    parts.push(format!("{}={}", k, v));
                }
            }
            _ => {}
        }
    }

    parts.join(", ")
}

/// Format the info string based on block type and config preferences
fn format_info_string(info_node: &SyntaxNode, info: &InfoString, config: &Config) -> String {
    log::debug!(
        "format_info_string: block_type={:?}, attribute_style={:?}, raw='{}'",
        info.block_type,
        config.code_blocks.attribute_style,
        info.raw
    );
    if config.code_blocks.attribute_style == AttributeStyle::Preserve {
        return if info.raw.is_empty() {
            String::new()
        } else {
            info.raw.clone()
        };
    }

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
            match config.code_blocks.attribute_style {
                AttributeStyle::Shortcut => {
                    // Keep shortcut form
                    if info.attributes.is_empty() {
                        language.clone()
                    } else {
                        format!(
                            "{} {{{}}}",
                            language,
                            format_attributes(&info.attributes, false)
                        )
                    }
                }
                AttributeStyle::Explicit => {
                    // Convert to explicit form: ```python -> ```{.python}
                    let mut attrs = vec![format!(".{}", language)];
                    attrs.extend(info.attributes.iter().map(|(k, v)| {
                        if let Some(val) = v {
                            format!("{}=\"{}\"", k, val)
                        } else {
                            k.clone()
                        }
                    }));
                    format!("{{{}}}", attrs.join(" "))
                }
                AttributeStyle::Preserve => unreachable!(), // Handled above
            }
        }
        CodeBlockType::DisplayExplicit { classes } => {
            // Display block with explicit Pandoc syntax
            match config.code_blocks.attribute_style {
                AttributeStyle::Explicit => {
                    // Keep explicit form - reconstruct from classes + attributes preserving order
                    // This is tricky - we've lost original order by splitting. Use raw for preserve.
                    let mut attrs: Vec<String> =
                        classes.iter().map(|c| format!(".{}", c)).collect();
                    attrs.extend(info.attributes.iter().map(|(k, v)| {
                        if let Some(val) = v {
                            format!("{}=\"{}\"", k, val)
                        } else {
                            k.clone()
                        }
                    }));
                    format!("{{{}}}", attrs.join(" "))
                }
                AttributeStyle::Shortcut => {
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
                AttributeStyle::Preserve => unreachable!(), // Handled above
            }
        }
        CodeBlockType::Executable { language } => {
            // Executable chunk: extract options from CST nodes
            // Always keep as {language} with attributes
            let options = extract_chunk_options_from_cst(info_node);
            if options.is_empty() {
                format!("{{{}}}", language)
            } else {
                format!(
                    "{{{}, {}}}",
                    language,
                    format_chunk_options_inline(&options)
                )
            }
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
    info_node: &SyntaxNode,
    info: &InfoString,
    content: &str,
    fence_char: char,
    fence_length: usize,
    config: &Config,
    output: &mut String,
) -> bool {
    let language = match &info.block_type {
        CodeBlockType::Executable { language } => language,
        _ => unreachable!("hashpipe only for executable chunks"),
    };

    // Classify options into simple (hashpipe) vs complex (inline)
    // Extract from CST nodes
    let (simple, complex) = hashpipe::split_options_from_cst(info_node);

    // Try to get hashpipe lines - returns None for unknown languages
    let hashpipe_lines = match hashpipe::format_as_hashpipe(language, &simple, config.line_width) {
        Some(lines) => lines,
        None => return false, // Unknown language - fall back to inline format
    };

    // Open fence with language and any complex options
    for _ in 0..fence_length {
        output.push(fence_char);
    }
    output.push('{');
    output.push_str(language);
    if !complex.is_empty() {
        output.push_str(", ");
        output.push_str(&format_chunk_options_inline(&complex));
    }
    output.push('}');
    output.push('\n');

    // Add hashpipe options
    for line in hashpipe_lines {
        output.push_str(&line);
        output.push('\n');
    }

    // Add content
    output.push_str(content);

    // Close fence
    for _ in 0..fence_length {
        output.push(fence_char);
    }
    output.push('\n');

    true // Successfully formatted as hashpipe
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

        if language.is_empty() {
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

            if let Some(prefix_str) = hashpipe::get_comment_prefix(&language)
                && let Some((header, body)) = split_hashpipe_header(&content, prefix_str)
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

/// Spawn external formatters for code blocks and await results.
/// Returns a map of (language, original code) -> formatted code (only successful formats).
#[cfg(feature = "lsp")]
pub async fn spawn_and_await_formatters(
    blocks: Vec<ExternalCodeBlock>,
    config: &Config,
) -> FormattedCodeMap {
    use std::sync::Arc;
    use tokio::sync::Semaphore;
    use tokio::task::JoinSet;

    let timeout = Duration::from_secs(30);
    let semaphore = Arc::new(Semaphore::new(config.external_max_parallel.max(1)));

    let mut join_set = JoinSet::new();

    // Spawn formatter tasks with bounded concurrency (one task per code block).
    for block in blocks {
        let lang = block.language.clone();
        let Some(formatter_configs) = config.formatters.get(&lang) else {
            continue;
        };
        if formatter_configs.is_empty() {
            continue; // Empty formatter list means no formatting
        }

        let permit = semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore closed");

        let formatter_configs = formatter_configs.clone();
        let code = block.formatter_input.clone();
        let original = block.original.clone();
        let hashpipe_prefix = block.hashpipe_prefix.clone();

        join_set.spawn(async move {
            let _permit = permit;

            // Format sequentially through the formatter chain
            let mut current_code = code;

            for (idx, formatter_cfg) in formatter_configs.iter().enumerate() {
                if formatter_cfg.cmd.is_empty() {
                    continue;
                }

                log::debug!(
                    "Formatting {} code with {} ({}/{} in chain)",
                    lang,
                    formatter_cfg.cmd,
                    idx + 1,
                    formatter_configs.len()
                );

                match format_code_async(&current_code, formatter_cfg, timeout).await {
                    Ok(formatted) => {
                        current_code = formatted;
                    }
                    Err(e) => {
                        eprintln!(
                            "Warning: {} formatter '{}' failed: {}. Using original code.",
                            lang, formatter_cfg.cmd, e
                        );
                        // Stop the chain on error and return original
                        return (lang, original, hashpipe_prefix, Err(e));
                    }
                }
            }

            (lang, original, hashpipe_prefix, Ok(current_code))
        });
    }

    let mut formatted: FormattedCodeMap = HashMap::new();

    while let Some(res) = join_set.join_next().await {
        if let Ok((lang, original_code, hashpipe_prefix, result)) = res {
            match result {
                Ok(formatted_code) => {
                    // Only store if content changed
                    if formatted_code != original_code {
                        let combined = if let Some(prefix) = hashpipe_prefix {
                            format!("{}{}", prefix, formatted_code)
                        } else {
                            formatted_code
                        };
                        formatted.insert((lang, original_code), combined);
                    }
                }
                Err(e) => {
                    log::warn!("Failed to format code: {}", e);
                    // Original code will be used (not in map)
                }
            }
        }
    }

    formatted
}

/// Run external formatters for code blocks synchronously using threads.
/// Returns a map of (language, original code) -> formatted code (only successful formats).
#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_and_await_formatters_sync(
    blocks: Vec<ExternalCodeBlock>,
    config: &Config,
) -> FormattedCodeMap {
    use std::time::Duration;
    let timeout = Duration::from_secs(30);

    crate::external_formatters_sync::run_formatters_parallel(
        blocks,
        &config.formatters,
        timeout,
        config.external_max_parallel,
    )
}

/// WASM version that returns empty HashMap (no external formatters in WASM)
#[cfg(target_arch = "wasm32")]
pub fn spawn_and_await_formatters_sync(
    _blocks: Vec<ExternalCodeBlock>,
    _config: &Config,
) -> FormattedCodeMap {
    HashMap::new()
}
