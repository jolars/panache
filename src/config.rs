use std::collections::HashMap;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize};

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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    /// Quarto shortcodes {{< name args >}}
    pub quarto_shortcodes: bool,
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
            quarto_shortcodes: false,
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
        ext.quarto_shortcodes = true;

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
            quarto_shortcodes: false,
        }
    }

    /// Merge user-specified extension overrides with flavor defaults.
    ///
    /// This is used to support partial extension overrides in config files.
    /// For example, if a user specifies `flavor = "quarto"` and then sets
    /// `[extensions] quarto_crossrefs = false`, we want all other extensions
    /// to use Quarto defaults, not Pandoc defaults.
    ///
    /// # Arguments
    /// * `user_overrides` - Map of extension names to their user-specified values
    /// * `flavor` - The flavor to use for default values
    ///
    /// # Returns
    /// A new Extensions struct with flavor defaults merged with user overrides
    pub fn merge_with_flavor(user_overrides: HashMap<String, bool>, flavor: Flavor) -> Self {
        use serde_json::{Map, Value};

        // Start with flavor defaults
        let defaults = Self::for_flavor(flavor);
        let defaults_value =
            serde_json::to_value(&defaults).expect("Failed to serialize flavor defaults");

        let mut merged = if let Value::Object(obj) = defaults_value {
            obj
        } else {
            Map::new()
        };

        // Apply user overrides
        for (key, value) in user_overrides {
            merged.insert(key, Value::Bool(value));
        }

        // Deserialize back to Extensions
        serde_json::from_value(Value::Object(merged))
            .expect("Failed to deserialize merged extensions")
    }
}

/// Configuration for code block formatting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
#[serde(rename_all = "kebab-case")]
pub struct CodeBlockConfig {
    /// Fence style: "backtick", "tilde", or "preserve"
    pub fence_style: FenceStyle,
    /// Attribute style: "shortcut", "explicit", or "preserve"
    pub attribute_style: AttributeStyle,
    /// Minimum fence length (default: 3)
    pub min_fence_length: usize,
    /// Whether to normalize indented code blocks to fenced blocks (risky, default: false)
    pub normalize_indented: bool,
}

/// Fence character preference for code blocks.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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
        Self {
            fence_style: FenceStyle::Backtick,
            attribute_style: AttributeStyle::Shortcut,
            min_fence_length: 3,
            normalize_indented: false,
        }
    }
}

/// Configuration for an external code formatter.
#[derive(Debug, Clone, PartialEq)]
pub struct FormatterConfig {
    /// Command to execute (e.g., "black", "air", "rustfmt")
    pub cmd: String,
    /// Arguments to pass to the command (e.g., ["-", "--line-length=80"])
    pub args: Vec<String>,
    /// Whether this formatter is enabled (deprecated, kept for backwards compatibility)
    pub enabled: bool,
    /// Whether the formatter reads from stdin (true) or requires a file path (false)
    pub stdin: bool,
}

/// NEW: Language → Formatter mapping value (single formatter or chain)
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum FormatterValue {
    /// Single formatter: r = "air"
    Single(String),
    /// Multiple formatters (sequential): python = ["isort", "black"]
    Multiple(Vec<String>),
}

/// NEW: Named formatter definition (formatters.NAME sections in new format)
/// OLD: Language-specific formatter config (formatters.LANG sections in old format)
///
/// In new format, if the definition name matches a built-in preset, unspecified fields
/// will inherit from that preset. This allows partial overrides like:
///
/// ```toml
/// [formatters.air]
/// args = ["format", "--custom"]  # Overrides args, inherits cmd/stdin from built-in "air"
/// ```
///
/// Additionally, you can modify arguments incrementally using `prepend_args` and `append_args`:
///
/// ```toml
/// [formatters.air]
/// append_args = ["-i", "2"]  # Adds args to end: ["format", "{}", "-i", "2"]
/// ```
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct FormatterDefinition {
    /// Reference to a built-in preset (e.g., "air", "black") - OLD FORMAT ONLY
    /// In new format, presets are referenced directly in [formatters] mapping
    pub preset: Option<String>,
    /// Custom command to execute (None = inherit from preset if name matches)
    pub cmd: Option<String>,
    /// Arguments to pass (None = inherit from preset if name matches)
    pub args: Option<Vec<String>>,
    /// Arguments to prepend to base args (from preset or explicit args)
    pub prepend_args: Option<Vec<String>>,
    /// Arguments to append to base args (from preset or explicit args)
    pub append_args: Option<Vec<String>>,
    /// Whether the formatter reads from stdin (None = inherit from preset if name matches)
    pub stdin: Option<bool>,
    /// DEPRECATED: Whether formatter is enabled (old format only)
    pub enabled: Option<bool>,
}

/// Internal struct for deserializing FormatterConfig with preset support.
#[derive(Debug, Deserialize)]
#[serde(default)]
struct RawFormatterConfig {
    /// Preset name (e.g., "air", "ruff") - mutually exclusive with cmd
    preset: Option<String>,
    /// Command to execute
    cmd: Option<String>,
    /// Arguments to pass to the command
    args: Option<Vec<String>>,
    /// Whether this formatter is enabled
    enabled: bool,
    /// Whether the formatter reads from stdin
    stdin: bool,
}

impl Default for RawFormatterConfig {
    fn default() -> Self {
        Self {
            preset: None,
            cmd: None,
            args: None,
            enabled: true,
            stdin: true,
        }
    }
}

impl<'de> Deserialize<'de> for FormatterConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawFormatterConfig::deserialize(deserializer)?;

        // Check mutual exclusivity of preset and cmd
        if raw.preset.is_some() && raw.cmd.is_some() {
            return Err(serde::de::Error::custom(
                "FormatterConfig: 'preset' and 'cmd' are mutually exclusive - use one or the other",
            ));
        }

        // If preset is specified, resolve it
        if let Some(preset_name) = raw.preset {
            let preset = get_formatter_preset(&preset_name).ok_or_else(|| {
                serde::de::Error::custom(format!(
                    "Unknown formatter preset: '{}'. Available presets: air, styler, ruff, black",
                    preset_name
                ))
            })?;

            // Return the preset, but respect enabled field if explicitly set
            Ok(FormatterConfig {
                cmd: preset.cmd,
                args: preset.args,
                enabled: raw.enabled,
                stdin: preset.stdin,
            })
        } else if let Some(cmd) = raw.cmd {
            // Custom configuration
            Ok(FormatterConfig {
                cmd,
                args: raw.args.unwrap_or_default(),
                enabled: raw.enabled,
                stdin: raw.stdin,
            })
        } else {
            // No preset and no cmd - return empty config
            // This can happen with Default::default()
            Ok(FormatterConfig {
                cmd: String::new(),
                args: raw.args.unwrap_or_default(),
                enabled: raw.enabled,
                stdin: raw.stdin,
            })
        }
    }
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

/// Get a built-in formatter preset by name.
/// Returns None if the preset doesn't exist.
pub fn get_formatter_preset(name: &str) -> Option<FormatterConfig> {
    match name {
        // YAML formatters
        "yamlfmt" => Some(FormatterConfig {
            cmd: "yamlfmt".to_string(),
            args: vec!["-".to_string()],
            enabled: true,
            stdin: true,
        }),
        "prettier" => Some(FormatterConfig {
            cmd: "prettier".to_string(),
            args: vec!["--parser".to_string(), "yaml".to_string()],
            enabled: true,
            stdin: true,
        }),
        // R formatters
        "air" => Some(FormatterConfig {
            cmd: "air".to_string(),
            args: vec!["format".to_string(), "{}".to_string()],
            enabled: true,
            stdin: false,
        }),
        "styler" => Some(FormatterConfig {
            cmd: "Rscript".to_string(),
            args: vec!["-e".to_string(), "styler::style_file('{}')".to_string()],
            enabled: true,
            stdin: false,
        }),

        // Python formatters
        "ruff" => Some(FormatterConfig {
            cmd: "ruff".to_string(),
            args: vec!["format".to_string()],
            enabled: true,
            stdin: true,
        }),
        "black" => Some(FormatterConfig {
            cmd: "black".to_string(),
            args: vec!["-".to_string()],
            enabled: true,
            stdin: true,
        }),

        _ => None,
    }
}

/// Get the default formatters HashMap with built-in presets.
/// Currently includes R (air) and Python (ruff).
pub fn default_formatters() -> HashMap<String, FormatterConfig> {
    let mut map = HashMap::new();
    map.insert("r".to_string(), get_formatter_preset("air").unwrap());
    map.insert("python".to_string(), get_formatter_preset("ruff").unwrap());
    map
}

/// Style for formatting math delimiters
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum MathDelimiterStyle {
    /// Preserve original delimiter style (\(...\) stays \(...\), $...$ stays $...$)
    #[default]
    Preserve,
    /// Normalize all to dollar syntax ($...$ and $$...$$)
    Dollars,
    /// Normalize all to backslash syntax (\(...\) and \[...\])
    Backslash,
}

/// Formatting style configuration.
/// Groups all style-related settings together.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
#[serde(rename_all = "kebab-case")]
pub struct StyleConfig {
    /// Text wrapping mode
    pub wrap: Option<WrapMode>,
    /// Blank line handling between blocks
    pub blank_lines: BlankLines,
    /// Math delimiter style preference
    pub math_delimiter_style: MathDelimiterStyle,
    /// Math indentation (spaces)
    pub math_indent: usize,
    /// Code block formatting preferences
    pub code_blocks: Option<CodeBlockConfig>,
}

impl Default for StyleConfig {
    fn default() -> Self {
        Self {
            wrap: Some(WrapMode::Reflow),
            blank_lines: BlankLines::Collapse,
            math_delimiter_style: MathDelimiterStyle::default(),
            math_indent: 0,
            code_blocks: None,
        }
    }
}

impl StyleConfig {
    // No flavor-specific defaults needed - just use field defaults
}

/// Internal deserialization struct that allows for optional fields
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct RawConfig {
    #[serde(default)]
    flavor: Flavor,
    #[serde(default)]
    extensions: Option<HashMap<String, bool>>,
    #[serde(default)]
    line_ending: Option<LineEnding>,
    #[serde(default = "default_line_width")]
    line_width: usize,

    // New preferred style section
    #[serde(default)]
    style: Option<StyleConfig>,

    // DEPRECATED: Old top-level style fields (kept for backwards compatibility)
    #[serde(default)]
    math_indent: usize,
    #[serde(default)]
    math_delimiter_style: MathDelimiterStyle,
    #[serde(default)]
    wrap: Option<WrapMode>,
    #[serde(default = "default_blank_lines")]
    blank_lines: BlankLines,
    #[serde(default)]
    code_blocks: Option<CodeBlockConfig>,

    // NEW: Language → Formatter(s) mapping
    // This will be a raw Value that we'll parse manually to handle both formats
    #[serde(default)]
    formatters: Option<toml::Value>,

    #[serde(default)]
    linters: HashMap<String, String>,
}

fn default_line_width() -> usize {
    80
}

fn default_blank_lines() -> BlankLines {
    BlankLines::Collapse
}

/// Resolve a single formatter name to a FormatterConfig.
///
/// Resolve a formatter name to a FormatterConfig.
///
/// Resolution order:
/// 1. Check if it's a named definition in formatter_definitions
///    - If name matches a built-in preset, inherit unspecified fields from preset
///    - If name doesn't match preset, require full cmd specification
/// 2. Fall back to built-in preset (no custom definition)
/// 3. Error if neither found
///
/// # Examples
///
/// ```toml
/// # Partial override - inherits cmd/stdin from built-in "air"
/// [formatters.air]
/// args = ["format", "--custom"]
///
/// # Append args to preset - final: ["format", "{}", "-i", "2"]
/// [formatters.air]
/// append_args = ["-i", "2"]
///
/// # Full custom - no preset match, requires cmd
/// [formatters.custom-fmt]
/// cmd = "my-formatter"
/// args = ["--flag"]
/// ```
fn resolve_formatter_name(
    name: &str,
    formatter_definitions: &HashMap<String, FormatterDefinition>,
) -> Result<FormatterConfig, String> {
    // Check for named definition first
    if let Some(definition) = formatter_definitions.get(name) {
        // Named definition exists - resolve it

        // NEW FORMAT: preset field not allowed in named definitions
        // (Use direct preset reference in [formatters] mapping instead)
        if definition.preset.is_some() {
            return Err(format!(
                "Formatter '{}': 'preset' field not allowed in named definitions. Use [formatters] mapping instead (e.g., `lang = \"{}\"`).",
                name, name
            ));
        }

        // Try to load built-in preset as base (if name matches)
        let preset = get_formatter_preset(name);

        // Build config by applying overrides to preset (or requiring cmd if no preset)
        match (preset, &definition.cmd) {
            // Case 1: Preset exists - use as base and apply overrides
            (Some(mut base_config), _) => {
                // Override cmd if specified
                if let Some(cmd) = &definition.cmd {
                    base_config.cmd = cmd.clone();
                }
                // Override args if specified
                if let Some(args) = &definition.args {
                    base_config.args = args.clone();
                }
                // Override stdin if specified
                if let Some(stdin) = definition.stdin {
                    base_config.stdin = stdin;
                }

                // Apply prepend_args and append_args modifiers
                apply_arg_modifiers(&mut base_config.args, definition);

                Ok(base_config)
            }
            // Case 2: No preset, but cmd specified - full custom formatter
            (None, Some(cmd)) => {
                let mut args = definition.args.clone().unwrap_or_default();

                // Apply prepend_args and append_args modifiers
                apply_arg_modifiers(&mut args, definition);

                Ok(FormatterConfig {
                    cmd: cmd.clone(),
                    args,
                    enabled: true,
                    stdin: definition.stdin.unwrap_or(true),
                })
            }
            // Case 3: No preset, no cmd - error
            (None, None) => Err(format!(
                "Formatter '{}': must specify 'cmd' field (not a known preset)",
                name
            )),
        }
    } else {
        // Not a named definition - check built-in presets
        get_formatter_preset(name).ok_or_else(|| {
            format!(
                "Unknown formatter '{}': not a named definition or built-in preset. \
                 Define it in [formatters.{}] section or use a known preset.",
                name, name
            )
        })
    }
}

/// Apply prepend_args and append_args modifiers to an argument list.
///
/// Modifiers are applied in order: prepend_args + base_args + append_args
/// If no base args exist, they're treated as empty (user responsibility).
fn apply_arg_modifiers(args: &mut Vec<String>, definition: &FormatterDefinition) {
    // Prepend args if specified
    if let Some(prepend) = &definition.prepend_args {
        let mut new_args = prepend.clone();
        new_args.append(args);
        *args = new_args;
    }

    // Append args if specified
    if let Some(append) = &definition.append_args {
        args.extend_from_slice(append);
    }
}

/// Resolve a language's formatter value (single or multiple) to a list of FormatterConfigs.
fn resolve_language_formatters(
    lang: &str,
    value: &FormatterValue,
    formatter_definitions: &HashMap<String, FormatterDefinition>,
) -> Result<Vec<FormatterConfig>, String> {
    let formatter_names = match value {
        FormatterValue::Single(name) => vec![name.as_str()],
        FormatterValue::Multiple(names) => names.iter().map(|s| s.as_str()).collect(),
    };

    // Resolve each formatter name
    formatter_names
        .into_iter()
        .map(|name| {
            resolve_formatter_name(name, formatter_definitions)
                .map_err(|e| format!("Language '{}': {}", lang, e))
        })
        .collect()
}

impl RawConfig {
    /// Finalize into Config, applying flavor-based defaults where needed
    fn finalize(self) -> Config {
        // Check for deprecated top-level style fields
        let has_deprecated_fields = self.wrap.is_some()
            || self.code_blocks.is_some()
            || self.math_indent != 0
            || self.math_delimiter_style != MathDelimiterStyle::default()
            || self.blank_lines != default_blank_lines();

        if has_deprecated_fields && self.style.is_none() {
            eprintln!(
                "Warning: top-level style fields (wrap, code-blocks, math-indent, etc.) \
                 are deprecated. Please move them under [style] section. \
                 See documentation for the new format."
            );
        }

        // Merge style config: prefer new [style] section, fall back to old fields
        let style = if let Some(mut style_config) = self.style {
            // New [style] section exists - use it, but warn if both formats present
            if has_deprecated_fields {
                eprintln!(
                    "Warning: Both [style] section and top-level style fields found. \
                     Using [style] section and ignoring top-level fields."
                );
            }

            // Fill in missing fields with defaults
            if style_config.code_blocks.is_none() {
                style_config.code_blocks = Some(CodeBlockConfig::default());
            }

            style_config
        } else {
            // Old format - construct StyleConfig from top-level fields
            let code_blocks = self.code_blocks.unwrap_or_default();

            StyleConfig {
                wrap: self.wrap.or(Some(WrapMode::Reflow)),
                blank_lines: self.blank_lines,
                math_delimiter_style: self.math_delimiter_style,
                math_indent: self.math_indent,
                code_blocks: Some(code_blocks),
            }
        };

        Config {
            extensions: self.extensions.map_or_else(
                || Extensions::for_flavor(self.flavor),
                |user_overrides| Extensions::merge_with_flavor(user_overrides, self.flavor),
            ),
            line_ending: self.line_ending.or(Some(LineEnding::Auto)),
            flavor: self.flavor,
            line_width: self.line_width,
            wrap: style.wrap,
            blank_lines: style.blank_lines,
            math_delimiter_style: style.math_delimiter_style,
            math_indent: style.math_indent,
            code_blocks: style.code_blocks.unwrap_or_default(),
            formatters: resolve_formatters(self.formatters),
            linters: self.linters,
        }
    }
}

/// Resolve formatter configuration from both old and new formats.
/// Returns HashMap<String, Vec<FormatterConfig>> for language → formatter(s) mapping.
fn resolve_formatters(
    raw_formatters: Option<toml::Value>,
) -> HashMap<String, Vec<FormatterConfig>> {
    let Some(value) = raw_formatters else {
        return HashMap::new();
    };

    // Try to determine which format this is
    let toml::Value::Table(table) = value else {
        eprintln!("Warning: Invalid formatters configuration - expected table");
        return HashMap::new();
    };

    // Strategy: Detect old format vs new format
    // Old format: ALL entries are tables with preset/cmd/args (language-specific configs)
    // New format: Mix of strings/arrays (language mappings) and optionally tables (named definitions)

    let has_string_or_array = table
        .values()
        .any(|v| matches!(v, toml::Value::String(_) | toml::Value::Array(_)));

    if has_string_or_array {
        // New format detected (has language mappings as strings/arrays)
        resolve_new_format_formatters(table)
    } else {
        // Old format (all entries are tables)
        resolve_old_format_formatters(table)
    }
}

/// Resolve new format: [formatters] = { r = "air", python = ["isort", "black"] }
/// Plus optional [formatters.air] and [formatters.isort] definitions.
fn resolve_new_format_formatters(
    table: toml::map::Map<String, toml::Value>,
) -> HashMap<String, Vec<FormatterConfig>> {
    let mut mappings = HashMap::new();
    let mut definitions = HashMap::new();

    // First pass: separate mappings from definitions
    for (key, value) in table {
        match &value {
            toml::Value::String(_) | toml::Value::Array(_) => {
                // This is a language mapping
                let formatter_value: Result<FormatterValue, _> = value.try_into();
                match formatter_value {
                    Ok(fv) => {
                        mappings.insert(key, fv);
                    }
                    Err(e) => {
                        eprintln!("Error parsing formatter value for '{}': {}", key, e);
                    }
                }
            }
            toml::Value::Table(_) => {
                // This is a named formatter definition
                let definition: Result<FormatterDefinition, _> = value.try_into();
                match definition {
                    Ok(def) => {
                        definitions.insert(key, def);
                    }
                    Err(e) => {
                        eprintln!("Error parsing formatter definition '{}': {}", key, e);
                    }
                }
            }
            _ => {
                eprintln!(
                    "Warning: Invalid formatter entry '{}' - must be string, array, or table",
                    key
                );
            }
        }
    }

    // Second pass: resolve mappings using definitions
    let mut resolved = HashMap::new();
    for (lang, value) in mappings {
        match resolve_language_formatters(&lang, &value, &definitions) {
            Ok(configs) if !configs.is_empty() => {
                resolved.insert(lang, configs);
            }
            Ok(_) => {} // Empty list
            Err(e) => {
                eprintln!("Error resolving formatters for language '{}': {}", lang, e);
                eprintln!("Skipping formatter for '{}'", lang);
            }
        }
    }

    resolved
}

/// Resolve old format: [formatters.r] with preset/cmd fields directly.
fn resolve_old_format_formatters(
    table: toml::map::Map<String, toml::Value>,
) -> HashMap<String, Vec<FormatterConfig>> {
    eprintln!(
        "Warning: Old formatter configuration format detected. \
         Please migrate to the new format with [formatters] section. \
         See documentation for the new format."
    );

    let mut resolved = HashMap::new();
    for (lang, value) in table {
        let definition: Result<FormatterDefinition, _> = value.try_into();
        match definition {
            Ok(def) => {
                // Skip if disabled (old format only)
                // enabled is Option<bool> now, so check for Some(false)
                if def.enabled == Some(false) {
                    continue;
                }

                match resolve_old_format_definition(&lang, &def) {
                    Ok(config) => {
                        resolved.insert(lang, vec![config]);
                    }
                    Err(e) => {
                        eprintln!("Error in old formatter config for '{}': {}", lang, e);
                        eprintln!("Skipping formatter for '{}'", lang);
                    }
                }
            }
            Err(e) => {
                eprintln!("Error parsing old formatter config for '{}': {}", lang, e);
            }
        }
    }

    resolved
}

/// Resolve old-format formatter definition (inline preset/cmd in formatters.LANG).
fn resolve_old_format_definition(
    _lang: &str,
    definition: &FormatterDefinition,
) -> Result<FormatterConfig, String> {
    // Check for conflicts
    if definition.preset.is_some() && definition.cmd.is_some() {
        return Err("'preset' and 'cmd' are mutually exclusive".to_string());
    }

    if let Some(preset_name) = &definition.preset {
        // Resolve preset
        let preset = get_formatter_preset(preset_name)
            .ok_or_else(|| format!("Unknown formatter preset '{}'", preset_name))?;

        Ok(FormatterConfig {
            cmd: preset.cmd,
            args: definition.args.clone().unwrap_or(preset.args),
            enabled: true, // enabled field checked by caller
            stdin: preset.stdin,
        })
    } else if let Some(cmd) = &definition.cmd {
        // Custom command
        Ok(FormatterConfig {
            cmd: cmd.clone(),
            args: definition.args.clone().unwrap_or_default(),
            enabled: true,
            stdin: definition.stdin.unwrap_or(true),
        })
    } else {
        Err("must specify either 'preset' or 'cmd'".to_string())
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub flavor: Flavor,
    pub extensions: Extensions,
    pub line_ending: Option<LineEnding>,
    pub line_width: usize,
    pub math_indent: usize,
    pub math_delimiter_style: MathDelimiterStyle,
    pub wrap: Option<WrapMode>,
    pub blank_lines: BlankLines,
    pub code_blocks: CodeBlockConfig,
    /// Language → Formatter(s) mapping (supports multiple formatters per language)
    pub formatters: HashMap<String, Vec<FormatterConfig>>,
    pub linters: HashMap<String, String>,
}

impl<'de> Deserialize<'de> for Config {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawConfig::deserialize(deserializer).map(|raw| raw.finalize())
    }
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
            code_blocks: CodeBlockConfig::default(),
            formatters: HashMap::new(), // Opt-in: empty by default
            linters: HashMap::new(),    // Opt-in: empty by default
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
///
/// Flavor detection logic (when input_file is provided):
/// - .qmd files: Always use Quarto flavor
/// - .Rmd files: Always use RMarkdown flavor
/// - .md files: Use `flavor` from config (defaults to Pandoc)
/// - Other extensions: Use `flavor` from config
///
/// The `flavor` config field determines the default flavor for .md files and stdin.
pub fn load(
    explicit: Option<&Path>,
    start_dir: &Path,
    input_file: Option<&Path>,
) -> io::Result<(Config, Option<PathBuf>)> {
    let (mut cfg, cfg_path) = if let Some(path) = explicit {
        let cfg = read_config(path)?;
        (cfg, Some(path.to_path_buf()))
    } else if let Some(p) = find_in_tree(start_dir)
        && let Ok(cfg) = read_config(&p)
    {
        (cfg, Some(p))
    } else if let Some(p) = xdg_config_path()
        && let Ok(cfg) = read_config(&p)
    {
        (cfg, Some(p))
    } else {
        log::debug!("No config file found, using defaults");
        (Config::default(), None)
    };

    // Detect flavor from file extension
    if let Some(input_path) = input_file
        && let Some(ext) = input_path.extension().and_then(|e| e.to_str())
    {
        let detected_flavor = match ext.to_lowercase().as_str() {
            "qmd" => {
                log::debug!("Using Quarto flavor for .qmd file");
                Some(Flavor::Quarto)
            }
            "rmd" => {
                log::debug!("Using RMarkdown flavor for .Rmd file");
                Some(Flavor::RMarkdown)
            }
            "md" => {
                // For .md files, use the flavor from config
                log::debug!("Using {:?} flavor for .md file (from config)", cfg.flavor);
                Some(cfg.flavor)
            }
            _ => None,
        };

        if let Some(flavor) = detected_flavor {
            cfg.flavor = flavor;
            // Update extensions to match the detected flavor
            cfg.extensions = Extensions::for_flavor(flavor);
        }
    }

    Ok((cfg, cfg_path))
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
        // Formatters are opt-in, so empty by default
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

        let python_fmt = &cfg.formatters.get("python").unwrap()[0];
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

        // Old format detected - should have 2 formatters (rust disabled)
        assert_eq!(cfg.formatters.len(), 2);

        let r_fmt = &cfg.formatters.get("r").unwrap()[0];
        assert_eq!(r_fmt.cmd, "air");
        assert_eq!(r_fmt.args, vec!["--preset=tidyverse"]);
        assert!(r_fmt.enabled);

        let py_fmt = &cfg.formatters.get("python").unwrap()[0];
        assert_eq!(py_fmt.cmd, "black");
        assert_eq!(py_fmt.args.len(), 2);

        // rust is disabled in old format, so it shouldn't be in the map
        assert!(!cfg.formatters.contains_key("rust"));
    }

    #[test]
    fn formatter_config_no_args() {
        let toml_str = r#"
            [formatters.rustfmt]
            cmd = "rustfmt"
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        let fmt = &cfg.formatters.get("rustfmt").unwrap()[0];
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
        let fmt = &cfg.formatters.get("test").unwrap()[0];
        assert_eq!(fmt.cmd, "");
    }

    #[test]
    fn preset_resolution_air() {
        let toml_str = r#"
            [formatters.r]
            preset = "air"
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();
        let r_fmt = &cfg.formatters.get("r").unwrap()[0];
        assert_eq!(r_fmt.cmd, "air");
        assert_eq!(r_fmt.args, vec!["format", "{}"]);
        assert!(!r_fmt.stdin);
        assert!(r_fmt.enabled);
    }

    #[test]
    fn preset_resolution_ruff() {
        let toml_str = r#"
            [formatters.python]
            preset = "ruff"
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();
        let py_fmt = &cfg.formatters.get("python").unwrap()[0];
        assert_eq!(py_fmt.cmd, "ruff");
        assert_eq!(py_fmt.args, vec!["format"]);
        assert!(py_fmt.stdin);
        assert!(py_fmt.enabled);
    }

    #[test]
    fn preset_resolution_black() {
        let toml_str = r#"
            [formatters.python]
            preset = "black"
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();
        let py_fmt = &cfg.formatters.get("python").unwrap()[0];
        assert_eq!(py_fmt.cmd, "black");
        assert_eq!(py_fmt.args, vec!["-"]);
        assert!(py_fmt.stdin);
    }

    #[test]
    fn preset_and_cmd_mutually_exclusive() {
        let toml_str = r#"
            [formatters.r]
            preset = "air"
            cmd = "styler"
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();
        // The formatter should be skipped (error logged), so r shouldn't be in the map
        assert!(!cfg.formatters.contains_key("r"));
    }

    #[test]
    fn unknown_preset_fails() {
        let toml_str = r#"
            [formatters.r]
            preset = "nonexistent"
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();
        // The formatter should be skipped (error logged), so r shouldn't be in the map
        assert!(!cfg.formatters.contains_key("r"));
    }

    #[test]
    fn builtin_defaults_when_no_config() {
        let cfg = Config::default();
        // Formatters are opt-in, so empty by default
        assert!(cfg.formatters.is_empty());
    }

    #[test]
    fn user_config_adds_formatters() {
        let toml_str = r#"
            [formatters.r]
            cmd = "custom"
            args = ["--flag"]
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        // Only R should be configured
        assert_eq!(cfg.formatters.len(), 1);
        let r_fmt = &cfg.formatters.get("r").unwrap()[0];
        assert_eq!(r_fmt.cmd, "custom");
        assert_eq!(r_fmt.args, vec!["--flag"]);
    }

    #[test]
    fn empty_formatters_section_stays_empty() {
        let toml_str = r#"
            line_width = 100
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        // Formatters are opt-in, should be empty
        assert!(cfg.formatters.is_empty());
    }

    #[test]
    fn preset_with_enabled_false() {
        let toml_str = r#"
            [formatters.r]
            preset = "air"
            enabled = false
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();
        // Old format with enabled=false should not include the formatter
        assert!(!cfg.formatters.contains_key("r"));
    }

    #[test]
    fn code_blocks_flavor_defaults() {
        // CodeBlockConfig no longer varies by flavor - it has simple defaults
        let default_cfg = Config::default();
        assert_eq!(default_cfg.flavor, Flavor::Pandoc);
        assert_eq!(default_cfg.code_blocks.fence_style, FenceStyle::Backtick);
        assert_eq!(
            default_cfg.code_blocks.attribute_style,
            AttributeStyle::Shortcut
        );

        // All flavors get the same code block defaults
        let toml_str = r#"
            flavor = "quarto"
        "#;
        let quarto_cfg = toml::from_str::<Config>(toml_str).unwrap();
        assert_eq!(quarto_cfg.flavor, Flavor::Quarto);
        assert_eq!(
            quarto_cfg.code_blocks.attribute_style,
            AttributeStyle::Shortcut
        );
    }

    #[test]
    fn code_blocks_config_default() {
        // Test the Default impl
        let cb = CodeBlockConfig::default();
        assert_eq!(cb.fence_style, FenceStyle::Backtick);
        assert_eq!(cb.attribute_style, AttributeStyle::Shortcut);
        assert_eq!(cb.min_fence_length, 3);
        assert!(!cb.normalize_indented);
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

    #[test]
    fn extensions_merge_with_flavor_quarto() {
        // Test that extension overrides properly merge with Quarto flavor defaults
        let toml_str = r#"
            flavor = "quarto"
            
            [extensions]
            quarto_crossrefs = false
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        // The overridden extension should be false
        assert!(!cfg.extensions.quarto_crossrefs);

        // Other Quarto-specific extensions should still use Quarto defaults (true)
        assert!(cfg.extensions.quarto_callouts);
        assert!(cfg.extensions.quarto_shortcodes);

        // General Pandoc extensions should also use Quarto defaults
        assert!(cfg.extensions.citations);
        assert!(cfg.extensions.yaml_metadata_block);
        assert!(cfg.extensions.fenced_divs);
    }

    #[test]
    fn extensions_merge_with_flavor_pandoc() {
        // Test that extension overrides work with Pandoc flavor
        let toml_str = r#"
            flavor = "pandoc"
            
            [extensions]
            citations = false
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        // The overridden extension should be false
        assert!(!cfg.extensions.citations);

        // Other Pandoc extensions should still use Pandoc defaults (true)
        assert!(cfg.extensions.yaml_metadata_block);
        assert!(cfg.extensions.fenced_divs);

        // Quarto extensions should be false in Pandoc flavor
        assert!(!cfg.extensions.quarto_crossrefs);
        assert!(!cfg.extensions.quarto_callouts);
    }

    #[test]
    fn extensions_no_override_uses_flavor_defaults() {
        // Test that omitting [extensions] uses flavor defaults
        let toml_str = r#"
            flavor = "quarto"
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        // Should use Quarto defaults
        assert!(cfg.extensions.quarto_crossrefs);
        assert!(cfg.extensions.quarto_callouts);
        assert!(cfg.extensions.quarto_shortcodes);
    }

    #[test]
    fn extensions_empty_section_uses_flavor_defaults() {
        // Test that empty [extensions] section still uses flavor defaults
        let toml_str = r#"
            flavor = "quarto"
            
            [extensions]
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        // Should use Quarto defaults
        assert!(cfg.extensions.quarto_crossrefs);
        assert!(cfg.extensions.quarto_callouts);
        assert!(cfg.extensions.quarto_shortcodes);
    }

    #[test]
    fn extensions_multiple_overrides() {
        // Test multiple extension overrides
        let toml_str = r#"
            flavor = "quarto"
            
            [extensions]
            quarto_crossrefs = false
            citations = false
            emoji = true
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        // Overridden extensions
        assert!(!cfg.extensions.quarto_crossrefs);
        assert!(!cfg.extensions.citations);
        assert!(cfg.extensions.emoji);

        // Other Quarto defaults should remain
        assert!(cfg.extensions.quarto_callouts);
        assert!(cfg.extensions.quarto_shortcodes);
    }

    #[test]
    fn style_section_new_format() {
        let toml_str = r#"
            flavor = "quarto"
            
            [style]
            wrap = "reflow"
            blank-lines = "collapse"
            math-delimiter-style = "dollars"
            math-indent = 2
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        assert_eq!(cfg.wrap, Some(WrapMode::Reflow));
        assert_eq!(cfg.blank_lines, BlankLines::Collapse);
        assert_eq!(cfg.math_delimiter_style, MathDelimiterStyle::Dollars);
        assert_eq!(cfg.math_indent, 2);

        // code-blocks should get flavor defaults
        assert_eq!(cfg.code_blocks.fence_style, FenceStyle::Backtick);
        assert_eq!(cfg.code_blocks.attribute_style, AttributeStyle::Shortcut);
    }

    #[test]
    fn style_section_with_code_blocks() {
        let toml_str = r#"
            flavor = "pandoc"
            
            [style]
            wrap = "preserve"
            
            [style.code-blocks]
            fence-style = "tilde"
            attribute-style = "explicit"
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        assert_eq!(cfg.wrap, Some(WrapMode::Preserve));
        assert_eq!(cfg.code_blocks.fence_style, FenceStyle::Tilde);
        assert_eq!(cfg.code_blocks.attribute_style, AttributeStyle::Explicit);
    }

    #[test]
    fn backwards_compat_old_format_still_works() {
        let toml_str = r#"
            flavor = "quarto"
            wrap = "reflow"
            math-indent = 4
            
            [code-blocks]
            fence-style = "backtick"
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        // Old format should still work
        assert_eq!(cfg.wrap, Some(WrapMode::Reflow));
        assert_eq!(cfg.math_indent, 4);
        assert_eq!(cfg.code_blocks.fence_style, FenceStyle::Backtick);
    }

    #[test]
    fn style_section_takes_precedence() {
        let toml_str = r#"
            flavor = "quarto"
            
            # Old format (should be ignored)
            wrap = "preserve"
            math-indent = 10
            
            # New format (should take precedence)
            [style]
            wrap = "reflow"
            math-indent = 2
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        // New [style] section should win
        assert_eq!(cfg.wrap, Some(WrapMode::Reflow));
        assert_eq!(cfg.math_indent, 2);
    }
}

#[cfg(test)]
mod line_ending_test {
    use super::*;

    #[test]
    fn test_deserialize_line_ending_in_config() {
        #[derive(Deserialize)]
        struct TestConfig {
            line_ending: LineEnding,
        }

        let cfg: TestConfig = toml::from_str(r#"line_ending = "lf""#).unwrap();
        assert_eq!(cfg.line_ending, LineEnding::Lf);

        let cfg2: TestConfig = toml::from_str(r#"line_ending = "auto""#).unwrap();
        assert_eq!(cfg2.line_ending, LineEnding::Auto);

        let cfg3: TestConfig = toml::from_str(r#"line_ending = "crlf""#).unwrap();
        assert_eq!(cfg3.line_ending, LineEnding::Crlf);
    }
}

#[cfg(test)]
mod raw_config_test {
    use super::*;

    #[test]
    fn test_raw_config_line_ending() {
        // Must use hyphen (line-ending) not underscore due to #[serde(rename_all = "kebab-case")]
        let cfg: Config = toml::from_str(r#"line-ending = "lf""#).unwrap();
        assert_eq!(cfg.line_ending, Some(LineEnding::Lf));

        // Test that it goes through RawConfig properly
        let content = r#"
        line-ending = "crlf"
        line-width = 100
        "#;
        let cfg2: Config = toml::from_str(content).unwrap();
        assert_eq!(cfg2.line_ending, Some(LineEnding::Crlf));
        assert_eq!(cfg2.line_width, 100);
    }
}

#[cfg(test)]
mod field_name_test {
    use super::*;

    #[test]
    fn test_line_ending_field_name() {
        // The RawConfig uses #[serde(rename_all = "kebab-case")] so field names use hyphens
        let cfg: Config = toml::from_str(r#"line-ending = "lf""#).unwrap();
        assert_eq!(cfg.line_ending, Some(LineEnding::Lf));

        // Test all three values
        let cfg_auto: Config = toml::from_str(r#"line-ending = "auto""#).unwrap();
        assert_eq!(cfg_auto.line_ending, Some(LineEnding::Auto));

        let cfg_crlf: Config = toml::from_str(r#"line-ending = "crlf""#).unwrap();
        assert_eq!(cfg_crlf.line_ending, Some(LineEnding::Crlf));
    }
}

#[cfg(test)]
mod code_blocks_config_test {
    use super::*;

    #[test]
    fn test_partial_code_blocks_override() {
        // User overrides only attribute_style, other fields should use flavor defaults
        let toml_str = r#"
            flavor = "pandoc"
            
            [code-blocks]
            attribute-style = "explicit"
        "#;
        let cfg: Config = toml::from_str(toml_str).unwrap();

        // User override should apply
        assert_eq!(cfg.code_blocks.attribute_style, AttributeStyle::Explicit);

        // Flavor defaults should fill in other fields
        assert_eq!(cfg.code_blocks.fence_style, FenceStyle::Backtick);
        assert_eq!(cfg.code_blocks.min_fence_length, 3);
        assert!(!cfg.code_blocks.normalize_indented);
    }

    #[test]
    fn test_multiple_code_blocks_overrides() {
        let toml_str = r#"
            flavor = "quarto"
            
            [code-blocks]
            attribute-style = "explicit"
            min-fence-length = 5
        "#;
        let cfg: Config = toml::from_str(toml_str).unwrap();

        // User overrides
        assert_eq!(cfg.code_blocks.attribute_style, AttributeStyle::Explicit);
        assert_eq!(cfg.code_blocks.min_fence_length, 5);

        // Flavor defaults (Quarto uses Shortcut by default, but overridden)
        assert_eq!(cfg.code_blocks.fence_style, FenceStyle::Backtick);
        assert!(!cfg.code_blocks.normalize_indented);
    }

    #[test]
    fn test_no_code_blocks_override_uses_flavor_defaults() {
        let toml_str = r#"
            flavor = "quarto"
        "#;
        let cfg: Config = toml::from_str(toml_str).unwrap();

        // Should use Quarto defaults
        assert_eq!(cfg.code_blocks.attribute_style, AttributeStyle::Shortcut);
        assert_eq!(cfg.code_blocks.fence_style, FenceStyle::Backtick);
        assert_eq!(cfg.code_blocks.min_fence_length, 3);
        assert!(!cfg.code_blocks.normalize_indented);
    }

    // ===== New Formatter Format Tests =====

    #[test]
    fn new_format_single_formatter() {
        let toml_str = r#"
            [formatters]
            r = "air"
            python = "black"
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        assert_eq!(cfg.formatters.len(), 2);

        // Check R formatter (resolved from built-in preset)
        let r_fmts = cfg.formatters.get("r").unwrap();
        assert_eq!(r_fmts.len(), 1);
        assert_eq!(r_fmts[0].cmd, "air");
        assert_eq!(r_fmts[0].args, vec!["format", "{}"]);

        // Check Python formatter (resolved from built-in preset)
        let py_fmts = cfg.formatters.get("python").unwrap();
        assert_eq!(py_fmts.len(), 1);
        assert_eq!(py_fmts[0].cmd, "black");
    }

    #[test]
    fn new_format_multiple_formatters() {
        let toml_str = r#"
            [formatters]
            python = ["ruff", "black"]
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        let py_fmts = cfg.formatters.get("python").unwrap();
        assert_eq!(py_fmts.len(), 2);
        assert_eq!(py_fmts[0].cmd, "ruff");
        assert_eq!(py_fmts[1].cmd, "black");
    }

    #[test]
    fn new_format_with_custom_definition() {
        let toml_str = r#"
            [formatters]
            r = "custom-air"
            
            [formatters.custom-air]
            cmd = "air"
            args = ["format", "--custom-flag"]
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        let r_fmts = cfg.formatters.get("r").unwrap();
        assert_eq!(r_fmts.len(), 1);
        assert_eq!(r_fmts[0].cmd, "air");
        assert_eq!(r_fmts[0].args, vec!["format", "--custom-flag"]);
    }

    #[test]
    fn new_format_empty_array() {
        let toml_str = r#"
            [formatters]
            r = []
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        // Empty array means no formatting for this language
        assert!(!cfg.formatters.contains_key("r"));
    }

    #[test]
    fn new_format_reusable_definition() {
        let toml_str = r#"
            [formatters]
            javascript = "prettier"
            typescript = "prettier"
            json = "prettier"
            
            [formatters.prettier]
            cmd = "prettier"
            args = ["--print-width=100"]
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        assert_eq!(cfg.formatters.len(), 3);

        // All should use the same prettier config
        for lang in ["javascript", "typescript", "json"] {
            let fmts = cfg.formatters.get(lang).unwrap();
            assert_eq!(fmts.len(), 1);
            assert_eq!(fmts[0].cmd, "prettier");
            assert_eq!(fmts[0].args, vec!["--print-width=100"]);
        }
    }

    // ===== Preset inheritance tests =====

    #[test]
    fn preset_inheritance_override_only_args() {
        let toml_str = r#"
            [formatters]
            r = "air"
            
            [formatters.air]
            args = ["format", "--custom-flag", "{}"]
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        let r_fmts = cfg.formatters.get("r").unwrap();
        assert_eq!(r_fmts.len(), 1);
        // cmd and stdin inherited from built-in "air" preset
        assert_eq!(r_fmts[0].cmd, "air");
        assert!(!r_fmts[0].stdin);
        // args overridden
        assert_eq!(r_fmts[0].args, vec!["format", "--custom-flag", "{}"]);
    }

    #[test]
    fn preset_inheritance_override_only_cmd() {
        let toml_str = r#"
            [formatters]
            r = "air"
            
            [formatters.air]
            cmd = "custom-air"
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        let r_fmts = cfg.formatters.get("r").unwrap();
        assert_eq!(r_fmts.len(), 1);
        // cmd overridden
        assert_eq!(r_fmts[0].cmd, "custom-air");
        // args and stdin inherited from built-in "air" preset
        assert_eq!(r_fmts[0].args, vec!["format", "{}"]);
        assert!(!r_fmts[0].stdin);
    }

    #[test]
    fn preset_inheritance_override_only_stdin() {
        let toml_str = r#"
            [formatters]
            r = "air"
            
            [formatters.air]
            stdin = true
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        let r_fmts = cfg.formatters.get("r").unwrap();
        assert_eq!(r_fmts.len(), 1);
        // cmd and args inherited from built-in "air" preset
        assert_eq!(r_fmts[0].cmd, "air");
        assert_eq!(r_fmts[0].args, vec!["format", "{}"]);
        // stdin overridden
        assert!(r_fmts[0].stdin);
    }

    #[test]
    fn preset_inheritance_override_multiple_fields() {
        let toml_str = r#"
            [formatters]
            python = "black"
            
            [formatters.black]
            args = ["--line-length=100"]
            stdin = false
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        let py_fmts = cfg.formatters.get("python").unwrap();
        assert_eq!(py_fmts.len(), 1);
        // cmd inherited
        assert_eq!(py_fmts[0].cmd, "black");
        // args and stdin overridden
        assert_eq!(py_fmts[0].args, vec!["--line-length=100"]);
        assert!(!py_fmts[0].stdin);
    }

    #[test]
    fn preset_inheritance_override_all_fields() {
        let toml_str = r#"
            [formatters]
            r = "air"
            
            [formatters.air]
            cmd = "totally-different"
            args = ["custom"]
            stdin = true
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        let r_fmts = cfg.formatters.get("r").unwrap();
        assert_eq!(r_fmts.len(), 1);
        // All fields overridden (complete replacement)
        assert_eq!(r_fmts[0].cmd, "totally-different");
        assert_eq!(r_fmts[0].args, vec!["custom"]);
        assert!(r_fmts[0].stdin);
    }

    #[test]
    fn preset_inheritance_empty_definition_uses_preset() {
        let toml_str = r#"
            [formatters]
            r = "air"
            
            [formatters.air]
            # Empty definition - should use preset as-is
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        let r_fmts = cfg.formatters.get("r").unwrap();
        assert_eq!(r_fmts.len(), 1);
        // All fields from built-in preset
        assert_eq!(r_fmts[0].cmd, "air");
        assert_eq!(r_fmts[0].args, vec!["format", "{}"]);
        assert!(!r_fmts[0].stdin);
    }

    #[test]
    fn preset_inheritance_unknown_name_without_cmd_errors() {
        let toml_str = r#"
            [formatters]
            r = "unknown-formatter"
            
            [formatters.unknown-formatter]
            args = ["--flag"]
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        // Should fail to resolve - unknown preset and no cmd
        // Error logged, formatter not included in map
        assert!(!cfg.formatters.contains_key("r"));
    }

    #[test]
    fn preset_inheritance_unknown_name_with_cmd_works() {
        let toml_str = r#"
            [formatters]
            r = "unknown-formatter"
            
            [formatters.unknown-formatter]
            cmd = "my-custom-formatter"
            args = ["--flag"]
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        let r_fmts = cfg.formatters.get("r").unwrap();
        assert_eq!(r_fmts.len(), 1);
        // Should work - has cmd even though name doesn't match preset
        assert_eq!(r_fmts[0].cmd, "my-custom-formatter");
        assert_eq!(r_fmts[0].args, vec!["--flag"]);
        assert!(r_fmts[0].stdin); // default
    }

    // ===== Tests for append_args and prepend_args =====

    #[test]
    fn append_args_with_preset_inheritance() {
        let toml_str = r#"
            [formatters]
            r = "air"
            
            [formatters.air]
            append_args = ["-i", "2"]
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        let r_fmts = cfg.formatters.get("r").unwrap();
        assert_eq!(r_fmts.len(), 1);
        // Preset args: ["format", "{}"]
        // After append: ["format", "{}", "-i", "2"]
        assert_eq!(r_fmts[0].cmd, "air");
        assert_eq!(r_fmts[0].args, vec!["format", "{}", "-i", "2"]);
        assert!(!r_fmts[0].stdin);
    }

    #[test]
    fn prepend_args_with_preset_inheritance() {
        let toml_str = r#"
            [formatters]
            r = "air"
            
            [formatters.air]
            prepend_args = ["--verbose"]
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        let r_fmts = cfg.formatters.get("r").unwrap();
        assert_eq!(r_fmts.len(), 1);
        // Preset args: ["format", "{}"]
        // After prepend: ["--verbose", "format", "{}"]
        assert_eq!(r_fmts[0].cmd, "air");
        assert_eq!(r_fmts[0].args, vec!["--verbose", "format", "{}"]);
        assert!(!r_fmts[0].stdin);
    }

    #[test]
    fn both_prepend_and_append_args() {
        let toml_str = r#"
            [formatters]
            r = "air"
            
            [formatters.air]
            prepend_args = ["--verbose"]
            append_args = ["-i", "2"]
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        let r_fmts = cfg.formatters.get("r").unwrap();
        assert_eq!(r_fmts.len(), 1);
        // Preset args: ["format", "{}"]
        // After prepend + append: ["--verbose", "format", "{}", "-i", "2"]
        assert_eq!(r_fmts[0].args, vec!["--verbose", "format", "{}", "-i", "2"]);
    }

    #[test]
    fn append_args_with_explicit_args() {
        let toml_str = r#"
            [formatters]
            r = "custom"
            
            [formatters.custom]
            cmd = "shfmt"
            args = ["-filename", "$FILENAME"]
            append_args = ["-i", "2"]
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        let r_fmts = cfg.formatters.get("r").unwrap();
        assert_eq!(r_fmts.len(), 1);
        // Explicit args with append: ["-filename", "$FILENAME", "-i", "2"]
        assert_eq!(r_fmts[0].cmd, "shfmt");
        assert_eq!(r_fmts[0].args, vec!["-filename", "$FILENAME", "-i", "2"]);
    }

    #[test]
    fn prepend_args_with_explicit_args() {
        let toml_str = r#"
            [formatters]
            r = "custom"
            
            [formatters.custom]
            cmd = "formatter"
            args = ["input.txt"]
            prepend_args = ["--config", "cfg.toml"]
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        let r_fmts = cfg.formatters.get("r").unwrap();
        assert_eq!(r_fmts.len(), 1);
        // Explicit args with prepend: ["--config", "cfg.toml", "input.txt"]
        assert_eq!(r_fmts[0].args, vec!["--config", "cfg.toml", "input.txt"]);
    }

    #[test]
    fn args_override_with_append_still_applies() {
        let toml_str = r#"
            [formatters]
            r = "air"
            
            [formatters.air]
            args = ["custom", "override"]
            append_args = ["--extra"]
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        let r_fmts = cfg.formatters.get("r").unwrap();
        assert_eq!(r_fmts.len(), 1);
        // Overridden args + append: ["custom", "override", "--extra"]
        assert_eq!(r_fmts[0].args, vec!["custom", "override", "--extra"]);
    }

    #[test]
    fn empty_append_prepend_arrays() {
        let toml_str = r#"
            [formatters]
            r = "air"
            
            [formatters.air]
            prepend_args = []
            append_args = []
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        let r_fmts = cfg.formatters.get("r").unwrap();
        assert_eq!(r_fmts.len(), 1);
        // Empty modifiers = no-op, preset args unchanged
        assert_eq!(r_fmts[0].args, vec!["format", "{}"]);
    }

    #[test]
    fn modifiers_without_base_args() {
        let toml_str = r#"
            [formatters]
            r = "custom"
            
            [formatters.custom]
            cmd = "formatter"
            prepend_args = ["--flag"]
            append_args = ["--other"]
        "#;
        let cfg = toml::from_str::<Config>(toml_str).unwrap();

        let r_fmts = cfg.formatters.get("r").unwrap();
        assert_eq!(r_fmts.len(), 1);
        // No base args (no preset, no explicit args), modifiers create args from scratch
        // Result: ["--flag", "--other"]
        assert_eq!(r_fmts[0].args, vec!["--flag", "--other"]);
    }
}
