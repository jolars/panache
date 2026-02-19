use crate::config::Config;
use crate::syntax::SyntaxNode;
use std::collections::HashMap;

mod blockquotes;
mod code_blocks;
mod core;
mod fenced_divs;
mod hashpipe;
mod headings;
mod indent_utils;
mod inline;
mod lists;
mod metadata;
mod paragraphs;
mod shortcodes;
mod tables;
mod utils;
mod wrapping;

// Re-export the main types
pub use core::Formatter;

// Public API functions
#[cfg(feature = "lsp")]
pub async fn format_tree_async(
    tree: &SyntaxNode,
    config: &Config,
    range: Option<(usize, usize)>,
) -> String {
    log::info!(
        "Formatting document with config: line_width={}, wrap={:?}",
        config.line_width,
        config.wrap
    );

    let input = tree.text().to_string();

    // Step 1: Spawn external formatters immediately (run in background)
    let formatted_code_future = if !config.formatters.is_empty() {
        let code_blocks = code_blocks::collect_code_blocks(tree, &input);
        if !code_blocks.is_empty() {
            log::debug!(
                "Found {} code blocks, spawning formatters...",
                code_blocks.len()
            );
            let config_clone = config.clone();
            Some(tokio::spawn(async move {
                code_blocks::spawn_and_await_formatters(code_blocks, &config_clone).await
            }))
        } else {
            None
        }
    } else {
        None
    };

    // Step 1b: Spawn YAML frontmatter formatter if configured (uses same config as yaml code blocks)
    let formatted_yaml_future = if let Some(yaml_configs) = config.formatters.get("yaml") {
        if !yaml_configs.is_empty() {
            if let Some(yaml_content) = metadata::collect_yaml_metadata(tree) {
                log::debug!("Found YAML metadata, spawning formatter...");
                let yaml_configs = yaml_configs.clone();
                Some(tokio::spawn(async move {
                    use crate::external_formatters::format_code_async;
                    use std::time::Duration;
                    let timeout = Duration::from_secs(30);

                    // Format sequentially through YAML formatter chain
                    let mut current_yaml = yaml_content.clone();

                    for (idx, yaml_config) in yaml_configs.iter().enumerate() {
                        if yaml_config.cmd.is_empty() {
                            continue;
                        }

                        log::info!(
                            "Formatting YAML metadata with {} ({}/{} in chain)",
                            yaml_config.cmd,
                            idx + 1,
                            yaml_configs.len()
                        );

                        match format_code_async(&current_yaml, yaml_config, timeout).await {
                            Ok(formatted) => {
                                current_yaml = formatted;
                            }
                            Err(e) => {
                                eprintln!(
                                    "Warning: YAML formatter '{}' failed: {}. Using original content.",
                                    yaml_config.cmd, e
                                );
                                // Stop chain on error
                                return Err(e);
                            }
                        }
                    }

                    Ok(current_yaml)
                }))
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // Step 2: Format markdown (runs while formatters are working in background)
    let formatter_with_empty_map = Formatter::new(config.clone(), HashMap::new(), range);
    let mut output = formatter_with_empty_map.format(tree);

    // Step 3: Await formatter results and apply if available
    if let Some(handle) = formatted_code_future
        && let Ok(formatted_code) = handle.await
        && !formatted_code.is_empty()
    {
        log::debug!("Applying {} formatted code blocks", formatted_code.len());
        // Replace code in the output
        for (original, formatted) in &formatted_code {
            output = output.replace(original, formatted);
        }
    }

    // Step 4: Await YAML formatter result and apply if available
    if let Some(handle) = formatted_yaml_future
        && let Ok(Ok(formatted_yaml)) = handle.await
    {
        // Collect original YAML to find and replace
        if let Some(original_yaml) = metadata::collect_yaml_metadata(tree) {
            log::debug!(
                "Applying formatted YAML: {} bytes -> {} bytes",
                original_yaml.len(),
                formatted_yaml.len()
            );
            // Wrap formatted YAML with newline to preserve frontmatter structure
            // collect_yaml_metadata strips the leading newline after ---, so add it back
            let wrapped_formatted = format!("\n{}\n", formatted_yaml.trim_end());
            // Look for the pattern: newline + original_yaml + newline
            // and replace with: newline + formatted_yaml + newline
            output = output.replace(&format!("\n{}\n", original_yaml), &wrapped_formatted);
        }
    }

    log::info!("Formatting complete: {} bytes output", output.len());
    output
}

pub fn format_tree(tree: &SyntaxNode, config: &Config, range: Option<(usize, usize)>) -> String {
    log::info!(
        "Formatting document with config: line_width={}, wrap={:?}",
        config.line_width,
        config.wrap
    );

    let input = tree.text().to_string();

    // Step 1: Run external formatters synchronously if configured
    let formatted_code = if !config.formatters.is_empty() {
        let code_blocks = code_blocks::collect_code_blocks(tree, &input);
        if !code_blocks.is_empty() {
            log::debug!(
                "Found {} code blocks, spawning formatters...",
                code_blocks.len()
            );
            code_blocks::spawn_and_await_formatters_sync(code_blocks, config)
        } else {
            HashMap::new()
        }
    } else {
        HashMap::new()
    };

    // Step 1b: Run YAML frontmatter formatter synchronously if configured (uses same config as yaml code blocks)
    let formatted_yaml = if let Some(yaml_configs) = config.formatters.get("yaml") {
        if !yaml_configs.is_empty() {
            if let Some(yaml_content) = metadata::collect_yaml_metadata(tree) {
                log::debug!("Found YAML metadata, running formatter chain...");
                use crate::external_formatters_sync::format_code_sync;
                use std::time::Duration;
                let timeout = Duration::from_secs(30);

                // Format sequentially through YAML formatter chain
                let mut current_yaml = yaml_content.clone();
                let mut success = true;

                for (idx, yaml_config) in yaml_configs.iter().enumerate() {
                    if yaml_config.cmd.is_empty() {
                        continue;
                    }

                    log::info!(
                        "Formatting YAML metadata with {} ({}/{} in chain)",
                        yaml_config.cmd,
                        idx + 1,
                        yaml_configs.len()
                    );

                    match format_code_sync(&current_yaml, yaml_config, timeout) {
                        Ok(formatted) => {
                            current_yaml = formatted;
                        }
                        Err(e) => {
                            log::warn!(
                                "YAML formatter '{}' failed: {}. Using original content.",
                                yaml_config.cmd,
                                e
                            );
                            success = false;
                            break;
                        }
                    }
                }

                if success && current_yaml != yaml_content {
                    Some((yaml_content, current_yaml))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // Step 2: Format markdown with formatted code blocks
    let mut output = Formatter::new(config.clone(), formatted_code.clone(), range).format(tree);

    // Step 3: Apply formatted code blocks if any
    if !formatted_code.is_empty() {
        log::debug!("Applying {} formatted code blocks", formatted_code.len());
        for (original, formatted) in &formatted_code {
            output = output.replace(original, formatted);
        }
    }

    // Step 4: Apply formatted YAML if available
    if let Some((original_yaml, formatted_yaml)) = formatted_yaml {
        log::debug!(
            "Applying formatted YAML: {} bytes -> {} bytes",
            original_yaml.len(),
            formatted_yaml.len()
        );
        // Wrap formatted YAML with newline to preserve frontmatter structure
        // collect_yaml_metadata strips the leading newline after ---, so add it back
        let wrapped_formatted = format!("\n{}\n", formatted_yaml.trim_end());
        // Look for the pattern: newline + original_yaml + newline
        // and replace with: newline + formatted_yaml + newline
        output = output.replace(&format!("\n{}\n", original_yaml), &wrapped_formatted);
    }

    log::info!("Formatting complete: {} bytes output", output.len());
    output
}
