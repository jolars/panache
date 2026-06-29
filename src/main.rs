use std::fs;
use std::io::{self, IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use clap::Parser;
use similar::{ChangeTag, TextDiff};

use panache::{format, parse};
use serde_json::json;

mod cache;
mod cli;
mod diagnostic_renderer;
use cache::{
    CachedLintDocument, CliCache, FormatCacheMode, FormatStoreArgs, global_cache_base_dir,
    resolve_cache_dir_for_cli,
};
use cli::{Cli, CliFlavor, ColorMode, Commands, DebugChecks, DebugCommands, ParseOutput};
use diagnostic_renderer::print_diagnostics;
use panache::config::{Flavor, WrapMode};

impl From<CliFlavor> for Flavor {
    fn from(value: CliFlavor) -> Self {
        match value {
            CliFlavor::Pandoc => Flavor::Pandoc,
            CliFlavor::Quarto => Flavor::Quarto,
            CliFlavor::RMarkdown => Flavor::RMarkdown,
            CliFlavor::Gfm => Flavor::Gfm,
            CliFlavor::CommonMark => Flavor::CommonMark,
            CliFlavor::MultiMarkdown => Flavor::MultiMarkdown,
            CliFlavor::Mdsvex => Flavor::Mdsvex,
            CliFlavor::Myst => Flavor::Myst,
        }
    }
}

/// Apply `panache format -o key=value` overrides on top of a loaded config.
fn apply_format_overrides(cfg: &mut panache::Config, overrides: &[String]) -> Result<(), String> {
    let mut extension_overrides: std::collections::HashMap<String, bool> =
        std::collections::HashMap::new();
    for raw in overrides {
        let (key, value) = raw
            .split_once('=')
            .ok_or_else(|| format!("invalid --option `{raw}`: expected key=value"))?;
        let key = key.trim();
        let value = value.trim();
        match key {
            "line-width" => {
                let n: usize = value.parse().map_err(|_| {
                    format!("invalid value for `line-width`: `{value}` (expected positive integer)")
                })?;
                if n == 0 {
                    return Err(
                        "invalid value for `line-width`: 0 (expected positive integer)".into(),
                    );
                }
                cfg.line_width = n;
            }
            "wrap" => {
                let mode = match value {
                    "reflow" => WrapMode::Reflow,
                    "sentence" => WrapMode::Sentence,
                    "semantic" => WrapMode::Semantic,
                    "preserve" => WrapMode::Preserve,
                    other => {
                        return Err(format!(
                            "invalid value for `wrap`: `{other}` (expected one of: reflow, sentence, semantic, preserve)"
                        ));
                    }
                };
                cfg.wrap = Some(mode);
            }
            "table-indent" => {
                let indent: usize = value.parse().map_err(|_| {
                    format!(
                        "invalid value for `table-indent`: `{value}` (expected an integer 0, 1, 2, or 3)"
                    )
                })?;
                if indent > 3 {
                    return Err(format!(
                        "invalid value for `table-indent`: `{indent}` (expected 0, 1, 2, or 3)"
                    ));
                }
                cfg.table_indent = indent;
            }
            ext_key if ext_key.starts_with("extensions.") => {
                let name = ext_key["extensions.".len()..].trim();
                if name.is_empty() {
                    return Err(format!(
                        "invalid --option `{raw}`: missing extension name after `extensions.`"
                    ));
                }
                let bool_value = match value.to_ascii_lowercase().as_str() {
                    "true" | "1" | "yes" | "on" => true,
                    "false" | "0" | "no" | "off" => false,
                    _ => {
                        return Err(format!(
                            "invalid value for `{ext_key}`: `{value}` (expected boolean: true/false)"
                        ));
                    }
                };
                extension_overrides.insert(name.to_string(), bool_value);
            }
            other => {
                return Err(format!(
                    "unknown config key in --option: `{other}` (supported: line-width, wrap, table-indent, extensions.<name>)"
                ));
            }
        }
    }
    if !extension_overrides.is_empty() {
        cfg.extensions.apply_overrides(extension_overrides.clone());
        cfg.formatter_extensions
            .apply_overrides(extension_overrides);
    }
    Ok(())
}

/// Supported file extensions for formatting
const SUPPORTED_EXTENSIONS: &[&str] = &[
    "md",
    "qmd",
    "Rmd",
    "rmd",
    "Rmarkdown",
    "rmarkdown",
    "markdown",
    "mdown",
    "mkd",
    // mdsvex: bare `.svx`. The compound `.svelte.md` ends in `.md`, so it is
    // already accepted by the `md` entry above.
    "svx",
];

fn init_logger(debug_log: Option<&Path>) {
    let Some(path) = debug_log else {
        env_logger::Builder::from_default_env().init();
        return;
    };

    let mut builder = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("panache=debug"),
    );
    if let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        builder.target(env_logger::Target::Pipe(Box::new(file)));
    }
    builder.format_timestamp_millis();
    builder.init();
    log::info!("LSP debug logging enabled at {}", path.display());
}

fn init_lsp_debug_log() -> io::Result<PathBuf> {
    let mut base = dirs::state_dir().unwrap_or_else(|| PathBuf::from("."));
    base.push("panache");
    fs::create_dir_all(&base)?;
    base.push("lsp-debug.log");
    Ok(base)
}

struct PathFilters {
    exclude: panache::config::GlobMatcher,
    include: panache::config::GlobMatcher,
}

fn effective_exclude_patterns(cfg: &panache::Config) -> Vec<String> {
    let mut patterns = cfg.exclude.clone().unwrap_or_else(|| {
        panache::config::DEFAULT_EXCLUDE_PATTERNS
            .iter()
            .map(|s| s.to_string())
            .collect()
    });
    patterns.extend(cfg.extend_exclude.iter().cloned());
    patterns
}

fn effective_include_patterns(cfg: &panache::Config) -> Vec<String> {
    let mut patterns = cfg.include.clone().unwrap_or_else(|| {
        panache::config::DEFAULT_INCLUDE_PATTERNS
            .iter()
            .map(|s| s.to_string())
            .collect()
    });
    patterns.extend(cfg.extend_include.iter().cloned());
    patterns
}

fn build_path_filters(cfg: &panache::Config) -> io::Result<PathFilters> {
    // Anchoring is applied by computing config-dir-relative paths in
    // `expand_paths`; the matchers themselves are anchor-agnostic.
    let exclude = panache::config::GlobMatcher::build(&effective_exclude_patterns(cfg))
        .map_err(io::Error::other)?;
    let include = panache::config::GlobMatcher::build(&effective_include_patterns(cfg))
        .map_err(io::Error::other)?;
    Ok(PathFilters { exclude, include })
}

fn relative_path_from_root(path: &Path, root: &Path) -> Option<PathBuf> {
    if let Ok(rel) = path.strip_prefix(root) {
        return Some(rel.to_path_buf());
    }
    let canonical_path = path.canonicalize().ok()?;
    let canonical_root = root.canonicalize().ok()?;
    canonical_path
        .strip_prefix(&canonical_root)
        .ok()
        .map(Path::to_path_buf)
}

/// Expand paths to include all supported files, recursively handling directories
fn expand_paths(
    paths: &[PathBuf],
    cfg: &panache::Config,
    anchor: &Path,
    force_exclude: bool,
    accept_any_extension: bool,
) -> io::Result<Vec<PathBuf>> {
    use ignore::WalkBuilder;

    let mut files = Vec::new();
    // One matcher set; paths are matched config-dir-relative (the unified rule).
    let filters = build_path_filters(cfg)?;

    for path in paths {
        if path.is_file() {
            let rel_path = relative_path_from_root(path, anchor)
                .or_else(|| path.file_name().map(PathBuf::from))
                .unwrap_or_else(|| path.to_path_buf());
            let rel = rel_path.to_string_lossy().replace('\\', "/");
            if force_exclude && filters.exclude.is_match(&rel) {
                continue;
            }
            if accept_any_extension {
                files.push(path.clone());
            } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if SUPPORTED_EXTENSIONS.contains(&ext) {
                    files.push(path.clone());
                } else {
                    eprintln!(
                        "Warning: Skipping unsupported file type: {}",
                        path.display()
                    );
                }
            } else {
                eprintln!(
                    "Warning: Skipping file without extension: {}",
                    path.display()
                );
            }
        } else if path.is_dir() {
            // Walk directory recursively, respecting .gitignore
            let walker = WalkBuilder::new(path)
                .hidden(false) // Don't skip hidden files by default
                .git_ignore(true) // Respect .gitignore
                .git_global(true) // Respect global gitignore
                .build();

            for entry in walker {
                let entry = entry.map_err(io::Error::other)?;
                let entry_path = entry.path();
                if entry_path.is_dir() {
                    continue;
                }
                let rel_path = relative_path_from_root(entry_path, anchor)
                    .unwrap_or_else(|| entry_path.to_path_buf());
                let rel = rel_path.to_string_lossy().replace('\\', "/");
                if filters.exclude.is_match(&rel) {
                    continue;
                }
                if !filters.include.is_match(&rel) {
                    continue;
                }
                if entry_path.is_file() {
                    files.push(entry_path.to_path_buf());
                }
            }
        } else {
            eprintln!("Warning: Path not found: {}", path.display());
        }
    }

    Ok(files)
}

/// Effective worker count for processing `n_files` items. Returns 1 when
/// `n_files <= 1` (no point spinning up rayon for one item) or when the user
/// explicitly forces serial via `--jobs 1`. A `cli_jobs` of 0 means auto:
/// fall back to `available_parallelism()`.
fn effective_parallelism(cli_jobs: usize, n_files: usize) -> usize {
    if n_files <= 1 || cli_jobs == 1 {
        return 1;
    }
    if cli_jobs > 0 {
        return cli_jobs.min(n_files);
    }
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(n_files)
}

/// Per-file share of the shared external-tool budget when the outer pool
/// processes `workers` files concurrently.
///
/// The shared budget ([`panache::init_external_tool_budget`]) caps the *global*
/// live-subprocess count, so each in-flight file may dispatch up to its share
/// without the total ever exceeding the user-configured ceiling. Splitting the
/// budget — rather than forcing 1-per-file — lets a handful of files still
/// saturate it (3 files, budget 8 → 3 each; the budget then caps live
/// subprocesses at 8), while a large batch collapses to 1-per-file
/// (`workers >= budget`) and avoids oversubscribing threads across many inner
/// pools.
fn per_file_external_parallel(budget: usize, workers: usize) -> usize {
    budget.div_ceil(workers.max(1)).max(1)
}

/// Build a rayon thread pool sized to `n` workers.
fn build_pool(n: usize) -> rayon::ThreadPool {
    rayon::ThreadPoolBuilder::new()
        .num_threads(n)
        .thread_name(|i| format!("panache-worker-{i}"))
        .build()
        .expect("failed to build rayon thread pool")
}

/// Parse a range string like "5:10" into (start_line, end_line)
fn parse_range(range_str: &str) -> Result<(usize, usize), String> {
    let parts: Vec<&str> = range_str.split(':').collect();
    if parts.len() != 2 {
        return Err(format!(
            "Invalid range format '{}'. Expected START:END (e.g., 5:10)",
            range_str
        ));
    }

    let start = parts[0]
        .parse::<usize>()
        .map_err(|_| format!("Invalid start line '{}'", parts[0]))?;
    let end = parts[1]
        .parse::<usize>()
        .map_err(|_| format!("Invalid end line '{}'", parts[1]))?;

    if start == 0 || end == 0 {
        return Err("Line numbers must be 1-indexed (start from 1)".to_string());
    }

    if start > end {
        return Err(format!(
            "Start line ({}) must be less than or equal to end line ({})",
            start, end
        ));
    }

    Ok((start, end))
}

fn read_all(path: Option<&PathBuf>) -> io::Result<String> {
    match path {
        Some(p) => fs::read_to_string(p),
        None => {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf)?;
            Ok(buf)
        }
    }
}

/// Treat the `-` argument as the conventional Unix stdin sentinel: collapse
/// `["-"]` to an empty list (which the subcommands already interpret as
/// "read from stdin"). Mixing `-` with real paths is ambiguous, so reject it.
fn normalize_input_paths(files: Vec<PathBuf>) -> io::Result<Vec<PathBuf>> {
    let has_dash = files.iter().any(|p| p.as_os_str() == "-");
    if !has_dash {
        return Ok(files);
    }
    if files.len() > 1 {
        return Err(io::Error::other(
            "'-' (stdin) cannot be combined with file path arguments",
        ));
    }
    Ok(Vec::new())
}

/// Same convention as `normalize_input_paths` for the `parse` subcommand,
/// which takes a single optional path.
fn normalize_parse_path(file: Option<PathBuf>) -> Option<PathBuf> {
    match file {
        Some(p) if p.as_os_str() == "-" => None,
        other => other,
    }
}

fn file_count_label(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("{count} {singular}")
    } else {
        format!("{count} {plural}")
    }
}

fn remove_dir_if_exists(path: &Path) -> io::Result<bool> {
    let mut attempt: usize = 0;
    loop {
        match fs::remove_dir_all(path) {
            Ok(()) => return Ok(true),
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(err) => {
                if !should_retry_remove(&err, attempt) {
                    return Err(err);
                }
                std::thread::sleep(std::time::Duration::from_millis(25u64 << attempt));
                attempt += 1;
            }
        }
    }
}

// Windows AV / Search Indexer can briefly hold open handles on freshly-written
// files, surfacing as PermissionDenied from `remove_dir_all`. Back off and try
// again before giving up.
#[cfg(windows)]
fn should_retry_remove(err: &io::Error, attempt: usize) -> bool {
    attempt < 5 && err.kind() == io::ErrorKind::PermissionDenied
}

#[cfg(not(windows))]
fn should_retry_remove(_err: &io::Error, _attempt: usize) -> bool {
    false
}

/// Walk `path` and return `(file_count, total_bytes)` for every regular file beneath it.
/// Returns `None` if the directory does not exist. Inaccessible entries are skipped.
fn summarize_dir(path: &Path) -> io::Result<Option<(usize, u64)>> {
    if !path.exists() {
        return Ok(None);
    }
    let mut files = 0usize;
    let mut bytes = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err),
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                stack.push(entry.path());
            } else if file_type.is_file()
                && let Ok(meta) = entry.metadata()
            {
                files += 1;
                bytes = bytes.saturating_add(meta.len());
            }
        }
    }
    Ok(Some((files, bytes)))
}

/// Format a byte count using IEC binary units (KiB/MiB/GiB), or plain bytes under 1 KiB.
fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    let bytes_f = bytes as f64;
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes_f < KIB * KIB {
        format!("{:.1} KiB", bytes_f / KIB)
    } else if bytes_f < KIB * KIB * KIB {
        format!("{:.2} MiB", bytes_f / (KIB * KIB))
    } else {
        format!("{:.2} GiB", bytes_f / (KIB * KIB * KIB))
    }
}

fn format_clean_summary(summary: Option<(usize, u64)>) -> String {
    match summary {
        Some((files, bytes)) => {
            let file_word = if files == 1 { "file" } else { "files" };
            format!(" ({files} {file_word}, {})", format_bytes(bytes))
        }
        None => String::new(),
    }
}

fn open_cli_cache_best_effort(
    cfg: &panache::Config,
    explicit_config: Option<&Path>,
    start_dir: &Path,
) -> Option<CliCache> {
    match CliCache::open(cfg, explicit_config, start_dir) {
        Ok(cache) => cache,
        Err(err) => {
            log::warn!("Disabling CLI cache for this run: {err}");
            None
        }
    }
}

fn start_dir_for(input_path: Option<&Path>) -> io::Result<PathBuf> {
    if let Some(p) = input_path {
        Ok(p.parent().unwrap_or(Path::new(".")).to_path_buf())
    } else {
        std::env::current_dir()
    }
}

fn has_explicit_file_targets(paths: &[PathBuf]) -> bool {
    paths.iter().any(|path| !path.is_dir())
}

fn load_config_for_cli(
    config_path: Option<&Path>,
    isolated: bool,
    cli_cache_dir: Option<&Path>,
    start_dir: &Path,
    input_path: Option<&Path>,
    flavor_override: Option<Flavor>,
) -> io::Result<(panache::Config, panache::config::ConfigSource)> {
    let mut loaded = if !isolated {
        panache::config::load(config_path, start_dir, input_path, flavor_override)?
    } else {
        let mut cfg = panache::Config::default();
        // Delegate to the canonical extension→flavor map so `--isolated` stays
        // in lockstep with the config-file path (notably mdsvex's `.svx` and the
        // compound `.svelte.md`); a reduced match here previously omitted them.
        let isolated_flavor = flavor_override
            .or_else(|| input_path.and_then(|p| panache::config::detect_flavor_from_path(p, &cfg)));

        if let Some(flavor) = isolated_flavor {
            cfg.flavor = flavor;
            cfg.extensions = panache::config::Extensions::for_flavor(flavor);
        }
        (cfg, panache::config::ConfigSource::None)
    };

    if let Some(cache_dir) = cli_cache_dir {
        loaded.0.cache_dir = Some(cache_dir.to_string_lossy().to_string());
    }

    Ok(loaded)
}

fn color_enabled(mode: ColorMode, no_color: bool) -> bool {
    resolve_color(
        mode,
        no_color,
        std::env::var_os("NO_COLOR").is_some(),
        std::env::var_os("TERM").as_deref(),
        io::stdout().is_terminal(),
    )
}

fn resolve_color(
    mode: ColorMode,
    no_color_flag: bool,
    no_color_env: bool,
    term_env: Option<&std::ffi::OsStr>,
    stdout_is_terminal: bool,
) -> bool {
    if no_color_flag {
        return false;
    }
    match mode {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => {
            if no_color_env {
                return false;
            }
            match term_env {
                Some(term) if term == "dumb" => return false,
                None => return false,
                _ => {}
            }
            stdout_is_terminal
        }
    }
}

fn print_diff(file_path: &str, original: &str, formatted: &str, use_color: bool) {
    let diff = TextDiff::from_lines(original, formatted);

    for (idx, group) in diff.grouped_ops(3).iter().enumerate() {
        if idx > 0 {
            println!("---");
        }

        // Print header similar to rustfmt
        println!("Diff in {}:{}:", file_path, group[0].old_range().start + 1);

        for op in group {
            for change in diff.iter_changes(op) {
                let (sign, style) = match change.tag() {
                    ChangeTag::Delete => ("-", "\x1b[31m"), // red
                    ChangeTag::Insert => ("+", "\x1b[32m"), // green
                    ChangeTag::Equal => (" ", "\x1b[0m"),   // normal
                };

                if use_color {
                    print!("{}{}{}", style, sign, change.value());
                } else {
                    print!("{}{}", sign, change.value());
                }

                // Reset color at end of line if it was colored
                if use_color && change.tag() != ChangeTag::Equal {
                    print!("\x1b[0m");
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
enum CheckKind {
    Losslessness,
    Idempotency,
}

impl CheckKind {
    fn label(self) -> &'static str {
        match self {
            CheckKind::Losslessness => "losslessness",
            CheckKind::Idempotency => "idempotency",
        }
    }
}

fn sanitize_path_for_filename(path: &str) -> String {
    path.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[derive(Clone)]
struct DebugFailure {
    kind: CheckKind,
    left: String,
    right: String,
}

fn build_debug_failure_report(
    checks: DebugChecks,
    files_checked: usize,
    failures: &[(String, DebugFailure)],
) -> String {
    let mut out = String::new();
    out.push_str("# Debug-format regression report\n\n");
    out.push_str(&format!(
        "- Checks: `{}`\n- Files checked: {}\n- Failures: {}\n\n",
        format!("{:?}", checks).to_lowercase(),
        files_checked,
        failures.len()
    ));

    if failures.is_empty() {
        out.push_str("All checks passed.\n");
        return out;
    }

    out.push_str("## Failures\n\n");
    for (idx, (file, failure)) in failures.iter().enumerate() {
        let diff = TextDiff::from_lines(&failure.left, &failure.right);
        let location_line = diff
            .grouped_ops(0)
            .first()
            .and_then(|group| group.first().map(|op| op.old_range().start + 1))
            .unwrap_or(1);

        out.push_str(&format!(
            "### {}. `{}` ({})\n\n",
            idx + 1,
            file,
            failure.kind.label()
        ));
        out.push_str(&format!("- Approx. diff start line: {}\n\n", location_line));
        out.push_str("```diff\n");
        for change in diff.iter_all_changes() {
            let sign = match change.tag() {
                ChangeTag::Delete => "-",
                ChangeTag::Insert => "+",
                ChangeTag::Equal => " ",
            };
            out.push_str(sign);
            out.push_str(change.value());
        }
        out.push_str("```\n\n");
    }

    out
}

#[derive(Default)]
struct DebugRunArtifacts {
    losslessness: Option<(String, String)>,
    idempotency: Option<(String, String, String)>,
    failures: Vec<DebugFailure>,
}

fn write_debug_artifacts(
    dump_dir: &Path,
    stem: &str,
    artifacts: &DebugRunArtifacts,
    dump_passes: bool,
) -> io::Result<()> {
    fs::create_dir_all(dump_dir)?;

    if let Some((input, tree_text)) = artifacts.losslessness.as_ref()
        && (dump_passes
            || artifacts
                .failures
                .iter()
                .any(|failure| matches!(failure.kind, CheckKind::Losslessness)))
    {
        fs::write(
            dump_dir.join(format!("{stem}.losslessness.input.txt")),
            input,
        )?;
        fs::write(
            dump_dir.join(format!("{stem}.losslessness.parsed.txt")),
            tree_text,
        )?;
    }

    if let Some((input, once, twice)) = artifacts.idempotency.as_ref()
        && (dump_passes
            || artifacts
                .failures
                .iter()
                .any(|failure| matches!(failure.kind, CheckKind::Idempotency)))
    {
        fs::write(
            dump_dir.join(format!("{stem}.idempotency.input.txt")),
            input,
        )?;
        fs::write(dump_dir.join(format!("{stem}.idempotency.once.txt")), once)?;
        fs::write(
            dump_dir.join(format!("{stem}.idempotency.twice.txt")),
            twice,
        )?;
    }

    for failure in &artifacts.failures {
        let kind = failure.kind.label();
        fs::write(
            dump_dir.join(format!("{stem}.{kind}.left.txt")),
            &failure.left,
        )?;
        fs::write(
            dump_dir.join(format!("{stem}.{kind}.right.txt")),
            &failure.right,
        )?;
    }

    Ok(())
}

fn run_debug_checks_for_content(
    input: &str,
    cfg: &panache::Config,
    checks: DebugChecks,
    target_label: &str,
) -> DebugRunArtifacts {
    let mut artifacts = DebugRunArtifacts::default();
    log::debug!(
        "debug format: start checks={} target={}",
        format!("{:?}", checks).to_lowercase(),
        target_label
    );

    if matches!(checks, DebugChecks::Losslessness | DebugChecks::All) {
        log::debug!("debug format: losslessness start target={}", target_label);
        let tree_text = parse(input, Some(cfg.clone())).text().to_string();
        artifacts.losslessness = Some((input.to_string(), tree_text.clone()));
        if input != tree_text {
            artifacts.failures.push(DebugFailure {
                kind: CheckKind::Losslessness,
                left: input.to_string(),
                right: tree_text,
            });
        }
        log::debug!("debug format: losslessness end target={}", target_label);
    }

    if matches!(checks, DebugChecks::Idempotency | DebugChecks::All) {
        log::debug!(
            "debug format: idempotency pass1 start target={}",
            target_label
        );
        let once = format(input, Some(cfg.clone()), None);
        log::debug!(
            "debug format: idempotency pass1 end target={}",
            target_label
        );
        log::debug!(
            "debug format: idempotency pass2 start target={}",
            target_label
        );
        let twice = format(&once, Some(cfg.clone()), None);
        log::debug!(
            "debug format: idempotency pass2 end target={}",
            target_label
        );
        artifacts.idempotency = Some((input.to_string(), once.clone(), twice.clone()));
        if once != twice {
            artifacts.failures.push(DebugFailure {
                kind: CheckKind::Idempotency,
                left: once,
                right: twice,
            });
        }
    }

    log::debug!(
        "debug format: end target={} failures={}",
        target_label,
        artifacts.failures.len()
    );
    artifacts
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();
    let use_color = color_enabled(cli.color, cli.no_color);
    panache::set_warning_color_override(use_color);
    let debug_log = match &cli.command {
        Commands::Lsp { debug } if *debug => Some(init_lsp_debug_log()?),
        _ => None,
    };
    init_logger(debug_log.as_deref());

    match cli.command {
        Commands::Parse {
            file,
            to,
            json,
            verify,
        } => {
            if verify {
                eprintln!(
                    "Warning: `panache parse --verify` is deprecated; use `panache debug format --checks losslessness`."
                );
            }
            let file = normalize_parse_path(file);
            let input_path = file.as_deref().or(cli.stdin_filename.as_deref());
            let start_dir = start_dir_for(input_path)?;
            let (cfg, cfg_source) = load_config_for_cli(
                cli.config.as_deref(),
                cli.isolated,
                cli.cache_dir.as_deref(),
                &start_dir,
                input_path,
                cli.flavor.map(Flavor::from),
            )?;

            if let Some(path) = cfg_source.path() {
                log::debug!("Using config from: {}", path.display());
            } else {
                log::debug!("Using default config");
            }

            let input = read_all(file.as_ref())?;
            let tree = parse(&input, Some(cfg));
            if verify {
                let tree_text = tree.text().to_string();
                if input != tree_text {
                    let file_label = file.as_ref().and_then(|p| p.to_str()).unwrap_or("<stdin>");
                    eprintln!(
                        "Verification failed (losslessness): parser output differs from input"
                    );
                    print_diff(file_label, &input, &tree_text, use_color);
                    std::process::exit(1);
                }
            }
            if let Some(json_path) = json {
                let json_value = panache::syntax::cst_to_json(&tree);
                let json_output =
                    serde_json::to_string_pretty(&json_value).map_err(io::Error::other)?;
                fs::write(json_path, json_output)?;
            }
            if !cli.quiet {
                match to {
                    ParseOutput::Cst => println!("{:#?}", tree),
                    ParseOutput::PandocAst => {
                        println!("{}", panache::parser::to_pandoc_ast(&tree));
                    }
                    ParseOutput::PandocJson => {
                        println!("{}", panache::parser::to_pandoc_json(&tree));
                    }
                }
            }
            Ok(())
        }
        Commands::Format {
            files,
            check,
            range,
            verify,
            force_exclude,
            option,
        } => {
            if verify {
                eprintln!(
                    "Warning: `panache format --verify` is deprecated; use `panache debug format --checks all`."
                );
            }
            let files = match normalize_input_paths(files) {
                Ok(files) => files,
                Err(err) => {
                    eprintln!("Error: {}", err);
                    std::process::exit(1);
                }
            };
            // Parse range if provided (only valid for single file or stdin)
            let parsed_range = if let Some(range_str) = range {
                if files.len() > 1 {
                    eprintln!("Error: --range cannot be used with multiple files");
                    std::process::exit(1);
                }
                match parse_range(&range_str) {
                    Ok(r) => Some(r),
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
            } else {
                None
            };

            // Handle stdin case
            if files.is_empty() {
                let start_dir = start_dir_for(cli.stdin_filename.as_deref())?;
                let (mut cfg, cfg_source) = load_config_for_cli(
                    cli.config.as_deref(),
                    cli.isolated,
                    cli.cache_dir.as_deref(),
                    &start_dir,
                    cli.stdin_filename.as_deref(),
                    cli.flavor.map(Flavor::from),
                )?;
                if let Err(err) = apply_format_overrides(&mut cfg, &option) {
                    eprintln!("Error: {err}");
                    std::process::exit(2);
                }

                if let Some(path) = cfg_source.path() {
                    log::debug!("Using config from: {}", path.display());
                } else {
                    log::debug!("Using default config");
                }

                let input = read_all(None)?;
                if verify {
                    let tree = parse(&input, Some(cfg.clone()));
                    let tree_text = tree.text().to_string();
                    if input != tree_text {
                        eprintln!(
                            "Verification failed (losslessness): parser output differs from input"
                        );
                        print_diff("<stdin>", &input, &tree_text, use_color);
                        std::process::exit(1);
                    }
                }
                let output = format(&input, Some(cfg.clone()), parsed_range);
                if verify {
                    let output_twice = format(&output, Some(cfg), parsed_range);
                    if output != output_twice {
                        eprintln!(
                            "Verification failed (idempotency): format(format(x)) != format(x)"
                        );
                        print_diff("<stdin>", &output, &output_twice, use_color);
                        std::process::exit(1);
                    }
                }

                if check {
                    if input != output {
                        print_diff("<stdin>", &input, &output, use_color);
                        std::process::exit(1);
                    }
                } else {
                    // Stdin: output to stdout
                    print!("{output}");
                }

                return Ok(());
            }

            // Expand paths (handle directories)
            let traversal_anchor = files.first().map(PathBuf::as_path);
            let traversal_start_dir = if let Some(anchor) = traversal_anchor {
                if anchor.is_dir() {
                    anchor.to_path_buf()
                } else {
                    start_dir_for(Some(anchor))?
                }
            } else {
                start_dir_for(None)?
            };
            let (traversal_cfg, traversal_cfg_source) = load_config_for_cli(
                cli.config.as_deref(),
                cli.isolated,
                cli.cache_dir.as_deref(),
                &traversal_start_dir,
                traversal_anchor,
                cli.flavor.map(Flavor::from),
            )?;
            let anchor = panache::config::anchor_dir(&traversal_cfg_source, &traversal_start_dir);
            let expanded_files = expand_paths(
                &files,
                &traversal_cfg,
                &anchor,
                force_exclude,
                cli.flavor.is_some(),
            )?;
            let mut cache = if cli.no_cache || !traversal_cfg.cache {
                None
            } else {
                open_cli_cache_best_effort(
                    &traversal_cfg,
                    cli.config.as_deref(),
                    &traversal_start_dir,
                )
            };

            if expanded_files.is_empty() {
                if force_exclude {
                    return Ok(());
                }
                if has_explicit_file_targets(&files) {
                    eprintln!("Error: No supported files found");
                    std::process::exit(1);
                }
                if !cli.quiet {
                    println!("No supported files found");
                }
                return Ok(());
            }

            // Per-file work runs either serially or across a rayon pool. Inner
            // external-formatter parallelism is forced to 1 when running multiple
            // files in parallel — benchmarks showed nested rayon pools over-
            // subscribe the CPU once the outer loop saturates cores.
            let workers = effective_parallelism(cli.jobs, expanded_files.len());
            let parallel = workers > 1;

            struct FormatOutcome {
                file_path: PathBuf,
                input: String,
                output: String,
            }

            let cache_shared: Option<Arc<Mutex<CliCache>>> =
                cache.take().map(|c| Arc::new(Mutex::new(c)));

            let process_file = |file_path: &PathBuf| -> io::Result<FormatOutcome> {
                let start_dir = file_path.parent().unwrap_or(Path::new(".")).to_path_buf();
                let (mut cfg, cfg_source) = load_config_for_cli(
                    cli.config.as_deref(),
                    cli.isolated,
                    cli.cache_dir.as_deref(),
                    &start_dir,
                    Some(file_path),
                    cli.flavor.map(Flavor::from),
                )?;
                if let Err(err) = apply_format_overrides(&mut cfg, &option) {
                    eprintln!("Error: {err}");
                    std::process::exit(2);
                }
                // Size the shared external-tool budget from the user-configured
                // value, then split that ceiling across the files processed
                // concurrently so a few files can saturate it while a large
                // batch stays at ~1-per-file.
                panache::init_external_tool_budget(cfg.external_max_parallel);
                if parallel {
                    cfg.external_max_parallel =
                        per_file_external_parallel(cfg.external_max_parallel, workers);
                }

                if let Some(path) = cfg_source.path() {
                    log::debug!("Using config from: {}", path.display());
                } else {
                    log::debug!("Using default config");
                }

                let input = fs::read_to_string(file_path)?;
                let mode = if check {
                    FormatCacheMode::Check
                } else {
                    FormatCacheMode::Write
                };

                if verify {
                    let tree = parse(&input, Some(cfg.clone()));
                    let tree_text = tree.text().to_string();
                    if input != tree_text {
                        let file_name = file_path.to_str().unwrap_or("<unknown>");
                        eprintln!(
                            "Verification failed (losslessness): parser output differs from input"
                        );
                        print_diff(file_name, &input, &tree_text, use_color);
                        std::process::exit(1);
                    }
                }

                let output = if !verify && parsed_range.is_none() {
                    if let Some(cache_handle) = cache_shared.as_ref() {
                        let file_fingerprint = CliCache::file_fingerprint(&input);
                        let config_fingerprint = CliCache::config_fingerprint(&cfg);
                        let tool_fingerprint = CliCache::tool_fingerprint();
                        let cached = {
                            let guard = cache_handle.lock().unwrap();
                            if guard.supports_format_mode(&cfg, mode) {
                                guard
                                    .get_format(
                                        file_path,
                                        mode,
                                        &file_fingerprint,
                                        &config_fingerprint,
                                        &tool_fingerprint,
                                    )
                                    .map(|hit| hit.1)
                            } else {
                                None
                            }
                        };
                        if let Some(cached) = cached {
                            cached
                        } else {
                            let output = format(&input, Some(cfg.clone()), parsed_range);
                            let mut guard = cache_handle.lock().unwrap();
                            if guard.supports_format_mode(&cfg, mode) {
                                let unchanged = input == output;
                                guard.put_format(
                                    file_path,
                                    mode,
                                    FormatStoreArgs {
                                        file_fingerprint,
                                        config_fingerprint,
                                        tool_fingerprint,
                                        unchanged,
                                        output: output.clone(),
                                    },
                                );
                            }
                            output
                        }
                    } else {
                        format(&input, Some(cfg.clone()), parsed_range)
                    }
                } else {
                    format(&input, Some(cfg.clone()), parsed_range)
                };

                if verify {
                    let output_twice = format(&output, Some(cfg), parsed_range);
                    if output != output_twice {
                        let file_name = file_path.to_str().unwrap_or("<unknown>");
                        eprintln!(
                            "Verification failed (idempotency): format(format(x)) != format(x)"
                        );
                        print_diff(file_name, &output, &output_twice, use_color);
                        std::process::exit(1);
                    }
                }

                Ok(FormatOutcome {
                    file_path: file_path.clone(),
                    input,
                    output,
                })
            };

            let outcomes: Vec<io::Result<FormatOutcome>> = if parallel {
                use rayon::prelude::*;
                let pool = build_pool(workers);
                pool.install(|| expanded_files.par_iter().map(&process_file).collect())
            } else {
                expanded_files.iter().map(&process_file).collect()
            };

            // Recover the cache for the final flush.
            if let Some(handle) = cache_shared {
                cache = Some(
                    Arc::try_unwrap(handle)
                        .map_err(|_| {
                            io::Error::other("cache Arc still shared after parallel pass")
                        })?
                        .into_inner()
                        .map_err(|e| io::Error::other(format!("cache mutex poisoned: {e}")))?,
                );
            }

            // Sequential post-pass: emit messages, write files, tally counters.
            // Keeps output deterministic in input order.
            let mut all_formatted = true;
            let mut reformatted_count = 0usize;
            let mut unchanged_count = 0usize;
            for outcome in outcomes {
                let o = outcome?;
                if check {
                    if o.input != o.output {
                        let file_name = o.file_path.to_str().unwrap_or("<unknown>");
                        print_diff(file_name, &o.input, &o.output, use_color);
                        all_formatted = false;
                    } else if expanded_files.len() == 1 && !cli.quiet {
                        println!("{} is correctly formatted", o.file_path.display());
                    }
                } else if !verify {
                    if o.input != o.output {
                        fs::write(&o.file_path, &o.output)?;
                        if !cli.quiet {
                            println!("Formatted {}", o.file_path.display());
                        }
                        reformatted_count += 1;
                    } else {
                        unchanged_count += 1;
                    }
                }
            }

            if check {
                if all_formatted {
                    if expanded_files.len() > 1 && !cli.quiet {
                        println!("All {} files are correctly formatted", expanded_files.len());
                    }
                } else {
                    std::process::exit(1);
                }
            } else if !verify && !cli.quiet {
                if reformatted_count == 0 {
                    println!(
                        "{}",
                        file_count_label(
                            unchanged_count,
                            "file left unchanged",
                            "files left unchanged"
                        )
                    );
                } else {
                    println!(
                        "{}, {}",
                        file_count_label(
                            reformatted_count,
                            "file reformatted",
                            "files reformatted"
                        ),
                        file_count_label(
                            unchanged_count,
                            "file left unchanged",
                            "files left unchanged"
                        )
                    );
                }
            }
            if let Some(cache_ref) = cache.as_mut() {
                cache_ref.save_if_dirty()?;
            }

            Ok(())
        }
        Commands::Clean { all, dry_run } => {
            let start_dir = start_dir_for(None)?;
            let (cfg, _) = load_config_for_cli(
                cli.config.as_deref(),
                cli.isolated,
                cli.cache_dir.as_deref(),
                &start_dir,
                None,
                cli.flavor.map(Flavor::from),
            )?;

            let report_clean = |message: String| {
                if !cli.quiet {
                    println!("{message}");
                }
            };

            let summarize = |path: &Path| -> io::Result<Option<(usize, u64)>> {
                if dry_run || cli.verbose {
                    summarize_dir(path)
                } else {
                    Ok(None)
                }
            };

            let removed_verb = if dry_run { "Would remove" } else { "Removed" };
            let act = |path: &Path| -> io::Result<bool> {
                if dry_run {
                    Ok(path.exists())
                } else {
                    remove_dir_if_exists(path)
                }
            };

            if all {
                if cfg.cache_dir.is_some() {
                    let cache_dir =
                        resolve_cache_dir_for_cli(&cfg, cli.config.as_deref(), &start_dir)?;
                    let summary = summarize(&cache_dir)?;
                    let removed = act(&cache_dir)?;
                    if removed {
                        report_clean(format!(
                            "{removed_verb} cache directory {}{}",
                            cache_dir.display(),
                            format_clean_summary(summary)
                        ));
                    } else {
                        report_clean(format!(
                            "No cache directory found at {}",
                            cache_dir.display()
                        ));
                    }
                } else if let Some(global_base) = global_cache_base_dir() {
                    let summary = summarize(&global_base)?;
                    let removed = act(&global_base)?;
                    if removed {
                        report_clean(format!(
                            "{removed_verb} all cache buckets at {}{}",
                            global_base.display(),
                            format_clean_summary(summary)
                        ));
                    } else {
                        report_clean(format!(
                            "No cache buckets found at {}",
                            global_base.display()
                        ));
                    }
                } else {
                    let cache_dir =
                        resolve_cache_dir_for_cli(&cfg, cli.config.as_deref(), &start_dir)?;
                    let summary = summarize(&cache_dir)?;
                    let removed = act(&cache_dir)?;
                    if removed {
                        report_clean(format!(
                            "{removed_verb} cache directory {}{}",
                            cache_dir.display(),
                            format_clean_summary(summary)
                        ));
                    } else {
                        report_clean(format!(
                            "No cache directory found at {}",
                            cache_dir.display()
                        ));
                    }
                }
            } else {
                let cache_dir = resolve_cache_dir_for_cli(&cfg, cli.config.as_deref(), &start_dir)?;
                let summary = summarize(&cache_dir)?;
                let removed = act(&cache_dir)?;
                if removed {
                    report_clean(format!(
                        "{removed_verb} cache directory {}{}",
                        cache_dir.display(),
                        format_clean_summary(summary)
                    ));
                } else {
                    report_clean(format!(
                        "No cache directory found at {}",
                        cache_dir.display()
                    ));
                }
            }

            Ok(())
        }
        Commands::Debug { command } => match command {
            DebugCommands::Format {
                files,
                checks,
                json,
                report,
                dump_dir,
                dump_passes,
                force_exclude,
            } => {
                if json && report {
                    eprintln!("Error: --json and --report cannot be used together");
                    std::process::exit(1);
                }
                if dump_passes && dump_dir.is_none() {
                    eprintln!("Error: --dump-passes requires --dump-dir <DIR>");
                    std::process::exit(1);
                }

                let files = match normalize_input_paths(files) {
                    Ok(files) => files,
                    Err(err) => {
                        eprintln!("Error: {}", err);
                        std::process::exit(1);
                    }
                };
                let use_stdin = files.is_empty();
                let targets = if use_stdin {
                    vec![]
                } else {
                    let traversal_anchor = files.first().map(PathBuf::as_path);
                    let traversal_start_dir = if let Some(anchor) = traversal_anchor {
                        if anchor.is_dir() {
                            anchor.to_path_buf()
                        } else {
                            start_dir_for(Some(anchor))?
                        }
                    } else {
                        start_dir_for(None)?
                    };
                    let (traversal_cfg, traversal_cfg_source) = load_config_for_cli(
                        cli.config.as_deref(),
                        cli.isolated,
                        cli.cache_dir.as_deref(),
                        &traversal_start_dir,
                        traversal_anchor,
                        cli.flavor.map(Flavor::from),
                    )?;
                    let anchor =
                        panache::config::anchor_dir(&traversal_cfg_source, &traversal_start_dir);
                    expand_paths(
                        &files,
                        &traversal_cfg,
                        &anchor,
                        force_exclude,
                        cli.flavor.is_some(),
                    )?
                };

                if !use_stdin && targets.is_empty() {
                    if has_explicit_file_targets(&files) {
                        eprintln!("Error: No supported files found");
                        std::process::exit(1);
                    }
                    if json {
                        let output = json!({
                            "checks": format!("{:?}", checks).to_lowercase(),
                            "files_checked": 0,
                            "failure_count": 0,
                            "failures": Vec::<serde_json::Value>::new(),
                        });
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&output).map_err(io::Error::other)?
                        );
                    } else if !cli.quiet {
                        println!("No supported files found");
                    }
                    return Ok(());
                }

                let mut files_checked = 0usize;
                let mut failure_count = 0usize;
                let mut json_failures = Vec::new();
                let mut collected_failures: Vec<(String, DebugFailure)> = Vec::new();

                if use_stdin {
                    let start_dir = start_dir_for(cli.stdin_filename.as_deref())?;
                    let (cfg, _) = load_config_for_cli(
                        cli.config.as_deref(),
                        cli.isolated,
                        cli.cache_dir.as_deref(),
                        &start_dir,
                        cli.stdin_filename.as_deref(),
                        cli.flavor.map(Flavor::from),
                    )?;
                    let input = read_all(None)?;
                    files_checked += 1;

                    let artifacts = run_debug_checks_for_content(&input, &cfg, checks, "<stdin>");
                    if let Some(dir) = dump_dir.as_ref() {
                        write_debug_artifacts(dir, "stdin", &artifacts, dump_passes)?;
                    }

                    for failure in &artifacts.failures {
                        failure_count += 1;
                        if !json && !report {
                            eprintln!("Debug check failed ({}) in <stdin>", failure.kind.label());
                            print_diff("<stdin>", &failure.left, &failure.right, use_color);
                        }
                        json_failures.push(json!({
                            "file": "<stdin>",
                            "kind": failure.kind.label(),
                        }));
                        if report {
                            collected_failures.push(("<stdin>".to_string(), failure.clone()));
                        }
                    }
                } else {
                    for file_path in &targets {
                        let start_dir = file_path.parent().unwrap_or(Path::new(".")).to_path_buf();
                        let (cfg, _) = load_config_for_cli(
                            cli.config.as_deref(),
                            cli.isolated,
                            cli.cache_dir.as_deref(),
                            &start_dir,
                            Some(file_path),
                            cli.flavor.map(Flavor::from),
                        )?;
                        let input = fs::read_to_string(file_path)?;
                        files_checked += 1;
                        let file_label = file_path.to_str().unwrap_or("<unknown>");

                        let artifacts =
                            run_debug_checks_for_content(&input, &cfg, checks, file_label);
                        if let Some(dir) = dump_dir.as_ref() {
                            let safe = sanitize_path_for_filename(file_label);
                            write_debug_artifacts(dir, &safe, &artifacts, dump_passes)?;
                        }

                        for failure in &artifacts.failures {
                            failure_count += 1;
                            if !json && !report {
                                eprintln!(
                                    "Debug check failed ({}) in {}",
                                    failure.kind.label(),
                                    file_label
                                );
                                print_diff(file_label, &failure.left, &failure.right, use_color);
                            }
                            json_failures.push(json!({
                                "file": file_label,
                                "kind": failure.kind.label(),
                            }));
                            if report {
                                collected_failures.push((file_label.to_string(), failure.clone()));
                            }
                        }
                    }
                }

                if json {
                    let output = json!({
                        "checks": format!("{:?}", checks).to_lowercase(),
                        "files_checked": files_checked,
                        "failure_count": failure_count,
                        "failures": json_failures,
                    });
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&output).map_err(io::Error::other)?
                    );
                } else if report {
                    let markdown =
                        build_debug_failure_report(checks, files_checked, &collected_failures);
                    println!("{markdown}");
                } else if failure_count == 0 && !cli.quiet {
                    println!(
                        "All checks passed (checks: {}, files: {})",
                        format!("{:?}", checks).to_lowercase(),
                        files_checked
                    );
                }

                if dump_passes
                    && !json
                    && !cli.quiet
                    && let Some(dir) = dump_dir.as_ref()
                {
                    eprintln!("Wrote debug artifacts to {}", dir.display());
                }

                if failure_count > 0 && !json && !report && !cli.quiet && dump_dir.is_none() {
                    eprintln!(
                        "Tip: rerun with --dump-dir <DIR> --dump-passes to inspect input, parse, and format passes."
                    );
                }

                if failure_count > 0 {
                    std::process::exit(1);
                }
                Ok(())
            }
        },
        #[cfg(feature = "lsp")]
        Commands::Lsp { .. } => {
            panache::lsp::run()?;
            Ok(())
        }
        Commands::Lint {
            files,
            check,
            fix,
            unsafe_fixes,
            message_format,
            force_exclude,
        } => {
            let files = match normalize_input_paths(files) {
                Ok(files) => files,
                Err(err) => {
                    eprintln!("Error: {}", err);
                    std::process::exit(1);
                }
            };
            // Quarto project manifests (`_quarto.yml` / `_metadata.yml`) are
            // standalone YAML, not markdown documents. Pull explicit manifest
            // targets out of the normal document pipeline and validate them
            // against the Quarto schema directly. The filename detects as Quarto
            // (see `detect_flavor`), so schema validation applies unless an
            // explicit `--flavor` overrides it — matching how a `.qmd` behaves.
            let (manifest_files, files): (Vec<PathBuf>, Vec<PathBuf>) = files
                .into_iter()
                .partition(|p| panache::linter::quarto_schema::manifest_schema_root(p).is_some());
            // Handle stdin case
            if files.is_empty() && manifest_files.is_empty() {
                let start_dir = start_dir_for(cli.stdin_filename.as_deref())?;
                let (cfg, cfg_source) = load_config_for_cli(
                    cli.config.as_deref(),
                    cli.isolated,
                    cli.cache_dir.as_deref(),
                    &start_dir,
                    cli.stdin_filename.as_deref(),
                    cli.flavor.map(Flavor::from),
                )?;

                if let Some(path) = cfg_source.path() {
                    log::debug!("Using config from: {}", path.display());
                } else {
                    log::debug!("Using default config");
                }

                let input = read_all(None)?;
                let tree = parse(&input, Some(cfg.clone()));
                let stdin_path = cli
                    .stdin_filename
                    .as_deref()
                    .unwrap_or(Path::new("stdin.md"));
                let metadata = panache::metadata::extract_project_metadata(&tree, stdin_path).ok();
                let mut diagnostics = panache::linter::lint_with_external_sync_and_metadata(
                    &tree,
                    &input,
                    &cfg,
                    metadata.as_ref(),
                );
                let db = panache::salsa::SalsaDb::default();
                let yaml_diags = panache::salsa::built_in_lint_plan(
                    &db,
                    panache::salsa::FileText::from_str(&db, input.clone()),
                    panache::salsa::FileConfig::new(&db, cfg.clone()),
                )
                .diagnostics
                .iter()
                .filter(|d| d.code == "yaml-parse-error")
                .cloned()
                .collect::<Vec<_>>();
                merge_missing_diagnostics(&mut diagnostics, yaml_diags);

                if diagnostics.is_empty() {
                    if !check && !cli.quiet {
                        println!("No issues found");
                    }
                    return Ok(());
                }

                if fix {
                    let fixed_output = apply_fixes(&input, &diagnostics, unsafe_fixes);
                    print!("{}", fixed_output);
                    let unsafe_skipped = if unsafe_fixes {
                        0
                    } else {
                        count_unsafe_fixes(&diagnostics)
                    };
                    if unsafe_skipped > 0 && !cli.quiet {
                        eprintln!("{}", unsafe_fixes_hint(unsafe_skipped));
                    }
                } else if !cli.quiet {
                    print_diagnostics(
                        &diagnostics,
                        None,
                        Some(&input),
                        use_color,
                        message_format,
                        true,
                    );
                }

                if check {
                    std::process::exit(1);
                }

                return Ok(());
            }

            // Expand paths (handle directories)
            let traversal_anchor = files.first().map(PathBuf::as_path);
            let traversal_start_dir = if let Some(anchor) = traversal_anchor {
                if anchor.is_dir() {
                    anchor.to_path_buf()
                } else {
                    start_dir_for(Some(anchor))?
                }
            } else {
                start_dir_for(None)?
            };
            let (traversal_cfg, traversal_cfg_source) = load_config_for_cli(
                cli.config.as_deref(),
                cli.isolated,
                cli.cache_dir.as_deref(),
                &traversal_start_dir,
                traversal_anchor,
                cli.flavor.map(Flavor::from),
            )?;
            let anchor = panache::config::anchor_dir(&traversal_cfg_source, &traversal_start_dir);
            let expanded_files = expand_paths(
                &files,
                &traversal_cfg,
                &anchor,
                force_exclude,
                cli.flavor.is_some(),
            )?;
            let mut cache = if cli.no_cache || !traversal_cfg.cache {
                None
            } else {
                open_cli_cache_best_effort(
                    &traversal_cfg,
                    cli.config.as_deref(),
                    &traversal_start_dir,
                )
            };

            if expanded_files.is_empty() && manifest_files.is_empty() {
                if force_exclude {
                    return Ok(());
                }
                if has_explicit_file_targets(&files) {
                    eprintln!("Error: No supported files found");
                    std::process::exit(1);
                }
                if !cli.quiet {
                    println!("No supported files found");
                }
                return Ok(());
            }

            let workers = effective_parallelism(cli.jobs, expanded_files.len());
            let parallel = workers > 1;

            struct LintOutcome {
                file_path: PathBuf,
                root_doc: Option<LintedDocument>,
                included_docs: Vec<LintedDocument>,
            }

            let cache_shared: Option<Arc<Mutex<CliCache>>> =
                cache.take().map(|c| Arc::new(Mutex::new(c)));

            let process_file = |file_path: &PathBuf| -> io::Result<LintOutcome> {
                let start_dir = file_path.parent().unwrap_or(Path::new(".")).to_path_buf();
                let (mut cfg, cfg_source) = load_config_for_cli(
                    cli.config.as_deref(),
                    cli.isolated,
                    cli.cache_dir.as_deref(),
                    &start_dir,
                    Some(file_path),
                    cli.flavor.map(Flavor::from),
                )?;
                // Size the shared external-tool budget from the user-configured
                // value, then split that ceiling across the files processed
                // concurrently so a few files can saturate it while a large
                // batch stays at ~1-per-file.
                panache::init_external_tool_budget(cfg.external_max_parallel);
                if parallel {
                    cfg.external_max_parallel =
                        per_file_external_parallel(cfg.external_max_parallel, workers);
                }

                if let Some(path) = cfg_source.path() {
                    log::debug!("Using config from: {}", path.display());
                } else {
                    log::debug!("Using default config");
                }

                let root_input = fs::read_to_string(file_path)?;

                let documents = if let Some(cache_handle) = cache_shared.as_ref() {
                    let file_fingerprint = CliCache::file_fingerprint(&root_input);
                    let config_fingerprint = CliCache::config_fingerprint(&cfg);
                    let tool_fingerprint = CliCache::tool_fingerprint();

                    let cached_lookup = {
                        let guard = cache_handle.lock().unwrap();
                        if guard.supports_lint(&cfg) {
                            guard
                                .get_lint(
                                    file_path,
                                    &file_fingerprint,
                                    &config_fingerprint,
                                    &tool_fingerprint,
                                )
                                .filter(|docs| cached_lint_documents_are_fresh(docs))
                        } else {
                            None
                        }
                    };
                    if let Some(cached_documents) = cached_lookup {
                        cached_documents
                            .iter()
                            .map(linted_document_from_cached)
                            .collect::<Vec<_>>()
                    } else {
                        let documents = lint_documents_with_includes(file_path, &root_input, &cfg)?;
                        let mut guard = cache_handle.lock().unwrap();
                        if guard.supports_lint(&cfg) {
                            let cached_docs = documents
                                .iter()
                                .map(cached_lint_document_from_linted)
                                .collect::<Vec<_>>();
                            guard.put_lint(
                                file_path,
                                file_fingerprint,
                                config_fingerprint,
                                tool_fingerprint,
                                cached_docs,
                            );
                        }
                        documents
                    }
                } else {
                    lint_documents_with_includes(file_path, &root_input, &cfg)?
                };

                let root_doc = documents.iter().find(|doc| &doc.path == file_path).cloned();
                let mut included_docs: Vec<LintedDocument> = documents
                    .into_iter()
                    .filter(|doc| &doc.path != file_path)
                    .collect();
                included_docs.sort_by(|a, b| a.path.cmp(&b.path));

                Ok(LintOutcome {
                    file_path: file_path.clone(),
                    root_doc,
                    included_docs,
                })
            };

            let outcomes: Vec<io::Result<LintOutcome>> = if parallel {
                use rayon::prelude::*;
                let pool = build_pool(workers);
                pool.install(|| expanded_files.par_iter().map(&process_file).collect())
            } else {
                expanded_files.iter().map(&process_file).collect()
            };

            if let Some(handle) = cache_shared {
                cache = Some(
                    Arc::try_unwrap(handle)
                        .map_err(|_| {
                            io::Error::other("cache Arc still shared after parallel pass")
                        })?
                        .into_inner()
                        .map_err(|e| io::Error::other(format!("cache mutex poisoned: {e}")))?,
                );
            }

            let mut any_issues = false;
            let mut total_issues = 0;
            for outcome in outcomes {
                let LintOutcome {
                    file_path,
                    root_doc,
                    included_docs,
                } = outcome?;

                let Some(root_doc) = root_doc else {
                    continue;
                };

                if !root_doc.diagnostics.is_empty() {
                    any_issues = true;
                    total_issues += root_doc.diagnostics.len();

                    if fix {
                        let fixable = root_doc
                            .diagnostics
                            .iter()
                            .filter(|d| fix_will_apply(d, unsafe_fixes))
                            .count();
                        // Diagnostics left unfixed: no applicable fix, plus
                        // unsafe fixes skipped because --unsafe-fixes wasn't
                        // passed. These are still reported to the user.
                        let remaining: Vec<_> = root_doc
                            .diagnostics
                            .iter()
                            .filter(|d| !fix_will_apply(d, unsafe_fixes))
                            .cloned()
                            .collect();
                        // The summary's "no auto-fix available" count is only
                        // the genuinely unfixable diagnostics; an available-but-
                        // skipped unsafe fix is surfaced by the hint instead.
                        let no_fix_count = root_doc
                            .diagnostics
                            .iter()
                            .filter(|d| d.fix.is_none())
                            .count();
                        let unsafe_skipped = if unsafe_fixes {
                            0
                        } else {
                            count_unsafe_fixes(&root_doc.diagnostics)
                        };
                        if fixable > 0 {
                            let fixed_output =
                                apply_fixes(&root_doc.input, &root_doc.diagnostics, unsafe_fixes);
                            fs::write(&file_path, fixed_output)?;
                        }
                        if !remaining.is_empty() && !cli.quiet {
                            print_diagnostics(
                                &remaining,
                                Some(file_path.as_path()),
                                Some(&root_doc.input),
                                use_color,
                                message_format,
                                false,
                            );
                        }
                        if !cli.quiet {
                            print_fix_summary(fixable, no_fix_count, &file_path);
                            if unsafe_skipped > 0 {
                                println!("{}", unsafe_fixes_hint(unsafe_skipped));
                            }
                        }
                    } else if !cli.quiet {
                        print_diagnostics(
                            &root_doc.diagnostics,
                            Some(file_path.as_path()),
                            Some(&root_doc.input),
                            use_color,
                            message_format,
                            true,
                        );
                    }
                }

                if !fix {
                    for doc in &included_docs {
                        if doc.diagnostics.is_empty() {
                            continue;
                        }
                        any_issues = true;
                        total_issues += doc.diagnostics.len();
                        if !cli.quiet {
                            print_diagnostics(
                                &doc.diagnostics,
                                Some(doc.path.as_path()),
                                Some(&doc.input),
                                use_color,
                                message_format,
                                true,
                            );
                        }
                    }
                }
            }
            if let Some(cache_ref) = cache.as_mut() {
                cache_ref.save_if_dirty()?;
            }

            // Validate explicit Quarto manifest targets against the schema. These
            // bypass the document pipeline (and its cache); there are typically
            // only a handful per invocation, so a simple sequential pass is fine.
            for manifest_path in &manifest_files {
                let manifest_doc = match lint_quarto_manifest(
                    manifest_path,
                    cli.config.as_deref(),
                    cli.isolated,
                    cli.cache_dir.as_deref(),
                    cli.flavor.map(Flavor::from),
                ) {
                    Ok(doc) => doc,
                    Err(err) => {
                        eprintln!("Error: {}: {}", manifest_path.display(), err);
                        std::process::exit(1);
                    }
                };
                if manifest_doc.diagnostics.is_empty() {
                    continue;
                }
                any_issues = true;
                total_issues += manifest_doc.diagnostics.len();
                if !cli.quiet {
                    // Manifest diagnostics carry no auto-fixes; print them as-is
                    // even under `--fix`.
                    print_diagnostics(
                        &manifest_doc.diagnostics,
                        Some(manifest_path.as_path()),
                        Some(&manifest_doc.input),
                        use_color,
                        message_format,
                        true,
                    );
                }
            }

            // Project-level bibliography defects: when a directory (project) was
            // scanned, check the manifests that contribute to the scanned
            // documents for duplicate keys in their declared bibliographies. A
            // duplicate inside a project-level bibliography is reported once
            // here, against `_quarto.yml`, instead of once per inheriting
            // document. A single-file lint stays quiet about the ambient
            // manifest (no directory in scope); explicit manifest targets are
            // already handled by the loop above.
            if files.iter().any(|p| p.is_dir()) {
                let explicit: std::collections::HashSet<PathBuf> =
                    manifest_files.iter().cloned().collect();
                let mut discovered: std::collections::BTreeSet<PathBuf> =
                    std::collections::BTreeSet::new();
                for doc in &expanded_files {
                    for manifest in panache::metadata::project_manifests_for(doc) {
                        if !explicit.contains(&manifest) {
                            discovered.insert(manifest);
                        }
                    }
                }
                for manifest_path in discovered {
                    let manifest_doc = match lint_manifest_bibliography(
                        &manifest_path,
                        cli.config.as_deref(),
                        cli.isolated,
                        cli.cache_dir.as_deref(),
                        cli.flavor.map(Flavor::from),
                    ) {
                        Ok(doc) => doc,
                        Err(err) => {
                            eprintln!("Error: {}: {}", manifest_path.display(), err);
                            std::process::exit(1);
                        }
                    };
                    if manifest_doc.diagnostics.is_empty() {
                        continue;
                    }
                    any_issues = true;
                    total_issues += manifest_doc.diagnostics.len();
                    if !cli.quiet {
                        print_diagnostics(
                            &manifest_doc.diagnostics,
                            Some(manifest_path.as_path()),
                            Some(&manifest_doc.input),
                            use_color,
                            message_format,
                            true,
                        );
                    }
                }
            }

            let total_files = expanded_files.len() + manifest_files.len();

            if !any_issues && !check && !cli.quiet {
                println!("No issues found in {} file(s)", total_files);
            }

            if check && any_issues {
                eprintln!(
                    "\nFound {} issue(s) across {} file(s)",
                    total_issues, total_files
                );
                std::process::exit(1);
            }

            Ok(())
        }
    }
}

#[derive(Debug, Clone)]
struct LintedDocument {
    path: PathBuf,
    input: String,
    diagnostics: Vec<panache::linter::Diagnostic>,
}

/// Lint a standalone Quarto project manifest (`_quarto.yml` / `_metadata.yml`)
/// against the Quarto schema. The schema half honors the `quarto-schema` rule
/// toggle (resolved from config near the manifest); YAML parse errors are always
/// reported. The caller guarantees `path` is a recognized manifest.
fn lint_quarto_manifest(
    path: &Path,
    config: Option<&Path>,
    isolated: bool,
    cache_dir: Option<&Path>,
    flavor: Option<Flavor>,
) -> io::Result<LintedDocument> {
    let input = fs::read_to_string(path)?;
    let root = panache::linter::quarto_schema::manifest_schema_root(path)
        .expect("caller only passes recognized manifest paths");
    let start_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let (cfg, _cfg_source) =
        load_config_for_cli(config, isolated, cache_dir, &start_dir, Some(path), flavor)?;
    // The manifest filename detects as Quarto, so `cfg.flavor` is Quarto unless
    // an explicit `--flavor` overrode it; gate the schema half on that (and the
    // rule toggles) so the CLI and LSP agree on when manifests are validated.
    // Type/enum ride the on-by-default `quarto-schema` rule; unknown-key is the
    // opt-in `quarto-schema-unknown-key` rule.
    let quarto = cfg.flavor == Flavor::Quarto;
    let type_enum_enabled = quarto && cfg.lint.is_rule_enabled("quarto-schema");
    let unknown_key_enabled = quarto
        && cfg
            .lint
            .is_rule_explicitly_enabled("quarto-schema-unknown-key");
    let mut diagnostics = panache::linter::quarto_schema::lint_manifest_text(
        &input,
        root,
        type_enum_enabled,
        unknown_key_enabled,
    );
    // A manifest can declare a `bibliography`; check those bibs for duplicate
    // keys here so an explicitly-targeted `_quarto.yml` reports them on itself.
    if cfg.extensions.citations && cfg.lint.is_rule_enabled("citation-keys") {
        diagnostics.extend(
            panache::linter::metadata_diagnostics::manifest_bibliography_diagnostics(path, &input),
        );
        diagnostics.sort_by_key(|d| (d.location.line, d.location.column));
    }
    Ok(LintedDocument {
        path: path.to_path_buf(),
        input,
        diagnostics,
    })
}

/// Lint a project manifest (`_quarto.yml`/`_metadata.yml`) for duplicate keys in
/// the bibliographies it declares, anchored to its own `bibliography:` value(s).
/// Used for manifests discovered during a project (directory) lint, where the
/// manifest itself is not an explicit target.
fn lint_manifest_bibliography(
    path: &Path,
    config: Option<&Path>,
    isolated: bool,
    cache_dir: Option<&Path>,
    flavor: Option<Flavor>,
) -> io::Result<LintedDocument> {
    let input = fs::read_to_string(path)?;
    let start_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let (cfg, _cfg_source) =
        load_config_for_cli(config, isolated, cache_dir, &start_dir, Some(path), flavor)?;
    let diagnostics = if cfg.extensions.citations && cfg.lint.is_rule_enabled("citation-keys") {
        panache::linter::metadata_diagnostics::manifest_bibliography_diagnostics(path, &input)
    } else {
        Vec::new()
    };
    Ok(LintedDocument {
        path: path.to_path_buf(),
        input,
        diagnostics,
    })
}

fn lint_documents_with_includes(
    root_path: &PathBuf,
    root_input: &str,
    cfg: &panache::Config,
) -> io::Result<Vec<LintedDocument>> {
    use std::collections::HashSet;

    let mut results = Vec::new();
    let mut visited = HashSet::new();
    let mut active = HashSet::new();
    let mut db = panache::salsa::SalsaDb::default();
    // Construct one FileConfig handle per batch and one FileText per file.
    // Salsa cache keys are handle identity, not value equality, so reusing
    // these across built_in_lint_plan / project_graph::accumulated within a
    // single file's lint is the only way to get cache hits.
    //
    // The eager project_graph call was removed: project_graph is salsa-tracked
    // and computed on demand by project_graph::accumulated when (and only when)
    // we determine the document participates in a project. For flat directories
    // of standalone files this avoids 1+ parse per file.
    let file_config = panache::salsa::FileConfig::new(&db, cfg.clone());
    // Register the root in the VFS (assigning it a FileId + FileSet entry) so the
    // discovery loop's `intern_file(root_path)` resolves to this same input
    // rather than minting a duplicate and re-reading the root from disk. The
    // returned handle is reused below for cache hits across queries.
    let root_file_text = db.update_file_text(root_path.clone(), root_input.to_string());
    // `Db::file_text` is a pure lookup (audit §3.2/§3.3): it no longer lazy-loads
    // includes/bibliography from disk inside queries. Load the project's
    // referenced files onto the writer up front so `project_graph` and
    // `metadata` (cross-doc diagnostics, bibliography parse) see them.
    db.load_referenced_files(root_file_text, file_config, root_path.clone());
    lint_loaded_document_with_includes(
        root_path,
        root_input,
        Some(root_file_text),
        cfg,
        file_config,
        &mut results,
        &mut visited,
        &mut active,
        &db,
    )?;
    Ok(results)
}

#[allow(clippy::too_many_arguments, clippy::only_used_in_recursion)]
fn lint_loaded_document_with_includes(
    doc_path: &PathBuf,
    input: &str,
    file_text: Option<panache::salsa::FileText>,
    cfg: &panache::Config,
    file_config: panache::salsa::FileConfig,
    results: &mut Vec<LintedDocument>,
    visited: &mut std::collections::HashSet<PathBuf>,
    active: &mut std::collections::HashSet<PathBuf>,
    db: &panache::salsa::SalsaDb,
) -> io::Result<()> {
    if !visited.insert(doc_path.clone()) {
        return Ok(());
    }

    active.insert(doc_path.clone());

    // Reuse the root file's FileText handle (constructed in
    // lint_documents_with_includes) so the salsa cache hits across
    // project_graph and built_in_lint_plan. For included files reuse the handle
    // `load_referenced_files` already registered in the VFS, so the queries can
    // resolve the document's path from its `FileText` identity (audit §3.3 / G3);
    // mint a fresh (pathless) one only as a last resort.
    let file_text = file_text
        .or_else(|| db.file_text_if_cached(doc_path))
        .unwrap_or_else(|| panache::salsa::FileText::from_str(db, input));

    // Source built-in diagnostics from the salsa-tracked plan rather than
    // running the rule registry a second time in the host. Salsa's plan
    // already covers built-in rules, frontmatter-yaml errors, and the metadata
    // pipeline; the host adds external linters (sync) and project-graph
    // diagnostics on top.
    let plan = panache::salsa::built_in_lint_plan(db, file_text, file_config).clone();
    let mut diagnostics = plan.diagnostics;
    if !plan.external_jobs.is_empty() {
        diagnostics.extend(run_external_lint_jobs_sync(&plan.external_jobs, input));
        diagnostics.sort_by_key(|d| (d.location.line, d.location.column));
    }

    // Re-materialize the cached tree for the include scan; it is shared with
    // built_in_lint_plan/project_graph via salsa so this is a refcount bump,
    // not a parse.
    let tree = panache::salsa::parsed_tree_root(db, file_text, file_config);
    let base_dir = doc_path.parent().unwrap_or(Path::new("."));
    let roots = panache::includes::find_project_roots(doc_path);
    let project_root = roots.quarto.clone();
    let resolution =
        panache::includes::collect_includes(&tree, input, base_dir, project_root.as_deref(), cfg);

    diagnostics.extend(resolution.diagnostics);
    // Project-graph diagnostics only apply to documents that participate in a
    // project (Quarto/bookdown root) or that pull in includes. For the
    // overwhelmingly common flat-directory case (e.g. linting a folder of
    // standalone .md files) skip the salsa accumulator entirely; this is the
    // dominant per-file cost on large many-file batches.
    if roots.quarto.is_some() || roots.bookdown.is_some() || !resolution.includes.is_empty() {
        let graph_diags = panache::salsa::project_graph::accumulated::<
            panache::salsa::GraphDiagnostic,
        >(db, file_text, file_config);
        for entry in graph_diags {
            if entry.0.path == *doc_path {
                diagnostics.push(entry.0.diagnostic.clone());
            }
        }
    }

    for include in &resolution.includes {
        if active.contains(&include.path) {
            diagnostics.push(panache::includes::include_cycle_diagnostic(
                input,
                include.range,
                &include.path,
            ));
            continue;
        }
        if visited.contains(&include.path) {
            continue;
        }
        match fs::read_to_string(&include.path) {
            Ok(include_input) => {
                lint_loaded_document_with_includes(
                    &include.path,
                    &include_input,
                    None,
                    cfg,
                    file_config,
                    results,
                    visited,
                    active,
                    db,
                )?;
            }
            Err(err) => {
                diagnostics.push(panache::includes::include_read_error_diagnostic(
                    input,
                    include.range,
                    &include.path,
                    &err.to_string(),
                ));
            }
        }
    }

    diagnostics.sort_by_key(|d| (d.location.line, d.location.column));
    results.push(LintedDocument {
        path: doc_path.clone(),
        input: input.to_string(),
        diagnostics,
    });

    active.remove(doc_path);
    Ok(())
}

fn print_fix_summary(fixed: usize, remaining: usize, file: &Path) {
    match (fixed, remaining) {
        (0, 0) => {}
        (0, _) => println!(
            "Found {} issue(s) in {} (no auto-fix available)",
            remaining,
            file.display()
        ),
        (_, 0) => println!("Fixed {} issue(s) in {}", fixed, file.display()),
        (_, _) => println!(
            "Fixed {} issue(s) in {} ({} remaining; no auto-fix available)",
            fixed,
            file.display(),
            remaining
        ),
    }
}

/// Whether a diagnostic's fix would be applied under the current safety mode:
/// it must carry a fix that is either safe or, with `allow_unsafe`, any fix.
fn fix_will_apply(diag: &panache::linter::Diagnostic, allow_unsafe: bool) -> bool {
    use panache::linter::FixSafety;
    diag.fix
        .as_ref()
        .is_some_and(|f| allow_unsafe || f.safety == FixSafety::Safe)
}

/// Number of diagnostics carrying an unsafe fix (regardless of whether it will
/// be applied), used to surface the `--unsafe-fixes` hint.
fn count_unsafe_fixes(diagnostics: &[panache::linter::Diagnostic]) -> usize {
    use panache::linter::FixSafety;
    diagnostics
        .iter()
        .filter(|d| {
            d.fix
                .as_ref()
                .is_some_and(|f| f.safety == FixSafety::Unsafe)
        })
        .count()
}

fn unsafe_fixes_hint(count: usize) -> String {
    format!("{count} unsafe fix(es) available; run with --unsafe-fixes to apply.")
}

fn apply_fixes(
    input: &str,
    diagnostics: &[panache::linter::Diagnostic],
    allow_unsafe: bool,
) -> String {
    use panache::linter::FixSafety;
    use panache::linter::diagnostics::Edit;

    let mut edits: Vec<&Edit> = diagnostics
        .iter()
        .filter_map(|d| d.fix.as_ref())
        .filter(|f| allow_unsafe || f.safety == FixSafety::Safe)
        .flat_map(|f| &f.edits)
        .collect();

    edits.sort_by_key(|e| e.range.start());

    let mut output = String::new();
    let mut last_end = 0;

    for edit in edits {
        let start: usize = edit.range.start().into();
        let end: usize = edit.range.end().into();

        output.push_str(&input[last_end..start]);
        output.push_str(&edit.replacement);
        last_end = end;
    }

    output.push_str(&input[last_end..]);
    output
}

fn merge_missing_diagnostics(
    diagnostics: &mut Vec<panache::linter::Diagnostic>,
    additional: Vec<panache::linter::Diagnostic>,
) {
    for diag in additional {
        if diagnostics.iter().any(|existing| {
            existing.code == diag.code && existing.location.range == diag.location.range
        }) {
            continue;
        }
        diagnostics.push(diag);
    }
}

/// Synchronously dispatch the external-linter jobs collected by
/// `built_in_lint_plan` and return their diagnostics. Mirrors the LSP path
/// (`diagnostics.rs`), so the CLI and LSP share the same pre-computed plan
/// instead of re-running the rule registry per-host.
fn run_external_lint_jobs_sync(
    jobs: &[panache::salsa::ExternalLintJob],
    input: &str,
) -> Vec<panache::linter::Diagnostic> {
    use panache::linter::external_linters::ExternalLinterRegistry;
    use panache::linter::external_linters_sync::run_linter_sync;

    let registry = ExternalLinterRegistry::new();
    let mut out = Vec::new();
    for job in jobs {
        match run_linter_sync(
            &job.linter_name,
            &job.language,
            &job.content,
            input,
            &registry,
            Some(&job.mappings),
        ) {
            Ok(diags) => out.extend(diags),
            Err(err) => log::warn!("External linter '{}' failed: {}", job.linter_name, err),
        }
    }
    out
}

fn cached_lint_documents_are_fresh(documents: &[CachedLintDocument]) -> bool {
    documents.iter().all(|doc| {
        let path = PathBuf::from(&doc.path);
        fs::read_to_string(path).is_ok_and(|current| current == doc.input)
    })
}

fn cached_lint_document_from_linted(doc: &LintedDocument) -> CachedLintDocument {
    CachedLintDocument {
        path: doc.path.to_string_lossy().to_string(),
        input: doc.input.clone(),
        diagnostics: doc
            .diagnostics
            .iter()
            .map(cached_diagnostic_from_runtime)
            .collect(),
    }
}

fn linted_document_from_cached(doc: &CachedLintDocument) -> LintedDocument {
    LintedDocument {
        path: PathBuf::from(&doc.path),
        input: doc.input.clone(),
        diagnostics: doc
            .diagnostics
            .iter()
            .map(runtime_diagnostic_from_cached)
            .collect(),
    }
}

fn cached_diagnostic_from_runtime(diag: &panache::linter::Diagnostic) -> cache::CachedDiagnostic {
    use cache::{
        CachedDiagnostic, CachedDiagnosticNote, CachedDiagnosticNoteKind, CachedDiagnosticOrigin,
        CachedEdit, CachedFix, CachedLocation, CachedSeverity,
    };

    let severity = match diag.severity {
        panache::linter::Severity::Error => CachedSeverity::Error,
        panache::linter::Severity::Warning => CachedSeverity::Warning,
        panache::linter::Severity::Info => CachedSeverity::Info,
    };
    let origin = match diag.origin {
        panache::linter::DiagnosticOrigin::BuiltIn => CachedDiagnosticOrigin::BuiltIn,
        panache::linter::DiagnosticOrigin::External => CachedDiagnosticOrigin::External,
    };
    let notes = diag
        .notes
        .iter()
        .map(|note| CachedDiagnosticNote {
            kind: match note.kind {
                panache::linter::DiagnosticNoteKind::Note => CachedDiagnosticNoteKind::Note,
                panache::linter::DiagnosticNoteKind::Help => CachedDiagnosticNoteKind::Help,
            },
            message: note.message.clone(),
        })
        .collect();
    let fix = diag.fix.as_ref().map(|fix| CachedFix {
        message: fix.message.clone(),
        edits: fix
            .edits
            .iter()
            .map(|edit| CachedEdit {
                start: u32::from(edit.range.start()),
                end: u32::from(edit.range.end()),
                replacement: edit.replacement.clone(),
            })
            .collect(),
        safety: match fix.safety {
            panache::linter::FixSafety::Safe => cache::CachedFixSafety::Safe,
            panache::linter::FixSafety::Unsafe => cache::CachedFixSafety::Unsafe,
        },
    });

    CachedDiagnostic {
        severity,
        location: CachedLocation {
            line: diag.location.line,
            column: diag.location.column,
            start: u32::from(diag.location.range.start()),
            end: u32::from(diag.location.range.end()),
        },
        message: diag.message.clone(),
        code: diag.code.clone(),
        origin,
        notes,
        fix,
    }
}

fn runtime_diagnostic_from_cached(diag: &cache::CachedDiagnostic) -> panache::linter::Diagnostic {
    use rowan::{TextRange, TextSize};

    let severity = match diag.severity {
        cache::CachedSeverity::Error => panache::linter::Severity::Error,
        cache::CachedSeverity::Warning => panache::linter::Severity::Warning,
        cache::CachedSeverity::Info => panache::linter::Severity::Info,
    };
    let origin = match diag.origin {
        cache::CachedDiagnosticOrigin::BuiltIn => panache::linter::DiagnosticOrigin::BuiltIn,
        cache::CachedDiagnosticOrigin::External => panache::linter::DiagnosticOrigin::External,
    };
    let notes = diag
        .notes
        .iter()
        .map(|note| panache::linter::DiagnosticNote {
            kind: match note.kind {
                cache::CachedDiagnosticNoteKind::Note => panache::linter::DiagnosticNoteKind::Note,
                cache::CachedDiagnosticNoteKind::Help => panache::linter::DiagnosticNoteKind::Help,
            },
            message: note.message.clone(),
        })
        .collect();
    let fix = diag.fix.as_ref().map(|fix| panache::linter::Fix {
        message: fix.message.clone(),
        edits: fix
            .edits
            .iter()
            .map(|edit| panache::linter::diagnostics::Edit {
                range: TextRange::new(TextSize::from(edit.start), TextSize::from(edit.end)),
                replacement: edit.replacement.clone(),
            })
            .collect(),
        safety: match fix.safety {
            cache::CachedFixSafety::Safe => panache::linter::FixSafety::Safe,
            cache::CachedFixSafety::Unsafe => panache::linter::FixSafety::Unsafe,
        },
    });

    panache::linter::Diagnostic {
        severity,
        location: panache::linter::Location {
            line: diag.location.line,
            column: diag.location.column,
            range: TextRange::new(
                TextSize::from(diag.location.start),
                TextSize::from(diag.location.end),
            ),
        },
        message: diag.message.clone(),
        code: diag.code.clone(),
        origin,
        notes,
        fix,
    }
}

#[cfg(test)]
mod tests {
    use super::{ColorMode, per_file_external_parallel, resolve_color};
    use std::ffi::OsStr;

    #[test]
    fn few_files_split_the_budget_to_saturate_it() {
        // 3 files sharing a budget of 8: each dispatches up to 3, so the inner
        // pools can collectively keep the budget (capped at 8) busy.
        assert_eq!(per_file_external_parallel(8, 3), 3);
        // 2 files split an even budget exactly.
        assert_eq!(per_file_external_parallel(8, 2), 4);
    }

    #[test]
    fn many_files_collapse_to_one_per_file() {
        // workers >= budget: no point dispatching more than one per file.
        assert_eq!(per_file_external_parallel(8, 8), 1);
        assert_eq!(per_file_external_parallel(8, 16), 1);
    }

    #[test]
    fn per_file_share_never_drops_below_one() {
        assert_eq!(per_file_external_parallel(1, 4), 1);
        assert_eq!(per_file_external_parallel(0, 4), 1);
        // A degenerate zero worker count must not divide by zero.
        assert_eq!(per_file_external_parallel(4, 0), 4);
    }

    #[test]
    fn auto_disables_color_when_term_is_dumb() {
        assert!(!resolve_color(
            ColorMode::Auto,
            false,
            false,
            Some(OsStr::new("dumb")),
            true,
        ));
    }

    #[test]
    fn auto_disables_color_when_term_is_unset() {
        assert!(!resolve_color(ColorMode::Auto, false, false, None, true));
    }

    #[test]
    fn auto_enables_color_on_tty_with_real_term() {
        assert!(resolve_color(
            ColorMode::Auto,
            false,
            false,
            Some(OsStr::new("xterm-256color")),
            true,
        ));
    }

    #[test]
    fn auto_disables_color_when_not_a_tty() {
        assert!(!resolve_color(
            ColorMode::Auto,
            false,
            false,
            Some(OsStr::new("xterm-256color")),
            false,
        ));
    }

    #[test]
    fn always_overrides_dumb_term() {
        assert!(resolve_color(
            ColorMode::Always,
            false,
            false,
            Some(OsStr::new("dumb")),
            false,
        ));
    }

    #[test]
    fn no_color_flag_overrides_always() {
        assert!(!resolve_color(
            ColorMode::Always,
            true,
            false,
            Some(OsStr::new("xterm-256color")),
            true,
        ));
    }

    mod format_overrides {
        use crate::apply_format_overrides;

        fn cfg() -> panache::Config {
            panache::Config::default()
        }

        #[test]
        fn extensions_dot_key_toggles_extension() {
            let mut c = cfg();
            assert!(!c.extensions.east_asian_line_breaks);
            assert!(!c.formatter_extensions.east_asian_line_breaks);
            apply_format_overrides(
                &mut c,
                &["extensions.east-asian-line-breaks=true".to_string()],
            )
            .unwrap();
            assert!(c.extensions.east_asian_line_breaks);
            assert!(c.formatter_extensions.east_asian_line_breaks);
        }

        #[test]
        fn extensions_dot_key_accepts_snake_case_alias() {
            let mut c = cfg();
            apply_format_overrides(
                &mut c,
                &["extensions.east_asian_line_breaks=true".to_string()],
            )
            .unwrap();
            assert!(c.extensions.east_asian_line_breaks);
        }

        #[test]
        fn extensions_dot_key_rejects_non_boolean_value() {
            let mut c = cfg();
            let err = apply_format_overrides(
                &mut c,
                &["extensions.east-asian-line-breaks=maybe".to_string()],
            )
            .unwrap_err();
            assert!(err.contains("expected boolean"), "{err}");
        }

        #[test]
        fn extensions_dot_key_requires_name() {
            let mut c = cfg();
            let err =
                apply_format_overrides(&mut c, &["extensions.=true".to_string()]).unwrap_err();
            assert!(err.contains("missing extension name"), "{err}");
        }

        #[test]
        fn unknown_top_level_key_still_errors() {
            let mut c = cfg();
            let err = apply_format_overrides(&mut c, &["nope=1".to_string()]).unwrap_err();
            assert!(err.contains("unknown config key"), "{err}");
        }

        #[test]
        fn table_indent_override_sets_value() {
            let mut c = cfg();
            assert_eq!(c.table_indent, 2);
            apply_format_overrides(&mut c, &["table-indent=0".to_string()]).unwrap();
            assert_eq!(c.table_indent, 0);
        }

        #[test]
        fn table_indent_override_rejects_out_of_range_value() {
            let mut c = cfg();
            let err = apply_format_overrides(&mut c, &["table-indent=4".to_string()]).unwrap_err();
            assert!(err.contains("invalid value for `table-indent`"), "{err}");
        }

        #[test]
        fn table_indent_override_rejects_non_integer_value() {
            let mut c = cfg();
            let err =
                apply_format_overrides(&mut c, &["table-indent=flush".to_string()]).unwrap_err();
            assert!(err.contains("invalid value for `table-indent`"), "{err}");
        }
    }
}
