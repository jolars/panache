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
pub use panache_parser::Extensions;
pub use panache_parser::Flavor;
pub use panache_parser::PandocCompat;
pub use panache_parser::ParserConfig;
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
pub use types::TabStopMode;
pub use types::WrapMode;

pub const DEFAULT_EXCLUDE_PATTERNS: &[&str] = &[
    ".Rproj.user/",
    ".bzr/",
    ".cache/",
    ".devevn/",
    ".direnv/",
    ".git/",
    ".hg/",
    ".julia/",
    ".mypy_cache/",
    ".nox/",
    ".pytest_cache/",
    ".ruff_cache/",
    ".svn/",
    ".tmp/",
    ".tox/",
    ".venv/",
    ".vscode/",
    "_book/",
    "_build/",
    "_freeze/",
    "_site/",
    "build/",
    "dist/",
    "node_modules/",
    "renv/",
    "target/",
    "tests/testthat/_snaps",
    "**/LICENSE.md",
];

pub const DEFAULT_INCLUDE_PATTERNS: &[&str] =
    &["*.md", "*.qmd", "*.Rmd", "*.markdown", "*.mdown", "*.mkd"];

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

fn parse_config_str(s: &str, path: &Path) -> io::Result<Config> {
    check_deprecated_extension_names(s, path);
    check_deprecated_formatter_names(s, path);
    check_deprecated_code_block_style_options(s, path);

    let config: Config = toml::from_str(s).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid config {}: {e}", path.display()),
        )
    })?;

    Ok(config)
}

fn read_config(path: &Path) -> io::Result<Config> {
    log::debug!("Reading config from: {}", path.display());
    let s = fs::read_to_string(path)?;
    let config = parse_config_str(&s, path)?;
    log::debug!("Loaded config from: {}", path.display());
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

    if let Some(flavor) = detect_flavor(input_file, cfg_path.as_deref(), &cfg) {
        cfg.flavor = flavor;
        cfg.extensions = if let Some(path) = cfg_path.as_deref() {
            fs::read_to_string(path)
                .ok()
                .and_then(|s| toml::from_str::<toml::Value>(&s).ok())
                .map(|root| resolve_extensions_for_flavor(root.get("extensions"), flavor))
                .unwrap_or_else(|| Extensions::for_flavor(flavor))
        } else {
            Extensions::for_flavor(flavor)
        };
    }

    Ok((cfg, cfg_path))
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

fn detect_flavor(
    input_file: Option<&Path>,
    cfg_path: Option<&Path>,
    cfg: &Config,
) -> Option<Flavor> {
    let input_path = input_file?;
    let ext = input_path.extension().and_then(|e| e.to_str())?;
    let ext_lower = ext.to_lowercase();

    match ext_lower.as_str() {
        "qmd" => Some(Flavor::Quarto),
        "rmd" => Some(Flavor::RMarkdown),
        _ if MARKDOWN_FAMILY_EXTENSIONS.contains(&ext_lower.as_str()) => {
            let base_dir = cfg_path.and_then(Path::parent);
            let override_flavor =
                detect_flavor_override(input_path, base_dir, &cfg.flavor_overrides);
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
