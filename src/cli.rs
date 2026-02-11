use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "panache")]
#[command(author, version)]
#[command(about = "A formatter for Quarto, Pandoc, and Markdown documents")]
#[command(
    long_about = "Panache is a CLI formatter and LSP for Quarto (.qmd), Pandoc, and Markdown files \
    written in Rust. It understands Quarto/Pandoc-specific syntax that other formatters like \
    Prettier and mdformat struggle with, including fenced divs, tables, and math formatting."
)]
#[command(after_help = "\
EXAMPLES:

    # Format a file to stdout
    panache format document.qmd

    # Format from stdin
    cat document.qmd | panache format

    # Check if a file is formatted
    panache format --check document.qmd

    # Format in place
    panache format --write document.qmd

    # Use custom config
    panache format --config custom.toml document.qmd

    # Parse and inspect AST
    panache parse document.qmd

CONFIGURATION:

Panache looks for configuration files in this order:
  1. Explicit --config path
  2. panache.toml or .panache.toml in current/parent directories
  3. ~/.config/panache/config.toml (XDG)
  4. Built-in defaults

Example .panache.toml:

    flavor = \"quarto\"
    line_width = 80

    [extensions]
    hard_line_breaks = false
    citations = true

For more information, visit: https://github.com/jolars/panache")]
#[command(arg_required_else_help = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Path to config file
    #[arg(long, global = true)]
    #[arg(help = "Path to configuration file")]
    #[arg(
        long_help = "Path to a custom configuration file. If not specified, panache will \
        search for .panache.toml or panache.toml in the current directory and its parents, \
        then fall back to ~/.config/panache/config.toml."
    )]
    pub config: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Format a Quarto, Pandoc, or Markdown document
    #[command(
        long_about = "Format a Quarto, Pandoc, or Markdown document according to panache's \
        formatting rules. By default, outputs the formatted content to stdout. Use --write \
        to format in place or --check to verify formatting without making changes."
    )]
    #[command(after_help = "\
EXAMPLES:

    # Format to stdout
    panache format document.qmd

    # Format from stdin
    echo '# Heading' | panache format

    # Check formatting (exit code 1 if not formatted)
    panache format --check document.qmd

    # Format in place
    panache format --write document.qmd

FORMATTING RULES:

  - Default 80 character line width (configurable)
  - Wraps paragraphs while preserving inline code/math whitespace
  - Converts setext headings to ATX format
  - Preserves frontmatter and code blocks
  - Handles Quarto-specific syntax (fenced divs, math blocks)
  - Auto-formats tables for consistency
  - Formatting is idempotent (format twice = format once)")]
    Format {
        /// Input file (stdin if not provided)
        #[arg(help = "Input file path")]
        #[arg(
            long_help = "Path to the input file to format. If not provided, reads from stdin. \
            Supports .qmd, .md, .Rmd, and other Markdown-based formats."
        )]
        file: Option<PathBuf>,

        /// Check if files are formatted without making changes
        #[arg(long)]
        #[arg(help = "Check if file is formatted (exit code 1 if not)")]
        #[arg(
            long_help = "Check if the file is already formatted according to panache's rules \
            without making any changes. If the file is not formatted, displays a diff and exits \
            with code 1. If formatted, exits with code 0. Useful for CI/CD pipelines."
        )]
        check: bool,

        /// Format files in place
        #[arg(long)]
        #[arg(help = "Format the file in place")]
        #[arg(
            long_help = "Write the formatted output back to the input file, modifying it in place. \
            Cannot be used with stdin input. It's recommended to use version control before using \
            this option."
        )]
        write: bool,
    },
    /// Parse and display the AST tree for debugging
    #[command(
        long_about = "Parse a document and display its Abstract Syntax Tree (AST) for debugging \
        and understanding how panache interprets the document structure. The AST shows all block \
        and inline elements detected by the parser."
    )]
    #[command(after_help = "\
EXAMPLES:

    # Parse a file and show AST
    panache parse document.qmd

    # Parse from stdin
    echo '# Heading' | panache parse

    # Parse with custom config (affects extension parsing)
    panache parse --config .panache.toml document.qmd

The AST output shows the concrete syntax tree built by the parser, including:
  - Block elements (headings, paragraphs, code blocks, lists, tables)
  - Inline elements (emphasis, code, math, links, footnotes)
  - Container blocks (blockquotes, fenced divs)
  - Metadata (YAML/TOML frontmatter)")]
    Parse {
        /// Input file (stdin if not provided)
        #[arg(help = "Input file path")]
        #[arg(
            long_help = "Path to the input file to parse. If not provided, reads from stdin. \
            The parser respects extension flags from the configuration file."
        )]
        file: Option<PathBuf>,
    },
    /// Start the Language Server Protocol server
    #[command(
        long_about = "Start the panache Language Server Protocol (LSP) server for editor \
        integration. The LSP server provides formatting capabilities to editors like VS Code, \
        Neovim, and others that support LSP."
    )]
    #[command(after_help = "\
The LSP server communicates via stdin/stdout and is typically launched automatically by your \
editor's LSP client. You generally don't need to run this command manually.

For editor configuration examples, see: https://github.com/jolars/panache#editor-integration")]
    Lsp,
}
