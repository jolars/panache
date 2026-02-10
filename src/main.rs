use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use clap::Parser;
use similar::{ChangeTag, TextDiff};

use panache::{format, parse};

mod cli;
use cli::{Cli, Commands};

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

#[tokio::main]
async fn main() -> io::Result<()> {
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
        Commands::Format { file, check, write } => {
            let start_dir = start_dir_for(&file)?;
            let (cfg, cfg_path) =
                panache::config::load(cli.config.as_deref(), &start_dir, file.as_deref())?;

            if let Some(path) = &cfg_path {
                log::debug!("Using config from: {}", path.display());
            } else {
                log::debug!("Using default config");
            }

            let input = read_all(file.as_ref())?;
            let output = format(&input, Some(cfg)).await;

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
            } else if write {
                if let Some(file_path) = &file {
                    fs::write(file_path, &output)?;
                    println!("Formatted {}", file_path.display());
                } else {
                    eprintln!("Cannot use --write with stdin input");
                    std::process::exit(1);
                }
            } else {
                print!("{output}");
            }

            Ok(())
        }
        Commands::Lsp => {
            panache::lsp::run().await?;
            Ok(())
        }
    }
}
