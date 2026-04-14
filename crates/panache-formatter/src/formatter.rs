use crate::config::Config;
use crate::syntax::{SyntaxNode, YamlFrontmatterRegion};

mod blockquotes;
pub mod code_blocks;
mod core;
mod fenced_divs;
mod hashpipe;
mod headings;
mod indent_utils;
mod inline;
mod inline_layout;
mod lists;
mod metadata;
mod paragraphs;
mod sentence_wrap;
mod shortcodes;
mod tables;
mod utils;

// Re-export the main types
pub use core::Formatter;

// Public API functions
pub fn format_tree(tree: &SyntaxNode, config: &Config, range: Option<(usize, usize)>) -> String {
    log::debug!(
        "Formatting document with config: line_width={}, wrap={:?}",
        config.line_width,
        config.wrap
    );

    let frontmatter_region = metadata::collect_yaml_frontmatter_region(tree);
    #[cfg(not(target_arch = "wasm32"))]
    let frontmatter_yaml = frontmatter_region
        .as_ref()
        .map(|region| region.content.trim_end().to_string());

    let formatted_code = code_blocks::FormattedCodeMap::new();

    // Step 1: Run YAML frontmatter formatter synchronously with built-in YAML engine
    #[cfg(not(target_arch = "wasm32"))]
    let formatted_yaml = if let Some(yaml_content) = frontmatter_yaml.clone() {
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
        if let Some(region) = frontmatter_region.as_ref()
            && let Some(replaced) = apply_formatted_yaml_at_range(
                &output,
                region,
                &format!("{}\n", formatted_yaml.trim_end()),
            )
        {
            output = replaced;
        } else {
            log::warn!("Skipping YAML apply: no valid frontmatter region range");
        }
    }

    log::debug!("Formatting complete: {} bytes output", output.len());

    // Ensure exactly one trailing newline
    output.trim_end().to_string() + "\n"
}

fn apply_formatted_yaml_at_range(
    output: &str,
    region: &YamlFrontmatterRegion,
    formatted_yaml_with_trailing_newline: &str,
) -> Option<String> {
    if region.content_range.end > output.len()
        || region.content_range.start > region.content_range.end
    {
        return None;
    }
    let mut out = String::with_capacity(
        output.len() - (region.content_range.end - region.content_range.start)
            + formatted_yaml_with_trailing_newline.len(),
    );
    out.push_str(&output[..region.content_range.start]);
    out.push_str(formatted_yaml_with_trailing_newline);
    out.push_str(&output[region.content_range.end..]);
    Some(out)
}
