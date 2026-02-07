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
    RMarkdown,
    /// GitHub Flavored Markdown
    Gfm,
    /// CommonMark (minimal standard extensions)
    CommonMark,
}

/// Pandoc/Markdown extensions configuration.
/// Each field represents a specific Pandoc extension.
/// Extensions marked with a comment indicate implementation status.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Extensions {
    // ===== Block-level extensions =====

    // Headings
    /// ✅ Require blank line before headers (default: enabled)
    pub blank_before_header: bool,
    /// 🚧 Full attribute syntax on headers {#id .class key=value}
    pub header_attributes: bool,
    /// ❌ Auto-generate reference links for headers
    pub implicit_header_references: bool,

    // Block quotes
    /// ✅ Require blank line before blockquotes (default: enabled)
    pub blank_before_blockquote: bool,

    // Lists
    /// ❌ Fancy list markers (roman numerals, letters, etc.)
    pub fancy_lists: bool,
    /// ❌ Start ordered lists at arbitrary numbers
    pub startnum: bool,
    /// ❌ Example lists with (@) markers
    pub example_lists: bool,
    /// ❌ GitHub-style task lists - [ ] and - [x]
    pub task_lists: bool,
    /// ❌ Term/definition syntax
    pub definition_lists: bool,

    // Code blocks
    /// ✅ Fenced code blocks with backticks
    pub backtick_code_blocks: bool,
    /// ✅ Fenced code blocks with tildes
    pub fenced_code_blocks: bool,
    /// ✅ Attributes on fenced code blocks {.language #id}
    pub fenced_code_attributes: bool,
    /// ❌ Attributes on inline code
    pub inline_code_attributes: bool,

    // Tables
    /// ❌ Simple table syntax
    pub simple_tables: bool,
    /// ❌ Multiline cell content in tables
    pub multiline_tables: bool,
    /// ❌ Grid-style tables
    pub grid_tables: bool,
    /// ❌ Pipe tables (GitHub/PHP Markdown style)
    pub pipe_tables: bool,
    /// ❌ Table captions
    pub table_captions: bool,

    // Divs
    /// ✅ Fenced divs ::: {.class}
    pub fenced_divs: bool,
    /// ❌ HTML <div> elements
    pub native_divs: bool,

    // Other block elements
    /// ❌ Line blocks for poetry | prefix
    pub line_blocks: bool,

    // ===== Inline elements =====

    // Emphasis
    /// ✅ Underscores don't trigger emphasis in snake_case
    pub intraword_underscores: bool,
    /// ❌ Strikethrough ~~text~~
    pub strikeout: bool,
    /// ❌ Superscript and subscript ^super^ ~sub~
    pub superscript: bool,
    pub subscript: bool,

    // Links
    /// ✅ Inline links [text](url)
    pub inline_links: bool,
    /// ❌ Reference links [text][ref]
    pub reference_links: bool,
    /// ❌ Shortcut reference links [ref] without second []
    pub shortcut_reference_links: bool,
    /// ❌ Attributes on links [text](url){.class}
    pub link_attributes: bool,
    /// ✅ Automatic links <http://example.com>
    pub autolinks: bool,

    // Images
    /// ✅ Inline images ![alt](url)
    pub inline_images: bool,
    /// ❌ Paragraph with just image becomes figure
    pub implicit_figures: bool,

    // Math
    /// ✅ Dollar-delimited math $x$ and $$equation$$
    pub tex_math_dollars: bool,

    // Footnotes
    /// ❌ Inline footnotes ^[text]
    pub inline_footnotes: bool,
    /// ❌ Reference footnotes `[^1]` (requires footnote parsing)
    pub footnotes: bool,

    // Citations
    /// ❌ Citation syntax [@cite]
    pub citations: bool,

    // Spans
    /// ❌ Bracketed spans [text]{.class}
    pub bracketed_spans: bool,
    /// ❌ HTML <span> elements
    pub native_spans: bool,

    // ===== Metadata =====
    /// ✅ YAML metadata block
    pub yaml_metadata_block: bool,
    /// ✅ Pandoc title block (Title/Author/Date)
    pub pandoc_title_block: bool,

    // ===== Raw content =====
    /// ❌ Raw HTML blocks and inline
    pub raw_html: bool,
    /// ❌ Markdown inside HTML blocks
    pub markdown_in_html_blocks: bool,
    /// ❌ LaTeX commands and environments
    pub raw_tex: bool,

    // ===== Escapes and special characters =====
    /// ✅ Backslash escapes any symbol
    pub all_symbols_escapable: bool,
    /// ✅ Backslash at line end = hard line break
    pub escaped_line_breaks: bool,

    // ===== NON-DEFAULT EXTENSIONS =====
    // These are disabled by default in Pandoc
    /// ❌ [NON-DEFAULT] Bare URLs become links
    pub autolink_bare_uris: bool,
    /// ❌ [NON-DEFAULT] Newline = <br>
    pub hard_line_breaks: bool,
    /// ❌ [NON-DEFAULT] :emoji: syntax
    pub emoji: bool,
    /// ❌ [NON-DEFAULT] Highlighted ==text==
    pub mark: bool,

    // ===== Quarto-specific extensions =====
    /// ❌ Quarto callout blocks (.callout-note, etc.)
    pub quarto_callouts: bool,
    /// ❌ Quarto cross-references @fig-id, @tbl-id
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
            implicit_header_references: false,

            // Lists
            fancy_lists: false,
            startnum: true,
            example_lists: false,
            task_lists: false,
            definition_lists: true,

            // Code
            backtick_code_blocks: true,
            fenced_code_blocks: true,
            fenced_code_attributes: true,
            inline_code_attributes: false,

            // Tables
            simple_tables: true,
            multiline_tables: true,
            grid_tables: true,
            pipe_tables: true,
            table_captions: true,

            // Divs
            fenced_divs: true,
            native_divs: false,

            // Other blocks
            line_blocks: false,

            // Inline
            intraword_underscores: true,
            strikeout: false,
            superscript: false,
            subscript: false,

            // Links
            inline_links: true,
            reference_links: true,
            shortcut_reference_links: false,
            link_attributes: false,
            autolinks: true,

            // Images
            inline_images: true,
            implicit_figures: false,

            // Math
            tex_math_dollars: true,

            // Footnotes
            inline_footnotes: false,
            footnotes: true,

            // Citations
            citations: false,

            // Spans
            bracketed_spans: false,
            native_spans: false,

            // Metadata
            yaml_metadata_block: true,
            pandoc_title_block: true,

            // Raw
            raw_html: true,
            markdown_in_html_blocks: false,
            raw_tex: false,

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
            implicit_header_references: false,

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

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub flavor: Flavor,
    pub extensions: Extensions,
    pub line_ending: Option<LineEnding>,
    pub line_width: usize,
    pub math_indent: usize,
    pub wrap: Option<WrapMode>,
    pub blank_lines: BlankLines,
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
            wrap: Some(WrapMode::Reflow),
            blank_lines: BlankLines::Collapse,
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
    toml::from_str::<Config>(s).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid config {}: {e}", path.display()),
        )
    })
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

#[test]
fn missing_fields_panics_on_unwrap() {
    let toml_str = r#"
        wrap = "reflow"
    "#;
    let cfg = toml::from_str::<Config>(toml_str).unwrap();
    let line_width = cfg.line_width; // This will panic and fail the test
    assert_eq!(line_width, 80);
}
