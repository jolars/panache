use wasm_bindgen::prelude::*;

use panache::config::{BlankLines, Flavor, LineEnding, MathDelimiterStyle, TabStopMode, WrapMode};

fn parse_flavor(value: &str) -> Option<Flavor> {
    match value.to_ascii_lowercase().as_str() {
        "pandoc" => Some(Flavor::Pandoc),
        "quarto" => Some(Flavor::Quarto),
        "rmarkdown" | "r-markdown" => Some(Flavor::RMarkdown),
        "gfm" => Some(Flavor::Gfm),
        "commonmark" | "common-mark" => Some(Flavor::CommonMark),
        _ => None,
    }
}

fn parse_wrap_mode(value: &str) -> Option<WrapMode> {
    match value.to_ascii_lowercase().as_str() {
        "preserve" => Some(WrapMode::Preserve),
        "reflow" => Some(WrapMode::Reflow),
        "sentence" => Some(WrapMode::Sentence),
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

fn parse_tab_stops(value: &str) -> Option<TabStopMode> {
    match value.to_ascii_lowercase().as_str() {
        "normalize" => Some(TabStopMode::Normalize),
        "preserve" => Some(TabStopMode::Preserve),
        _ => None,
    }
}

#[wasm_bindgen]
pub fn format_qmd(input: &str, line_width: Option<usize>) -> String {
    let cfg = panache::ConfigBuilder::default()
        .line_width(line_width.unwrap_or(80))
        .build();
    panache::format(input, Some(cfg), None)
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
    tab_stops: Option<String>,
    tab_width: Option<usize>,
    math_indent: Option<usize>,
) -> Result<String, JsValue> {
    let mut cfg = panache::Config::default();

    if let Some(width) = line_width {
        cfg.line_width = width;
    }

    if let Some(flavor) = flavor {
        let parsed = parse_flavor(&flavor)
            .ok_or_else(|| JsValue::from_str(&format!("Unsupported flavor: {flavor}")))?;
        cfg.flavor = parsed;
        cfg.extensions = panache::config::Extensions::for_flavor(parsed);
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

    Ok(panache::format(input, Some(cfg), None))
}

// Optional: expose tokenizer/AST for debugging
#[wasm_bindgen]
pub fn tokenize_debug(input: &str) -> String {
    // return a simple debug string; or serialize to JSON if you add serde
    let tree = panache::parse(input, None);
    format!("{tree:#?}")
}
