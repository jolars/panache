use crate::config::Config;
use crate::syntax::SyntaxNode;

mod blockquotes;
pub mod code_blocks;
mod core;
mod fenced_divs;
mod hashpipe;
mod headings;
mod indent_utils;
mod inline;
mod lists;
mod math_delimiters;
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

    // Step 1: Run external formatters and apply inline by block identity.
    let formatted_code = if !config.formatters.is_empty() {
        let code_blocks = code_blocks::collect_code_blocks(tree, &input, config);
        if !code_blocks.is_empty() {
            log::debug!(
                "Found {} code blocks, spawning formatters...",
                code_blocks.len()
            );
            code_blocks::spawn_and_await_formatters(code_blocks, config).await
        } else {
            code_blocks::FormattedCodeMap::new()
        }
    } else {
        code_blocks::FormattedCodeMap::new()
    };

    // Step 1b: Format YAML frontmatter with built-in YAML engine
    let yaml_config = config.clone();
    let formatted_yaml_future = metadata::collect_yaml_metadata(tree).map(|yaml_content| {
        tokio::spawn(async move {
            crate::yaml_engine::format_yaml_with_config(&yaml_content, &yaml_config)
        })
    });

    // Step 2: Format markdown with external code substitutions applied inline.
    let mut output = Formatter::new(config.clone(), formatted_code, range).format(tree);

    // Step 3: Await YAML formatter result and apply if available
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

    // Ensure exactly one trailing newline
    output.trim_end().to_string() + "\n"
}

pub fn format_tree(tree: &SyntaxNode, config: &Config, range: Option<(usize, usize)>) -> String {
    log::debug!(
        "Formatting document with config: line_width={}, wrap={:?}",
        config.line_width,
        config.wrap
    );

    let input = tree.text().to_string();

    // Step 1: Run external formatters synchronously if configured
    let formatted_code = if !config.formatters.is_empty() {
        let code_blocks = code_blocks::collect_code_blocks(tree, &input, config);
        if !code_blocks.is_empty() {
            log::debug!(
                "Found {} code blocks, spawning formatters...",
                code_blocks.len()
            );
            code_blocks::spawn_and_await_formatters_sync(code_blocks, config)
        } else {
            code_blocks::FormattedCodeMap::new()
        }
    } else {
        code_blocks::FormattedCodeMap::new()
    };

    // Step 1b: Run YAML frontmatter formatter synchronously with built-in YAML engine
    #[cfg(not(target_arch = "wasm32"))]
    let formatted_yaml = if let Some(yaml_content) = metadata::collect_yaml_metadata(tree) {
        match crate::yaml_engine::format_yaml_with_config(&yaml_content, config) {
            Ok(formatted) if formatted != yaml_content => Some((yaml_content, formatted)),
            _ => None,
        }
    } else {
        None
    };

    #[cfg(target_arch = "wasm32")]
    let formatted_yaml: Option<(String, String)> = None;

    // Step 2: Format markdown, applying externally formatted code blocks inline
    let mut output = Formatter::new(config.clone(), formatted_code, range).format(tree);

    // Step 3: Apply formatted YAML if available
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

    log::debug!("Formatting complete: {} bytes output", output.len());

    // Ensure exactly one trailing newline
    output.trim_end().to_string() + "\n"
}
