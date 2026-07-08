use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use panache_formatter::config::FormatterExtensions;
use panache_parser::{Extensions, Flavor, PandocCompat, ParserOptions};

use super::formatter_presets;

mod schema_helpers;

/// Configuration for an external code formatter.
#[derive(Debug, Clone, PartialEq)]
pub struct FormatterConfig {
    /// Command to execute (e.g., "black", "air", "rustfmt")
    pub cmd: String,
    /// Arguments to pass to the command (e.g., ["-", "--line-length=80"])
    pub args: Vec<String>,
    /// Whether the formatter reads from stdin (true) or requires a file path (false)
    pub stdin: bool,
}

/// NEW: Language → Formatter mapping value (single formatter or chain)
#[derive(Debug, Clone, Deserialize, JsonSchema, PartialEq)]
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
/// Additionally, you can modify arguments incrementally using `prepend-args` and `append-args`:
///
/// ```toml
/// [formatters.air]
/// append-args = ["-i", "2"]  # Adds args to end: ["format", "{}", "-i", "2"]
/// ```
#[derive(Debug, Clone, Deserialize, JsonSchema, PartialEq, Default)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
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
    /// Whether the formatter reads from stdin
    stdin: bool,
}

impl Default for RawFormatterConfig {
    fn default() -> Self {
        Self {
            preset: None,
            cmd: None,
            args: None,
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
                let available = formatter_preset_names().join(", ");
                serde::de::Error::custom(format!(
                    "Unknown formatter preset: '{}'. Available presets: {}",
                    preset_name, available
                ))
            })?;

            Ok(FormatterConfig {
                cmd: preset.cmd,
                args: preset.args,
                stdin: preset.stdin,
            })
        } else if let Some(cmd) = raw.cmd {
            // Custom configuration
            Ok(FormatterConfig {
                cmd,
                args: raw.args.unwrap_or_default(),
                stdin: raw.stdin,
            })
        } else {
            // No preset and no cmd - return empty config
            // This can happen with Default::default()
            Ok(FormatterConfig {
                cmd: String::new(),
                args: raw.args.unwrap_or_default(),
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
            stdin: true,
        }
    }
}

/// Get a built-in formatter preset by name.
/// Returns None if the preset doesn't exist.
fn get_formatter_preset(name: &str) -> Option<FormatterConfig> {
    formatter_presets::get_formatter_preset(name)
}

/// Canonical built-in formatter preset names used for docs and diagnostics.
fn formatter_preset_names() -> &'static [&'static str] {
    formatter_presets::formatter_preset_names()
}

fn formatter_preset_supported_languages(name: &str) -> Option<&'static [&'static str]> {
    formatter_presets::formatter_preset_supported_languages(name)
}

fn normalize_formatter_language(language: &str) -> String {
    language.trim().to_ascii_lowercase().replace('_', "-")
}

fn validate_formatter_language_for_preset(lang: &str, formatter_name: &str) -> Result<(), String> {
    let Some(supported) = formatter_preset_supported_languages(formatter_name) else {
        return Ok(()); // custom formatter or unknown preset handled elsewhere
    };

    let normalized_lang = normalize_formatter_language(lang);
    let matches = supported
        .iter()
        .any(|supported_lang| *supported_lang == normalized_lang);

    if matches {
        return Ok(());
    }

    Err(format!(
        "Language '{}': formatter '{}' does not support this language. Supported languages: {}",
        lang,
        formatter_name,
        supported.join(", ")
    ))
}

/// Get the default formatters HashMap with built-in presets.
/// Currently includes R (air) and Python (ruff).
#[allow(dead_code)]
pub fn default_formatters() -> HashMap<String, FormatterConfig> {
    let mut map = HashMap::new();
    map.insert("r".to_string(), get_formatter_preset("air").unwrap());
    map.insert("python".to_string(), get_formatter_preset("ruff").unwrap());
    map
}

/// Style for formatting math delimiters
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq, Default)]
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

/// Tab stop handling for formatter output.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TabStopMode {
    /// Normalize tabs to spaces (4-column tab stop).
    #[default]
    Normalize,
    /// Preserve tabs in literal code spans/blocks.
    Preserve,
}

/// Largest accepted value for `table-indent`.
pub const MAX_TABLE_INDENT: usize = 3;

/// Default value for `table-indent`.
pub const DEFAULT_TABLE_INDENT: usize = 2;

/// Deserialize `table-indent`, rejecting values outside `0..=3`.
fn deserialize_table_indent<'de, D>(deserializer: D) -> Result<usize, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = usize::deserialize(deserializer)?;
    if value > MAX_TABLE_INDENT {
        return Err(serde::de::Error::custom(format!(
            "table-indent must be 0, 1, 2, or 3 (got {value})"
        )));
    }
    Ok(value)
}

/// User-supplied no-break abbreviations for sentence wrapping.
///
/// Accepts either a flat list applied to every document, or a table keyed by
/// primary language subtag with a `default` bucket:
///
/// ```toml
/// # flat
/// no-break-abbreviations = ["např.", "tzv."]
///
/// # per-language
/// [format.no-break-abbreviations]
/// default = ["etc."]
/// cs = ["např."]
/// ```
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum NoBreakAbbreviations {
    /// A single list merged into every document's profile.
    Flat(Vec<String>),
    /// Per-language buckets keyed by primary subtag (`cs`, `de`, ...), plus an
    /// optional `default` bucket applied to every document.
    PerLanguage(std::collections::BTreeMap<String, Vec<String>>),
}

impl JsonSchema for NoBreakAbbreviations {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "NoBreakAbbreviations".into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        // serde `untagged` accepts two shapes; schemars can't derive this, so
        // describe both by hand (see `.claude/rules/config.md`):
        //   no-break-abbreviations = ["etc."]              (flat list)
        //   [format.no-break-abbreviations] de = ["bzw."]  (per-language table)
        schemars::json_schema!({
            "anyOf": [
                { "type": "array", "items": { "type": "string" } },
                {
                    "type": "object",
                    "additionalProperties": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                }
            ]
        })
    }
}

/// Formatting style configuration.
/// Groups all style-related settings together.
#[derive(Debug, Clone, Deserialize, JsonSchema, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct StyleConfig {
    /// Maximum line width before wrapping. `None` falls back to the deprecated
    /// top-level `line-width`, then the built-in default (80).
    pub line_width: Option<usize>,
    /// Line ending style. `None` falls back to the deprecated top-level
    /// `line-ending`, then `auto`.
    pub line_ending: Option<LineEnding>,
    /// Text wrapping mode
    pub wrap: Option<WrapMode>,
    /// Blank line handling between blocks
    pub blank_lines: BlankLines,
    /// Math delimiter style preference
    pub math_delimiter_style: MathDelimiterStyle,
    /// Math indentation (spaces)
    pub math_indent: usize,
    /// Indentation (columns) for top-level pipe, simple, and multiline tables.
    /// Accepts 0--3; grid tables stay flush at column 0 regardless.
    #[serde(deserialize_with = "deserialize_table_indent")]
    #[schemars(range(min = 0, max = 3))]
    pub table_indent: usize,
    /// Tab stop handling (normalize or preserve)
    pub tab_stops: TabStopMode,
    /// Tab width for expanding tabs when normalizing
    pub tab_width: usize,
    /// Use panache-native greedy wrapping instead of textwrap.
    pub built_in_greedy_wrap: bool,
    /// Extra abbreviations whose trailing period must not end a sentence (used
    /// by `wrap = "sentence"`). Merged with the built-in per-language profile.
    pub no_break_abbreviations: Option<NoBreakAbbreviations>,
    /// Fallback document language for sentence wrapping when the document has no
    /// YAML `lang:`. A code such as `de` or `pt-BR`.
    pub lang: Option<String>,
}

impl Default for StyleConfig {
    fn default() -> Self {
        Self {
            line_width: None,
            line_ending: None,
            wrap: Some(WrapMode::Reflow),
            blank_lines: BlankLines::Collapse,
            math_delimiter_style: MathDelimiterStyle::default(),
            math_indent: 2,
            table_indent: DEFAULT_TABLE_INDENT,
            tab_stops: TabStopMode::Normalize,
            tab_width: 4,
            built_in_greedy_wrap: true,
            no_break_abbreviations: None,
            lang: None,
        }
    }
}

impl StyleConfig {
    // No flavor-specific defaults needed - just use field defaults
}

/// Experimental, opt-in features.
///
/// Everything under `[experimental]` is unstable: behavior and the option
/// surface itself may change (or be removed) **without a major release**. Do not
/// depend on it for stable output.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct ExperimentalConfig {
    /// Reformat the *content* of math (`$...$`, `$$...$$`, environments) —
    /// collapse inline whitespace, indent environment bodies, normalize `\\`
    /// line breaks, align `&` columns, and apply precedence-aware operator
    /// spacing (`a+b` → `a + b`, unary `-x` stays tight). Default off: math is
    /// emitted verbatim. No macro rewriting or `\frac` canonicalization.
    pub format_math: bool,
}

/// Linter configuration.
/// Preferred shape is `[lint.rules] rule-name = true/false`.
/// Legacy `[lint] rule-name = true/false` is still supported (deprecated).
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct LintConfig {
    pub rules: HashMap<String, bool>,
    /// Resolved Quarto compatibility target for the `quarto-schema` rule.
    ///
    /// This is populated from `[compat] quarto` during config finalization, not
    /// parsed from `[lint]`. Kept here because the `quarto-schema` rule is its
    /// only consumer. See [`CompatConfig::quarto`].
    #[serde(rename = "quarto-version", skip_serializing_if = "Option::is_none")]
    pub quarto_version: Option<String>,
}

impl LintConfig {
    fn normalize_rule_name(name: &str) -> String {
        name.trim().to_lowercase().replace('_', "-")
    }

    fn normalize(mut self) -> Self {
        self.rules = self
            .rules
            .into_iter()
            .map(|(name, enabled)| (Self::normalize_rule_name(&name), enabled))
            .collect();
        self
    }

    pub fn is_rule_enabled(&self, rule_name: &str) -> bool {
        let normalized = Self::normalize_rule_name(rule_name);
        self.rules.get(&normalized).copied().unwrap_or(true)
    }

    /// Like `is_rule_enabled`, but the default is `false` when the rule is
    /// absent from the user's config. Use for opt-in rules whose default
    /// behavior would generate noise for most users.
    pub fn is_rule_explicitly_enabled(&self, rule_name: &str) -> bool {
        let normalized = Self::normalize_rule_name(rule_name);
        self.rules.get(&normalized).copied().unwrap_or(false)
    }
}

impl JsonSchema for LintConfig {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "LintConfig".into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        // Two accepted shapes:
        //   [lint.rules] my-rule = true   (preferred)
        //   [lint] my-rule = true         (legacy, deprecated)
        // Modelled as an object whose `rules` field is a string→bool map and
        // whose additionalProperties are also bools (for the legacy shape).
        schemars::json_schema!({
            "type": "object",
            "description": "Linter configuration.",
            "properties": {
                "rules": {
                    "type": "object",
                    "description": "Map of lint rule names to enabled/disabled. \
                                    Preferred over the legacy flat `[lint]` shape.",
                    "additionalProperties": { "type": "boolean" },
                },
            },
            "additionalProperties": { "type": "boolean" },
        })
    }
}

impl<'de> Deserialize<'de> for LintConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = toml::Value::deserialize(deserializer)?;
        let mut rules = HashMap::new();

        let mut table = value
            .as_table()
            .cloned()
            .ok_or_else(|| serde::de::Error::custom("expected [lint] table"))?;

        // `quarto-version` moved to `[compat] quarto`. Catch the old key with a
        // pointed migration error rather than the generic "use [lint.rules]"
        // message the legacy bool loop would otherwise emit.
        if table.contains_key("quarto-version") {
            return Err(serde::de::Error::custom(
                "[lint] quarto-version moved to [compat] quarto; \
                 set `[compat]\\nquarto = \"...\"` instead",
            ));
        }

        // Rules live under [lint.rules]. The legacy flat `[lint] rule = true`
        // shape was removed in 3.0.
        if let Some(rules_value) = table.remove("rules") {
            let rules_table = rules_value
                .as_table()
                .ok_or_else(|| serde::de::Error::custom("[lint.rules] must be a table"))?;
            for (name, enabled) in rules_table {
                let enabled = enabled.as_bool().ok_or_else(|| {
                    serde::de::Error::custom(format!(
                        "[lint.rules] entry '{}' must be true or false",
                        name
                    ))
                })?;
                rules.insert(name.clone(), enabled);
            }
        }

        // Any remaining top-level key is the removed flat shape.
        if let Some((name, _)) = table.iter().next() {
            return Err(serde::de::Error::custom(format!(
                "Unsupported [lint] key '{}'; put rule toggles under [lint.rules]",
                name
            )));
        }

        Ok(Self {
            rules,
            // Populated later from `[compat] quarto` during finalization.
            quarto_version: None,
        }
        .normalize())
    }
}

/// Compatibility targets for the upstream toolchain you author for.
///
/// Co-locates the "which version of the upstream tool do I target" knobs.
/// `pandoc` drives how the parser disambiguates ambiguous syntax; `quarto`
/// selects the vendored schema the `quarto-schema` lint rule validates against.
/// Configured via the `[compat]` section:
///
/// ```toml
/// [compat]
/// pandoc = "3.9"
/// quarto = "1.9"
/// ```
#[derive(Debug, Clone, Deserialize, JsonSchema, PartialEq, Default)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct CompatConfig {
    /// Pandoc release whose ambiguous-syntax behavior the parser emulates
    /// (`latest`, `3.7`, `3.9`). Supersedes the deprecated top-level
    /// `pandoc-compat` key.
    pub pandoc: Option<PandocCompat>,
    /// Quarto release whose vendored schema the `quarto-schema` rule validates
    /// against (e.g. `"1.9"`). Currently one version is bundled, so this is an
    /// advisory pin; it reserves the key for selecting among bundled versions
    /// later.
    pub quarto: Option<String>,
}

/// Internal deserialization struct that allows for optional fields
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct RawConfig {
    /// Path to another config file to inherit from and override (Ruff-style).
    /// Resolved relative to the file that declares it; a leading `~` expands to
    /// the home directory. Chains transitively (`a` extends `b` extends `c`).
    /// This key is consumed during the raw-TOML merge in the loader, so by the
    /// time a `RawConfig` is deserialized it is already inert; the field exists
    /// only so the key is accepted under `deny_unknown_fields` and appears in
    /// the schema.
    #[serde(default)]
    #[allow(dead_code)]
    extend: Option<String>,
    #[serde(default)]
    flavor: Flavor,
    #[serde(default)]
    #[schemars(schema_with = "schema_helpers::extensions_schema")]
    extensions: Option<toml::Value>,
    /// DEPRECATED top-level alias: use `[format] line-ending` instead. Still
    /// read as a fallback when `[format]` omits it.
    #[serde(default)]
    line_ending: Option<LineEnding>,
    /// DEPRECATED top-level alias: use `[format] line-width` instead. Still
    /// read as a fallback when `[format]` omits it.
    #[serde(default)]
    line_width: Option<usize>,
    /// DEPRECATED: use `[compat] pandoc` instead. Still read as an alias.
    #[serde(default)]
    pandoc_compat: Option<PandocCompat>,

    /// Compatibility targets (`[compat]`): `pandoc` and `quarto` versions.
    #[serde(default)]
    compat: Option<CompatConfig>,

    // Preferred formatting section
    #[serde(default)]
    #[serde(rename = "format")]
    format_section: Option<StyleConfig>,

    /// DEPRECATED no-op: `blank-lines` is retained only so older configs keep
    /// parsing and `check_deprecated_blank_lines` can warn. When no `[format]`
    /// section is present this top-level value still feeds `blank_lines`.
    #[serde(default = "default_blank_lines")]
    blank_lines: BlankLines,
    // Language → Formatter(s) mapping (parsed manually as a raw Value).
    #[serde(default)]
    #[schemars(schema_with = "schema_helpers::formatters_schema")]
    formatters: Option<toml::Value>,

    /// Max parallel external tool invocations (formatters/linters) per document.
    #[serde(default)]
    external_max_parallel: Option<usize>,

    #[serde(default)]
    linters: HashMap<String, String>,
    #[serde(default)]
    lint: Option<LintConfig>,
    #[serde(default)]
    cache_dir: Option<String>,
    /// Enable the on-disk lint/format cache (default: true). Set to `false` to
    /// disable cache reads and writes for the project.
    #[serde(default)]
    cache: Option<bool>,
    #[serde(default)]
    exclude: Option<Vec<String>>,
    #[serde(default)]
    extend_exclude: Vec<String>,
    #[serde(default)]
    include: Option<Vec<String>>,
    #[serde(default)]
    extend_include: Vec<String>,
    #[serde(default)]
    flavor_overrides: HashMap<String, Flavor>,

    /// Opt-in experimental features (`[experimental]`). Unstable surface.
    #[serde(default)]
    experimental: Option<ExperimentalConfig>,

    /// Extra cross-reference key prefixes for crossref-injecting extensions
    /// (e.g. pseudocode's `@algo-`). Keys with these prefixes parse as
    /// cross-references rather than citations.
    #[serde(default)]
    crossref_prefixes: Vec<String>,
}

fn default_line_width() -> usize {
    80
}

fn default_external_max_parallel() -> usize {
    // Conservative cap: documents may have hundreds of code blocks.
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, 8)
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
            // Language guards apply only to built-in presets. Named formatter
            // definitions are user-owned and may be intentionally reused across
            // languages (e.g. a custom "prettier" definition).
            if !formatter_definitions.contains_key(name) {
                validate_formatter_language_for_preset(lang, name)?;
            }
            resolve_formatter_name(name, formatter_definitions)
                .map_err(|e| format!("Language '{}': {}", lang, e))
        })
        .collect()
}

impl RawConfig {
    /// Finalize into Config, applying flavor-based defaults where needed
    fn finalize(self) -> Config {
        let compat = self.compat.unwrap_or_default();

        if self.pandoc_compat.is_some() {
            eprintln!(
                "Warning: top-level `pandoc-compat` is deprecated; \
                 use `[compat] pandoc` instead."
            );
        }
        // `[compat] pandoc` wins over the deprecated top-level alias.
        let resolved_pandoc_compat = compat.pandoc.or(self.pandoc_compat).unwrap_or_default();

        // `[format]` is the only formatting section. When it is absent, the
        // deprecated top-level `blank-lines` alias still feeds `blank_lines`;
        // every other setting takes its default.
        let had_format_section = self.format_section.is_some();
        let style = self.format_section.unwrap_or_default();
        let blank_lines = if had_format_section {
            style.blank_lines
        } else {
            self.blank_lines
        };

        // `line-width`/`line-ending` now live under `[format]`; the top-level
        // keys are deprecated aliases. The `[format]` value wins when both are
        // set; otherwise fall back to the top-level alias, then the default.
        if self.line_width.is_some() {
            eprintln!(
                "Warning: top-level `line-width` is deprecated; \
                 use `[format] line-width` instead."
            );
        }
        if self.line_ending.is_some() {
            eprintln!(
                "Warning: top-level `line-ending` is deprecated; \
                 use `[format] line-ending` instead."
            );
        }
        let line_width = style
            .line_width
            .or(self.line_width)
            .unwrap_or_else(default_line_width);
        let line_ending = style
            .line_ending
            .or(self.line_ending)
            .or(Some(LineEnding::Auto));

        Config {
            extensions: super::resolve_extensions_for_flavor(self.extensions.as_ref(), self.flavor),
            formatter_extensions: super::resolve_formatter_extensions_for_flavor(
                self.extensions.as_ref(),
                self.flavor,
            ),
            line_ending,
            flavor: self.flavor,
            line_width,
            wrap: style.wrap,
            blank_lines,
            math_delimiter_style: style.math_delimiter_style,
            math_indent: style.math_indent,
            table_indent: style.table_indent,
            tab_stops: style.tab_stops,
            tab_width: style.tab_width,
            formatters: resolve_formatters(self.formatters),
            linters: self.linters,
            lint: {
                let mut lint = self.lint.unwrap_or_default().normalize();
                lint.quarto_version = compat.quarto;
                lint
            },
            cache_dir: self.cache_dir,
            cache: self.cache.unwrap_or(true),
            external_max_parallel: self
                .external_max_parallel
                .unwrap_or_else(default_external_max_parallel),
            parser: resolved_pandoc_compat,
            built_in_greedy_wrap: style.built_in_greedy_wrap,
            no_break_abbreviations: style.no_break_abbreviations,
            lang: style.lang,
            exclude: self.exclude,
            extend_exclude: self.extend_exclude,
            include: self.include,
            extend_include: self.extend_include,
            flavor_overrides: self.flavor_overrides,
            experimental: self.experimental.unwrap_or_default(),
            crossref_prefixes: self.crossref_prefixes,
        }
    }
}

/// Resolve formatter configuration into a language → formatter(s) mapping.
///
/// The shape is `[formatters] r = "air", python = ["isort", "black"]` with
/// optional `[formatters.<name>]` definition tables. The legacy per-language
/// `[formatters.<lang>]` config format was removed in 3.0.
fn resolve_formatters(
    raw_formatters: Option<toml::Value>,
) -> HashMap<String, Vec<FormatterConfig>> {
    let Some(value) = raw_formatters else {
        return HashMap::new();
    };

    let toml::Value::Table(table) = value else {
        eprintln!("Warning: Invalid formatters configuration - expected table");
        return HashMap::new();
    };

    resolve_formatter_table(table)
}

/// Resolve `[formatters] = { r = "air", python = ["isort", "black"] }` plus any
/// `[formatters.air]` / `[formatters.isort]` definitions.
fn resolve_formatter_table(
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

#[derive(Debug, Clone)]
pub struct Config {
    pub flavor: Flavor,
    pub extensions: Extensions,
    pub formatter_extensions: FormatterExtensions,
    pub line_ending: Option<LineEnding>,
    pub line_width: usize,
    pub math_indent: usize,
    pub math_delimiter_style: MathDelimiterStyle,
    /// Indentation (columns) for top-level pipe, simple, and multiline tables.
    pub table_indent: usize,
    pub tab_stops: TabStopMode,
    pub tab_width: usize,
    pub wrap: Option<WrapMode>,
    pub blank_lines: BlankLines,
    /// Language → Formatter(s) mapping (supports multiple formatters per language)
    pub formatters: HashMap<String, Vec<FormatterConfig>>,
    pub linters: HashMap<String, String>,
    /// Max parallel external tool invocations (formatters/linters) per document.
    pub external_max_parallel: usize,
    /// Compatibility target for ambiguous Pandoc behavior.
    pub parser: PandocCompat,
    /// Extra cross-reference key prefixes (top-level `crossref-prefixes`) for
    /// crossref-injecting extensions (e.g. pseudocode's `@algo-`). Keys with
    /// these prefixes parse as cross-references rather than citations.
    pub crossref_prefixes: Vec<String>,
    /// Linter rule toggles.
    pub lint: LintConfig,
    /// Optional cache directory override.
    pub cache_dir: Option<String>,
    /// Whether the on-disk lint/format cache is enabled (default: true).
    pub cache: bool,
    pub built_in_greedy_wrap: bool,
    /// Extra no-break abbreviations for sentence wrapping (see [`StyleConfig`]).
    pub no_break_abbreviations: Option<NoBreakAbbreviations>,
    /// Fallback document language for sentence wrapping.
    pub lang: Option<String>,
    pub exclude: Option<Vec<String>>,
    pub extend_exclude: Vec<String>,
    pub include: Option<Vec<String>>,
    pub extend_include: Vec<String>,
    pub flavor_overrides: HashMap<String, Flavor>,
    /// Opt-in experimental features (`[experimental]`). Unstable surface.
    pub experimental: ExperimentalConfig,
}

impl<'de> Deserialize<'de> for Config {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawConfig::deserialize(deserializer).map(|raw| raw.finalize())
    }
}

impl JsonSchema for Config {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "PanacheConfig".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        // `Config` is the post-finalization runtime type; the user-facing
        // TOML shape is described by `RawConfig`. Delegate so the generated
        // schema reflects what users actually write, including all
        // deprecated/legacy aliases that `RawConfig` still accepts.
        <RawConfig as JsonSchema>::json_schema(generator)
    }
}

impl Default for Config {
    fn default() -> Self {
        let flavor = Flavor::default();
        Self {
            flavor,
            extensions: Extensions::for_flavor(flavor),
            formatter_extensions: FormatterExtensions::for_flavor(flavor),
            line_ending: Some(LineEnding::Auto),
            line_width: 80,
            math_indent: 2,
            math_delimiter_style: MathDelimiterStyle::default(),
            table_indent: DEFAULT_TABLE_INDENT,
            tab_stops: TabStopMode::Normalize,
            tab_width: 4,
            wrap: Some(WrapMode::Reflow),
            blank_lines: BlankLines::Collapse,
            formatters: HashMap::new(), // Opt-in: empty by default
            linters: HashMap::new(),    // Opt-in: empty by default
            external_max_parallel: default_external_max_parallel(),
            parser: PandocCompat::default(),
            crossref_prefixes: Vec::new(),
            lint: LintConfig::default(),
            cache_dir: None,
            cache: true,
            built_in_greedy_wrap: true,
            no_break_abbreviations: None,
            lang: None,
            exclude: None,
            extend_exclude: Vec::new(),
            include: None,
            extend_include: Vec::new(),
            flavor_overrides: HashMap::new(),
            experimental: ExperimentalConfig::default(),
        }
    }
}

impl Config {
    pub fn parser_options(&self) -> ParserOptions {
        ParserOptions {
            flavor: self.flavor,
            dialect: panache_parser::Dialect::for_flavor(self.flavor),
            extensions: self.extensions.clone(),
            pandoc_compat: self.parser,
            crossref_prefixes: self.crossref_prefixes.clone(),
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

#[derive(Debug, Clone, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum WrapMode {
    Preserve,
    Reflow,
    Sentence,
    Semantic,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum LineEnding {
    Auto,
    Lf,
    Crlf,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum BlankLines {
    /// Preserve original blank lines (any number)
    Preserve,
    /// Collapse multiple consecutive blank lines to a single blank line
    Collapse,
}
