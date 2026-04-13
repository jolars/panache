use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize};

use panache_parser::{Extensions, Flavor, PandocCompat, ParserOptions};

use super::formatter_presets;

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
/// Additionally, you can modify arguments incrementally using `prepend-args` and `append-args`:
///
/// ```toml
/// [formatters.air]
/// append-args = ["-i", "2"]  # Adds args to end: ["format", "{}", "-i", "2"]
/// ```
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default)]
#[serde(rename_all = "kebab-case")]
pub struct FormatterDefinition {
    /// Reference to a built-in preset (e.g., "air", "black") - OLD FORMAT ONLY
    /// In new format, presets are referenced directly in [formatters] mapping
    pub preset: Option<String>,
    /// Custom command to execute (None = inherit from preset if name matches)
    pub cmd: Option<String>,
    /// Arguments to pass (None = inherit from preset if name matches)
    pub args: Option<Vec<String>>,
    /// Arguments to prepend to base args (from preset or explicit args)
    #[serde(alias = "prepend_args")]
    pub prepend_args: Option<Vec<String>>,
    /// Arguments to append to base args (from preset or explicit args)
    #[serde(alias = "append_args")]
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
                let available = formatter_preset_names().join(", ");
                serde::de::Error::custom(format!(
                    "Unknown formatter preset: '{}'. Available presets: {}",
                    preset_name, available
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

/// Tab stop handling for formatter output.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TabStopMode {
    /// Normalize tabs to spaces (4-column tab stop).
    #[default]
    Normalize,
    /// Preserve tabs in literal code spans/blocks.
    Preserve,
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
    /// Tab stop handling (normalize or preserve)
    pub tab_stops: TabStopMode,
    /// Tab width for expanding tabs when normalizing
    pub tab_width: usize,
    /// Use panache-native greedy wrapping instead of textwrap.
    pub built_in_greedy_wrap: bool,
}

impl Default for StyleConfig {
    fn default() -> Self {
        Self {
            wrap: Some(WrapMode::Reflow),
            blank_lines: BlankLines::Collapse,
            math_delimiter_style: MathDelimiterStyle::default(),
            math_indent: 0,
            tab_stops: TabStopMode::Normalize,
            tab_width: 4,
            built_in_greedy_wrap: true,
        }
    }
}

impl StyleConfig {
    // No flavor-specific defaults needed - just use field defaults
}

/// Linter configuration.
/// Preferred shape is `[lint.rules] rule-name = true/false`.
/// Legacy `[lint] rule-name = true/false` is still supported (deprecated).
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct LintConfig {
    pub rules: HashMap<String, bool>,
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
}

impl<'de> Deserialize<'de> for LintConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = toml::Value::deserialize(deserializer)?;
        let mut rules = HashMap::new();
        let mut used_legacy_shape = false;

        let mut table = value
            .as_table()
            .cloned()
            .ok_or_else(|| serde::de::Error::custom("expected [lint] table"))?;

        // New shape: [lint.rules]
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

        // Legacy shape: [lint] rule-name = true/false
        for (name, enabled) in table {
            let enabled = enabled.as_bool().ok_or_else(|| {
                serde::de::Error::custom(format!(
                    "Unsupported [lint] key '{}'; use [lint.rules] for rule toggles",
                    name
                ))
            })?;
            used_legacy_shape = true;
            rules.insert(name, enabled);
        }

        if used_legacy_shape {
            eprintln!(
                "Warning: [lint] rule = true/false is deprecated; use [lint.rules] rule = true/false."
            );
        }

        Ok(Self { rules }.normalize())
    }
}

/// Internal deserialization struct that allows for optional fields
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct RawConfig {
    #[serde(default)]
    flavor: Flavor,
    #[serde(default)]
    extensions: Option<toml::Value>,
    #[serde(default)]
    line_ending: Option<LineEnding>,
    #[serde(default = "default_line_width")]
    line_width: usize,
    #[serde(default)]
    pandoc_compat: Option<PandocCompat>,

    // New preferred formatting section
    #[serde(default)]
    #[serde(rename = "format")]
    format_section: Option<StyleConfig>,

    // DEPRECATED: [style] section (kept for backwards compatibility)
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
    tab_stops: TabStopMode,
    #[serde(default = "default_tab_width")]
    tab_width: usize,
    // NEW: Language → Formatter(s) mapping
    // This will be a raw Value that we'll parse manually to handle both formats
    #[serde(default)]
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

fn default_tab_width() -> usize {
    4
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
        let resolved_pandoc_compat = self.pandoc_compat.unwrap_or_default();

        // Check for deprecated top-level style fields
        let has_deprecated_fields = self.wrap.is_some()
            || self.math_indent != 0
            || self.math_delimiter_style != MathDelimiterStyle::default()
            || self.blank_lines != default_blank_lines()
            || self.tab_stops != TabStopMode::Normalize
            || self.tab_width != default_tab_width();

        if has_deprecated_fields && self.format_section.is_none() && self.style.is_none() {
            eprintln!(
                "Warning: top-level style fields (wrap, math-indent, etc.) \
                 are deprecated. Please move them under [format] section. \
                 See documentation for the new format."
            );
        }

        // Merge formatting config: prefer [format], then deprecated [style], then old top-level fields.
        let style = if let Some(format_config) = self.format_section {
            if self.style.is_some() {
                eprintln!(
                    "Warning: Both [format] and deprecated [style] sections found. \
                     Using [format] section."
                );
            }
            if has_deprecated_fields {
                eprintln!(
                    "Warning: Both [format] section and top-level style fields found. \
                     Using [format] section and ignoring top-level fields."
                );
            }

            format_config
        } else if let Some(style_config) = self.style {
            eprintln!("Warning: [style] section is deprecated. Please use [format] instead.");
            if has_deprecated_fields {
                eprintln!(
                    "Warning: Both deprecated [style] section and top-level style fields found. \
                     Using [style] section and ignoring top-level fields."
                );
            }
            style_config
        } else {
            // Old format - construct StyleConfig from top-level fields
            StyleConfig {
                wrap: self.wrap.or(Some(WrapMode::Reflow)),
                blank_lines: self.blank_lines,
                math_delimiter_style: self.math_delimiter_style,
                math_indent: self.math_indent,
                tab_stops: self.tab_stops,
                tab_width: self.tab_width,
                built_in_greedy_wrap: true,
            }
        };

        Config {
            extensions: super::resolve_extensions_for_flavor(self.extensions.as_ref(), self.flavor),
            line_ending: self.line_ending.or(Some(LineEnding::Auto)),
            flavor: self.flavor,
            line_width: self.line_width,
            wrap: style.wrap,
            blank_lines: style.blank_lines,
            math_delimiter_style: style.math_delimiter_style,
            math_indent: style.math_indent,
            tab_stops: style.tab_stops,
            tab_width: style.tab_width,
            formatters: resolve_formatters(self.formatters),
            linters: self.linters,
            lint: self.lint.unwrap_or_default().normalize(),
            cache_dir: self.cache_dir,
            external_max_parallel: self
                .external_max_parallel
                .unwrap_or_else(default_external_max_parallel),
            parser: resolved_pandoc_compat,
            built_in_greedy_wrap: style.built_in_greedy_wrap,
            exclude: self.exclude,
            extend_exclude: self.extend_exclude,
            include: self.include,
            extend_include: self.extend_include,
            flavor_overrides: self.flavor_overrides,
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
        let preset = get_formatter_preset(preset_name).ok_or_else(|| {
            let available = formatter_preset_names().join(", ");
            format!(
                "Unknown formatter preset '{}'. Available presets: {}",
                preset_name, available
            )
        })?;

        let mut args = definition.args.clone().unwrap_or(preset.args);

        // Apply prepend/append modifiers
        apply_arg_modifiers(&mut args, definition);

        Ok(FormatterConfig {
            cmd: preset.cmd,
            args,
            enabled: true, // enabled field checked by caller
            stdin: preset.stdin,
        })
    } else if let Some(cmd) = &definition.cmd {
        // Custom command
        let mut args = definition.args.clone().unwrap_or_default();

        // Apply prepend/append modifiers
        apply_arg_modifiers(&mut args, definition);

        Ok(FormatterConfig {
            cmd: cmd.clone(),
            args,
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
    /// Linter rule toggles.
    pub lint: LintConfig,
    /// Optional CLI cache directory override.
    pub cache_dir: Option<String>,
    pub built_in_greedy_wrap: bool,
    pub exclude: Option<Vec<String>>,
    pub extend_exclude: Vec<String>,
    pub include: Option<Vec<String>>,
    pub extend_include: Vec<String>,
    pub flavor_overrides: HashMap<String, Flavor>,
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
            tab_stops: TabStopMode::Normalize,
            tab_width: 4,
            wrap: Some(WrapMode::Reflow),
            blank_lines: BlankLines::Collapse,
            formatters: HashMap::new(), // Opt-in: empty by default
            linters: HashMap::new(),    // Opt-in: empty by default
            external_max_parallel: default_external_max_parallel(),
            parser: PandocCompat::default(),
            lint: LintConfig::default(),
            cache_dir: None,
            built_in_greedy_wrap: true,
            exclude: None,
            extend_exclude: Vec::new(),
            include: None,
            extend_include: Vec::new(),
            flavor_overrides: HashMap::new(),
        }
    }
}

impl Config {
    pub fn parser_options(&self) -> ParserOptions {
        ParserOptions {
            flavor: self.flavor,
            extensions: self.extensions.clone(),
            pandoc_compat: self.parser,
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

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum WrapMode {
    Preserve,
    Reflow,
    Sentence,
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
