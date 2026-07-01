use std::collections::HashMap;
use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};

mod formatter_presets;
mod types;

pub use formatter_presets::FormatterPresetMetadata;
pub use formatter_presets::all_formatter_preset_metadata;
pub use formatter_presets::formatter_preset_names;
pub use formatter_presets::formatter_preset_supported_languages;
pub use formatter_presets::formatter_presets_for_language;
pub use formatter_presets::get_formatter_preset;
pub use panache_formatter::config::FormatterExtensions;
pub use panache_parser::Extensions;
pub use panache_parser::Flavor;
pub use panache_parser::PandocCompat;
pub use panache_parser::ParserOptions;
pub use types::BlankLines;
pub use types::Config;
pub use types::ConfigBuilder;
pub use types::FormatterConfig;
pub use types::FormatterDefinition;
pub use types::FormatterValue;
pub use types::LineEnding;
pub use types::LintConfig;
pub use types::MathDelimiterStyle;
pub use types::NoBreakAbbreviations;
pub use types::TabStopMode;
pub use types::WrapMode;

// Globset forms (the engine `GlobMatcher` is built on): `**/<dir>/**` excludes
// a directory of that name at any depth and everything under it, mirroring the
// gitignore semantics these patterns previously had. User-written patterns may
// still use the shorter gitignore style (`target/`, `*.md`); `GlobMatcher`
// normalizes them the same way (see `expand_glob_pattern`).
pub const DEFAULT_EXCLUDE_PATTERNS: &[&str] = &[
    "**/.Rproj.user/**",
    "**/.bzr/**",
    "**/.cache/**",
    "**/.devevn/**",
    "**/.direnv/**",
    "**/.git/**",
    "**/.hg/**",
    "**/.julia/**",
    "**/.mypy_cache/**",
    "**/.nox/**",
    "**/.pytest_cache/**",
    "**/.ruff_cache/**",
    "**/.svn/**",
    "**/.tmp/**",
    "**/.tox/**",
    "**/.venv/**",
    "**/.vscode/**",
    "**/_book/**",
    "**/_build/**",
    "**/_freeze/**",
    "**/_site/**",
    "**/build/**",
    "**/dist/**",
    "**/node_modules/**",
    "**/renv/**",
    "**/target/**",
    "**/tests/testthat/_snaps/**",
    "**/LICENSE.md",
];

pub const DEFAULT_INCLUDE_PATTERNS: &[&str] = &[
    "**/*.md",
    "**/*.qmd",
    "**/*.Rmd",
    "**/*.rmd",
    "**/*.Rmarkdown",
    "**/*.rmarkdown",
    "**/*.markdown",
    "**/*.mdown",
    "**/*.mkd",
    // `.svelte.md` is already covered by the `**/*.md` glob above; only the bare
    // `.svx` extension needs its own pattern.
    "**/*.svx",
];

const CANDIDATE_NAMES: &[&str] = &[".panache.toml", "panache.toml"];
const MARKDOWN_FAMILY_EXTENSIONS: &[&str] = &["md", "markdown", "mdown", "mkd"];

fn check_deprecated_extension_names(s: &str, path: &Path) {
    let Ok(toml_value) = toml::from_str::<toml::Value>(s) else {
        return;
    };

    let Some(extensions_table) = toml_value
        .as_table()
        .and_then(|t| t.get("extensions"))
        .and_then(|v| v.as_table())
    else {
        return;
    };

    let deprecated_names: Vec<&str> = extensions_table
        .keys()
        .filter(|k| k.contains('_'))
        .map(|k| k.as_str())
        .collect();

    if !deprecated_names.is_empty() {
        eprintln!(
            "Warning: Deprecated snake_case extension names found in {}:",
            path.display()
        );
        eprintln!("  The following extensions use deprecated snake_case naming:");
        for name in &deprecated_names {
            let kebab = name.replace('_', "-");
            eprintln!("    {} -> {} (use kebab-case)", name, kebab);
        }
        eprintln!("  Snake_case extension names are deprecated and will be removed in v1.0.0.");
        eprintln!(
            "  Please update your config to use kebab-case (e.g., quarto-crossrefs instead of quarto_crossrefs)."
        );
    }
}

fn check_deprecated_formatter_names(s: &str, path: &Path) {
    let Ok(toml_value) = toml::from_str::<toml::Value>(s) else {
        return;
    };

    let Some(formatters_table) = toml_value
        .as_table()
        .and_then(|t| t.get("formatters"))
        .and_then(|v| v.as_table())
    else {
        return;
    };

    let mut found_deprecated = false;
    for (formatter_name, formatter_value) in formatters_table {
        if let Some(formatter_def) = formatter_value.as_table() {
            let deprecated_fields: Vec<&str> = formatter_def
                .keys()
                .filter(|k| matches!(k.as_str(), "prepend_args" | "append_args"))
                .map(|k| k.as_str())
                .collect();

            if !deprecated_fields.is_empty() {
                if !found_deprecated {
                    eprintln!(
                        "Warning: Deprecated snake_case formatter field names found in {}:",
                        path.display()
                    );
                    found_deprecated = true;
                }
                eprintln!("  In [formatters.{}]:", formatter_name);
                for field in deprecated_fields {
                    let kebab = field.replace('_', "-");
                    eprintln!("    {} -> {}", field, kebab);
                }
            }
        }
    }

    if found_deprecated {
        eprintln!(
            "  Snake_case formatter field names are deprecated and will be removed in v1.0.0."
        );
        eprintln!(
            "  Please update your config to use kebab-case (e.g., prepend-args instead of prepend_args)."
        );
    }
}

fn check_deprecated_code_block_style_options(s: &str, path: &Path) {
    let Ok(toml_value) = toml::from_str::<toml::Value>(s) else {
        return;
    };
    let Some(root) = toml_value.as_table() else {
        return;
    };

    let top_level = root.contains_key("code-blocks");
    let format_nested = root
        .get("format")
        .and_then(|v| v.as_table())
        .is_some_and(|format| format.contains_key("code-blocks"));
    let style_nested = root
        .get("style")
        .and_then(|v| v.as_table())
        .is_some_and(|style| style.contains_key("code-blocks"));

    if top_level || format_nested || style_nested {
        eprintln!(
            "Warning: Deprecated code block style options found in {}:",
            path.display()
        );
        if format_nested {
            eprintln!("  - [format.code-blocks]");
        }
        if top_level {
            eprintln!("  - [code-blocks]");
        }
        if style_nested {
            eprintln!("  - [style.code-blocks]");
        }
        eprintln!("  These options are now no-ops and will be removed in a future release.");
    }
}

fn check_deprecated_blank_lines(s: &str, path: &Path) {
    let Ok(toml_value) = toml::from_str::<toml::Value>(s) else {
        return;
    };
    let Some(root) = toml_value.as_table() else {
        return;
    };

    fn has_blank_lines(table: &toml::map::Map<String, toml::Value>) -> bool {
        table.contains_key("blank-lines") || table.contains_key("blank_lines")
    }

    let top_level = has_blank_lines(root);
    let format_nested = root
        .get("format")
        .and_then(|v| v.as_table())
        .is_some_and(has_blank_lines);
    let style_nested = root
        .get("style")
        .and_then(|v| v.as_table())
        .is_some_and(has_blank_lines);

    if top_level || format_nested || style_nested {
        eprintln!(
            "Warning: Deprecated `blank-lines` setting found in {}:",
            path.display()
        );
        if format_nested {
            eprintln!("  - [format] blank-lines");
        }
        if top_level {
            eprintln!("  - blank-lines (top-level)");
        }
        if style_nested {
            eprintln!("  - [style] blank-lines");
        }
        eprintln!("  This option is now a no-op and will be removed in a future release.");
    }
}

/// A config file that was found but could not be parsed.
///
/// Unlike a plain [`io::Error`] string, this preserves the structured pieces a
/// rich consumer needs: the offending file's `path`, the optional byte `span`
/// of the error within that file (from `toml`'s parser, used by the LSP to
/// anchor a diagnostic), and the underlying `message`. It is embedded as the
/// source of the [`io::Error`] that [`load`] returns, so CLI callers print it
/// as before while the LSP can recover the span via
/// [`io::Error::get_ref`] + `downcast_ref::<ConfigError>()`.
#[derive(Clone)]
pub struct ConfigError {
    /// The config file that failed to parse.
    pub path: PathBuf,
    /// Byte range of the error within the file, when the parser reports one.
    pub span: Option<Range<usize>>,
    /// The underlying parser/validation message (without the `invalid config
    /// <path>:` prefix that [`fmt::Display`] adds).
    pub message: String,
}

// `main() -> io::Result<()>` prints a returned error via `Debug`, so mirror
// `Display` here to keep the CLI's `Error: invalid config ...: ...` message
// readable instead of dumping the struct fields.
impl fmt::Debug for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid config {}: {}",
            self.path.display(),
            self.message
        )
    }
}

impl std::error::Error for ConfigError {}

impl From<ConfigError> for io::Error {
    fn from(err: ConfigError) -> Self {
        io::Error::new(io::ErrorKind::InvalidData, err)
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn parse_config_str(s: &str, path: &Path) -> io::Result<Config> {
    parse_config_detailed(s, path).map_err(io::Error::from)
}

/// Parse a config file's contents, preserving the structured [`ConfigError`] on
/// failure. [`parse_config_str`] wraps the error into an [`io::Error`] for the
/// existing `io::Result` callers.
fn parse_config_detailed(s: &str, path: &Path) -> Result<Config, ConfigError> {
    check_deprecated_extension_names(s, path);
    check_deprecated_formatter_names(s, path);
    check_deprecated_code_block_style_options(s, path);
    check_deprecated_blank_lines(s, path);

    if let Err(msg) = validate_extension_names(s) {
        return Err(ConfigError {
            path: path.to_path_buf(),
            span: None,
            message: msg,
        });
    }

    toml::from_str(s).map_err(|e| ConfigError {
        path: path.to_path_buf(),
        span: e.span(),
        message: e.to_string(),
    })
}

/// True if `name` is a known extension at either the parser or formatter
/// layer (since users write both under a single `[extensions]` table).
fn is_known_extension_name(name: &str) -> bool {
    Extensions::is_known_name(name) || FormatterExtensions::is_known_name(name)
}

/// All extension names users may legally write, sorted and de-duplicated.
/// Cached as a `Vec` so callers can `binary_search` and so the JSON Schema
/// generator can emit the list deterministically.
fn all_known_extension_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = Extensions::KNOWN_NAMES
        .iter()
        .chain(FormatterExtensions::KNOWN_NAMES.iter())
        .copied()
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// All flavor names users may use as `[extensions.<flavor>]` subtable keys.
const KNOWN_FLAVOR_KEYS: &[&str] = &[
    "pandoc",
    "quarto",
    "rmarkdown",
    "r-markdown",
    "gfm",
    "commonmark",
    "common-mark",
    "multimarkdown",
    "multi-markdown",
    "mdsvex",
    "myst",
];

/// Suggest the closest valid name from `candidates` for an unknown `input`
/// using a small edit-distance budget. Returns `None` when nothing close
/// enough is found.
fn closest_match<'a>(input: &str, candidates: &[&'a str]) -> Option<&'a str> {
    fn edit_distance(a: &str, b: &str) -> usize {
        let (a, b) = (a.as_bytes(), b.as_bytes());
        let mut prev: Vec<usize> = (0..=b.len()).collect();
        let mut curr = vec![0; b.len() + 1];
        for (i, &ai) in a.iter().enumerate() {
            curr[0] = i + 1;
            for (j, &bj) in b.iter().enumerate() {
                let cost = if ai == bj { 0 } else { 1 };
                curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
            }
            std::mem::swap(&mut prev, &mut curr);
        }
        prev[b.len()]
    }

    let normalized = input.replace('_', "-");
    let budget = 3.min(normalized.len() / 3 + 1);
    candidates
        .iter()
        .map(|c| (*c, edit_distance(&normalized, c)))
        .filter(|(_, d)| *d <= budget)
        .min_by_key(|(_, d)| *d)
        .map(|(c, _)| c)
}

/// Walk the raw TOML `[extensions]` table and reject unknown extension
/// names (both at the top level and inside per-flavor subtables) and unknown
/// per-flavor subtable keys. Returns the user-facing error text on failure.
fn validate_extension_names(s: &str) -> Result<(), String> {
    let Ok(value) = toml::from_str::<toml::Value>(s) else {
        // Real TOML parse error — serde will surface it.
        return Ok(());
    };

    let Some(ext_table) = value
        .as_table()
        .and_then(|t| t.get("extensions"))
        .and_then(|v| v.as_table())
    else {
        return Ok(());
    };

    let known_exts = all_known_extension_names();

    for (key, val) in ext_table {
        match val {
            toml::Value::Boolean(_) => {
                if !is_known_extension_name(key) {
                    return Err(unknown_extension_error(key, &known_exts, None));
                }
            }
            toml::Value::Table(flavor_table) => {
                if parse_flavor_key(key).is_none() {
                    return Err(unknown_flavor_subtable_error(key));
                }
                for sub_key in flavor_table.keys() {
                    if !is_known_extension_name(sub_key) {
                        return Err(unknown_extension_error(sub_key, &known_exts, Some(key)));
                    }
                }
            }
            _ => {
                // Wrong-shape entries are non-fatal: `resolve_extensions_for_flavor`
                // still emits a warning and skips them, matching legacy behavior.
            }
        }
    }

    Ok(())
}

fn unknown_extension_error(name: &str, known: &[&str], in_flavor: Option<&str>) -> String {
    let mut msg = match in_flavor {
        Some(f) => format!("unknown extension `{name}` in [extensions.{f}]"),
        None => format!("unknown extension `{name}` in [extensions]"),
    };
    if let Some(suggestion) = closest_match(name, known) {
        msg.push_str(&format!("; did you mean `{suggestion}`?"));
    }
    msg
}

fn unknown_flavor_subtable_error(name: &str) -> String {
    let mut msg = format!("unknown flavor subtable [extensions.{name}]");
    if let Some(suggestion) = closest_match(name, KNOWN_FLAVOR_KEYS) {
        msg.push_str(&format!("; did you mean `[extensions.{suggestion}]`?"));
    }
    msg
}

/// Read `path`, resolving any Ruff-style `extend` chain, and return the
/// finalized [`Config`], the merged raw `[extensions]` value (so
/// [`apply_flavor`] can re-resolve extensions against the chosen flavor without
/// re-reading disk), and the canonical paths of every file that contributed
/// (leaf first, roots last) so the LSP can watch them.
///
/// The common no-`extend` case takes a fast path that deserializes straight from
/// the original string, preserving byte-accurate error spans. Only configs that
/// actually declare `extend` pay for the raw-table merge (which loses spans,
/// since a merged table has no single source file to point into).
fn read_config_with_chain(
    path: &Path,
) -> Result<(Config, Option<toml::Value>, Vec<PathBuf>), ConfigError> {
    log::debug!("Reading config from: {}", path.display());
    let s = fs::read_to_string(path).map_err(|e| ConfigError {
        path: path.to_path_buf(),
        span: None,
        message: e.to_string(),
    })?;

    let table = toml::from_str::<toml::Table>(&s).ok();
    let has_extend = table.as_ref().is_some_and(|t| t.contains_key("extend"));

    if !has_extend {
        let config = parse_config_detailed(&s, path)?;
        let extensions = table.and_then(|t| t.get("extensions").cloned());
        log::debug!("Loaded config from: {}", path.display());
        return Ok((config, extensions, vec![canonical(path)]));
    }

    let mut chain = Vec::new();
    let merged = load_merged_toml(path, &mut chain)?;
    let config = finalize_merged_table(&merged, path)?;
    let extensions = merged.get("extensions").cloned();
    log::debug!(
        "Loaded config from: {} (extends {} file(s))",
        path.display(),
        chain.len().saturating_sub(1)
    );
    Ok((config, extensions, chain))
}

/// Canonicalize for stable identity comparisons, falling back to the path as
/// given when the file can't be resolved (already-reported errors handle the
/// truly-missing case).
fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Read `path` and recursively fold in its `extend` chain, returning the
/// deep-merged raw TOML table (child keys override parents). Appends each
/// visited file's canonical path to `chain` (leaf first). Errors on cycles and
/// on missing/unreadable extended files.
fn load_merged_toml(path: &Path, chain: &mut Vec<PathBuf>) -> Result<toml::Table, ConfigError> {
    let canon = canonical(path);
    if chain.contains(&canon) {
        let names: Vec<String> = chain
            .iter()
            .chain(std::iter::once(&canon))
            .map(|p| p.display().to_string())
            .collect();
        return Err(ConfigError {
            path: path.to_path_buf(),
            span: None,
            message: format!(
                "Circular configuration detected: {}",
                names.join(" extends ")
            ),
        });
    }
    chain.push(canon);

    let s = fs::read_to_string(path).map_err(|e| ConfigError {
        path: path.to_path_buf(),
        span: None,
        message: format!("failed to read extended config: {e}"),
    })?;

    // Per-file deprecation/validation checks so warnings carry this file's path.
    check_deprecated_extension_names(&s, path);
    check_deprecated_formatter_names(&s, path);
    check_deprecated_code_block_style_options(&s, path);
    check_deprecated_blank_lines(&s, path);
    if let Err(msg) = validate_extension_names(&s) {
        return Err(ConfigError {
            path: path.to_path_buf(),
            span: None,
            message: msg,
        });
    }

    let mut table = toml::from_str::<toml::Table>(&s).map_err(|e| ConfigError {
        path: path.to_path_buf(),
        span: e.span(),
        message: e.to_string(),
    })?;

    if let Some(extend_val) = table.get("extend") {
        let extend_str = extend_val.as_str().ok_or_else(|| ConfigError {
            path: path.to_path_buf(),
            span: None,
            message: "`extend` must be a string path to another config file".to_string(),
        })?;
        let base_path = resolve_extend_path(extend_str, path);
        // The base is merged first; the current file's keys then override it.
        let mut base = load_merged_toml(&base_path, chain)?;
        merge_toml_tables(&mut base, table);
        table = base;
    }

    Ok(table)
}

/// Resolve an `extend` value against the directory of the file that declares it
/// (not CWD), expanding a leading `~`. Absolute paths are used as-is.
fn resolve_extend_path(extend: &str, from_file: &Path) -> PathBuf {
    let expanded = expand_tilde(extend);
    if expanded.is_absolute() {
        return expanded;
    }
    from_file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(expanded)
}

fn expand_tilde(path: &str) -> PathBuf {
    if path == "~"
        && let Ok(home) = env::var("HOME")
    {
        return PathBuf::from(home);
    }
    if let Some(rest) = path.strip_prefix("~/")
        && let Ok(home) = env::var("HOME")
    {
        return Path::new(&home).join(rest);
    }
    PathBuf::from(path)
}

/// Deep-merge `over` onto `base`, with `over` (the extending, more-derived
/// config) winning. Sub-tables recurse so a partial override (e.g. one
/// `[format]` key) keeps the base's sibling keys. The additive `extend-exclude`
/// / `extend-include` arrays concatenate across the chain; every other value
/// (scalars, plain `exclude`/`include` arrays) is replaced.
fn merge_toml_tables(base: &mut toml::Table, over: toml::Table) {
    for (key, over_val) in over {
        if !base.contains_key(&key) {
            base.insert(key, over_val);
            continue;
        }
        let additive = key == "extend-exclude" || key == "extend-include";
        let base_val = base.get_mut(&key).expect("key present");
        match over_val {
            toml::Value::Array(over_arr) if additive && base_val.is_array() => {
                base_val
                    .as_array_mut()
                    .expect("checked is_array")
                    .extend(over_arr);
            }
            toml::Value::Table(over_tbl) if base_val.is_table() => {
                merge_toml_tables(base_val.as_table_mut().expect("checked is_table"), over_tbl);
            }
            other => *base_val = other,
        }
    }
}

/// Deserialize a merged raw table into a finalized [`Config`]. Spans are lost
/// (the merge has no single source file), so errors point at the leaf file.
fn finalize_merged_table(table: &toml::Table, leaf: &Path) -> Result<Config, ConfigError> {
    toml::Value::Table(table.clone())
        .try_into::<Config>()
        .map_err(|e| ConfigError {
            path: leaf.to_path_buf(),
            span: None,
            message: e.to_string(),
        })
}

/// Walk up from `start_dir` looking for a `panache.toml` / `.panache.toml`.
///
/// `boundary`, when set, caps the walk: the boundary directory itself is
/// searched, but ancestors above it are not. Callers normally derive this
/// from [`project_boundary`] so that discovery stops at the project root
/// (the nearest `.git` ancestor) instead of leaking into unrelated
/// directories like `/tmp` or `$HOME`.
fn find_in_tree(start_dir: &Path, boundary: Option<&Path>) -> Option<PathBuf> {
    for dir in start_dir.ancestors() {
        for name in CANDIDATE_NAMES {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
        // The dot-config convention: `<dir>/.config/panache.toml`. Checked
        // *after* the bare names so a top-level `panache.toml` wins within the
        // same directory; the per-directory ascent still makes the nearest
        // config win across directories.
        let nested = dir.join(".config").join("panache.toml");
        if nested.is_file() {
            return Some(nested);
        }
        if matches!(boundary, Some(b) if dir == b) {
            return None;
        }
    }
    None
}

/// Find the project root by walking up from `start_dir` looking for `.git`.
///
/// Both regular repositories (`.git/` directory) and worktrees (`.git` file)
/// count. Returns `None` if no `.git` ancestor exists; callers then fall
/// back to today's unbounded walk, which is acceptable for the rare
/// standalone-file case.
fn project_boundary(start_dir: &Path) -> Option<PathBuf> {
    for dir in start_dir.ancestors() {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
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

/// Which configuration source [`load`] resolved, carrying its path.
///
/// The directory of the carried path is where relative globs declared in that
/// config anchor (see [`anchor_dir`]) — except for [`ConfigSource::Global`],
/// the XDG user config, which has no project location and therefore no anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    /// Loaded from an explicit `--config <path>`.
    Explicit(PathBuf),
    /// Discovered by walking up the directory tree from the input.
    Discovered(PathBuf),
    /// The global `~/.config/panache/config.toml` (XDG user config).
    Global(PathBuf),
    /// No config file found; built-in defaults are in use.
    None,
}

impl ConfigSource {
    /// Path of the resolved config file, if any.
    pub fn path(&self) -> Option<&Path> {
        match self {
            ConfigSource::Explicit(p) | ConfigSource::Discovered(p) | ConfigSource::Global(p) => {
                Some(p)
            }
            ConfigSource::None => None,
        }
    }

    /// The project directory that relative globs in this config anchor against
    /// (its own directory, with a `.config/` wrapper unwrapped to the project
    /// root). `None` for the global XDG config and the no-config case, which
    /// have no project location.
    pub fn project_anchor(&self) -> Option<PathBuf> {
        match self {
            ConfigSource::Explicit(p) | ConfigSource::Discovered(p) => {
                p.parent().map(unwrap_dot_config)
            }
            ConfigSource::Global(_) | ConfigSource::None => None,
        }
    }
}

pub fn load(
    explicit: Option<&Path>,
    start_dir: &Path,
    input_file: Option<&Path>,
    flavor_override: Option<Flavor>,
) -> io::Result<(Config, ConfigSource)> {
    let (cfg, source, _chain) = load_with_chain(explicit, start_dir, input_file, flavor_override)?;
    Ok((cfg, source))
}

/// Like [`load`], but also returns the canonical paths of every config file
/// that contributed (the resolved file plus its transitive `extend` chain).
/// The LSP uses this to watch base configs so open documents reload when an
/// extended file changes; CLI callers ignore it via [`load`].
pub fn load_with_chain(
    explicit: Option<&Path>,
    start_dir: &Path,
    input_file: Option<&Path>,
    flavor_override: Option<Flavor>,
) -> io::Result<(Config, ConfigSource, Vec<PathBuf>)> {
    let boundary = project_boundary(start_dir);
    let (mut cfg, source, extensions, chain) = if let Some(path) = explicit {
        let (cfg, ext, chain) = read_config_with_chain(path).map_err(io::Error::from)?;
        (cfg, ConfigSource::Explicit(path.to_path_buf()), ext, chain)
    } else if let Some(p) = find_in_tree(start_dir, boundary.as_deref()) {
        // A discovered config that fails to parse is fatal: it is the config
        // that *would* apply, so silently falling through to the global/default
        // config (the old `&& let Ok(cfg)` behavior) let a typo'd project
        // `panache.toml` be ignored by both the CLI and the LSP.
        let (cfg, ext, chain) = read_config_with_chain(&p).map_err(io::Error::from)?;
        (cfg, ConfigSource::Discovered(p), ext, chain)
    } else if let Some(p) = xdg_config_path()
        && let Ok((cfg, ext, chain)) = read_config_with_chain(&p)
    {
        (cfg, ConfigSource::Global(p), ext, chain)
    } else {
        log::debug!("No config file found, using defaults");
        (Config::default(), ConfigSource::None, None, Vec::new())
    };

    let anchor = source.project_anchor();
    let resolved_flavor =
        flavor_override.or_else(|| detect_flavor(input_file, anchor.as_deref(), &cfg));

    if let Some(flavor) = resolved_flavor {
        apply_flavor(&mut cfg, flavor, extensions.as_ref());
    }

    Ok((cfg, source, chain))
}

/// Re-resolve flavor-dependent extensions from the already-merged raw
/// `[extensions]` value. Passing the merged value (rather than re-reading the
/// config file) keeps `extend`ed base extensions in play and avoids a second
/// disk read. `None` means no `[extensions]` table, so flavor defaults apply.
fn apply_flavor(cfg: &mut Config, flavor: Flavor, extensions: Option<&toml::Value>) {
    cfg.flavor = flavor;
    cfg.extensions = resolve_extensions_for_flavor(extensions, flavor);
    cfg.formatter_extensions = resolve_formatter_extensions_for_flavor(extensions, flavor);
}

fn parse_flavor_key(s: &str) -> Option<Flavor> {
    match s.replace('_', "-").to_lowercase().as_str() {
        "pandoc" => Some(Flavor::Pandoc),
        "quarto" => Some(Flavor::Quarto),
        "rmarkdown" | "r-markdown" => Some(Flavor::RMarkdown),
        "gfm" => Some(Flavor::Gfm),
        "common-mark" | "commonmark" => Some(Flavor::CommonMark),
        "multimarkdown" | "multi-markdown" => Some(Flavor::MultiMarkdown),
        "mdsvex" => Some(Flavor::Mdsvex),
        "myst" => Some(Flavor::Myst),
        _ => None,
    }
}

fn resolve_extensions_for_flavor(
    extensions_value: Option<&toml::Value>,
    flavor: Flavor,
) -> Extensions {
    let Some(value) = extensions_value else {
        return Extensions::for_flavor(flavor);
    };

    let Some(table) = value.as_table() else {
        eprintln!("Warning: [extensions] must be a table; using flavor defaults.");
        return Extensions::for_flavor(flavor);
    };

    let mut global_overrides = HashMap::new();
    let mut flavor_overrides = HashMap::new();

    for (key, val) in table {
        if let Some(enabled) = val.as_bool() {
            global_overrides.insert(key.clone(), enabled);
            continue;
        }

        let Some(flavor_table) = val.as_table() else {
            eprintln!(
                "Warning: [extensions] entry '{}' must be a boolean or table; ignoring.",
                key
            );
            continue;
        };

        let Some(target_flavor) = parse_flavor_key(key) else {
            eprintln!(
                "Warning: [extensions.{}] is not a known flavor table; ignoring.",
                key
            );
            continue;
        };

        if target_flavor != flavor {
            continue;
        }

        for (sub_key, sub_val) in flavor_table {
            let Some(enabled) = sub_val.as_bool() else {
                eprintln!(
                    "Warning: [extensions.{}] entry '{}' must be true or false; ignoring.",
                    key, sub_key
                );
                continue;
            };
            flavor_overrides.insert(sub_key.clone(), enabled);
        }
    }

    global_overrides.extend(flavor_overrides);
    Extensions::merge_with_flavor(global_overrides, flavor)
}

fn resolve_formatter_extensions_for_flavor(
    extensions_value: Option<&toml::Value>,
    flavor: Flavor,
) -> FormatterExtensions {
    let Some(value) = extensions_value else {
        return FormatterExtensions::for_flavor(flavor);
    };

    let Some(table) = value.as_table() else {
        eprintln!("Warning: [extensions] must be a table; using flavor defaults.");
        return FormatterExtensions::for_flavor(flavor);
    };

    let mut global_overrides = HashMap::new();
    let mut flavor_overrides = HashMap::new();

    for (key, val) in table {
        if let Some(enabled) = val.as_bool() {
            global_overrides.insert(key.clone(), enabled);
            continue;
        }

        let Some(flavor_table) = val.as_table() else {
            eprintln!(
                "Warning: [extensions] entry '{}' must be a boolean or table; ignoring.",
                key
            );
            continue;
        };

        let Some(target_flavor) = parse_flavor_key(key) else {
            eprintln!(
                "Warning: [extensions.{}] is not a known flavor table; ignoring.",
                key
            );
            continue;
        };

        if target_flavor != flavor {
            continue;
        }

        for (sub_key, sub_val) in flavor_table {
            let Some(enabled) = sub_val.as_bool() else {
                eprintln!(
                    "Warning: [extensions.{}] entry '{}' must be true or false; ignoring.",
                    key, sub_key
                );
                continue;
            };
            flavor_overrides.insert(sub_key.clone(), enabled);
        }
    }

    global_overrides.extend(flavor_overrides);
    FormatterExtensions::merge_with_flavor(global_overrides, flavor)
}

/// Extension-based flavor detection for fallback callers that don't run the
/// full config walk: the LSP no-config default (`default_config_for_uri`) and
/// the CLI `--isolated` path. Both previously hand-rolled a reduced match that
/// silently omitted mdsvex; delegating here keeps the recognized extension set
/// (including the compound `.svelte.md`) in lockstep with the canonical
/// [`detect_flavor`].
pub fn detect_flavor_from_path(input_file: &Path, cfg: &Config) -> Option<Flavor> {
    detect_flavor(Some(input_file), None, cfg)
}

fn detect_flavor(input_file: Option<&Path>, anchor: Option<&Path>, cfg: &Config) -> Option<Flavor> {
    let input_path = input_file?;

    // Quarto project manifests are `.yml`, but the filename is itself a Quarto
    // marker (Quarto is their only consumer), so they detect as Quarto the same
    // way `.qmd` does — an explicit `--flavor`/`flavor-overrides` still wins
    // upstream. See `linter::quarto_schema::manifest_schema_root`.
    if is_quarto_manifest_filename(input_path) {
        return Some(Flavor::Quarto);
    }

    // mdsvex uses both `.svx` and the compound `.svelte.md`. The latter ends in
    // `.md`, so check the full file name before the single-extension match below
    // routes it into the Markdown family. Plain `.svelte` is a code component,
    // not Markdown, so it is intentionally left unmapped.
    if let Some(name) = input_path.file_name().and_then(|n| n.to_str())
        && name.to_lowercase().ends_with(".svelte.md")
    {
        return Some(Flavor::Mdsvex);
    }

    let ext = input_path.extension().and_then(|e| e.to_str())?;
    let ext_lower = ext.to_lowercase();

    match ext_lower.as_str() {
        "qmd" => Some(Flavor::Quarto),
        "rmd" | "rmarkdown" => Some(Flavor::RMarkdown),
        "svx" => Some(Flavor::Mdsvex),
        _ if MARKDOWN_FAMILY_EXTENSIONS.contains(&ext_lower.as_str()) => {
            let override_flavor = detect_flavor_override(input_path, anchor, &cfg.flavor_overrides);
            Some(override_flavor.unwrap_or(cfg.flavor))
        }
        _ => None,
    }
}

/// Whether `path`'s file name is a Quarto project manifest (`_quarto.yml` or
/// `_metadata.yml`). Kept in lockstep with
/// `linter::quarto_schema::manifest_schema_root`, which maps the same names to
/// their schema roots.
fn is_quarto_manifest_filename(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|n| n.to_str()),
        Some("_quarto.yml" | "_metadata.yml")
    )
}

fn detect_flavor_override(
    input_path: &Path,
    base_dir: Option<&Path>,
    overrides: &HashMap<String, Flavor>,
) -> Option<Flavor> {
    if overrides.is_empty() {
        return None;
    }

    let full_path = normalize_path_for_matching(input_path);
    let rel_path = base_dir
        .and_then(|base| input_path.strip_prefix(base).ok())
        .map(normalize_path_for_matching);
    let file_name = input_path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_string());

    let mut best: Option<((usize, usize, usize), Flavor)> = None;
    for (pattern, flavor) in overrides {
        let matched = glob_matches_path(pattern, &full_path)
            || rel_path
                .as_deref()
                .is_some_and(|relative| glob_matches_path(pattern, relative))
            || file_name
                .as_deref()
                .is_some_and(|name| glob_matches_path(pattern, name));
        if !matched {
            continue;
        }

        let score = pattern_specificity(pattern);
        if best.is_none_or(|(best_score, _)| score > best_score) {
            best = Some((score, *flavor));
        }
    }

    best.map(|(_, flavor)| flavor)
}

fn normalize_path_for_matching(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn pattern_specificity(pattern: &str) -> (usize, usize, usize) {
    let literal_len = pattern
        .chars()
        .filter(|c| !matches!(c, '*' | '?' | '[' | ']' | '{' | '}'))
        .count();
    let wildcard_count = pattern
        .chars()
        .filter(|c| matches!(c, '*' | '?' | '[' | ']' | '{' | '}'))
        .count();
    let depth = pattern.matches('/').count();
    (literal_len, usize::MAX - wildcard_count, depth)
}

fn glob_matches_path(pattern: &str, candidate: &str) -> bool {
    let Ok(glob) = globset::GlobBuilder::new(pattern)
        .literal_separator(true)
        .backslash_escape(true)
        .build()
    else {
        return false;
    };
    glob.compile_matcher().is_match(candidate)
}

/// `<dir>/.config` → `<dir>` (the dot-config convention is purely cosmetic);
/// any other directory is returned unchanged. Literal final-component check —
/// no canonicalization — to match [`find_in_tree`]'s literal `.config` probe.
fn unwrap_dot_config(dir: &Path) -> PathBuf {
    if dir.file_name().and_then(|n| n.to_str()) == Some(".config")
        && let Some(parent) = dir.parent()
    {
        return parent.to_path_buf();
    }
    dir.to_path_buf()
}

/// Directory that relative globs declared in `source` anchor against (the
/// single rule shared by `flavor-overrides` and `exclude`/`include`).
///
/// A discovered or explicit config anchors at its own directory, with a
/// `.config/` wrapper unwrapped to the project root so a `.config/panache.toml`
/// behaves exactly like a `panache.toml` in the directory above it. The global
/// XDG user config has no project location, so it (and the no-config case) fall
/// back to `fallback` — the cwd for the CLI, or the input file's directory for
/// the LSP.
pub fn anchor_dir(source: &ConfigSource, fallback: &Path) -> PathBuf {
    source
        .project_anchor()
        .unwrap_or_else(|| fallback.to_path_buf())
}

/// Expand one user/default glob into globset patterns, layering gitignore-style
/// ergonomics on top of `globset` (which, with `literal_separator(true)`, never
/// lets `*` cross `/` and does not treat a bare name as "at any depth").
///
/// - bare name (`*.md`, `target`) → `**/<name>` (any depth) and `**/<name>/**`
///   (contents, when it names a directory)
/// - trailing slash (`tests/`) → `**/<name>/**` (directory contents only; the
///   directory entry itself is never tested during traversal)
/// - embedded slash (`docs/**/*.qmd`, `a/b/`) → anchored at the config dir,
///   plus a `/**` contents variant
///
/// Already-explicit patterns like `**/target/**` contain a slash, so they hit
/// the anchored branch and are preserved as-is (the extra `/**` variant is
/// harmless), keeping the rule idempotent over the rewritten defaults.
fn expand_glob_pattern(pattern: &str, out: &mut Vec<String>) {
    let core = pattern.trim_end_matches('/');
    if core.is_empty() {
        return;
    }
    let had_trailing_slash = pattern.ends_with('/');
    let anchored = core.contains('/');
    match (had_trailing_slash, anchored) {
        (true, true) => out.push(format!("{core}/**")),
        (true, false) => out.push(format!("**/{core}/**")),
        (false, true) => {
            out.push(core.to_string());
            out.push(format!("{core}/**"));
        }
        (false, false) => {
            out.push(format!("**/{core}"));
            out.push(format!("**/{core}/**"));
        }
    }
}

/// A set of `exclude`/`include` globs, anchored at a config directory and
/// matched against config-directory-relative, forward-slashed paths. Backed by
/// `globset` (the single engine shared with `flavor-overrides`); negation
/// (`!pattern`) is intentionally unsupported.
pub struct GlobMatcher {
    set: globset::GlobSet,
}

impl GlobMatcher {
    /// Compile `patterns` (gitignore-style; see [`expand_glob_pattern`]).
    pub fn build(patterns: &[String]) -> Result<Self, globset::Error> {
        let mut builder = globset::GlobSetBuilder::new();
        let mut expanded = Vec::new();
        for pattern in patterns {
            expanded.clear();
            expand_glob_pattern(pattern, &mut expanded);
            for glob in &expanded {
                builder.add(
                    globset::GlobBuilder::new(glob)
                        .literal_separator(true)
                        .backslash_escape(true)
                        .build()?,
                );
            }
        }
        Ok(Self {
            set: builder.build()?,
        })
    }

    /// Whether `rel` (a config-dir-relative, forward-slashed path) matches.
    pub fn is_match(&self, rel: &str) -> bool {
        self.set.is_match(rel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_flavor_maps_rmarkdown_extension() {
        let cfg = Config::default();
        let detected = detect_flavor(Some(Path::new("doc.rmarkdown")), None, &cfg);
        assert_eq!(detected, Some(Flavor::RMarkdown));
    }

    #[test]
    fn detect_flavor_maps_mixed_case_rmarkdown_extension() {
        let cfg = Config::default();
        let detected = detect_flavor(Some(Path::new("doc.Rmarkdown")), None, &cfg);
        assert_eq!(detected, Some(Flavor::RMarkdown));
    }

    #[test]
    fn detect_flavor_maps_svx_extension() {
        let cfg = Config::default();
        assert_eq!(
            detect_flavor(Some(Path::new("doc.svx")), None, &cfg),
            Some(Flavor::Mdsvex)
        );
        assert_eq!(
            detect_flavor(Some(Path::new("doc.SVX")), None, &cfg),
            Some(Flavor::Mdsvex)
        );
    }

    #[test]
    fn detect_flavor_maps_compound_svelte_md_extension() {
        let cfg = Config::default();
        assert_eq!(
            detect_flavor(Some(Path::new("page.svelte.md")), None, &cfg),
            Some(Flavor::Mdsvex)
        );
    }

    #[test]
    fn detect_flavor_does_not_map_plain_svelte_extension() {
        // A `.svelte` file is a code component, not Markdown.
        let cfg = Config::default();
        assert_eq!(
            detect_flavor(Some(Path::new("App.svelte")), None, &cfg),
            None
        );
    }

    #[test]
    fn detect_flavor_maps_quarto_manifest_filenames() {
        let cfg = Config::default();
        assert_eq!(
            detect_flavor(Some(Path::new("/p/_quarto.yml")), None, &cfg),
            Some(Flavor::Quarto)
        );
        assert_eq!(
            detect_flavor(Some(Path::new("/p/sub/_metadata.yml")), None, &cfg),
            Some(Flavor::Quarto)
        );
        // A plain `.yml` is not a manifest marker.
        assert_eq!(
            detect_flavor(Some(Path::new("/p/config.yml")), None, &cfg),
            None
        );
    }

    #[test]
    fn flavor_override_beats_manifest_filename() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let manifest = tmp.path().join("_quarto.yml");
        std::fs::write(&manifest, "").unwrap();

        // Explicit `--flavor pandoc` wins over the manifest's Quarto marker,
        // exactly as it does for a `.qmd` document.
        let (cfg, _) = load(None, tmp.path(), Some(&manifest), Some(Flavor::Pandoc)).expect("load");
        assert_eq!(cfg.flavor, Flavor::Pandoc);
    }

    #[test]
    fn flavor_override_beats_extension_inference() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let qmd = tmp.path().join("doc.qmd");
        std::fs::write(&qmd, "").unwrap();

        let (cfg, _) = load(None, tmp.path(), Some(&qmd), Some(Flavor::Pandoc)).expect("load");
        assert_eq!(cfg.flavor, Flavor::Pandoc);
    }

    #[test]
    fn flavor_override_beats_config_flavor_key() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cfg_path = tmp.path().join("panache.toml");
        std::fs::write(&cfg_path, "flavor = \"quarto\"\n").unwrap();

        let (cfg, _) = load(None, tmp.path(), None, Some(Flavor::Gfm)).expect("load");
        assert_eq!(cfg.flavor, Flavor::Gfm);
    }

    #[test]
    fn flavor_override_beats_flavor_overrides_glob() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cfg_path = tmp.path().join("panache.toml");
        std::fs::write(&cfg_path, "[flavor-overrides]\n\"*.md\" = \"quarto\"\n").unwrap();
        let md = tmp.path().join("doc.md");
        std::fs::write(&md, "").unwrap();

        let (cfg, _) = load(None, tmp.path(), Some(&md), Some(Flavor::Gfm)).expect("load");
        assert_eq!(cfg.flavor, Flavor::Gfm);
    }

    #[test]
    fn flavor_override_dot_config_anchors_at_project_root() {
        // A `.config/panache.toml` flavor-override glob must resolve relative to
        // the project root (the dir above `.config/`), so `docs/*.md` matches a
        // `docs/x.md` at the root — not `docs/` under `.config/`.
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join(".config")).unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(
            root.join(".config").join("panache.toml"),
            "[flavor-overrides]\n\"docs/*.md\" = \"quarto\"\n",
        )
        .unwrap();
        let md = root.join("docs").join("x.md");
        std::fs::write(&md, "").unwrap();

        let (cfg, _) = load(None, root, Some(&md), None).expect("load");
        assert_eq!(
            cfg.flavor,
            Flavor::Quarto,
            "`.config/panache.toml` flavor-override globs must anchor at the project root"
        );
    }

    #[test]
    fn flavor_override_still_merges_extensions_overrides() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cfg_path = tmp.path().join("panache.toml");
        // Disable an extension that is normally on for Pandoc.
        std::fs::write(
            &cfg_path,
            "flavor = \"quarto\"\n\n[extensions]\nfenced-divs = false\n",
        )
        .unwrap();

        let (cfg, _) = load(None, tmp.path(), None, Some(Flavor::Pandoc)).expect("load");
        assert_eq!(cfg.flavor, Flavor::Pandoc);
        // The user override turns off fenced_divs even though Pandoc default would enable it.
        assert!(!cfg.extensions.fenced_divs);
    }

    #[test]
    fn flavor_override_uses_overridden_flavor_table() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cfg_path = tmp.path().join("panache.toml");
        // The config's flavor key says quarto, with a quarto-specific override that
        // enables fenced_divs and a pandoc-specific override that disables it.
        // When --flavor pandoc is supplied, only the [extensions.pandoc] table should
        // apply (not the quarto one).
        std::fs::write(
            &cfg_path,
            "flavor = \"quarto\"\n\n\
             [extensions.quarto]\nfenced-divs = true\n\n\
             [extensions.pandoc]\nfenced-divs = false\n",
        )
        .unwrap();

        let (cfg, _) = load(None, tmp.path(), None, Some(Flavor::Pandoc)).expect("load");
        assert_eq!(cfg.flavor, Flavor::Pandoc);
        assert!(!cfg.extensions.fenced_divs);
    }

    #[test]
    fn find_in_tree_stops_at_boundary() {
        let tmp = tempfile::tempdir().expect("tempdir");
        // Place a panache.toml ABOVE the boundary; walking with the boundary
        // set must not return it.
        let outside = tmp.path().join("panache.toml");
        std::fs::write(&outside, "").unwrap();
        let workspace = tmp.path().join("workspace");
        let nested = workspace.join("sub");
        std::fs::create_dir_all(&nested).unwrap();

        let found = find_in_tree(&nested, Some(&workspace));
        assert!(
            found.is_none(),
            "boundary must prevent ascent above workspace, got {found:?}"
        );

        // Without the boundary, the outer config is found (today's CLI behavior).
        let unbounded = find_in_tree(&nested, None);
        assert_eq!(unbounded.as_deref(), Some(outside.as_path()));
    }

    #[test]
    fn find_in_tree_returns_boundary_local_config() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let workspace = tmp.path().join("ws");
        let nested = workspace.join("docs");
        std::fs::create_dir_all(&nested).unwrap();
        let cfg = workspace.join("panache.toml");
        std::fs::write(&cfg, "").unwrap();

        let found = find_in_tree(&nested, Some(&workspace));
        assert_eq!(found.as_deref(), Some(cfg.as_path()));
    }

    #[test]
    fn find_in_tree_discovers_dot_config_panache_toml() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let workspace = tmp.path().join("ws");
        let nested = workspace.join("docs");
        std::fs::create_dir_all(&nested).unwrap();
        let cfg_dir = workspace.join(".config");
        std::fs::create_dir_all(&cfg_dir).unwrap();
        let cfg = cfg_dir.join("panache.toml");
        std::fs::write(&cfg, "").unwrap();

        let found = find_in_tree(&nested, Some(&workspace));
        assert_eq!(found.as_deref(), Some(cfg.as_path()));
    }

    #[test]
    fn find_in_tree_prefers_bare_config_over_dot_config() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let workspace = tmp.path().join("ws");
        std::fs::create_dir_all(workspace.join(".config")).unwrap();
        let bare = workspace.join("panache.toml");
        std::fs::write(&bare, "").unwrap();
        std::fs::write(workspace.join(".config").join("panache.toml"), "").unwrap();

        let found = find_in_tree(&workspace, Some(&workspace));
        assert_eq!(
            found.as_deref(),
            Some(bare.as_path()),
            "a bare panache.toml must win over .config/panache.toml in the same dir"
        );
    }

    #[test]
    fn find_in_tree_prefers_nearest_dot_config_over_ancestor() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let workspace = tmp.path().join("ws");
        let sub = workspace.join("sub");
        std::fs::create_dir_all(sub.join(".config")).unwrap();
        std::fs::write(workspace.join("panache.toml"), "").unwrap();
        let near = sub.join(".config").join("panache.toml");
        std::fs::write(&near, "").unwrap();

        let found = find_in_tree(&sub, Some(&workspace));
        assert_eq!(
            found.as_deref(),
            Some(near.as_path()),
            "a nearer .config/panache.toml must win over an ancestor's panache.toml"
        );
    }

    #[test]
    fn find_in_tree_dot_config_above_boundary_not_inherited() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cfg_dir = tmp.path().join(".config");
        std::fs::create_dir_all(&cfg_dir).unwrap();
        std::fs::write(cfg_dir.join("panache.toml"), "").unwrap();
        let workspace = tmp.path().join("workspace");
        let nested = workspace.join("sub");
        std::fs::create_dir_all(&nested).unwrap();

        let found = find_in_tree(&nested, Some(&workspace));
        assert!(
            found.is_none(),
            "boundary must prevent ascent to a .config above workspace, got {found:?}"
        );
    }

    #[test]
    fn project_boundary_stops_at_git_directory() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path().join("repo");
        let sub = repo.join("src").join("docs");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(repo.join(".git")).unwrap();

        let found = project_boundary(&sub).expect("boundary");
        assert_eq!(found, repo);
    }

    #[test]
    fn project_boundary_accepts_git_file_for_worktrees() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let worktree = tmp.path().join("wt");
        std::fs::create_dir_all(&worktree).unwrap();
        // Worktrees use a `.git` *file* (gitdir pointer), not a directory.
        std::fs::write(worktree.join(".git"), "gitdir: /some/where\n").unwrap();

        let found = project_boundary(&worktree).expect("boundary");
        assert_eq!(found, worktree);
    }

    #[test]
    fn project_boundary_is_none_when_no_git_ancestor() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let sub = tmp.path().join("nogit");
        std::fs::create_dir_all(&sub).unwrap();
        assert!(project_boundary(&sub).is_none());
    }

    #[test]
    fn load_does_not_inherit_config_above_git_root() {
        // A panache.toml above the .git boundary must not be picked up.
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("panache.toml"), "line-width = 7\n").unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let doc = repo.join("doc.qmd");
        std::fs::write(&doc, "").unwrap();

        let (cfg, source) = load(None, &repo, Some(&doc), None).expect("load");
        assert_eq!(
            source,
            ConfigSource::None,
            "must not pick up panache.toml above .git boundary"
        );
        // Sanity check: defaults are used, not line-width=7 from the stray file.
        assert_ne!(cfg.line_width, 7);
    }

    // --- `extend` (Ruff-style config inheritance) ------------------------------

    #[test]
    fn extend_child_overrides_scalar_and_inherits_rest() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            tmp.path().join("base.toml"),
            "[format]\nline-width = 80\nwrap = \"preserve\"\n",
        )
        .unwrap();
        let child = tmp.path().join("panache.toml");
        std::fs::write(
            &child,
            "extend = \"base.toml\"\n[format]\nline-width = 100\n",
        )
        .unwrap();

        let (cfg, _src) = load(Some(&child), tmp.path(), None, None).expect("load");
        assert_eq!(cfg.line_width, 100, "child overrides the base scalar");
        assert_eq!(
            cfg.wrap,
            Some(WrapMode::Preserve),
            "a base `[format]` key the child omits is inherited (nested-table merge)"
        );
    }

    #[test]
    fn extend_inherits_base_extensions_and_merges_with_flavor() {
        // Guards the `apply_flavor` refactor: extensions contributed by a base
        // must survive and still merge onto flavor defaults.
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            tmp.path().join("base.toml"),
            "flavor = \"quarto\"\n\n[extensions]\nfenced-divs = false\n",
        )
        .unwrap();
        let child = tmp.path().join("panache.toml");
        std::fs::write(&child, "extend = \"base.toml\"\n").unwrap();

        let (cfg, _src) = load(Some(&child), tmp.path(), None, None).expect("load");
        assert_eq!(cfg.flavor, Flavor::Quarto, "base flavor inherited");
        assert!(
            !cfg.extensions.fenced_divs,
            "base's extension override survives the merge and flavor resolution"
        );
    }

    #[test]
    fn extend_exclude_accumulates_but_exclude_replaces() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            tmp.path().join("base.toml"),
            "exclude = [\"base-only/**\"]\nextend-exclude = [\"from-base/**\"]\n",
        )
        .unwrap();
        let child = tmp.path().join("panache.toml");
        std::fs::write(
            &child,
            "extend = \"base.toml\"\nexclude = [\"child-only/**\"]\nextend-exclude = [\"from-child/**\"]\n",
        )
        .unwrap();

        let (cfg, _src) = load(Some(&child), tmp.path(), None, None).expect("load");
        assert_eq!(
            cfg.exclude.as_deref(),
            Some(["child-only/**".to_string()].as_slice()),
            "plain `exclude` replaces the base value"
        );
        assert_eq!(
            cfg.extend_exclude,
            vec!["from-base/**".to_string(), "from-child/**".to_string()],
            "`extend-exclude` concatenates parent then child across the chain"
        );
    }

    #[test]
    fn extend_chains_transitively() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("c.toml"), "[format]\nline-width = 30\n").unwrap();
        std::fs::write(
            tmp.path().join("b.toml"),
            "extend = \"c.toml\"\n[format]\ntab-width = 3\n",
        )
        .unwrap();
        let a = tmp.path().join("a.toml");
        std::fs::write(&a, "extend = \"b.toml\"\n").unwrap();

        let (cfg, _src) = load(Some(&a), tmp.path(), None, None).expect("load");
        assert_eq!(
            cfg.line_width, 30,
            "grandparent value flows through the chain"
        );
        assert_eq!(cfg.tab_width, 3, "parent value flows through the chain");
    }

    #[test]
    fn extend_resolves_relative_to_declaring_file() {
        // The `extend` path is relative to the child file's own directory, not
        // the CWD or the walk's start dir.
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("base.toml"), "[format]\nline-width = 42\n").unwrap();
        let sub = tmp.path().join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let child = sub.join("panache.toml");
        std::fs::write(&child, "extend = \"../base.toml\"\n").unwrap();

        let (cfg, _src) = load(Some(&child), &sub, None, None).expect("load");
        assert_eq!(cfg.line_width, 42);
    }

    #[test]
    fn extend_may_cross_git_boundary() {
        // Unlike discovery, an explicit `extend` is user-intentional (like
        // `--config`) and is not capped by the `.git` project boundary.
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("base.toml"), "[format]\nline-width = 55\n").unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let child = repo.join("panache.toml");
        std::fs::write(&child, "extend = \"../base.toml\"\n").unwrap();

        let (cfg, _src) = load(Some(&child), &repo, None, None).expect("load");
        assert_eq!(
            cfg.line_width, 55,
            "an extended base above the .git root is still loaded"
        );
    }

    #[test]
    fn extend_cycle_is_a_clean_error() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("a.toml"), "extend = \"b.toml\"\n").unwrap();
        std::fs::write(tmp.path().join("b.toml"), "extend = \"a.toml\"\n").unwrap();
        let a = tmp.path().join("a.toml");

        let err = load(Some(&a), tmp.path(), None, None).expect_err("cycle must error");
        assert!(
            err.to_string().contains("Circular configuration detected"),
            "expected a circular-config error, got: {err}"
        );
    }

    #[test]
    fn extend_missing_base_is_an_error() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let child = tmp.path().join("panache.toml");
        std::fs::write(&child, "extend = \"does-not-exist.toml\"\n").unwrap();

        let err = load(Some(&child), tmp.path(), None, None).expect_err("missing base must error");
        let msg = err.to_string();
        assert!(
            msg.contains("does-not-exist.toml"),
            "error must name the missing base, got: {msg}"
        );
    }

    #[test]
    fn extend_chain_reports_every_contributing_file() {
        // The chain returned by `load_with_chain` (used by the LSP to watch base
        // configs) lists the leaf plus its transitive bases.
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("base.toml"), "[format]\nline-width = 20\n").unwrap();
        let child = tmp.path().join("panache.toml");
        std::fs::write(&child, "extend = \"base.toml\"\n").unwrap();

        let (_cfg, _src, chain) =
            load_with_chain(Some(&child), tmp.path(), None, None).expect("load");
        assert_eq!(chain.len(), 2, "chain covers leaf + base");
        assert!(
            chain.contains(&canonical(&child)),
            "chain includes the leaf"
        );
        assert!(
            chain.contains(&canonical(&tmp.path().join("base.toml"))),
            "chain includes the extended base"
        );
    }

    #[test]
    fn no_extend_returns_single_element_chain() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let child = tmp.path().join("panache.toml");
        std::fs::write(&child, "[format]\nline-width = 20\n").unwrap();

        let (_cfg, _src, chain) =
            load_with_chain(Some(&child), tmp.path(), None, None).expect("load");
        assert_eq!(chain, vec![canonical(&child)]);
    }

    #[test]
    fn deprecated_blank_lines_still_parses() {
        // Soft-removed: setting it must not error so existing user TOMLs keep
        // working. The warning is emitted via stderr (not asserted here).
        let toml = "[format]\nblank-lines = \"preserve\"\n";
        let cfg = parse_config_str(toml, Path::new("panache.toml"))
            .expect("config with deprecated blank-lines must still parse");
        assert_eq!(cfg.line_width, 80, "unrelated defaults preserved");
    }

    #[test]
    fn deprecated_top_level_blank_lines_still_parses() {
        let toml = "blank-lines = \"collapse\"\n";
        parse_config_str(toml, Path::new("panache.toml"))
            .expect("top-level blank-lines key must still parse");
    }

    #[test]
    fn compat_quarto_resolves_into_lint_config() {
        let toml = "[compat]\nquarto = \"1.9\"\n[lint.rules]\nquarto-schema = false\n";
        let cfg = parse_config_str(toml, Path::new("panache.toml"))
            .expect("[compat] quarto + rule toggle must parse");
        assert_eq!(cfg.lint.quarto_version.as_deref(), Some("1.9"));
        assert!(!cfg.lint.is_rule_enabled("quarto-schema"));
    }

    #[test]
    fn compat_quarto_must_be_string() {
        let toml = "[compat]\nquarto = true\n";
        let err = parse_config_str(toml, Path::new("panache.toml"))
            .expect_err("non-string [compat] quarto must error");
        assert!(
            err.to_string().contains("quarto"),
            "error must name the key: {err}"
        );
    }

    #[test]
    fn lint_quarto_version_is_rejected_with_migration_hint() {
        // The key moved to `[compat] quarto`; the old spelling must point there.
        let toml = "[lint]\nquarto-version = \"1.9\"\n";
        let err = parse_config_str(toml, Path::new("panache.toml"))
            .expect_err("[lint] quarto-version must error after the move");
        assert!(
            err.to_string().contains("[compat] quarto"),
            "error must point to the new key: {err}"
        );
    }

    #[test]
    fn compat_pandoc_sets_parser_target() {
        let toml = "[compat]\npandoc = \"3.7\"\n";
        let cfg =
            parse_config_str(toml, Path::new("panache.toml")).expect("[compat] pandoc must parse");
        assert_eq!(cfg.parser, PandocCompat::V3_7);
    }

    #[test]
    fn deprecated_top_level_pandoc_compat_still_applies() {
        let toml = "pandoc-compat = \"3.7\"\n";
        let cfg = parse_config_str(toml, Path::new("panache.toml"))
            .expect("deprecated top-level pandoc-compat must still parse");
        assert_eq!(cfg.parser, PandocCompat::V3_7);
    }

    #[test]
    fn compat_pandoc_wins_over_deprecated_top_level_alias() {
        let toml = "pandoc-compat = \"3.7\"\n[compat]\npandoc = \"3.9\"\n";
        let cfg = parse_config_str(toml, Path::new("panache.toml"))
            .expect("both pandoc-compat keys must parse");
        assert_eq!(
            cfg.parser,
            PandocCompat::V3_9,
            "[compat] pandoc takes precedence over the deprecated alias"
        );
    }

    #[test]
    fn unknown_top_level_key_is_rejected() {
        // Typo of `line-width` — we used to silently drop it.
        let toml = "lin-width = 100\n";
        let err = parse_config_str(toml, Path::new("panache.toml"))
            .expect_err("typo'd top-level key must error");
        let msg = err.to_string();
        assert!(
            msg.contains("lin-width") && msg.contains("unknown field"),
            "error must name the offending key: {msg}"
        );
    }

    #[test]
    fn parse_config_detailed_reports_span_for_unknown_key() {
        // The LSP anchors a diagnostic on the offending key; the structured
        // error must carry a byte span pointing at it.
        let toml = "lin-width = 100\n";
        let err = parse_config_detailed(toml, Path::new("panache.toml"))
            .expect_err("typo'd key must error");
        let span = err.span.expect("toml parse error must carry a span");
        assert_eq!(
            &toml[span], "lin-width",
            "span must cover the offending key"
        );
    }

    #[test]
    fn config_error_survives_io_error_round_trip() {
        // `load` returns `io::Result`; the LSP recovers the structured error by
        // downcasting the io::Error's source.
        let toml = "lin-width = 100\n";
        let io_err =
            parse_config_str(toml, Path::new("panache.toml")).expect_err("typo'd key must error");
        let cfg_err = io_err
            .get_ref()
            .and_then(|e| e.downcast_ref::<ConfigError>())
            .expect("io::Error must carry a ConfigError source");
        assert!(cfg_err.span.is_some(), "recovered error keeps its span");
        assert_eq!(cfg_err.path, Path::new("panache.toml"));
    }

    #[test]
    fn discovered_broken_config_errors_instead_of_falling_back() {
        // A typo'd discovered `panache.toml` must fail loudly, not silently
        // fall through to the global/default config.
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();
        std::fs::write(dir.join("panache.toml"), "lin-width = 100\n").unwrap();

        let err = load(None, dir, None, None)
            .expect_err("broken discovered config must surface as an error");
        let cfg_err = err
            .get_ref()
            .and_then(|e| e.downcast_ref::<ConfigError>())
            .expect("load error must carry a ConfigError source");
        assert!(
            cfg_err.message.contains("unknown field"),
            "error must name the parse failure: {cfg_err}"
        );
    }

    #[test]
    fn unknown_key_inside_format_section_is_rejected() {
        let toml = "[format]\nwrapp = \"reflow\"\n";
        let err = parse_config_str(toml, Path::new("panache.toml"))
            .expect_err("typo'd [format] key must error");
        assert!(
            err.to_string().contains("wrapp"),
            "error must name the offending key: {err}"
        );
    }

    #[test]
    fn table_indent_defaults_to_two() {
        let cfg = parse_config_str("", Path::new("panache.toml")).expect("empty config parses");
        assert_eq!(cfg.table_indent, 2);
    }

    #[test]
    fn line_width_parses_from_format_section() {
        let toml = "[format]\nline-width = 100\n";
        let cfg = parse_config_str(toml, Path::new("panache.toml"))
            .expect("[format] line-width must parse");
        assert_eq!(cfg.line_width, 100);
    }

    #[test]
    fn line_ending_parses_from_format_section() {
        let toml = "[format]\nline-ending = \"lf\"\n";
        let cfg = parse_config_str(toml, Path::new("panache.toml"))
            .expect("[format] line-ending must parse");
        assert_eq!(cfg.line_ending, Some(LineEnding::Lf));
    }

    #[test]
    fn deprecated_top_level_line_width_still_applies() {
        // Back-compat: top-level `line-width` (no `[format]` key) is honored.
        let toml = "line-width = 100\n";
        let cfg = parse_config_str(toml, Path::new("panache.toml"))
            .expect("top-level line-width must still parse");
        assert_eq!(cfg.line_width, 100);
    }

    #[test]
    fn deprecated_top_level_line_ending_still_applies() {
        let toml = "line-ending = \"crlf\"\n";
        let cfg = parse_config_str(toml, Path::new("panache.toml"))
            .expect("top-level line-ending must still parse");
        assert_eq!(cfg.line_ending, Some(LineEnding::Crlf));
    }

    #[test]
    fn format_line_width_wins_over_top_level() {
        // When both are set, the canonical `[format]` value takes precedence.
        let toml = "line-width = 40\n[format]\nline-width = 100\n";
        let cfg = parse_config_str(toml, Path::new("panache.toml"))
            .expect("both line-width keys must parse");
        assert_eq!(cfg.line_width, 100);
    }

    #[test]
    fn format_line_ending_wins_over_top_level() {
        let toml = "line-ending = \"lf\"\n[format]\nline-ending = \"crlf\"\n";
        let cfg = parse_config_str(toml, Path::new("panache.toml"))
            .expect("both line-ending keys must parse");
        assert_eq!(cfg.line_ending, Some(LineEnding::Crlf));
    }

    #[test]
    fn line_width_defaults_to_eighty() {
        let cfg = parse_config_str("", Path::new("panache.toml")).expect("empty config parses");
        assert_eq!(cfg.line_width, 80);
        assert_eq!(cfg.line_ending, Some(LineEnding::Auto));
    }

    #[test]
    fn table_indent_parses_from_format_section() {
        let toml = "[format]\ntable-indent = 0\n";
        let cfg =
            parse_config_str(toml, Path::new("panache.toml")).expect("table-indent = 0 must parse");
        assert_eq!(cfg.table_indent, 0);
    }

    #[test]
    fn table_indent_accepts_max_of_three() {
        let toml = "[format]\ntable-indent = 3\n";
        let cfg =
            parse_config_str(toml, Path::new("panache.toml")).expect("table-indent = 3 must parse");
        assert_eq!(cfg.table_indent, 3);
    }

    #[test]
    fn out_of_range_table_indent_value_is_rejected() {
        let toml = "[format]\ntable-indent = 4\n";
        let err = parse_config_str(toml, Path::new("panache.toml"))
            .expect_err("table-indent > 3 must error");
        assert!(
            err.to_string().contains("table-indent"),
            "error must name the offending key: {err}"
        );
    }

    #[test]
    fn deprecated_code_blocks_table_still_parses() {
        // `[code-blocks]` is a no-op since the feature was removed, but older
        // configs still in the wild use it — the deprecation warning must keep
        // firing without `deny_unknown_fields` hard-failing the load.
        let toml = "flavor = \"pandoc\"\n[code-blocks]\nattribute-style = \"explicit\"\n";
        parse_config_str(toml, Path::new("panache.toml"))
            .expect("deprecated [code-blocks] table must still parse");
    }

    #[test]
    fn deprecated_format_code_blocks_subtable_still_parses() {
        let toml = "[format.code-blocks]\nattribute-style = \"explicit\"\n";
        parse_config_str(toml, Path::new("panache.toml"))
            .expect("deprecated [format.code-blocks] subtable must still parse");
    }

    #[test]
    fn experimental_format_math_defaults_off() {
        let cfg = parse_config_str("flavor = \"quarto\"\n", Path::new("panache.toml"))
            .expect("config without [experimental] must parse");
        assert!(
            !cfg.experimental.format_math,
            "format-math must default to false"
        );
    }

    #[test]
    fn experimental_format_math_opt_in_parses() {
        let toml = "[experimental]\nformat-math = true\n";
        let cfg = parse_config_str(toml, Path::new("panache.toml"))
            .expect("[experimental] format-math must parse");
        assert!(cfg.experimental.format_math, "opt-in must enable the gate");
    }

    #[test]
    fn unknown_key_inside_experimental_section_is_rejected() {
        let toml = "[experimental]\nformat-maths = true\n";
        let err = parse_config_str(toml, Path::new("panache.toml"))
            .expect_err("typo'd [experimental] key must error");
        assert!(
            err.to_string().contains("format-maths"),
            "error must name the offending key: {err}"
        );
    }

    #[test]
    fn unknown_extension_name_is_rejected() {
        let toml = "[extensions]\nquato-crossrefs = true\n";
        let err = parse_config_str(toml, Path::new("panache.toml"))
            .expect_err("typo'd extension must error");
        let msg = err.to_string();
        assert!(
            msg.contains("quato-crossrefs"),
            "error must name the typo: {msg}"
        );
        assert!(
            msg.contains("quarto-crossrefs"),
            "error must suggest the closest match: {msg}"
        );
    }

    #[test]
    fn unknown_extension_inside_flavor_subtable_is_rejected() {
        let toml = "[extensions.pandoc]\nnot-a-real-flag = true\n";
        let err = parse_config_str(toml, Path::new("panache.toml"))
            .expect_err("typo inside [extensions.pandoc] must error");
        let msg = err.to_string();
        assert!(
            msg.contains("not-a-real-flag") && msg.contains("[extensions.pandoc]"),
            "error must surface the offending key and table: {msg}"
        );
    }

    #[test]
    fn unknown_flavor_subtable_is_rejected() {
        let toml = "[extensions.qarto]\nfenced-divs = true\n";
        let err = parse_config_str(toml, Path::new("panache.toml"))
            .expect_err("typo'd flavor subtable must error");
        let msg = err.to_string();
        assert!(
            msg.contains("qarto") && msg.contains("quarto"),
            "error must name typo and suggest the closest flavor: {msg}"
        );
    }

    #[test]
    fn known_extension_under_flavor_subtable_still_parses() {
        let toml = "[extensions.pandoc]\nfenced-divs = false\n";
        parse_config_str(toml, Path::new("panache.toml"))
            .expect("valid per-flavor extension override must parse");
    }

    #[test]
    fn snake_case_extension_name_still_parses() {
        // Existing back-compat: snake_case names normalize to kebab-case.
        let toml = "[extensions]\nquarto_crossrefs = true\n";
        parse_config_str(toml, Path::new("panache.toml"))
            .expect("snake_case extension alias must still parse");
    }

    #[test]
    fn formatter_only_extension_name_is_accepted() {
        // `smart-quotes` lives only on `FormatterExtensions`, not `Extensions`.
        // The union validator must accept it.
        let toml = "[extensions]\nsmart-quotes = true\n";
        parse_config_str(toml, Path::new("panache.toml"))
            .expect("formatter-only extension must parse");
    }

    #[test]
    fn find_in_tree_prefers_nearest_config() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let workspace = tmp.path().join("ws");
        let inner = workspace.join("inner");
        std::fs::create_dir_all(&inner).unwrap();
        let outer_cfg = workspace.join("panache.toml");
        let inner_cfg = inner.join("panache.toml");
        std::fs::write(&outer_cfg, "").unwrap();
        std::fs::write(&inner_cfg, "").unwrap();

        let found = find_in_tree(&inner, Some(&workspace));
        assert_eq!(
            found.as_deref(),
            Some(inner_cfg.as_path()),
            "nearest config must win"
        );
    }

    #[test]
    fn unwrap_dot_config_strips_dot_config_component() {
        assert_eq!(
            unwrap_dot_config(Path::new("/proj/.config")),
            Path::new("/proj")
        );
        assert_eq!(unwrap_dot_config(Path::new("/proj")), Path::new("/proj"));
        // Only the literal final `.config` component is unwrapped.
        assert_eq!(
            unwrap_dot_config(Path::new("/proj/.config/sub")),
            Path::new("/proj/.config/sub")
        );
    }

    #[test]
    fn anchor_dir_unwraps_dot_config_for_project_configs() {
        let fallback = Path::new("/cwd");
        // Bare config: parent dir.
        assert_eq!(
            anchor_dir(
                &ConfigSource::Discovered(PathBuf::from("/proj/panache.toml")),
                fallback
            ),
            Path::new("/proj")
        );
        // `.config/panache.toml`: project root, not `.config/`.
        assert_eq!(
            anchor_dir(
                &ConfigSource::Discovered(PathBuf::from("/proj/.config/panache.toml")),
                fallback
            ),
            Path::new("/proj")
        );
        // Explicit follows the same rule.
        assert_eq!(
            anchor_dir(
                &ConfigSource::Explicit(PathBuf::from("/elsewhere/panache.toml")),
                fallback
            ),
            Path::new("/elsewhere")
        );
        // Global XDG config and the no-config case fall back (never `~/.config`).
        assert_eq!(
            anchor_dir(
                &ConfigSource::Global(PathBuf::from("/home/u/.config/panache/config.toml")),
                fallback
            ),
            fallback
        );
        assert_eq!(anchor_dir(&ConfigSource::None, fallback), fallback);
    }

    fn matches(patterns: &[&str], rel: &str) -> bool {
        let owned: Vec<String> = patterns.iter().map(|s| s.to_string()).collect();
        GlobMatcher::build(&owned).expect("build").is_match(rel)
    }

    #[test]
    fn glob_matcher_bare_name_matches_at_any_depth() {
        // gitignore-style: a bare `*.md` matches at the root and nested.
        assert!(matches(&["*.md"], "readme.md"));
        assert!(matches(&["*.md"], "docs/guide/intro.md"));
        assert!(!matches(&["*.md"], "docs/intro.qmd"));
        // A bare directory name (no slash) excludes its contents at any depth.
        assert!(matches(&["target"], "target/x.rs"));
        assert!(matches(&["target"], "a/target/x.rs"));
    }

    #[test]
    fn glob_matcher_trailing_slash_matches_directory_contents() {
        assert!(matches(&["tests/"], "tests/snapshot.md"));
        assert!(matches(&["tests/"], "a/tests/snapshot.md"));
        // The directory entry itself is never tested, but a sibling file is not
        // a directory and must not match.
        assert!(!matches(&["tests/"], "tests.md"));
    }

    #[test]
    fn glob_matcher_anchored_pattern_resolves_from_root() {
        assert!(matches(&["docs/**/*.qmd"], "docs/index.qmd"));
        assert!(matches(&["docs/**/*.qmd"], "docs/guides/intro.qmd"));
        // Anchored: a same-named file outside `docs/` does not match.
        assert!(!matches(&["docs/**/*.qmd"], "other/index.qmd"));
    }

    #[test]
    fn glob_matcher_preserves_explicit_default_forms() {
        // The rewritten defaults already contain `/`, so they round-trip
        // through expansion unchanged (idempotent, no double `**/`).
        assert!(matches(&["**/target/**"], "target/debug/app"));
        assert!(matches(&["**/target/**"], "crates/x/target/debug/app"));
        assert!(matches(&["**/*.md"], "readme.md"));
        assert!(matches(&["**/*.md"], "docs/intro.md"));
        assert!(matches(&["**/LICENSE.md"], "LICENSE.md"));
        assert!(matches(&["**/LICENSE.md"], "vendor/LICENSE.md"));
    }

    #[test]
    fn glob_matcher_default_patterns_compile_and_match() {
        let excludes: Vec<String> = DEFAULT_EXCLUDE_PATTERNS
            .iter()
            .map(|s| s.to_string())
            .collect();
        let m = GlobMatcher::build(&excludes).expect("default excludes build");
        assert!(m.is_match("node_modules/lib/index.md"));
        assert!(m.is_match(".git/HEAD"));
        assert!(m.is_match("tests/testthat/_snaps/x.md"));
        assert!(!m.is_match("docs/intro.qmd"));

        let includes: Vec<String> = DEFAULT_INCLUDE_PATTERNS
            .iter()
            .map(|s| s.to_string())
            .collect();
        let inc = GlobMatcher::build(&includes).expect("default includes build");
        assert!(inc.is_match("docs/guide/intro.qmd"));
        assert!(inc.is_match("readme.md"));
        assert!(!inc.is_match("script.py"));
    }
}
