use crate::config::Config;
#[cfg(feature = "lsp")]
use crate::external_formatters_common::{
    find_missing_formatter_commands, log_missing_formatter_commands,
};
use crate::external_formatters_sync;
use crate::syntax::{SyntaxKind, SyntaxNode, YamlFrontmatterRegion};
use panache_formatter::FormattedCodeMap;
use std::collections::HashMap;

fn to_formatter_config(config: &Config) -> panache_formatter::Config {
    let line_ending = config.line_ending.as_ref().map(|ending| match ending {
        crate::config::LineEnding::Auto => panache_formatter::LineEnding::Auto,
        crate::config::LineEnding::Lf => panache_formatter::LineEnding::Lf,
        crate::config::LineEnding::Crlf => panache_formatter::LineEnding::Crlf,
    });
    let math_delimiter_style = match config.math_delimiter_style {
        crate::config::MathDelimiterStyle::Preserve => {
            panache_formatter::MathDelimiterStyle::Preserve
        }
        crate::config::MathDelimiterStyle::Dollars => {
            panache_formatter::MathDelimiterStyle::Dollars
        }
        crate::config::MathDelimiterStyle::Backslash => {
            panache_formatter::MathDelimiterStyle::Backslash
        }
    };
    let tab_stops = match config.tab_stops {
        crate::config::TabStopMode::Normalize => panache_formatter::TabStopMode::Normalize,
        crate::config::TabStopMode::Preserve => panache_formatter::TabStopMode::Preserve,
    };
    let wrap = config.wrap.as_ref().map(|wrap| match wrap {
        crate::config::WrapMode::Preserve => panache_formatter::WrapMode::Preserve,
        crate::config::WrapMode::Reflow => panache_formatter::WrapMode::Reflow,
        crate::config::WrapMode::Sentence => panache_formatter::WrapMode::Sentence,
    });
    let blank_lines = match config.blank_lines {
        crate::config::BlankLines::Preserve => panache_formatter::BlankLines::Preserve,
        crate::config::BlankLines::Collapse => panache_formatter::BlankLines::Collapse,
    };

    let formatters: HashMap<String, Vec<panache_formatter::config::FormatterConfig>> = config
        .formatters
        .iter()
        .map(|(lang, entries)| {
            let mapped_entries = entries
                .iter()
                .map(|entry| panache_formatter::config::FormatterConfig {
                    cmd: entry.cmd.clone(),
                    args: entry.args.clone(),
                    enabled: entry.enabled,
                    stdin: entry.stdin,
                })
                .collect();
            (lang.clone(), mapped_entries)
        })
        .collect();

    panache_formatter::Config {
        flavor: config.flavor,
        extensions: config.extensions.clone(),
        line_ending,
        line_width: config.line_width,
        math_indent: config.math_indent,
        math_delimiter_style,
        tab_stops,
        tab_width: config.tab_width,
        wrap,
        blank_lines,
        formatters,
        external_max_parallel: config.external_max_parallel,
        parser: config.parser,
    }
}

fn collect_yaml_frontmatter_region(tree: &SyntaxNode) -> Option<YamlFrontmatterRegion> {
    let frontmatter = tree
        .children()
        .find(|node| node.kind() != SyntaxKind::BLANK_LINE)
        .filter(|node| node.kind() == SyntaxKind::YAML_METADATA)?;

    let content = frontmatter
        .children()
        .find(|child| child.kind() == SyntaxKind::YAML_METADATA_CONTENT)?;

    let host_start: usize = frontmatter.text_range().start().into();
    let host_end: usize = frontmatter.text_range().end().into();
    let content_start: usize = content.text_range().start().into();
    let content_end: usize = content.text_range().end().into();

    Some(YamlFrontmatterRegion {
        id: format!("frontmatter:{}:{}", content_start, content_end),
        host_range: host_start..host_end,
        content_range: content_start..content_end,
        content: content.text().to_string(),
    })
}

#[cfg(feature = "lsp")]
async fn format_code_blocks_async(
    blocks: Vec<panache_formatter::ExternalCodeBlock>,
    config: &Config,
) -> FormattedCodeMap {
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Semaphore;
    use tokio::task::JoinSet;

    let timeout = Duration::from_secs(30);
    let semaphore = Arc::new(Semaphore::new(config.external_max_parallel.max(1)));
    let missing_formatters = Arc::new(find_missing_formatter_commands(&config.formatters));
    log_missing_formatter_commands(&missing_formatters);

    let mut join_set = JoinSet::new();

    for block in blocks {
        let lang = block.language.clone();
        let Some(formatter_configs) = config.formatters.get(&lang) else {
            continue;
        };
        if formatter_configs.is_empty() {
            continue;
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
        let missing_formatters = Arc::clone(&missing_formatters);

        join_set.spawn(async move {
            let _permit = permit;
            let mut current_code = code;

            for (idx, formatter_cfg) in formatter_configs.iter().enumerate() {
                let formatter_cmd = formatter_cfg.cmd.trim();
                if formatter_cmd.is_empty() {
                    continue;
                }

                if missing_formatters.contains(formatter_cmd) {
                    return (lang, original, hashpipe_prefix, Ok(current_code));
                }

                log::debug!(
                    "Formatting {} code with {} ({}/{} in chain)",
                    lang,
                    formatter_cfg.cmd,
                    idx + 1,
                    formatter_configs.len()
                );

                match crate::external_formatters::format_code_async(
                    &current_code,
                    &lang,
                    formatter_cfg,
                    timeout,
                )
                .await
                {
                    Ok(formatted) => {
                        current_code = formatted;
                    }
                    Err(e) => {
                        eprintln!(
                            "Warning: {} formatter '{}' failed: {}. Using original code.",
                            lang, formatter_cfg.cmd, e
                        );
                        return (lang, original, hashpipe_prefix, Err(e));
                    }
                }
            }

            (lang, original, hashpipe_prefix, Ok(current_code))
        });
    }

    let mut formatted = FormattedCodeMap::new();

    while let Some(res) = join_set.join_next().await {
        if let Ok((lang, original_code, hashpipe_prefix, result)) = res {
            match result {
                Ok(formatted_code) => {
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
                }
            }
        }
    }

    formatted
}

#[cfg(not(target_arch = "wasm32"))]
fn format_code_blocks_sync(
    blocks: Vec<panache_formatter::ExternalCodeBlock>,
    config: &Config,
) -> FormattedCodeMap {
    use std::time::Duration;
    let timeout = Duration::from_secs(30);
    external_formatters_sync::run_formatters_parallel(
        blocks,
        &config.formatters,
        timeout,
        config.external_max_parallel,
    )
}

#[cfg(target_arch = "wasm32")]
fn format_code_blocks_sync(
    _blocks: Vec<panache_formatter::ExternalCodeBlock>,
    _config: &Config,
) -> FormattedCodeMap {
    FormattedCodeMap::new()
}

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
    let frontmatter_region = collect_yaml_frontmatter_region(tree);
    let formatter_config = to_formatter_config(config);
    #[cfg(not(target_arch = "wasm32"))]
    let frontmatter_yaml = frontmatter_region
        .as_ref()
        .map(|region| region.content.trim_end().to_string());

    let formatted_code = if !config.formatters.is_empty() {
        let code_blocks = panache_formatter::collect_code_blocks(tree, &input, &formatter_config);
        if !code_blocks.is_empty() {
            log::debug!(
                "Found {} code blocks, spawning formatters...",
                code_blocks.len()
            );
            format_code_blocks_async(code_blocks, config).await
        } else {
            FormattedCodeMap::new()
        }
    } else {
        FormattedCodeMap::new()
    };

    let yaml_config = config.clone();
    let formatted_yaml_future = frontmatter_yaml.clone().map(|yaml_content| {
        tokio::spawn(async move {
            crate::yaml_engine::format_yaml_with_config(&yaml_content, &yaml_config)
        })
    });

    let mut output =
        panache_formatter::formatter::Formatter::new(formatter_config, formatted_code, range)
            .format(tree);

    if let Some(handle) = formatted_yaml_future
        && let Ok(Ok(formatted_yaml)) = handle.await
    {
        let original_yaml = frontmatter_yaml.unwrap_or_default();
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

    log::info!("Formatting complete: {} bytes output", output.len());
    output.trim_end().to_string() + "\n"
}

pub fn format_tree(tree: &SyntaxNode, config: &Config, range: Option<(usize, usize)>) -> String {
    log::debug!(
        "Formatting document with config: line_width={}, wrap={:?}",
        config.line_width,
        config.wrap
    );

    let input = tree.text().to_string();
    let frontmatter_region = collect_yaml_frontmatter_region(tree);
    let formatter_config = to_formatter_config(config);
    #[cfg(not(target_arch = "wasm32"))]
    let frontmatter_yaml = frontmatter_region
        .as_ref()
        .map(|region| region.content.trim_end().to_string());

    let formatted_code = if !config.formatters.is_empty() {
        let code_blocks = panache_formatter::collect_code_blocks(tree, &input, &formatter_config);
        if !code_blocks.is_empty() {
            log::debug!(
                "Found {} code blocks, spawning formatters...",
                code_blocks.len()
            );
            format_code_blocks_sync(code_blocks, config)
        } else {
            FormattedCodeMap::new()
        }
    } else {
        FormattedCodeMap::new()
    };

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

    let mut output =
        panache_formatter::formatter::Formatter::new(formatter_config, formatted_code, range)
            .format(tree);

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
