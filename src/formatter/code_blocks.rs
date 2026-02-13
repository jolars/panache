use crate::config::{AttributeStyle, Config, FenceStyle, Flavor};
#[cfg(feature = "lsp")]
use crate::external_formatters::format_code_async;
use crate::parser::block_parser::code_blocks::{CodeBlockType, InfoString};
use crate::syntax::{SyntaxKind, SyntaxNode};
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
    let mut info_string_raw = String::new();
    let mut content = String::new();

    // Extract info string and content from the AST
    for child in node.children_with_tokens() {
        if let NodeOrToken::Node(n) = child {
            match n.kind() {
                SyntaxKind::CodeFenceOpen => {
                    // Find the info string
                    for token in n.children_with_tokens() {
                        if let NodeOrToken::Token(t) = token
                            && t.kind() == SyntaxKind::CodeInfo
                        {
                            info_string_raw = t.text().to_string();
                        }
                    }
                }
                SyntaxKind::CodeContent => {
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

    // Parse the info string
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
    let fence_length = determine_fence_length(
        final_content,
        fence_char,
        config.code_blocks.min_fence_length,
    );

    // Check if we should use hashpipe format for Quarto executable chunks
    let use_hashpipe = matches!(config.flavor, Flavor::Quarto | Flavor::RMarkdown)
        && matches!(&info.block_type, CodeBlockType::Executable { .. });

    if use_hashpipe {
        // Format as hashpipe with YAML-style options
        format_code_block_hashpipe(
            &info,
            final_content,
            fence_char,
            fence_length,
            config,
            output,
        );
    } else {
        // Format the info string based on config and block type (traditional inline)
        let formatted_info = format_info_string(&info, config);

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
}

/// Determine the minimum fence length needed to avoid conflicts with content
fn determine_fence_length(content: &str, fence_char: char, min_length: usize) -> usize {
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

    // Use at least one more than the longest sequence in content
    (max_sequence + 1).max(min_length)
}

/// Format the info string based on block type and config preferences
fn format_info_string(info: &InfoString, config: &Config) -> String {
    // For Preserve mode, use the raw string as-is
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
                format!(" {{{}}}", format_attributes(&info.attributes, false))
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
            // Executable chunk: preserve unquoted values (booleans/numbers/identifiers)
            // Always keep as {language} with attributes
            if info.attributes.is_empty() {
                format!("{{{}}}", language)
            } else {
                format!(
                    "{{{}, {}}}",
                    language,
                    format_attributes(&info.attributes, true)
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
fn format_code_block_hashpipe(
    info: &InfoString,
    content: &str,
    fence_char: char,
    fence_length: usize,
    config: &Config,
    output: &mut String,
) {
    let language = match &info.block_type {
        CodeBlockType::Executable { language } => language,
        _ => unreachable!("hashpipe only for executable chunks"),
    };

    // Classify options into simple (hashpipe) vs complex (inline)
    let (simple, complex) = hashpipe::split_options(&info.attributes);

    // Open fence with language and any complex options
    for _ in 0..fence_length {
        output.push(fence_char);
    }
    output.push('{');
    output.push_str(language);
    if !complex.is_empty() {
        output.push_str(", ");
        output.push_str(&format_attributes(&complex, true));
    }
    output.push('}');
    output.push('\n');

    // Add hashpipe options
    let hashpipe_lines = hashpipe::format_as_hashpipe(language, &simple, config.line_width);
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
}

/// Format attribute key-value pairs
///
/// For executable chunks, preserve unquoted values when they're safe identifiers
/// (no spaces, no special chars). This preserves R/Julia/Python chunk semantics.
fn format_attributes(attrs: &[(String, Option<String>)], preserve_unquoted: bool) -> String {
    attrs
        .iter()
        .map(|(k, v)| {
            if let Some(val) = v {
                // Check if value needs quotes
                let needs_quotes = if preserve_unquoted {
                    // For executable chunks, only quote if value contains spaces or special chars
                    val.is_empty()
                        || val
                            .chars()
                            .any(|c| c.is_whitespace() || c == '"' || c == '\\')
                } else {
                    // For display blocks, always quote
                    true
                };

                if needs_quotes {
                    format!("{}=\"{}\"", k, val)
                } else {
                    format!("{}={}", k, val)
                }
            } else {
                k.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Collect all code blocks and their info strings from the syntax tree.
pub fn collect_code_blocks(tree: &SyntaxNode) -> Vec<(String, String)> {
    let mut blocks = Vec::new();

    for node in tree.descendants() {
        if node.kind() == SyntaxKind::CodeBlock {
            let mut info_string_raw = String::new();
            let mut content = String::new();

            for child in node.children_with_tokens() {
                if let NodeOrToken::Node(n) = child {
                    match n.kind() {
                        SyntaxKind::CodeFenceOpen => {
                            for token in n.children_with_tokens() {
                                if let NodeOrToken::Token(t) = token
                                    && t.kind() == SyntaxKind::CodeInfo
                                {
                                    info_string_raw = t.text().to_string();
                                }
                            }
                        }
                        SyntaxKind::CodeContent => {
                            content = n.text().to_string();
                        }
                        _ => {}
                    }
                }
            }

            if !content.is_empty() {
                // Parse info string to check if it's a raw block
                let info = InfoString::parse(&info_string_raw);

                // Skip raw blocks - they should never be formatted
                if matches!(info.block_type, CodeBlockType::Raw { .. }) {
                    continue;
                }

                // Extract language from the parsed info string for matching with formatters
                let lang_key = match &info.block_type {
                    CodeBlockType::Executable { language } => language.to_lowercase(),
                    CodeBlockType::DisplayShortcut { language } => language.to_lowercase(),
                    CodeBlockType::DisplayExplicit { classes } => {
                        // Use first class as language (e.g., {.python})
                        classes
                            .first()
                            .map(|c| c.to_lowercase())
                            .unwrap_or_default()
                    }
                    CodeBlockType::Plain => String::new(),
                    CodeBlockType::Raw { .. } => unreachable!(), // Already filtered above
                };

                if !lang_key.is_empty() {
                    blocks.push((lang_key, content));
                }
            }
        }
    }

    blocks
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

    // Spawn all formatter tasks immediately
    for (lang, code) in blocks {
        if let Some(formatter_cfg) = config.formatters.get(&lang)
            && formatter_cfg.enabled
            && !formatter_cfg.cmd.is_empty()
        {
            let formatter_cfg = formatter_cfg.clone();
            let code = code.clone();
            let lang = lang.clone();

            let task = tokio::spawn(async move {
                log::info!("Formatting {} code with {}", lang, formatter_cfg.cmd);
                let result = format_code_async(&code, &formatter_cfg, timeout).await;
                (lang, code, result)
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
