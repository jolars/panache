use std::collections::HashMap;

pub use panache_parser::Dialect;
pub use panache_parser::Extensions as ParserExtensions;
pub use panache_parser::Flavor;
pub use panache_parser::PandocCompat;
pub use panache_parser::ParserOptions;

fn default_external_max_parallel() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, 8)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MathDelimiterStyle {
    /// Preserve original delimiter style (\(...\) stays \(...\), $...$ stays $...$)
    #[default]
    Preserve,
    /// Normalize all to dollar syntax ($...$ and $$...$$)
    Dollars,
    /// Normalize all to backslash syntax (\(...\) and \[...\])
    Backslash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabStopMode {
    /// Normalize tabs to spaces (4-column tab stop).
    #[default]
    Normalize,
    /// Preserve tabs in literal code spans/blocks.
    Preserve,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FormatterConfig {
    pub cmd: String,
    pub args: Vec<String>,
    pub enabled: bool,
    pub stdin: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WrapMode {
    Preserve,
    Reflow,
    Sentence,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LineEnding {
    Auto,
    Lf,
    Crlf,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BlankLines {
    /// Preserve original blank lines (any number)
    Preserve,
    /// Collapse multiple consecutive blank lines to a single blank line
    Collapse,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FormatterExtensions {
    pub blank_before_header: bool,
    pub bookdown_references: bool,
    pub escaped_line_breaks: bool,
    pub gfm_auto_identifiers: bool,
    pub quarto_crossrefs: bool,
    pub smart: bool,
    pub smart_quotes: bool,
}

impl Default for FormatterExtensions {
    fn default() -> Self {
        Self::for_flavor(Flavor::default())
    }
}

impl FormatterExtensions {
    pub fn for_flavor(flavor: Flavor) -> Self {
        let parser_defaults = ParserExtensions::for_flavor(flavor);
        let smart_default = matches!(flavor, Flavor::Pandoc | Flavor::Quarto | Flavor::RMarkdown);

        Self {
            blank_before_header: parser_defaults.blank_before_header,
            bookdown_references: parser_defaults.bookdown_references,
            escaped_line_breaks: parser_defaults.escaped_line_breaks,
            gfm_auto_identifiers: parser_defaults.gfm_auto_identifiers,
            quarto_crossrefs: parser_defaults.quarto_crossrefs,
            smart: smart_default,
            smart_quotes: false,
        }
    }

    pub fn merge_with_flavor(overrides: HashMap<String, bool>, flavor: Flavor) -> Self {
        let mut base = Self::for_flavor(flavor);
        for (key, value) in overrides {
            match key.replace('_', "-").to_ascii_lowercase().as_str() {
                "blank-before-header" => base.blank_before_header = value,
                "bookdown-references" => base.bookdown_references = value,
                "escaped-line-breaks" => base.escaped_line_breaks = value,
                "gfm-auto-identifiers" => base.gfm_auto_identifiers = value,
                "quarto-crossrefs" => base.quarto_crossrefs = value,
                "smart" => base.smart = value,
                "smart-quotes" => base.smart_quotes = value,
                _ => {}
            }
        }
        base
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub flavor: Flavor,
    pub parser_extensions: ParserExtensions,
    pub formatter_extensions: FormatterExtensions,
    pub line_ending: Option<LineEnding>,
    pub line_width: usize,
    pub math_indent: usize,
    pub math_delimiter_style: MathDelimiterStyle,
    pub tab_stops: TabStopMode,
    pub tab_width: usize,
    pub wrap: Option<WrapMode>,
    pub blank_lines: BlankLines,
    /// Language → Formatter(s) mapping (supports multiple formatters per language)
    pub formatters: HashMap<String, Vec<FormatterConfig>>,
    /// Max parallel external tool invocations (formatters/linters) per document.
    pub external_max_parallel: usize,
    /// Compatibility target for ambiguous Pandoc behavior.
    pub parser: PandocCompat,
}

impl Default for Config {
    fn default() -> Self {
        let flavor = Flavor::default();
        Self {
            flavor,
            parser_extensions: ParserExtensions::for_flavor(flavor),
            formatter_extensions: FormatterExtensions::for_flavor(flavor),
            line_ending: Some(LineEnding::Auto),
            line_width: 80,
            math_indent: 0,
            math_delimiter_style: MathDelimiterStyle::default(),
            tab_stops: TabStopMode::Normalize,
            tab_width: 4,
            wrap: Some(WrapMode::Reflow),
            blank_lines: BlankLines::Collapse,
            formatters: HashMap::new(), // Opt-in: empty by default
            external_max_parallel: default_external_max_parallel(),
            parser: PandocCompat::default(),
        }
    }
}

impl Config {
    pub fn parser_options(&self) -> ParserOptions {
        ParserOptions {
            flavor: self.flavor,
            dialect: Dialect::for_flavor(self.flavor),
            extensions: self.parser_extensions.clone(),
            pandoc_compat: self.parser,
            refdef_labels: None,
        }
    }
}

#[derive(Default, Clone)]
pub struct ConfigBuilder {
    config: Config,
}

impl ConfigBuilder {
    pub fn math_indent(mut self, indent: usize) -> Self {
        self.config.math_indent = indent;
        self
    }

    pub fn tab_stops(mut self, mode: TabStopMode) -> Self {
        self.config.tab_stops = mode;
        self
    }

    pub fn tab_width(mut self, width: usize) -> Self {
        self.config.tab_width = width;
        self
    }

    pub fn line_width(mut self, width: usize) -> Self {
        self.config.line_width = width;
        self
    }

    pub fn line_ending(mut self, ending: LineEnding) -> Self {
        self.config.line_ending = Some(ending);
        self
    }

    pub fn blank_lines(mut self, mode: BlankLines) -> Self {
        self.config.blank_lines = mode;
        self
    }

    pub fn build(self) -> Config {
        self.config
    }
}
