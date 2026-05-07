use anyhow::{Result, anyhow};
use dprint_core::configuration::{
    ConfigKeyMap, ConfigurationDiagnostic, GlobalConfiguration, get_unknown_property_diagnostics,
    get_value,
};
use dprint_core::generate_plugin_code;
use dprint_core::plugins::{
    CheckConfigUpdatesMessage, ConfigChange, FileMatchingInfo, FormatResult, PluginInfo,
    PluginResolveConfigurationResult, SyncFormatRequest, SyncHostFormatRequest, SyncPluginHandler,
};
use panache_formatter::Config;
use panache_formatter::config::{
    BlankLines, Flavor, FormatterExtensions, LineEnding, MathDelimiterStyle, ParserExtensions,
    TabStopMode, WrapMode,
};
use serde::Serialize;

const FILE_EXTENSIONS: &[&str] = &[
    "md",
    "qmd",
    "Rmd",
    "rmd",
    "Rmarkdown",
    "rmarkdown",
    "markdown",
    "mdown",
    "mkd",
];

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Configuration {
    flavor: String,
    line_width: u32,
    wrap: String,
    blank_lines: String,
    math_indent: u32,
    math_delimiter_style: String,
    tab_width: u32,
    tab_stops: String,
    line_ending: String,
}

#[derive(Default)]
pub struct PanacheHandler;

impl PanacheHandler {
    pub const fn new() -> Self {
        PanacheHandler
    }
}

fn parse_flavor(value: &str, diagnostics: &mut Vec<ConfigurationDiagnostic>) -> Flavor {
    match value.to_ascii_lowercase().as_str() {
        "pandoc" => Flavor::Pandoc,
        "quarto" => Flavor::Quarto,
        "rmarkdown" | "r-markdown" => Flavor::RMarkdown,
        "gfm" => Flavor::Gfm,
        "commonmark" | "common-mark" => Flavor::CommonMark,
        "multimarkdown" | "multi-markdown" => Flavor::MultiMarkdown,
        other => {
            diagnostics.push(ConfigurationDiagnostic {
                property_name: "flavor".to_string(),
                message: format!(
                    "Unknown flavor '{other}'. Expected one of: pandoc, quarto, rmarkdown, gfm, commonmark, multimarkdown."
                ),
            });
            Flavor::default()
        }
    }
}

fn parse_wrap(value: &str, diagnostics: &mut Vec<ConfigurationDiagnostic>) -> Option<WrapMode> {
    match value.to_ascii_lowercase().as_str() {
        "preserve" => Some(WrapMode::Preserve),
        "reflow" => Some(WrapMode::Reflow),
        "sentence" => Some(WrapMode::Sentence),
        other => {
            diagnostics.push(ConfigurationDiagnostic {
                property_name: "wrap".to_string(),
                message: format!(
                    "Unknown wrap mode '{other}'. Expected one of: preserve, reflow, sentence."
                ),
            });
            Some(WrapMode::Reflow)
        }
    }
}

fn parse_blank_lines(value: &str, diagnostics: &mut Vec<ConfigurationDiagnostic>) -> BlankLines {
    match value.to_ascii_lowercase().as_str() {
        "preserve" => BlankLines::Preserve,
        "collapse" => BlankLines::Collapse,
        other => {
            diagnostics.push(ConfigurationDiagnostic {
                property_name: "blankLines".to_string(),
                message: format!(
                    "Unknown blank-lines mode '{other}'. Expected one of: preserve, collapse."
                ),
            });
            BlankLines::Collapse
        }
    }
}

fn parse_math_delimiter(
    value: &str,
    diagnostics: &mut Vec<ConfigurationDiagnostic>,
) -> MathDelimiterStyle {
    match value.to_ascii_lowercase().as_str() {
        "preserve" => MathDelimiterStyle::Preserve,
        "dollars" => MathDelimiterStyle::Dollars,
        "backslash" => MathDelimiterStyle::Backslash,
        other => {
            diagnostics.push(ConfigurationDiagnostic {
                property_name: "mathDelimiterStyle".to_string(),
                message: format!(
                    "Unknown math delimiter style '{other}'. Expected one of: preserve, dollars, backslash."
                ),
            });
            MathDelimiterStyle::Preserve
        }
    }
}

fn parse_tab_stops(value: &str, diagnostics: &mut Vec<ConfigurationDiagnostic>) -> TabStopMode {
    match value.to_ascii_lowercase().as_str() {
        "normalize" => TabStopMode::Normalize,
        "preserve" => TabStopMode::Preserve,
        other => {
            diagnostics.push(ConfigurationDiagnostic {
                property_name: "tabStops".to_string(),
                message: format!(
                    "Unknown tab-stops mode '{other}'. Expected one of: normalize, preserve."
                ),
            });
            TabStopMode::Normalize
        }
    }
}

fn parse_line_ending(
    value: &str,
    diagnostics: &mut Vec<ConfigurationDiagnostic>,
) -> Option<LineEnding> {
    match value.to_ascii_lowercase().as_str() {
        "auto" => Some(LineEnding::Auto),
        "lf" => Some(LineEnding::Lf),
        "crlf" => Some(LineEnding::Crlf),
        other => {
            diagnostics.push(ConfigurationDiagnostic {
                property_name: "lineEnding".to_string(),
                message: format!("Unknown line ending '{other}'. Expected one of: auto, lf, crlf."),
            });
            Some(LineEnding::Auto)
        }
    }
}

fn detect_flavor_from_path(path: &std::path::Path) -> Option<Flavor> {
    let ext = path.extension().and_then(|e| e.to_str())?;
    match ext.to_ascii_lowercase().as_str() {
        "qmd" => Some(Flavor::Quarto),
        "rmd" | "rmarkdown" => Some(Flavor::RMarkdown),
        _ => None,
    }
}

fn build_panache_config(cfg: &Configuration, file_path: &std::path::Path) -> Config {
    let mut throwaway = Vec::new();
    let flavor = detect_flavor_from_path(file_path)
        .unwrap_or_else(|| parse_flavor(&cfg.flavor, &mut throwaway));
    Config {
        flavor,
        parser_extensions: ParserExtensions::for_flavor(flavor),
        formatter_extensions: FormatterExtensions::for_flavor(flavor),
        line_ending: parse_line_ending(&cfg.line_ending, &mut throwaway),
        line_width: cfg.line_width as usize,
        math_indent: cfg.math_indent as usize,
        math_delimiter_style: parse_math_delimiter(&cfg.math_delimiter_style, &mut throwaway),
        tab_stops: parse_tab_stops(&cfg.tab_stops, &mut throwaway),
        tab_width: cfg.tab_width as usize,
        wrap: parse_wrap(&cfg.wrap, &mut throwaway),
        blank_lines: parse_blank_lines(&cfg.blank_lines, &mut throwaway),
        ..Config::default()
    }
}

impl SyncPluginHandler<Configuration> for PanacheHandler {
    fn resolve_config(
        &mut self,
        config: ConfigKeyMap,
        global_config: &GlobalConfiguration,
    ) -> PluginResolveConfigurationResult<Configuration> {
        let mut config = config;
        let mut diagnostics = Vec::new();

        let line_width: u32 = get_value(
            &mut config,
            "lineWidth",
            global_config.line_width.unwrap_or(80),
            &mut diagnostics,
        );
        let tab_width: u32 = get_value(
            &mut config,
            "tabWidth",
            global_config.indent_width.map(u32::from).unwrap_or(4),
            &mut diagnostics,
        );

        let flavor: String = get_value(
            &mut config,
            "flavor",
            "pandoc".to_string(),
            &mut diagnostics,
        );
        let wrap: String = get_value(&mut config, "wrap", "reflow".to_string(), &mut diagnostics);
        let blank_lines: String = get_value(
            &mut config,
            "blankLines",
            "collapse".to_string(),
            &mut diagnostics,
        );
        let math_indent: u32 = get_value(&mut config, "mathIndent", 0, &mut diagnostics);
        let math_delimiter_style: String = get_value(
            &mut config,
            "mathDelimiterStyle",
            "preserve".to_string(),
            &mut diagnostics,
        );
        let tab_stops: String = get_value(
            &mut config,
            "tabStops",
            "normalize".to_string(),
            &mut diagnostics,
        );
        let line_ending: String = get_value(
            &mut config,
            "lineEnding",
            "auto".to_string(),
            &mut diagnostics,
        );

        let _ = parse_flavor(&flavor, &mut diagnostics);
        let _ = parse_wrap(&wrap, &mut diagnostics);
        let _ = parse_blank_lines(&blank_lines, &mut diagnostics);
        let _ = parse_math_delimiter(&math_delimiter_style, &mut diagnostics);
        let _ = parse_tab_stops(&tab_stops, &mut diagnostics);
        let _ = parse_line_ending(&line_ending, &mut diagnostics);

        diagnostics.extend(get_unknown_property_diagnostics(config));

        let resolved = Configuration {
            flavor,
            line_width,
            wrap,
            blank_lines,
            math_indent,
            math_delimiter_style,
            tab_width,
            tab_stops,
            line_ending,
        };

        PluginResolveConfigurationResult {
            config: resolved,
            diagnostics,
            file_matching: FileMatchingInfo {
                file_extensions: FILE_EXTENSIONS.iter().map(|s| (*s).to_string()).collect(),
                file_names: Vec::new(),
            },
        }
    }

    fn plugin_info(&mut self) -> PluginInfo {
        let version = env!("CARGO_PKG_VERSION").to_string();
        PluginInfo {
            name: env!("CARGO_PKG_NAME").to_string(),
            version: version.clone(),
            config_key: "panache".to_string(),
            help_url: "https://panache.bz".to_string(),
            config_schema_url: format!(
                "https://plugins.dprint.dev/jolars/panache/{version}/schema.json"
            ),
            update_url: Some("https://plugins.dprint.dev/jolars/panache/latest.json".to_string()),
        }
    }

    fn license_text(&mut self) -> String {
        include_str!("../../../LICENSE").to_string()
    }

    fn check_config_updates(
        &self,
        _message: CheckConfigUpdatesMessage,
    ) -> Result<Vec<ConfigChange>> {
        Ok(Vec::new())
    }

    fn format(
        &mut self,
        request: SyncFormatRequest<Configuration>,
        _format_with_host: impl FnMut(SyncHostFormatRequest) -> FormatResult,
    ) -> FormatResult {
        let file_text = String::from_utf8(request.file_bytes)
            .map_err(|e| anyhow!("input is not valid UTF-8: {e}"))?;

        let panache_config = build_panache_config(request.config, request.file_path);
        let range = request.range.as_ref().map(|r| (r.start, r.end));

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            panache_formatter::format(&file_text, Some(panache_config), range)
        }));

        match result {
            Ok(formatted) => {
                if formatted == file_text {
                    Ok(None)
                } else {
                    Ok(Some(formatted.into_bytes()))
                }
            }
            Err(payload) => {
                let message = if let Some(s) = payload.downcast_ref::<&'static str>() {
                    (*s).to_string()
                } else if let Some(s) = payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "panache panicked while formatting".to_string()
                };
                Err(anyhow!("panache panicked: {message}"))
            }
        }
    }
}

generate_plugin_code!(PanacheHandler, PanacheHandler::new());
