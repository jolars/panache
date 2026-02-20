use crate::config::{AttributeStyle, Config, FenceStyle, Flavor};
#[cfg(feature = "lsp")]
use crate::external_formatters::format_code_async;
use crate::parser::block_parser::code_blocks::{CodeBlockType, InfoString};
use crate::syntax::{AstNode, SyntaxKind, SyntaxNode};
use crate::utils;
use rowan::NodeOrToken;
use std::collections::HashMap;
#[cfg(feature = "lsp")]
use std::time::Duration;

use super::hashpipe;

/// Format a code block, normalizing fence markers and attributes based on config
pub(super) fn format_code_block(
    node: &SyntaxNode,
    config: &Config,
    formatted_code: &HashMap<String, String>,
    output: &mut String,
) {
    let mut info_node: Option<SyntaxNode> = None;
    let mut content = String::new();

    // Extract info node and content from the AST
    for child in node.children_with_tokens() {
        if let NodeOrToken::Node(n) = child {
            match n.kind() {
                SyntaxKind::CODE_FENCE_OPEN => {
                    // Find the info string - now it's a node, not a token
                    for child_token in n.children_with_tokens() {
                        if let NodeOrToken::Node(node) = child_token
                            && node.kind() == SyntaxKind::CODE_INFO
                        {
                            info_node = Some(node);
                        }
                    }
                }
                SyntaxKind::CODE_CONTENT => {
                    // Extract content, stripping leading WHITESPACE tokens on each line
                    // (for lossless parsing, indented code blocks preserve indentation as WHITESPACE)
                    for token in n.children_with_tokens() {
                        if let NodeOrToken::Token(t) = token {
                            match t.kind() {
                                SyntaxKind::WHITESPACE => {
                                    // Skip leading whitespace tokens (indentation)
                                    // They're preserved in the AST for losslessness, but
                                    // formatter strips them when converting to fenced code
                                }
                                SyntaxKind::TEXT | SyntaxKind::NEWLINE => {
                                    content.push_str(t.text());
                                }
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    let info_node = match info_node {
        Some(node) => node,
        None => {
            // No info string, just output basic fence
            let fence_char = match config.code_blocks.fence_style {
                FenceStyle::Backtick => '`',
                FenceStyle::Tilde => '~',
                FenceStyle::Preserve => '`',
            };
            let fence_length = determine_fence_length(&content, fence_char);
            output.push_str(&fence_char.to_string().repeat(fence_length));
            output.push('\n');
            output.push_str(&content);
            output.push_str(&fence_char.to_string().repeat(fence_length));
            output.push('\n');
            return;
        }
    };

    // Parse the info string to get block type
    let info_string_raw = info_node.text().to_string();
    let info = InfoString::parse(&info_string_raw);

    // Check if we have formatted version from external formatter
    let final_content = formatted_code.get(&content).unwrap_or(&content);

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
    let fence_length = determine_fence_length(final_content, fence_char);

    // Check if we should use hashpipe format for Quarto executable chunks
    let use_hashpipe = matches!(config.flavor, Flavor::Quarto | Flavor::RMarkdown)
        && matches!(&info.block_type, CodeBlockType::Executable { .. });

    if use_hashpipe {
        // Try to format as hashpipe with YAML-style options
        // Falls back to inline format if language comment syntax is unknown
        if format_code_block_hashpipe(
            &info_node,
            &info,
            final_content,
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
    output.push_str(final_content);
    for _ in 0..fence_length {
        output.push(fence_char);
    }
    output.push('\n');
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

    // Find CHUNK_OPTIONS node
    for child in info_node.children() {
        if child.kind() == SyntaxKind::CHUNK_OPTIONS {
            // Iterate through options and labels
            for opt_or_label in child.children() {
                if let Some(label) = ChunkLabel::cast(opt_or_label.clone()) {
                    // Label (no key, just value)
                    options.push((None, Some(label.text()), false));
                } else if let Some(opt) = ChunkOption::cast(opt_or_label) {
                    // Regular option with key=value
                    if let (Some(key), Some(value)) = (opt.key(), opt.value()) {
                        options.push((Some(key), Some(value), opt.is_quoted()));
                    }
                }
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
pub fn collect_code_blocks(tree: &SyntaxNode, input: &str) -> Vec<(String, String)> {
    let blocks_by_language = utils::collect_code_blocks(tree, input);

    let mut result = Vec::new();
    for (_language, blocks) in blocks_by_language {
        for block in blocks {
            result.push((block.language, block.content));
        }
    }

    result
}

/// Spawn external formatters for code blocks and await results.
/// Returns a HashMap of original code -> formatted code (only successful formats).
#[cfg(feature = "lsp")]
pub async fn spawn_and_await_formatters(
    blocks: Vec<(String, String)>,
    config: &Config,
) -> HashMap<String, String> {
    let mut tasks = Vec::new();
    let timeout = Duration::from_secs(30);

    // Spawn all formatter tasks immediately (one task per language)
    for (lang, code) in blocks {
        if let Some(formatter_configs) = config.formatters.get(&lang) {
            if formatter_configs.is_empty() {
                continue; // Empty formatter list means no formatting
            }

            let formatter_configs = formatter_configs.clone();
            let code = code.clone();
            let lang = lang.clone();

            let task = tokio::spawn(async move {
                // Format sequentially through the formatter chain
                let mut current_code = code.clone();

                for (idx, formatter_cfg) in formatter_configs.iter().enumerate() {
                    if formatter_cfg.cmd.is_empty() {
                        continue;
                    }

                    log::info!(
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
                            return (lang, code, Err(e));
                        }
                    }
                }

                (lang, code, Ok(current_code))
            });

            tasks.push(task);
        }
    }

    let mut formatted = HashMap::new();

    // Await all results
    for task in tasks {
        if let Ok((lang, original_code, result)) = task.await {
            match result {
                Ok(formatted_code) => {
                    log::debug!(
                        "Successfully formatted {} code: {} bytes -> {} bytes",
                        lang,
                        original_code.len(),
                        formatted_code.len()
                    );
                    // Only store if content changed
                    if formatted_code != original_code {
                        formatted.insert(original_code, formatted_code);
                    }
                }
                Err(e) => {
                    log::warn!("Failed to format {} code: {}", lang, e);
                    // Original code will be used (not in map)
                }
            }
        }
    }

    formatted
}

/// Run external formatters for code blocks synchronously using threads.
/// Returns a HashMap of original code -> formatted code (only successful formats).
#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_and_await_formatters_sync(
    blocks: Vec<(String, String)>,
    config: &Config,
) -> HashMap<String, String> {
    use std::time::Duration;
    let timeout = Duration::from_secs(30);

    crate::external_formatters_sync::run_formatters_parallel(blocks, &config.formatters, timeout)
}

/// WASM version that returns empty HashMap (no external formatters in WASM)
#[cfg(target_arch = "wasm32")]
pub fn spawn_and_await_formatters_sync(
    _blocks: Vec<(String, String)>,
    _config: &Config,
) -> HashMap<String, String> {
    HashMap::new()
}
