use std::fs;
use std::io::{self, IsTerminal, Read};
use std::path::{Path, PathBuf};

use clap::Parser;
use similar::{ChangeTag, TextDiff};

use panache::{format, parse};
use serde_json::json;

mod cache;
mod cli;
mod diagnostic_renderer;
use cache::{CachedLintDocument, CliCache, FormatCacheMode, FormatStoreArgs};
use cli::{Cli, ColorMode, Commands, DebugChecks, DebugCommands, TranslateProviderArg};
use diagnostic_renderer::print_diagnostics;

/// Supported file extensions for formatting
const SUPPORTED_EXTENSIONS: &[&str] = &["md", "qmd", "Rmd", "markdown", "mdown", "mkd"];

fn init_logger(debug_log: Option<&Path>) {
    if let Some(path) = debug_log {
        let mut builder = env_logger::Builder::new();
        if let Ok(file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            builder.target(env_logger::Target::Pipe(Box::new(file)));
        }
        builder.filter_level(log::LevelFilter::Info);
        builder.filter_module("panache::lsp", log::LevelFilter::Debug);
        builder.filter_module("panache::includes", log::LevelFilter::Debug);
        builder.format_timestamp_millis();
        builder.init();
        log::info!("LSP debug logging enabled at {}", path.display());
        return;
    }
    env_logger::Builder::from_default_env().init();
}

fn init_lsp_debug_log() -> io::Result<PathBuf> {
    let mut base = dirs::state_dir().unwrap_or_else(|| PathBuf::from("."));
    base.push("panache");
    fs::create_dir_all(&base)?;
    base.push("lsp-debug.log");
    Ok(base)
}

struct PathFilters {
    exclude: ignore::gitignore::Gitignore,
    include: ignore::gitignore::Gitignore,
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

fn build_path_filters(root: &Path, cfg: &panache::Config) -> io::Result<PathFilters> {
    let mut exclude_builder = ignore::gitignore::GitignoreBuilder::new(root);
    for pattern in effective_exclude_patterns(cfg) {
        exclude_builder
            .add_line(None, &pattern)
            .map_err(io::Error::other)?;
    }
    let exclude = exclude_builder.build().map_err(io::Error::other)?;

    let mut include_builder = ignore::gitignore::GitignoreBuilder::new(root);
    for pattern in effective_include_patterns(cfg) {
        include_builder
            .add_line(None, &pattern)
            .map_err(io::Error::other)?;
    }
    let include = include_builder.build().map_err(io::Error::other)?;

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
    filter_root: &Path,
    force_exclude: bool,
) -> io::Result<Vec<PathBuf>> {
    use ignore::WalkBuilder;

    let mut files = Vec::new();

    for path in paths {
        let matcher_root = if relative_path_from_root(path, filter_root).is_some() {
            filter_root
        } else if path.is_dir() {
            path.as_path()
        } else {
            path.parent().unwrap_or(filter_root)
        };
        let filters = build_path_filters(matcher_root, cfg)?;

        if path.is_file() {
            let rel_path = relative_path_from_root(path, matcher_root)
                .or_else(|| path.file_name().map(PathBuf::from))
                .unwrap_or_else(|| path.to_path_buf());
            if force_exclude
                && filters
                    .exclude
                    .matched_path_or_any_parents(&rel_path, false)
                    .is_ignore()
            {
                continue;
            }
            // Check if file has a supported extension
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
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
                let rel_path = relative_path_from_root(entry_path, matcher_root)
                    .unwrap_or_else(|| entry_path.to_path_buf());
                if entry_path.is_dir() {
                    continue;
                }
                if filters
                    .exclude
                    .matched_path_or_any_parents(&rel_path, false)
                    .is_ignore()
                {
                    continue;
                }
                if !filters.include.matched(&rel_path, false).is_ignore() {
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

fn file_count_label(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("{count} {singular}")
    } else {
        format!("{count} {plural}")
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

fn path_matching_root(
    explicit_config: Option<&Path>,
    discovered_config: Option<&Path>,
    traversal_start_dir: &Path,
) -> io::Result<PathBuf> {
    let cwd = std::env::current_dir()?;

    if explicit_config.is_some() {
        return Ok(cwd);
    }

    if let Some(config_path) = discovered_config
        && let Some(parent) = config_path.parent()
        && relative_path_from_root(traversal_start_dir, parent).is_some()
    {
        return Ok(parent.to_path_buf());
    }

    Ok(cwd)
}

fn load_config_for_cli(
    config_path: Option<&Path>,
    isolated: bool,
    cli_cache_dir: Option<&Path>,
    start_dir: &Path,
    input_path: Option<&Path>,
) -> io::Result<(panache::Config, Option<PathBuf>)> {
    let mut loaded = if !isolated {
        panache::config::load(config_path, start_dir, input_path)?
    } else {
        let mut cfg = panache::Config::default();
        if let Some(input_path) = input_path
            && let Some(ext) = input_path.extension().and_then(|e| e.to_str())
        {
            let detected_flavor = match ext.to_lowercase().as_str() {
                "qmd" => Some(panache::config::Flavor::Quarto),
                "rmd" => Some(panache::config::Flavor::RMarkdown),
                "md" => Some(cfg.flavor),
                _ => None,
            };

            if let Some(flavor) = detected_flavor {
                cfg.flavor = flavor;
                cfg.extensions = panache::config::Extensions::for_flavor(flavor);
            }
        }
        (cfg, None)
    };

    if let Some(cache_dir) = cli_cache_dir {
        loaded.0.cache_dir = Some(cache_dir.to_string_lossy().to_string());
    }

    Ok(loaded)
}

fn color_enabled(mode: ColorMode, no_color: bool) -> bool {
    if no_color {
        return false;
    }
    match mode {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => {
            if std::env::var_os("NO_COLOR").is_some() {
                return false;
            }
            io::stdout().is_terminal()
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

struct DebugFailure {
    kind: CheckKind,
    left: String,
    right: String,
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
) -> DebugRunArtifacts {
    let mut artifacts = DebugRunArtifacts::default();

    if matches!(checks, DebugChecks::Losslessness | DebugChecks::All) {
        let tree_text = parse(input, Some(cfg.clone())).text().to_string();
        artifacts.losslessness = Some((input.to_string(), tree_text.clone()));
        if input != tree_text {
            artifacts.failures.push(DebugFailure {
                kind: CheckKind::Losslessness,
                left: input.to_string(),
                right: tree_text,
            });
        }
    }

    if matches!(checks, DebugChecks::Idempotency | DebugChecks::All) {
        let once = format(input, Some(cfg.clone()), None);
        let twice = format(&once, Some(cfg.clone()), None);
        artifacts.idempotency = Some((input.to_string(), once.clone(), twice.clone()));
        if once != twice {
            artifacts.failures.push(DebugFailure {
                kind: CheckKind::Idempotency,
                left: once,
                right: twice,
            });
        }
    }

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
            json,
            quiet,
            verify,
        } => {
            if verify {
                eprintln!(
                    "Warning: `panache parse --verify` is deprecated; use `panache debug format --checks losslessness`."
                );
            }
            let input_path = file.as_deref().or(cli.stdin_filename.as_deref());
            let start_dir = start_dir_for(input_path)?;
            let (cfg, cfg_path) = load_config_for_cli(
                cli.config.as_deref(),
                cli.isolated,
                cli.cache_dir.as_deref(),
                &start_dir,
                input_path,
            )?;

            if let Some(path) = &cfg_path {
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
            } else if !quiet {
                println!("{:#?}", tree);
            }
            Ok(())
        }
        Commands::Format {
            files,
            check,
            range,
            verify,
            force_exclude,
        } => {
            if verify {
                eprintln!(
                    "Warning: `panache format --verify` is deprecated; use `panache debug format --checks all`."
                );
            }
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
                let (cfg, cfg_path) = load_config_for_cli(
                    cli.config.as_deref(),
                    cli.isolated,
                    cli.cache_dir.as_deref(),
                    &start_dir,
                    cli.stdin_filename.as_deref(),
                )?;

                if let Some(path) = &cfg_path {
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
            let (traversal_cfg, traversal_cfg_path) = load_config_for_cli(
                cli.config.as_deref(),
                cli.isolated,
                cli.cache_dir.as_deref(),
                &traversal_start_dir,
                traversal_anchor,
            )?;
            let matching_root = path_matching_root(
                cli.config.as_deref(),
                traversal_cfg_path.as_deref(),
                &traversal_start_dir,
            )?;
            let expanded_files =
                expand_paths(&files, &traversal_cfg, &matching_root, force_exclude)?;
            let mut cache = if cli.no_cache {
                None
            } else {
                CliCache::open(&traversal_cfg, cli.config.as_deref(), &traversal_start_dir)?
            };

            if expanded_files.is_empty() {
                if force_exclude {
                    return Ok(());
                }
                if has_explicit_file_targets(&files) {
                    eprintln!("Error: No supported files found");
                    std::process::exit(1);
                }
                println!("No supported files found");
                return Ok(());
            }

            // Handle file(s) case
            let mut all_formatted = true;
            let mut reformatted_count = 0usize;
            let mut unchanged_count = 0usize;

            for file_path in &expanded_files {
                let start_dir = file_path.parent().unwrap_or(Path::new(".")).to_path_buf();
                let (cfg, cfg_path) = load_config_for_cli(
                    cli.config.as_deref(),
                    cli.isolated,
                    cli.cache_dir.as_deref(),
                    &start_dir,
                    Some(file_path),
                )?;

                if let Some(path) = &cfg_path {
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
                let file_fingerprint = CliCache::file_fingerprint(&input);
                let config_fingerprint = CliCache::config_fingerprint(&cfg);
                let tool_fingerprint = CliCache::tool_fingerprint();
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
                    if let Some(cache_hit) = cache
                        .as_ref()
                        .filter(|cache| cache.supports_format_mode(&cfg, mode))
                        .and_then(|cache| {
                            cache.get_format(
                                file_path,
                                mode,
                                &file_fingerprint,
                                &config_fingerprint,
                                &tool_fingerprint,
                            )
                        })
                    {
                        cache_hit.1
                    } else {
                        let output = format(&input, Some(cfg.clone()), parsed_range);
                        if let Some(cache_ref) = cache
                            .as_mut()
                            .filter(|cache| cache.supports_format_mode(&cfg, mode))
                        {
                            let unchanged = input == output;
                            cache_ref.put_format(
                                file_path,
                                mode,
                                FormatStoreArgs {
                                    file_fingerprint: file_fingerprint.clone(),
                                    config_fingerprint: config_fingerprint.clone(),
                                    tool_fingerprint: tool_fingerprint.clone(),
                                    unchanged,
                                    output: output.clone(),
                                },
                            );
                        }
                        output
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

                if check {
                    if input != output {
                        let file_name = file_path.to_str().unwrap_or("<unknown>");
                        print_diff(file_name, &input, &output, use_color);
                        all_formatted = false;
                    } else if expanded_files.len() == 1 {
                        // Only print success for single file
                        println!("{} is correctly formatted", file_path.display());
                    }
                } else if !verify {
                    if input != output {
                        // Format in place (default for file paths)
                        fs::write(file_path, &output)?;
                        println!("Formatted {}", file_path.display());
                        reformatted_count += 1;
                    } else {
                        unchanged_count += 1;
                    }
                }
            }

            if check {
                if all_formatted {
                    if expanded_files.len() > 1 {
                        println!("All {} files are correctly formatted", expanded_files.len());
                    }
                } else {
                    std::process::exit(1);
                }
            } else if !verify {
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
        Commands::Debug { command } => match command {
            DebugCommands::Format {
                files,
                checks,
                json,
                dump_dir,
                dump_passes,
                force_exclude,
            } => {
                if dump_passes && dump_dir.is_none() {
                    eprintln!("Error: --dump-passes requires --dump-dir <DIR>");
                    std::process::exit(1);
                }

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
                    let (traversal_cfg, traversal_cfg_path) = load_config_for_cli(
                        cli.config.as_deref(),
                        cli.isolated,
                        cli.cache_dir.as_deref(),
                        &traversal_start_dir,
                        traversal_anchor,
                    )?;
                    let matching_root = path_matching_root(
                        cli.config.as_deref(),
                        traversal_cfg_path.as_deref(),
                        &traversal_start_dir,
                    )?;
                    expand_paths(&files, &traversal_cfg, &matching_root, force_exclude)?
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
                    } else {
                        println!("No supported files found");
                    }
                    return Ok(());
                }

                let mut files_checked = 0usize;
                let mut failure_count = 0usize;
                let mut json_failures = Vec::new();

                if use_stdin {
                    let start_dir = start_dir_for(cli.stdin_filename.as_deref())?;
                    let (cfg, _) = load_config_for_cli(
                        cli.config.as_deref(),
                        cli.isolated,
                        cli.cache_dir.as_deref(),
                        &start_dir,
                        cli.stdin_filename.as_deref(),
                    )?;
                    let input = read_all(None)?;
                    files_checked += 1;

                    let artifacts = run_debug_checks_for_content(&input, &cfg, checks);
                    if let Some(dir) = dump_dir.as_ref() {
                        write_debug_artifacts(dir, "stdin", &artifacts, dump_passes)?;
                    }

                    for failure in &artifacts.failures {
                        failure_count += 1;
                        if !json {
                            eprintln!("Debug check failed ({}) in <stdin>", failure.kind.label());
                            print_diff("<stdin>", &failure.left, &failure.right, use_color);
                        }
                        json_failures.push(json!({
                            "file": "<stdin>",
                            "kind": failure.kind.label(),
                        }));
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
                        )?;
                        let input = fs::read_to_string(file_path)?;
                        files_checked += 1;
                        let file_label = file_path.to_str().unwrap_or("<unknown>");

                        let artifacts = run_debug_checks_for_content(&input, &cfg, checks);
                        if let Some(dir) = dump_dir.as_ref() {
                            let safe = sanitize_path_for_filename(file_label);
                            write_debug_artifacts(dir, &safe, &artifacts, dump_passes)?;
                        }

                        for failure in &artifacts.failures {
                            failure_count += 1;
                            if !json {
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
                } else if failure_count == 0 {
                    println!(
                        "All checks passed (checks: {}, files: {})",
                        format!("{:?}", checks).to_lowercase(),
                        files_checked
                    );
                }

                if dump_passes
                    && !json
                    && let Some(dir) = dump_dir.as_ref()
                {
                    eprintln!("Wrote debug artifacts to {}", dir.display());
                }

                if failure_count > 0 && !json && dump_dir.is_none() {
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
            // LSP needs tokio runtime
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(async { panache::lsp::run().await })?;
            Ok(())
        }
        Commands::Lint {
            files,
            check,
            fix,
            message_format,
            force_exclude,
        } => {
            // Handle stdin case
            if files.is_empty() {
                let start_dir = start_dir_for(cli.stdin_filename.as_deref())?;
                let (cfg, cfg_path) = load_config_for_cli(
                    cli.config.as_deref(),
                    cli.isolated,
                    cli.cache_dir.as_deref(),
                    &start_dir,
                    cli.stdin_filename.as_deref(),
                )?;

                if let Some(path) = &cfg_path {
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
                    panache::salsa::FileText::new(&db, input.clone()),
                    panache::salsa::FileConfig::new(&db, cfg.clone()),
                    stdin_path.to_path_buf(),
                )
                .diagnostics
                .iter()
                .filter(|d| d.code == "yaml-parse-error")
                .cloned()
                .collect::<Vec<_>>();
                merge_missing_diagnostics(&mut diagnostics, yaml_diags);

                if diagnostics.is_empty() {
                    if !check {
                        println!("No issues found");
                    }
                    return Ok(());
                }

                if fix {
                    let fixed_output = apply_fixes(&input, &diagnostics);
                    print!("{}", fixed_output);
                } else {
                    print_diagnostics(&diagnostics, None, Some(&input), use_color, message_format);
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
            let (traversal_cfg, traversal_cfg_path) = load_config_for_cli(
                cli.config.as_deref(),
                cli.isolated,
                cli.cache_dir.as_deref(),
                &traversal_start_dir,
                traversal_anchor,
            )?;
            let matching_root = path_matching_root(
                cli.config.as_deref(),
                traversal_cfg_path.as_deref(),
                &traversal_start_dir,
            )?;
            let expanded_files =
                expand_paths(&files, &traversal_cfg, &matching_root, force_exclude)?;
            let mut cache = if cli.no_cache {
                None
            } else {
                CliCache::open(&traversal_cfg, cli.config.as_deref(), &traversal_start_dir)?
            };

            if expanded_files.is_empty() {
                if force_exclude {
                    return Ok(());
                }
                if has_explicit_file_targets(&files) {
                    eprintln!("Error: No supported files found");
                    std::process::exit(1);
                }
                println!("No supported files found");
                return Ok(());
            }

            // Lint files
            let mut any_issues = false;
            let mut total_issues = 0;

            for file_path in &expanded_files {
                let start_dir = file_path.parent().unwrap_or(Path::new(".")).to_path_buf();
                let (cfg, cfg_path) = load_config_for_cli(
                    cli.config.as_deref(),
                    cli.isolated,
                    cli.cache_dir.as_deref(),
                    &start_dir,
                    Some(file_path),
                )?;

                if let Some(path) = &cfg_path {
                    log::debug!("Using config from: {}", path.display());
                } else {
                    log::debug!("Using default config");
                }

                let root_input = fs::read_to_string(file_path)?;
                let file_fingerprint = CliCache::file_fingerprint(&root_input);
                let config_fingerprint = CliCache::config_fingerprint(&cfg);
                let tool_fingerprint = CliCache::tool_fingerprint();
                let documents = if let Some(cached_documents) = cache
                    .as_ref()
                    .filter(|cache| cache.supports_lint(&cfg))
                    .and_then(|cache| {
                        cache.get_lint(
                            file_path,
                            &file_fingerprint,
                            &config_fingerprint,
                            &tool_fingerprint,
                        )
                    })
                    .filter(|docs| cached_lint_documents_are_fresh(docs))
                {
                    cached_documents
                        .iter()
                        .map(linted_document_from_cached)
                        .collect::<Vec<_>>()
                } else {
                    let documents = lint_documents_with_includes(file_path, &root_input, &cfg)?;
                    if let Some(cache_ref) =
                        cache.as_mut().filter(|cache| cache.supports_lint(&cfg))
                    {
                        let cached_docs = documents
                            .iter()
                            .map(cached_lint_document_from_linted)
                            .collect::<Vec<_>>();
                        cache_ref.put_lint(
                            file_path,
                            file_fingerprint,
                            config_fingerprint,
                            tool_fingerprint,
                            cached_docs,
                        );
                    }
                    documents
                };
                let mut root_doc = documents.iter().find(|doc| &doc.path == file_path).cloned();
                let mut included_docs: Vec<LintedDocument> = documents
                    .into_iter()
                    .filter(|doc| &doc.path != file_path)
                    .collect();
                included_docs.sort_by(|a, b| a.path.cmp(&b.path));

                let Some(root_doc) = root_doc.take() else {
                    continue;
                };

                if !root_doc.diagnostics.is_empty() {
                    any_issues = true;
                    total_issues += root_doc.diagnostics.len();

                    if fix {
                        let fixed_output = apply_fixes(&root_doc.input, &root_doc.diagnostics);
                        fs::write(file_path, fixed_output)?;
                        println!(
                            "Fixed {} issue(s) in {}",
                            root_doc.diagnostics.len(),
                            file_path.display()
                        );
                    } else {
                        print_diagnostics(
                            &root_doc.diagnostics,
                            Some(file_path.as_path()),
                            Some(&root_doc.input),
                            use_color,
                            message_format,
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
                        print_diagnostics(
                            &doc.diagnostics,
                            Some(doc.path.as_path()),
                            Some(&doc.input),
                            use_color,
                            message_format,
                        );
                    }
                }
            }
            if let Some(cache_ref) = cache.as_mut() {
                cache_ref.save_if_dirty()?;
            }

            if !any_issues && !check {
                println!("No issues found in {} file(s)", expanded_files.len());
            }

            if check && any_issues {
                eprintln!(
                    "\nFound {} issue(s) across {} file(s)",
                    total_issues,
                    expanded_files.len()
                );
                std::process::exit(1);
            }

            Ok(())
        }
        Commands::Translate {
            files,
            provider,
            source_lang,
            target_lang,
            api_key,
            endpoint,
            stdout,
            force_exclude,
        } => {
            let provider = provider.map(|p| match p {
                TranslateProviderArg::Deepl => panache::config::TranslateProvider::Deepl,
                TranslateProviderArg::Libretranslate => {
                    panache::config::TranslateProvider::Libretranslate
                }
            });

            let overrides = panache::translate::TranslateOverrides {
                provider,
                source_lang,
                target_lang,
                api_key,
                endpoint,
            };

            if files.is_empty() {
                let start_dir = start_dir_for(cli.stdin_filename.as_deref())?;
                let (cfg, cfg_path) = load_config_for_cli(
                    cli.config.as_deref(),
                    cli.isolated,
                    cli.cache_dir.as_deref(),
                    &start_dir,
                    cli.stdin_filename.as_deref(),
                )?;
                if let Some(path) = &cfg_path {
                    log::debug!("Using config from: {}", path.display());
                } else {
                    log::debug!("Using default config");
                }
                let input = read_all(None)?;
                let output = panache::translate::translate_document(&input, &cfg, &overrides)
                    .map_err(|e| io::Error::other(e.to_string()))?;
                print!("{output}");
                return Ok(());
            }

            if files.len() == 2 && files[0].is_file() && !files[1].is_dir() {
                let input_path = &files[0];
                let output_path = &files[1];
                let start_dir = input_path.parent().unwrap_or(Path::new(".")).to_path_buf();
                let (cfg, cfg_path) = load_config_for_cli(
                    cli.config.as_deref(),
                    cli.isolated,
                    cli.cache_dir.as_deref(),
                    &start_dir,
                    Some(input_path),
                )?;
                if let Some(path) = &cfg_path {
                    log::debug!("Using config from: {}", path.display());
                } else {
                    log::debug!("Using default config");
                }
                let input = fs::read_to_string(input_path)?;
                let output = panache::translate::translate_document(&input, &cfg, &overrides)
                    .map_err(|e| io::Error::other(e.to_string()))?;
                fs::write(output_path, output)?;
                println!(
                    "Translated {} -> {}",
                    input_path.display(),
                    output_path.display()
                );
                return Ok(());
            }

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
            let (traversal_cfg, traversal_cfg_path) = load_config_for_cli(
                cli.config.as_deref(),
                cli.isolated,
                cli.cache_dir.as_deref(),
                &traversal_start_dir,
                traversal_anchor,
            )?;
            let matching_root = path_matching_root(
                cli.config.as_deref(),
                traversal_cfg_path.as_deref(),
                &traversal_start_dir,
            )?;
            let expanded_files =
                expand_paths(&files, &traversal_cfg, &matching_root, force_exclude)?;

            if expanded_files.is_empty() {
                if force_exclude {
                    return Ok(());
                }
                if has_explicit_file_targets(&files) {
                    eprintln!("Error: No supported files found");
                    std::process::exit(1);
                }
                println!("No supported files found");
                return Ok(());
            }

            for file_path in &expanded_files {
                let start_dir = file_path.parent().unwrap_or(Path::new(".")).to_path_buf();
                let (cfg, cfg_path) = load_config_for_cli(
                    cli.config.as_deref(),
                    cli.isolated,
                    cli.cache_dir.as_deref(),
                    &start_dir,
                    Some(file_path),
                )?;

                if let Some(path) = &cfg_path {
                    log::debug!("Using config from: {}", path.display());
                } else {
                    log::debug!("Using default config");
                }

                let input = fs::read_to_string(file_path)?;
                let output = panache::translate::translate_document(&input, &cfg, &overrides)
                    .map_err(|e| io::Error::other(e.to_string()))?;

                if stdout {
                    print!("{output}");
                } else {
                    fs::write(file_path, &output)?;
                    println!("Translated {}", file_path.display());
                }
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

fn lint_documents_with_includes(
    root_path: &PathBuf,
    root_input: &str,
    cfg: &panache::Config,
) -> io::Result<Vec<LintedDocument>> {
    use std::collections::HashSet;

    let mut results = Vec::new();
    let mut visited = HashSet::new();
    let mut active = HashSet::new();
    let db = panache::salsa::SalsaDb::default();
    let graph = {
        let file = panache::salsa::FileText::new(&db, root_input.to_string());
        let config = panache::salsa::FileConfig::new(&db, cfg.clone());
        panache::salsa::project_graph(&db, file, config, root_path.clone()).clone()
    };
    lint_loaded_document_with_includes(
        root_path,
        root_input,
        cfg,
        &mut results,
        &mut visited,
        &mut active,
        &graph,
        &db,
    )?;
    Ok(results)
}

#[allow(clippy::too_many_arguments, clippy::only_used_in_recursion)]
fn lint_loaded_document_with_includes(
    doc_path: &PathBuf,
    input: &str,
    cfg: &panache::Config,
    results: &mut Vec<LintedDocument>,
    visited: &mut std::collections::HashSet<PathBuf>,
    active: &mut std::collections::HashSet<PathBuf>,
    graph: &panache::salsa::ProjectGraph,
    db: &panache::salsa::SalsaDb,
) -> io::Result<()> {
    if !visited.insert(doc_path.clone()) {
        return Ok(());
    }

    active.insert(doc_path.clone());

    let tree = parse(input, Some(cfg.clone()));
    let metadata = panache::metadata::extract_project_metadata(&tree, doc_path).ok();
    let mut diagnostics =
        panache::linter::lint_with_external_sync_and_metadata(&tree, input, cfg, metadata.as_ref());
    let yaml_diags = panache::salsa::built_in_lint_plan(
        db,
        panache::salsa::FileText::new(db, input.to_string()),
        panache::salsa::FileConfig::new(db, cfg.clone()),
        doc_path.clone(),
    )
    .diagnostics
    .iter()
    .filter(|d| d.code == "yaml-parse-error")
    .cloned()
    .collect::<Vec<_>>();
    merge_missing_diagnostics(&mut diagnostics, yaml_diags);

    let base_dir = doc_path.parent().unwrap_or(Path::new("."));
    let project_root = panache::includes::find_quarto_root(doc_path);
    let resolution =
        panache::includes::collect_includes(&tree, input, base_dir, project_root.as_deref(), cfg);

    diagnostics.extend(resolution.diagnostics);
    let graph_diags = panache::salsa::project_graph::accumulated::<panache::salsa::GraphDiagnostic>(
        db,
        panache::salsa::FileText::new(db, input.to_string()),
        panache::salsa::FileConfig::new(db, cfg.clone()),
        doc_path.clone(),
    );
    for entry in graph_diags {
        if entry.0.path == *doc_path {
            diagnostics.push(entry.0.diagnostic.clone());
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
                    cfg,
                    results,
                    visited,
                    active,
                    graph,
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

fn apply_fixes(input: &str, diagnostics: &[panache::linter::Diagnostic]) -> String {
    use panache::linter::diagnostics::Edit;

    let mut edits: Vec<&Edit> = diagnostics
        .iter()
        .filter_map(|d| d.fix.as_ref())
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
