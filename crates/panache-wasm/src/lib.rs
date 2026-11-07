use std::collections::HashMap;

use wasm_bindgen::prelude::*;

use panache_formatter::config::{
    BlankLines, Flavor, FormatterExtensions, HorizontalRuleStyle, LineEnding, MathDelimiterStyle,
    MathMode, ParserExtensions, TabStopMode, WrapMode,
};

fn parse_flavor(value: &str) -> Option<Flavor> {
    match value.to_ascii_lowercase().as_str() {
        "pandoc" => Some(Flavor::Pandoc),
        "quarto" => Some(Flavor::Quarto),
        "rmarkdown" | "r-markdown" => Some(Flavor::RMarkdown),
        "gfm" => Some(Flavor::Gfm),
        "commonmark" | "common-mark" => Some(Flavor::CommonMark),
        "multimarkdown" | "multi-markdown" => Some(Flavor::MultiMarkdown),
        "mdsvex" => Some(Flavor::Mdsvex),
        "hugo" => Some(Flavor::Hugo),
        "myst" => Some(Flavor::Myst),
        _ => None,
    }
}

#[cfg(test)]
mod hugo_tests {
    #[test]
    fn accepts_hugo_flavor() {
        assert_eq!(super::parse_flavor("hugo"), Some(super::Flavor::Hugo));
    }

    #[test]
    fn accepts_myst_flavor() {
        assert_eq!(super::parse_flavor("myst"), Some(super::Flavor::Myst));
    }

    #[test]
    fn formats_with_hugo_defaults() {
        let input = "+++\ntitle = 'Keep   this'\n+++\n\n{{< foo\n>}}\n\nNormal   prose.\n";
        let output = super::format_qmd_with_options(
            input,
            None,
            Some("hugo".to_owned()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(output, input.replace("Normal   prose.", "Normal prose."));
    }

    #[test]
    fn parses_extension_overrides() {
        let overrides =
            super::parse_extension_overrides(" +definition-lists,-strikeout task-lists ").unwrap();
        assert_eq!(overrides.get("definition-lists"), Some(&true));
        assert_eq!(overrides.get("strikeout"), Some(&false));
        assert_eq!(overrides.get("task-lists"), Some(&true));
        assert!(super::parse_extension_overrides("+nope").is_err());
        // Formatter-only extensions are valid too.
        assert!(super::parse_extension_overrides("-smart +smart-quotes").is_ok());
    }

    #[test]
    fn extension_overrides_reach_formatter() {
        let output = super::format_qmd_with_options(
            "a  \nb\n",
            None,
            Some("pandoc".to_owned()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some("-escaped-line-breaks".to_owned()),
        )
        .unwrap();
        assert_eq!(output, "a  \nb\n");
    }

    #[test]
    fn extension_overrides_apply_on_top_of_flavor() {
        let format = |extensions: Option<&str>| {
            super::format_qmd_with_options(
                "Term\n:   Definition   here\n",
                None,
                Some("myst".to_owned()),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                extensions.map(str::to_owned),
            )
            .unwrap()
        };
        assert_eq!(format(None), "Term : Definition here\n");
        assert_eq!(
            format(Some("+definition-lists")),
            "Term\n:   Definition here\n"
        );
    }
}

fn parse_wrap_mode(value: &str) -> Option<WrapMode> {
    match value.to_ascii_lowercase().as_str() {
        "preserve" => Some(WrapMode::Preserve),
        "reflow" => Some(WrapMode::Reflow),
        "sentence" => Some(WrapMode::Sentence),
        "semantic" => Some(WrapMode::Semantic),
        _ => None,
    }
}

fn parse_blank_lines(value: &str) -> Option<BlankLines> {
    match value.to_ascii_lowercase().as_str() {
        "preserve" => Some(BlankLines::Preserve),
        "collapse" => Some(BlankLines::Collapse),
        _ => None,
    }
}

fn parse_line_ending(value: &str) -> Option<LineEnding> {
    match value.to_ascii_lowercase().as_str() {
        "auto" => Some(LineEnding::Auto),
        "lf" => Some(LineEnding::Lf),
        "crlf" => Some(LineEnding::Crlf),
        _ => None,
    }
}

fn parse_math_delimiter_style(value: &str) -> Option<MathDelimiterStyle> {
    match value.to_ascii_lowercase().as_str() {
        "preserve" => Some(MathDelimiterStyle::Preserve),
        "dollars" => Some(MathDelimiterStyle::Dollars),
        "backslash" => Some(MathDelimiterStyle::Backslash),
        _ => None,
    }
}

fn parse_math_mode(value: &str) -> Option<MathMode> {
    match value.to_ascii_lowercase().as_str() {
        "verbatim" => Some(MathMode::Verbatim),
        "preserve" => Some(MathMode::Preserve),
        "single-line" => Some(MathMode::SingleLine),
        "reflow" => Some(MathMode::Reflow),
        _ => None,
    }
}

fn parse_horizontal_rule_style(value: &str) -> Option<HorizontalRuleStyle> {
    match value.to_ascii_lowercase().as_str() {
        "line-width" => Some(HorizontalRuleStyle::LineWidth),
        "compact" => Some(HorizontalRuleStyle::Compact),
        _ => None,
    }
}

/// Parse pandoc-style overrides like `+definition-lists -strikeout` (commas also separate).
fn parse_extension_overrides(value: &str) -> Result<HashMap<String, bool>, String> {
    value
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|token| !token.is_empty())
        .map(|token| {
            let (enabled, name) = match token.strip_prefix('-') {
                Some(name) => (false, name),
                None => (true, token.strip_prefix('+').unwrap_or(token)),
            };
            if ParserExtensions::is_known_name(name) || FormatterExtensions::is_known_name(name) {
                Ok((name.to_owned(), enabled))
            } else {
                Err(format!("Unknown extension: {name}"))
            }
        })
        .collect()
}

fn parse_tab_stops(value: &str) -> Option<TabStopMode> {
    match value.to_ascii_lowercase().as_str() {
        "normalize" => Some(TabStopMode::Normalize),
        "preserve" => Some(TabStopMode::Preserve),
        _ => None,
    }
}

#[wasm_bindgen]
pub fn format_qmd(input: &str, line_width: Option<usize>) -> String {
    let cfg = panache_formatter::ConfigBuilder::default()
        .line_width(line_width.unwrap_or(80))
        .build();
    panache_formatter::format(input, Some(cfg), None)
}

#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn format_qmd_with_options(
    input: &str,
    line_width: Option<usize>,
    flavor: Option<String>,
    wrap: Option<String>,
    blank_lines: Option<String>,
    line_ending: Option<String>,
    math_delimiter_style: Option<String>,
    math: Option<String>,
    tab_stops: Option<String>,
    tab_width: Option<usize>,
    math_indent: Option<usize>,
    horizontal_rule_style: Option<String>,
    extensions: Option<String>,
) -> Result<String, JsValue> {
    let mut cfg = panache_formatter::Config::default();

    if let Some(width) = line_width {
        cfg.line_width = width;
    }

    if let Some(flavor) = flavor {
        let parsed = parse_flavor(&flavor)
            .ok_or_else(|| JsValue::from_str(&format!("Unsupported flavor: {flavor}")))?;
        cfg.flavor = parsed;
        cfg.parser_extensions = ParserExtensions::for_flavor(parsed);
        cfg.formatter_extensions = FormatterExtensions::for_flavor(parsed);
    }

    // After the flavor block, so overrides apply on top of the flavor defaults.
    if let Some(extensions) = extensions {
        let overrides =
            parse_extension_overrides(&extensions).map_err(|e| JsValue::from_str(&e))?;
        cfg.parser_extensions.apply_overrides(overrides.clone());
        cfg.formatter_extensions.apply_overrides(overrides);
    }

    if let Some(wrap) = wrap {
        cfg.wrap = Some(
            parse_wrap_mode(&wrap)
                .ok_or_else(|| JsValue::from_str(&format!("Unsupported wrap mode: {wrap}")))?,
        );
    }

    if let Some(blank_lines) = blank_lines {
        cfg.blank_lines = parse_blank_lines(&blank_lines).ok_or_else(|| {
            JsValue::from_str(&format!("Unsupported blank line mode: {blank_lines}"))
        })?;
    }

    if let Some(line_ending) = line_ending {
        cfg.line_ending = Some(parse_line_ending(&line_ending).ok_or_else(|| {
            JsValue::from_str(&format!("Unsupported line ending: {line_ending}"))
        })?);
    }

    if let Some(math_delimiter_style) = math_delimiter_style {
        cfg.math_delimiter_style =
            parse_math_delimiter_style(&math_delimiter_style).ok_or_else(|| {
                JsValue::from_str(&format!(
                    "Unsupported math delimiter style: {math_delimiter_style}"
                ))
            })?;
    }

    if let Some(math) = math {
        cfg.math = parse_math_mode(&math)
            .ok_or_else(|| JsValue::from_str(&format!("Unsupported math mode: {math}")))?;
    }

    if let Some(tab_stops) = tab_stops {
        cfg.tab_stops = parse_tab_stops(&tab_stops)
            .ok_or_else(|| JsValue::from_str(&format!("Unsupported tab stop mode: {tab_stops}")))?;
    }

    if let Some(tab_width) = tab_width {
        if tab_width == 0 {
            return Err(JsValue::from_str("tab_width must be greater than 0"));
        }
        cfg.tab_width = tab_width;
    }

    if let Some(math_indent) = math_indent {
        cfg.math_indent = math_indent;
    }

    if let Some(horizontal_rule_style) = horizontal_rule_style {
        cfg.horizontal_rule_style = parse_horizontal_rule_style(&horizontal_rule_style)
            .ok_or_else(|| {
                JsValue::from_str(&format!(
                    "Unsupported horizontal rule style: {horizontal_rule_style}"
                ))
            })?;
    }

    Ok(panache_formatter::format(input, Some(cfg), None))
}

// Optional: expose tokenizer/AST for debugging
#[wasm_bindgen]
pub fn tokenize_debug(input: &str) -> String {
    // return a simple debug string; or serialize to JSON if you add serde
    let tree = panache_parser::parse(input, None);
    format!("{tree:#?}")
}
