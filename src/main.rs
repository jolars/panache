use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use clap::Parser;
use similar::{ChangeTag, TextDiff};

use panache::{format, parse};

mod cli;
use cli::{Cli, Commands};

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

fn start_dir_for(input_path: &Option<PathBuf>) -> io::Result<PathBuf> {
    if let Some(p) = input_path {
        Ok(p.parent().unwrap_or(Path::new(".")).to_path_buf())
    } else {
        std::env::current_dir()
    }
}

fn print_diff(file_path: &str, original: &str, formatted: &str) {
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

                print!("{}{}{}", style, sign, change.value());

                // Reset color at end of line if it was colored
                if change.tag() != ChangeTag::Equal {
                    print!("\x1b[0m");
                }
            }
        }
    }
}

fn main() -> io::Result<()> {
    env_logger::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Parse { file } => {
            let start_dir = start_dir_for(&file)?;
            let (cfg, cfg_path) =
                panache::config::load(cli.config.as_deref(), &start_dir, file.as_deref())?;

            if let Some(path) = &cfg_path {
                log::debug!("Using config from: {}", path.display());
            } else {
                log::debug!("Using default config");
            }

            let input = read_all(file.as_ref())?;
            let tree = parse(&input, Some(cfg));
            println!("{:#?}", tree);
            Ok(())
        }
        Commands::Format { file, check, range } => {
            let start_dir = start_dir_for(&file)?;
            let (cfg, cfg_path) =
                panache::config::load(cli.config.as_deref(), &start_dir, file.as_deref())?;

            if let Some(path) = &cfg_path {
                log::debug!("Using config from: {}", path.display());
            } else {
                log::debug!("Using default config");
            }

            // Parse range if provided
            let parsed_range = if let Some(range_str) = range {
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

            let input = read_all(file.as_ref())?;
            let output = format(&input, Some(cfg), parsed_range);

            if check {
                if input != output {
                    let file_name = file.as_ref().and_then(|p| p.to_str()).unwrap_or("<stdin>");
                    print_diff(file_name, &input, &output);
                    std::process::exit(1);
                }
                // Only print success message if there's a file (not stdin)
                if file.is_some() {
                    println!("File is correctly formatted");
                }
            } else if let Some(file_path) = &file {
                // Format in place (default for file paths)
                fs::write(file_path, &output)?;
                println!("Formatted {}", file_path.display());
            } else {
                // Stdin: output to stdout
                print!("{output}");
            }

            Ok(())
        }
        #[cfg(feature = "lsp")]
        Commands::Lsp => {
            // LSP needs tokio runtime
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(async { panache::lsp::run().await })?;
            Ok(())
        }
        Commands::Lint { file, check, fix } => {
            let start_dir = start_dir_for(&file)?;
            let (cfg, cfg_path) =
                panache::config::load(cli.config.as_deref(), &start_dir, file.as_deref())?;

            if let Some(path) = &cfg_path {
                log::debug!("Using config from: {}", path.display());
            } else {
                log::debug!("Using default config");
            }

            let input = read_all(file.as_ref())?;
            // Normalize line endings for consistent AST positions
            let normalized_input = input.replace("\r\n", "\n");
            let tree = parse(&normalized_input, Some(cfg.clone()));
            let diagnostics = panache::linter::lint(&tree, &normalized_input, &cfg);

            if diagnostics.is_empty() {
                if !check {
                    println!("No issues found");
                }
                return Ok(());
            }

            if fix {
                let fixed_output = apply_fixes(&normalized_input, &diagnostics);
                print!("{}", fixed_output);
            } else {
                print_diagnostics(&diagnostics, file.as_ref());
            }

            if check {
                std::process::exit(1);
            }

            Ok(())
        }
    }
}

fn print_diagnostics(diagnostics: &[panache::linter::Diagnostic], file: Option<&PathBuf>) {
    use panache::linter::Severity;

    let file_name = file.and_then(|p| p.to_str()).unwrap_or("<stdin>");

    for diag in diagnostics {
        let severity_str = match diag.severity {
            Severity::Error => "\x1b[31merror\x1b[0m",     // red
            Severity::Warning => "\x1b[33mwarning\x1b[0m", // yellow
            Severity::Info => "\x1b[34minfo\x1b[0m",       // blue
        };

        println!(
            "{severity_str}[{}]: {} at {}:{}:{}",
            diag.code, diag.message, file_name, diag.location.line, diag.location.column
        );

        if let Some(fix) = &diag.fix {
            println!("  \x1b[36mhelp\x1b[0m: {}", fix.message); // cyan
        }
    }

    println!("\nFound {} issue(s)", diagnostics.len());
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
