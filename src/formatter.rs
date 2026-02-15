use crate::config::Config;
use crate::syntax::SyntaxNode;
use std::collections::HashMap;

mod blockquotes;
mod code_blocks;
mod core;
mod fenced_divs;
mod hashpipe;
mod headings;
mod inline;
mod lists;
mod metadata;
mod paragraphs;
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
    let formatted_yaml_future = if let Some(yaml_config) = config.formatters.get("yaml")
        && yaml_config.enabled
        && !yaml_config.cmd.is_empty()
    {
        if let Some(yaml_content) = metadata::collect_yaml_metadata(tree) {
            log::debug!("Found YAML metadata, spawning formatter...");
            let yaml_config = yaml_config.clone();
            Some(tokio::spawn(async move {
                use crate::external_formatters::format_code_async;
                use std::time::Duration;
                let timeout = Duration::from_secs(30);
                log::info!("Formatting YAML metadata with {}", yaml_config.cmd);
                format_code_async(&yaml_content, &yaml_config, timeout).await
            }))
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
            output = output.replace(&original_yaml, &formatted_yaml);
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
    let formatted_yaml = if let Some(yaml_config) = config.formatters.get("yaml")
        && yaml_config.enabled
        && !yaml_config.cmd.is_empty()
    {
        if let Some(yaml_content) = metadata::collect_yaml_metadata(tree) {
            log::debug!("Found YAML metadata, spawning formatter...");
            use crate::external_formatters_sync::format_code_sync;
            use std::time::Duration;
            let timeout = Duration::from_secs(30);
            log::info!("Formatting YAML metadata with {}", yaml_config.cmd);
            match format_code_sync(&yaml_content, yaml_config, timeout) {
                Ok(formatted) => Some((yaml_content, formatted)),
                Err(e) => {
                    log::warn!("Failed to format YAML metadata: {}", e);
                    None
                }
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
        output = output.replace(&original_yaml, &formatted_yaml);
    }

    log::info!("Formatting complete: {} bytes output", output.len());
    output
}
