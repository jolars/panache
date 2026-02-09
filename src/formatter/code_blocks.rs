use crate::config::Config;
use crate::external_formatters::format_code_async;
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;
use std::collections::HashMap;
use std::time::Duration;

/// Format a code block, normalizing fence markers to backticks
pub(super) fn format_code_block(
    node: &SyntaxNode,
    formatted_code: &HashMap<String, String>,
    output: &mut String,
) {
    let mut info_string = String::new();
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
                            info_string = t.text().to_string();
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

    // Check if we have formatted version from external formatter
    let final_content = formatted_code.get(&content).unwrap_or(&content);

    // Output normalized code block with exactly 3 backticks
    output.push_str("```");
    if !info_string.is_empty() {
        output.push_str(&info_string);
    }
    output.push('\n');
    output.push_str(final_content);
    output.push_str("```");
    output.push('\n');
}

/// Collect all code blocks and their info strings from the syntax tree.
pub fn collect_code_blocks(tree: &SyntaxNode) -> Vec<(String, String)> {
    let mut blocks = Vec::new();

    for node in tree.descendants() {
        if node.kind() == SyntaxKind::CodeBlock {
            let mut info_string = String::new();
            let mut content = String::new();

            for child in node.children_with_tokens() {
                if let NodeOrToken::Node(n) = child {
                    match n.kind() {
                        SyntaxKind::CodeFenceOpen => {
                            for token in n.children_with_tokens() {
                                if let NodeOrToken::Token(t) = token
                                    && t.kind() == SyntaxKind::CodeInfo
                                {
                                    info_string = t.text().to_string().trim().to_lowercase();
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
                blocks.push((info_string, content));
            }
        }
    }

    blocks
}

/// Spawn external formatters for code blocks and await results.
/// Returns a HashMap of original code -> formatted code (only successful formats).
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
