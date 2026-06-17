use std::collections::HashMap;
use std::env;
use std::fs;
use std::io;
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

fn parse_config_str(s: &str, path: &Path) -> io::Result<Config> {
    check_deprecated_extension_names(s, path);
    check_deprecated_formatter_names(s, path);
    check_deprecated_code_block_style_options(s, path);
    check_deprecated_blank_lines(s, path);

    if let Err(msg) = validate_extension_names(s) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid config {}: {msg}", path.display()),
        ));
    }

    let config: Config = toml::from_str(s).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid config {}: {e}", path.display()),
        )
    })?;

    Ok(config)
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

fn read_config(path: &Path) -> io::Result<Config> {
    log::debug!("Reading config from: {}", path.display());
    let s = fs::read_to_string(path)?;
    let config = parse_config_str(&s, path)?;
    log::debug!("Loaded config from: {}", path.display());
    Ok(config)
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
    let boundary = project_boundary(start_dir);
    let (mut cfg, source) = if let Some(path) = explicit {
        let cfg = read_config(path)?;
        (cfg, ConfigSource::Explicit(path.to_path_buf()))
    } else if let Some(p) = find_in_tree(start_dir, boundary.as_deref())
        && let Ok(cfg) = read_config(&p)
    {
        (cfg, ConfigSource::Discovered(p))
    } else if let Some(p) = xdg_config_path()
        && let Ok(cfg) = read_config(&p)
    {
        (cfg, ConfigSource::Global(p))
    } else {
        log::debug!("No config file found, using defaults");
        (Config::default(), ConfigSource::None)
    };

    let anchor = source.project_anchor();
    let resolved_flavor =
        flavor_override.or_else(|| detect_flavor(input_file, anchor.as_deref(), &cfg));

    if let Some(flavor) = resolved_flavor {
        // `apply_flavor` re-reads the config file, so it needs the actual path,
        // not the (possibly `.config/`-unwrapped) anchor directory.
        apply_flavor(&mut cfg, flavor, source.path());
    }

    Ok((cfg, source))
}

fn apply_flavor(cfg: &mut Config, flavor: Flavor, cfg_path: Option<&Path>) {
    cfg.flavor = flavor;
    if let Some(path) = cfg_path {
        fs::read_to_string(path)
            .ok()
            .and_then(|s| toml::from_str::<toml::Value>(&s).ok())
            .map(|root| {
                cfg.extensions = resolve_extensions_for_flavor(root.get("extensions"), flavor);
                cfg.formatter_extensions =
                    resolve_formatter_extensions_for_flavor(root.get("extensions"), flavor);
            })
            .unwrap_or_else(|| {
                cfg.extensions = Extensions::for_flavor(flavor);
                cfg.formatter_extensions = FormatterExtensions::for_flavor(flavor);
            });
    } else {
        cfg.extensions = Extensions::for_flavor(flavor);
        cfg.formatter_extensions = FormatterExtensions::for_flavor(flavor);
    }
}

fn parse_flavor_key(s: &str) -> Option<Flavor> {
    match s.replace('_', "-").to_lowercase().as_str() {
        "pandoc" => Some(Flavor::Pandoc),
        "quarto" => Some(Flavor::Quarto),
        "rmarkdown" | "r-markdown" => Some(Flavor::RMarkdown),
        "gfm" => Some(Flavor::Gfm),
        "common-mark" | "commonmark" => Some(Flavor::CommonMark),
        "multimarkdown" | "multi-markdown" => Some(Flavor::MultiMarkdown),
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

fn detect_flavor(input_file: Option<&Path>, anchor: Option<&Path>, cfg: &Config) -> Option<Flavor> {
    let input_path = input_file?;
    let ext = input_path.extension().and_then(|e| e.to_str())?;
    let ext_lower = ext.to_lowercase();

    match ext_lower.as_str() {
        "qmd" => Some(Flavor::Quarto),
        "rmd" | "rmarkdown" => Some(Flavor::RMarkdown),
        _ if MARKDOWN_FAMILY_EXTENSIONS.contains(&ext_lower.as_str()) => {
            let override_flavor = detect_flavor_override(input_path, anchor, &cfg.flavor_overrides);
            Some(override_flavor.unwrap_or(cfg.flavor))
        }
        _ => None,
    }
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
