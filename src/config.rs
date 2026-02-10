use std::collections::HashMap;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The flavor of Markdown to parse and format.
/// Each flavor has a different set of default extensions enabled.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Flavor {
    /// Standard Pandoc Markdown (default extensions enabled)
    #[default]
    Pandoc,
    /// Quarto-flavored Markdown (Pandoc + Quarto-specific extensions)
    Quarto,
    /// R Markdown (Pandoc + R-specific extensions)
    #[serde(rename = "rmarkdown")]
    RMarkdown,
    /// GitHub Flavored Markdown
    Gfm,
    /// CommonMark (minimal standard extensions)
    CommonMark,
}

/// Pandoc/Markdown extensions configuration.
/// Each field represents a specific Pandoc extension.
/// Extensions marked with a comment indicate implementation status.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct Extensions {
    // ===== Block-level extensions =====

    // Headings
    /// Require blank line before headers (default: enabled)
    pub blank_before_header: bool,
    /// Full attribute syntax on headers {#id .class key=value}
    pub header_attributes: bool,

    // Block quotes
    /// Require blank line before blockquotes (default: enabled)
    pub blank_before_blockquote: bool,

    // Lists
    /// Fancy list markers (roman numerals, letters, etc.)
    pub fancy_lists: bool,
    /// Start ordered lists at arbitrary numbers
    pub startnum: bool,
    /// Example lists with (@) markers
    pub example_lists: bool,
    /// GitHub-style task lists - [ ] and - [x]
    pub task_lists: bool,
    /// Term/definition syntax
    pub definition_lists: bool,

    // Code blocks
    /// Fenced code blocks with backticks
    pub backtick_code_blocks: bool,
    /// Fenced code blocks with tildes
    pub fenced_code_blocks: bool,
    /// Attributes on fenced code blocks {.language #id}
    pub fenced_code_attributes: bool,
    /// Attributes on inline code
    pub inline_code_attributes: bool,

    // Tables
    /// Simple table syntax
    pub simple_tables: bool,
    /// Multiline cell content in tables
    pub multiline_tables: bool,
    /// Grid-style tables
    pub grid_tables: bool,
    /// Pipe tables (GitHub/PHP Markdown style)
    pub pipe_tables: bool,
    /// Table captions
    pub table_captions: bool,

    // Divs
    /// Fenced divs ::: {.class}
    pub fenced_divs: bool,
    /// HTML <div> elements
    pub native_divs: bool,

    // Other block elements
    /// Line blocks for poetry | prefix
    pub line_blocks: bool,

    // ===== Inline elements =====

    // Emphasis
    /// Underscores don't trigger emphasis in snake_case
    pub intraword_underscores: bool,
    /// Strikethrough ~~text~~
    pub strikeout: bool,
    /// Superscript and subscript ^super^ ~sub~
    pub superscript: bool,
    pub subscript: bool,

    // Links
    /// Inline links [text](url)
    pub inline_links: bool,
    /// Reference links [text][ref]
    pub reference_links: bool,
    /// Shortcut reference links [ref] without second []
    pub shortcut_reference_links: bool,
    /// Attributes on links [text](url){.class}
    pub link_attributes: bool,
    /// Automatic links <http://example.com>
    pub autolinks: bool,

    // Images
    /// Inline images ![alt](url)
    pub inline_images: bool,
    /// Paragraph with just image becomes figure
    pub implicit_figures: bool,

    // Math
    /// Dollar-delimited math $x$ and $$equation$$
    pub tex_math_dollars: bool,
    /// [NON-DEFAULT] Single backslash math \(...\) and \[...\] (RMarkdown default)
    pub tex_math_single_backslash: bool,
    /// [NON-DEFAULT] Double backslash math \\(...\\) and \\[...\\]
    pub tex_math_double_backslash: bool,

    // Footnotes
    /// Inline footnotes ^[text]
    pub inline_footnotes: bool,
    /// Reference footnotes `[^1]` (requires footnote parsing)
    pub footnotes: bool,

    // Citations
    /// Citation syntax [@cite]
    pub citations: bool,

    // Spans
    /// Bracketed spans [text]{.class}
    pub bracketed_spans: bool,
    /// HTML <span> elements
    pub native_spans: bool,

    // ===== Metadata =====
    /// YAML metadata block
    pub yaml_metadata_block: bool,
    /// Pandoc title block (Title/Author/Date)
    pub pandoc_title_block: bool,

    // ===== Raw content =====
    /// Raw HTML blocks and inline
    pub raw_html: bool,
    /// Markdown inside HTML blocks
    pub markdown_in_html_blocks: bool,
    /// LaTeX commands and environments
    pub raw_tex: bool,
    /// Generic raw blocks with {=format} syntax
    pub raw_attribute: bool,

    // ===== Escapes and special characters =====
    /// Backslash escapes any symbol
    pub all_symbols_escapable: bool,
    /// Backslash at line end = hard line break
    pub escaped_line_breaks: bool,

    // ===== NON-DEFAULT EXTENSIONS =====
    // These are disabled by default in Pandoc
    /// [NON-DEFAULT] Bare URLs become links
    pub autolink_bare_uris: bool,
    /// [NON-DEFAULT] Newline = <br>
    pub hard_line_breaks: bool,
    /// [NON-DEFAULT] :emoji: syntax
    pub emoji: bool,
    /// [NON-DEFAULT] Highlighted ==text==
    pub mark: bool,

    // ===== Quarto-specific extensions =====
    /// Quarto callout blocks (.callout-note, etc.)
    pub quarto_callouts: bool,
    /// Quarto cross-references @fig-id, @tbl-id
    pub quarto_crossrefs: bool,
}

impl Default for Extensions {
    fn default() -> Self {
        Self::for_flavor(Flavor::default())
    }
}

impl Extensions {
    /// Get the default extension set for a given flavor.
    pub fn for_flavor(flavor: Flavor) -> Self {
        match flavor {
            Flavor::Pandoc => Self::pandoc_defaults(),
            Flavor::Quarto => Self::quarto_defaults(),
            Flavor::RMarkdown => Self::rmarkdown_defaults(),
            Flavor::Gfm => Self::gfm_defaults(),
            Flavor::CommonMark => Self::commonmark_defaults(),
        }
    }

    /// Standard Pandoc default extensions.
    fn pandoc_defaults() -> Self {
        Self {
            // Block-level - enabled by default in Pandoc
            blank_before_header: true,
            blank_before_blockquote: true,
            header_attributes: true,

            // Lists
            fancy_lists: true,
            startnum: true,
            example_lists: true,
            task_lists: true,
            definition_lists: true,

            // Code
            backtick_code_blocks: true,
            fenced_code_blocks: true,
            fenced_code_attributes: true,
            inline_code_attributes: true,

            // Tables
            simple_tables: true,
            multiline_tables: true,
            grid_tables: true,
            pipe_tables: true,
            table_captions: true,

            // Divs
            fenced_divs: true,
            native_divs: true,

            // Other blocks
            line_blocks: true,

            // Inline
            intraword_underscores: true,
            strikeout: true,
            superscript: true,
            subscript: true,

            // Links
            inline_links: true,
            reference_links: true,
            shortcut_reference_links: true,
            link_attributes: true,
            autolinks: true,

            // Images
            inline_images: true,
            implicit_figures: true,

            // Math
            tex_math_dollars: true,
            tex_math_single_backslash: false,
            tex_math_double_backslash: false,

            // Footnotes
            inline_footnotes: false,
            footnotes: true,

            // Citations
            citations: true,

            // Spans
            bracketed_spans: true,
            native_spans: true,

            // Metadata
            yaml_metadata_block: true,
            pandoc_title_block: true,

            // Raw
            raw_html: true,
            markdown_in_html_blocks: false,
            raw_tex: true,
            raw_attribute: true,

            // Escapes
            all_symbols_escapable: true,
            escaped_line_breaks: true,

            // Non-default (all OFF for Pandoc)
            autolink_bare_uris: false,
            hard_line_breaks: false,
            emoji: false,
            mark: false,

            // Quarto-specific (OFF for Pandoc)
            quarto_callouts: false,
            quarto_crossrefs: false,
        }
    }

    /// Quarto format defaults (Pandoc + Quarto extensions).
    fn quarto_defaults() -> Self {
        let mut ext = Self::pandoc_defaults();

        // Quarto enables additional extensions
        ext.task_lists = true;
        ext.implicit_figures = true;

        // Quarto-specific
        ext.quarto_callouts = true;
        ext.quarto_crossrefs = true;

        ext
    }

    /// R Markdown format defaults.
    fn rmarkdown_defaults() -> Self {
        let mut ext = Self::pandoc_defaults();

        // RMarkdown specifics
        ext.task_lists = true;
        ext.tex_math_dollars = true;
        ext.tex_math_single_backslash = true; // RMarkdown enables \(...\) and \[...\] by default

        ext
    }

    /// GitHub Flavored Markdown defaults.
    fn gfm_defaults() -> Self {
        let mut ext = Self::pandoc_defaults();

        // GFM-specific
        ext.pipe_tables = true;
        ext.task_lists = true;
        ext.strikeout = true;
        ext.autolink_bare_uris = true;

        // GFM doesn't support some Pandoc features
        ext.definition_lists = false;
        ext.footnotes = false;

        ext
    }

    /// CommonMark (minimal standard).
    fn commonmark_defaults() -> Self {
        Self {
            // CommonMark is minimal - most extensions OFF
            blank_before_header: true,
            blank_before_blockquote: true,
            header_attributes: false,

            fancy_lists: false,
            startnum: false,
            example_lists: false,
            task_lists: false,
            definition_lists: false,

            backtick_code_blocks: true,
            fenced_code_blocks: false,
            fenced_code_attributes: false,
            inline_code_attributes: false,

            simple_tables: false,
            multiline_tables: false,
            grid_tables: false,
            pipe_tables: false,
            table_captions: false,

            fenced_divs: false,
            native_divs: false,
            line_blocks: false,

            intraword_underscores: false,
            strikeout: false,
            superscript: false,
            subscript: false,

            inline_links: true,
            reference_links: true,
            shortcut_reference_links: false,
            link_attributes: false,
            autolinks: true,

            inline_images: true,
            implicit_figures: false,

            tex_math_dollars: false,
            tex_math_single_backslash: false,
            tex_math_double_backslash: false,

            inline_footnotes: false,
            footnotes: false,
            citations: false,

            bracketed_spans: false,
            native_spans: false,

            yaml_metadata_block: false,
            pandoc_title_block: false,

            raw_html: true,
            markdown_in_html_blocks: false,
            raw_tex: false,
            raw_attribute: false,

            all_symbols_escapable: true,
            escaped_line_breaks: true,

            autolink_bare_uris: false,
            hard_line_breaks: false,
            emoji: false,
            mark: false,

            quarto_callouts: false,
            quarto_crossrefs: false,
        }
    }
}

/// Configuration for code block formatting.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct CodeBlockConfig {
    /// Fence style: "backtick", "tilde", or "preserve"
    #[serde(rename = "fence-style")]
    pub fence_style: FenceStyle,
    /// Attribute style: "shortcut", "explicit", or "preserve"
    #[serde(rename = "attribute-style")]
    pub attribute_style: AttributeStyle,
    /// Minimum fence length (default: 3)
    #[serde(rename = "min-fence-length")]
    pub min_fence_length: usize,
    /// Whether to normalize indented code blocks to fenced blocks (risky, default: false)
    #[serde(rename = "normalize-indented")]
    pub normalize_indented: bool,
}

/// Fence character preference for code blocks.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FenceStyle {
    /// Use backticks (```)
    Backtick,
    /// Use tildes (~~~)
    Tilde,
    /// Keep original fence character
    Preserve,
}

/// Attribute syntax preference for code blocks.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AttributeStyle {
    /// Use shortcut form: ```python or ```python {.numberLines}
    Shortcut,
    /// Use explicit form: ```{.python} or ```{.python .numberLines}
    Explicit,
    /// Keep original attribute syntax
    Preserve,
}

impl Default for CodeBlockConfig {
    fn default() -> Self {
        Self::for_flavor(Flavor::default())
    }
}

impl CodeBlockConfig {
    /// Get the default code block config for a given flavor.
    pub fn for_flavor(flavor: Flavor) -> Self {
        match flavor {
            Flavor::Quarto | Flavor::RMarkdown => Self {
                fence_style: FenceStyle::Backtick,
                attribute_style: AttributeStyle::Shortcut,
                min_fence_length: 3,
                normalize_indented: false,
            },
            Flavor::Pandoc => Self {
                fence_style: FenceStyle::Backtick, // Changed from Preserve to be consistent
                attribute_style: AttributeStyle::Preserve, // Changed from Explicit to Preserve
                min_fence_length: 3,
                normalize_indented: false,
            },
            Flavor::Gfm | Flavor::CommonMark => Self {
                fence_style: FenceStyle::Backtick,
                attribute_style: AttributeStyle::Preserve,
                min_fence_length: 3,
                normalize_indented: false,
            },
        }
    }
}

/// Configuration for an external code formatter.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct FormatterConfig {
    /// Command to execute (e.g., "black", "air", "rustfmt")
    pub cmd: String,
    /// Arguments to pass to the command (e.g., ["-", "--line-length=80"])
    #[serde(default)]
    pub args: Vec<String>,
    /// Whether this formatter is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Whether the formatter reads from stdin (true) or requires a file path (false)
    #[serde(default = "default_true")]
    pub stdin: bool,
}

fn default_true() -> bool {
    true
}

impl Default for FormatterConfig {
    fn default() -> Self {
        Self {
            cmd: String::new(),
            args: Vec::new(),
            enabled: true,
            stdin: true,
        }
    }
}

/// Style for formatting math delimiters
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum MathDelimiterStyle {
    /// Preserve original delimiter style (\(...\) stays \(...\), $...$ stays $...$)
    Preserve,
    /// Normalize all to dollar syntax ($...$ and $$...$$)
    #[default]
    Dollars,
    /// Normalize all to backslash syntax (\(...\) and \[...\])
    Backslash,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub flavor: Flavor,
    pub extensions: Extensions,
    pub line_ending: Option<LineEnding>,
    pub line_width: usize,
    pub math_indent: usize,
    /// Style for math delimiters (preserve, dollars, backslash)
    #[serde(rename = "math-delimiter-style")]
    pub math_delimiter_style: MathDelimiterStyle,
    pub wrap: Option<WrapMode>,
    pub blank_lines: BlankLines,
    /// Code block formatting configuration
    #[serde(rename = "code-blocks")]
    pub code_blocks: CodeBlockConfig,
    /// External code formatters keyed by language name (e.g., "r", "python")
    #[serde(default)]
    pub formatters: HashMap<String, FormatterConfig>,
}

impl Default for Config {
    fn default() -> Self {
        let flavor = Flavor::default();
        Self {
            flavor,
            extensions: Extensions::for_flavor(flavor),
            line_ending: Some(LineEnding::Auto),
            line_width: 80,
            math_indent: 0,
            math_delimiter_style: MathDelimiterStyle::default(),
            wrap: Some(WrapMode::Reflow),
            blank_lines: BlankLines::Collapse,
            code_blocks: CodeBlockConfig::for_flavor(flavor),
            formatters: HashMap::new(),
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

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum WrapMode {
    Preserve,
    Reflow,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum LineEnding {
    Auto,
    Lf,
    Crlf,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum BlankLines {
    /// Preserve original blank lines (any number)
    Preserve,
    /// Collapse multiple consecutive blank lines to a single blank line
    Collapse,
}

const CANDIDATE_NAMES: &[&str] = &[".panache.toml", "panache.toml"];

fn parse_config_str(s: &str, path: &Path) -> io::Result<Config> {
    let mut config: Config = toml::from_str(s).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid config {}: {e}", path.display()),
        )
    })?;

    // IMPORTANT: If no extensions were explicitly set in the TOML,
    // serde will have used Extensions::default() (Pandoc defaults).
    // We need to apply flavor-specific defaults when the user didn't
    // explicitly override them. Since we can't detect which fields
    // were set vs. defaulted by serde, we compare with Extensions::default()
    // and if they match, replace with flavor-specific defaults.
    if config.extensions == Extensions::default() {
        config.extensions = Extensions::for_flavor(config.flavor);
    }

    Ok(config)
}

fn read_config(path: &Path) -> io::Result<Config> {
    log::debug!("Reading config from: {}", path.display());
    let s = fs::read_to_string(path)?;
    let config = parse_config_str(&s, path)?;
    log::info!("Loaded config from: {}", path.display());
    Ok(config)
}

fn find_in_tree(start_dir: &Path) -> Option<PathBuf> {
    for dir in start_dir.ancestors() {
        for name in CANDIDATE_NAMES {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

fn xdg_config_path() -> Option<PathBuf> {
    if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
        let p = Path::new(&xdg).join("panache").join("config.toml");
        if p.is_file() {
            return Some(p);
        }
    }
    if let Ok(home) = env::var("HOME") {
        let p = Path::new(&home)
            .join(".config")
            .join("panache")
            .join("config.toml");
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// Load configuration with precedence:
/// 1) explicit path (error if unreadable/invalid)
/// 2) walk up from start_dir: .panache.toml, panache.toml
/// 3) XDG: $XDG_CONFIG_HOME/panache/config.toml or ~/.config/panache/config.toml
/// 4) default config
pub fn load(explicit: Option<&Path>, start_dir: &Path) -> io::Result<(Config, Option<PathBuf>)> {
    if let Some(path) = explicit {
        let cfg = read_config(path)?;
        return Ok((cfg, Some(path.to_path_buf())));
    }

    if let Some(p) = find_in_tree(start_dir)
        && let Ok(cfg) = read_config(&p)
    {
        return Ok((cfg, Some(p)));
    }

    if let Some(p) = xdg_config_path()
        && let Ok(cfg) = read_config(&p)
    {
        return Ok((cfg, Some(p)));
    }

    log::debug!("No config file found, using defaults");
    Ok((Config::default(), None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_uses_defaults() {
        let toml_str = r#"
            wrap = "reflow"
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();
        assert_eq!(cfg.line_width, 80);
        assert!(cfg.formatters.is_empty());
    }

    #[test]
    fn formatter_config_basic() {
        let toml_str = r#"
            [formatters.python]
            cmd = "black"
            args = ["-"]
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        let python_fmt = cfg.formatters.get("python").unwrap();
        assert_eq!(python_fmt.cmd, "black");
        assert_eq!(python_fmt.args, vec!["-"]);
        assert!(python_fmt.enabled);
    }

    #[test]
    fn formatter_config_multiple_languages() {
        let toml_str = r#"
            [formatters.r]
            cmd = "air"
            args = ["--preset=tidyverse"]
            
            [formatters.python]
            cmd = "black"
            args = ["-", "--line-length=88"]
            
            [formatters.rust]
            cmd = "rustfmt"
            enabled = false
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        assert_eq!(cfg.formatters.len(), 3);

        let r_fmt = cfg.formatters.get("r").unwrap();
        assert_eq!(r_fmt.cmd, "air");
        assert_eq!(r_fmt.args, vec!["--preset=tidyverse"]);
        assert!(r_fmt.enabled);

        let py_fmt = cfg.formatters.get("python").unwrap();
        assert_eq!(py_fmt.cmd, "black");
        assert_eq!(py_fmt.args.len(), 2);

        let rust_fmt = cfg.formatters.get("rust").unwrap();
        assert_eq!(rust_fmt.cmd, "rustfmt");
        assert!(!rust_fmt.enabled);
    }

    #[test]
    fn formatter_config_no_args() {
        let toml_str = r#"
            [formatters.rustfmt]
            cmd = "rustfmt"
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        let fmt = cfg.formatters.get("rustfmt").unwrap();
        assert_eq!(fmt.cmd, "rustfmt");
        assert!(fmt.args.is_empty());
        assert!(fmt.enabled);
    }

    #[test]
    fn formatter_empty_cmd_is_valid() {
        // Empty cmd is technically valid in deserialization
        // Validation happens at runtime
        let toml_str = r#"
            [formatters.test]
            cmd = ""
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();
        let fmt = cfg.formatters.get("test").unwrap();
        assert_eq!(fmt.cmd, "");
    }

    #[test]
    fn code_blocks_flavor_defaults() {
        // When explicitly creating configs programmatically, need to build properly
        // The Default trait uses Pandoc flavor
        let default_cfg = Config::default();
        assert_eq!(default_cfg.flavor, Flavor::Pandoc);
        assert_eq!(default_cfg.code_blocks.fence_style, FenceStyle::Backtick);
        assert_eq!(
            default_cfg.code_blocks.attribute_style,
            AttributeStyle::Preserve
        );

        // To get flavor-specific code block config, it needs to be set explicitly
        // or loaded via from_toml which handles the flavor field
        let toml_str = r#"
            flavor = "quarto"
        "#;
        let quarto_cfg = toml::from_str::<Config>(toml_str).unwrap();
        assert_eq!(quarto_cfg.flavor, Flavor::Quarto);
        // But code_blocks still uses Default::default() from serde
        // This is a known limitation - users must explicitly set code-blocks if needed
    }

    #[test]
    fn code_blocks_config_for_flavor() {
        // Test the for_flavor method directly
        let quarto_cb = CodeBlockConfig::for_flavor(Flavor::Quarto);
        assert_eq!(quarto_cb.fence_style, FenceStyle::Backtick);
        assert_eq!(quarto_cb.attribute_style, AttributeStyle::Shortcut);

        let pandoc_cb = CodeBlockConfig::for_flavor(Flavor::Pandoc);
        assert_eq!(pandoc_cb.fence_style, FenceStyle::Backtick);
        assert_eq!(pandoc_cb.attribute_style, AttributeStyle::Preserve);
    }

    #[test]
    fn code_blocks_from_toml() {
        let toml_str = r#"
            flavor = "quarto"
            
            [code-blocks]
            fence-style = "tilde"
            attribute-style = "explicit"
            min-fence-length = 4
            normalize-indented = true
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        assert_eq!(cfg.code_blocks.fence_style, FenceStyle::Tilde);
        assert_eq!(cfg.code_blocks.attribute_style, AttributeStyle::Explicit);
        assert_eq!(cfg.code_blocks.min_fence_length, 4);
        assert!(cfg.code_blocks.normalize_indented);
    }

    #[test]
    fn code_blocks_partial_override() {
        let toml_str = r#"
            flavor = "quarto"
            
            [code-blocks]
            fence-style = "preserve"
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        // Note: Due to serde defaults, partial override uses Default impl, not flavor defaults
        // This is expected behavior - users can explicitly set all values if needed
        assert_eq!(cfg.code_blocks.fence_style, FenceStyle::Preserve);
        // These come from CodeBlockConfig::default(), not flavor-specific
        assert_eq!(cfg.code_blocks.min_fence_length, 3);
        assert!(!cfg.code_blocks.normalize_indented);
    }
}
