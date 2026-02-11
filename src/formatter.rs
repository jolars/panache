use crate::config::Config;
use crate::syntax::SyntaxNode;
use std::collections::HashMap;

mod blockquotes;
mod code_blocks;
mod core;
mod fenced_divs;
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

    // Step 1: Spawn all external formatters immediately (run in background)
    let formatted_code_future = if !config.formatters.is_empty() {
        let code_blocks = code_blocks::collect_code_blocks(tree);
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

    log::info!("Formatting complete: {} bytes output", output.len());
    output
}

pub fn format_tree(tree: &SyntaxNode, config: &Config, range: Option<(usize, usize)>) -> String {
    log::info!(
        "Formatting document with config: line_width={}, wrap={:?}",
        config.line_width,
        config.wrap
    );

    // Step 1: Run external formatters synchronously if configured
    let formatted_code = if !config.formatters.is_empty() {
        let code_blocks = code_blocks::collect_code_blocks(tree);
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

    // Step 2: Format markdown with formatted code blocks
    let mut output = Formatter::new(config.clone(), formatted_code.clone(), range).format(tree);

    // Step 3: Apply formatted code blocks if any
    if !formatted_code.is_empty() {
        log::debug!("Applying {} formatted code blocks", formatted_code.len());
        for (original, formatted) in &formatted_code {
            output = output.replace(original, formatted);
        }
    }

    log::info!("Formatting complete: {} bytes output", output.len());
    output
}
