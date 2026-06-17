use crate::config::Config;
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
        crate::config::WrapMode::Semantic => panache_formatter::WrapMode::Semantic,
    });
    let blank_lines = match config.blank_lines {
        crate::config::BlankLines::Preserve => panache_formatter::BlankLines::Preserve,
        crate::config::BlankLines::Collapse => panache_formatter::BlankLines::Collapse,
    };
    // Collapse the user-facing flat/per-language shapes into a single
    // language-keyed map; the formatter normalizes the entries at resolution
    // time. Keys are lowercased so they match the resolved language code.
    let no_break_abbreviations = match &config.no_break_abbreviations {
        None => std::collections::BTreeMap::new(),
        Some(crate::config::NoBreakAbbreviations::Flat(list)) => {
            std::collections::BTreeMap::from([("default".to_string(), list.clone())])
        }
        Some(crate::config::NoBreakAbbreviations::PerLanguage(by_lang)) => by_lang
            .iter()
            .map(|(key, list)| (key.to_lowercase(), list.clone()))
            .collect(),
    };
    let formatter_extensions = panache_formatter::config::FormatterExtensions {
        // Keep shared extension behavior aligned with parser-facing extensions.
        blank_before_header: config.extensions.blank_before_header,
        bookdown_references: config.extensions.bookdown_references,
        east_asian_line_breaks: config.extensions.east_asian_line_breaks,
        escaped_line_breaks: config.extensions.escaped_line_breaks,
        gfm_auto_identifiers: config.extensions.gfm_auto_identifiers,
        quarto_crossrefs: config.extensions.quarto_crossrefs,
        // Formatter-only smart toggles are owned separately.
        smart: config.formatter_extensions.smart,
        smart_quotes: config.formatter_extensions.smart_quotes,
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
        parser_extensions: config.extensions.clone(),
        formatter_extensions,
        line_ending,
        line_width: config.line_width,
        math_indent: config.math_indent,
        math_delimiter_style,
        table_indent: config.table_indent,
        tab_stops,
        tab_width: config.tab_width,
        wrap,
        blank_lines,
        lang: config.lang.clone(),
        no_break_abbreviations,
        formatters,
        external_max_parallel: config.external_max_parallel,
        parser: config.parser,
        experimental_format_math: config.experimental.format_math,
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
        match crate::yaml_engine::format_yaml_with_config(&yaml_content, &formatter_config) {
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
