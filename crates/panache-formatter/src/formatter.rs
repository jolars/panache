use crate::config::Config;
use crate::syntax::{SyntaxNode, YamlFrontmatterRegion};
use panache_parser::semantic::math::{ArgSpec, CommandSignature, SignatureScope};

mod blockquotes;
mod blocks;
pub mod code_blocks;
mod containers;
mod core;
mod document;
mod fenced_divs;
mod hashpipe;
mod headings;
mod indent_utils;
mod inline;
mod inline_layout;
mod lists;
pub mod math;
mod metadata;
mod paragraphs;
mod preserve;
mod raw;
mod sentence_wrap;
mod shortcodes;
mod smart;
mod tables;
mod utils;
#[allow(dead_code)]
pub mod yaml;

pub use code_blocks::ExternalCodeBlock;
pub use code_blocks::FormattedCodeMap;
pub use code_blocks::collect_code_blocks;
pub use core::Formatter;
pub use indent_utils::continuation_indent_at;

/// Derive the effective math signature scope for `tree`: the configured
/// `math-signatures` entries layered under the document's own `\newcommand`
/// and `\renewcommand` definitions.
///
/// Hosts that drive [`Formatter`] directly instead of going through
/// [`format_tree`] must apply this to their config; without it both configured
/// and document signatures are silently inert.
pub fn resolve_math_signature_scope(tree: &SyntaxNode, config: &Config) -> SignatureScope {
    let configured = config.math_signatures.iter().map(|(name, arguments)| {
        (
            name.clone(),
            CommandSignature {
                arguments: arguments
                    .iter()
                    .map(|argument| ArgSpec {
                        required: argument.kind == panache_parser::semantic::math::ArgKind::Brace,
                        kind: argument.kind,
                        domain: argument.domain,
                    })
                    .collect(),
            },
        )
    });
    SignatureScope::from_root_with_configured(tree, configured)
}

pub fn format_tree(tree: &SyntaxNode, config: &Config, range: Option<(usize, usize)>) -> String {
    format_tree_with_formatted_code(tree, config, range, FormattedCodeMap::new())
}

pub fn format_tree_with_formatted_code(
    tree: &SyntaxNode,
    config: &Config,
    range: Option<(usize, usize)>,
    formatted_code: FormattedCodeMap,
) -> String {
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

    let mut effective_config = config.clone();
    effective_config.math_signature_scope = resolve_math_signature_scope(tree, config);
    let mut output = Formatter::new(effective_config, formatted_code, range).format(tree);

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
