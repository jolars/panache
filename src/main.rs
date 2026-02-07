use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use panache::{format, parse};

#[derive(Parser)]
#[command(name = "panache")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "A formatter for Quarto documents")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Input file (stdin if not provided)
    #[arg(global = true)]
    file: Option<PathBuf>,

    /// Path to config file
    #[arg(long, global = true)]
    config: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Format a Quarto document (default)
    Format {
        /// Check if files are formatted without making changes
        #[arg(long)]
        check: bool,

        /// Format files in place
        #[arg(long)]
        write: bool,
    },
    /// Parse and display the AST tree for debugging
    Parse,
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

fn main() -> io::Result<()> {
    env_logger::init();

    let cli = Cli::parse();

    let start_dir = start_dir_for(&cli.file)?;
    let (cfg, cfg_path) = panache::config::load(cli.config.as_deref(), &start_dir)?;

    if let Some(path) = &cfg_path {
        log::debug!("Using config from: {}", path.display());
    } else {
        log::debug!("Using default config");
    }

    let input = read_all(cli.file.as_ref())?;

    match cli.command {
        Some(Commands::Parse) => {
            let tree = parse(&input);
            println!("{:#?}", tree);
            Ok(())
        }
        Some(Commands::Format { check, write }) => {
            let output = format(&input, Some(cfg));

            if check {
                if input != output {
                    eprintln!("File is not formatted");
                    std::process::exit(1);
                }
                println!("File is correctly formatted");
            } else if write {
                if let Some(file_path) = &cli.file {
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
        None => {
            // Default to format when no subcommand specified
            let output = format(&input, Some(cfg));
            print!("{output}");
            Ok(())
        }
    }
}
