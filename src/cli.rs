use clap::{Parser, Subcommand, ValueEnum};
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

    # Format a file in place
    panache format document.qmd

    # Format from stdin to stdout
    cat document.qmd | panache format

    # Check if a file is formatted
    panache format --check document.qmd

    # Use custom config
    panache format --config custom.toml document.qmd

    # Parse and inspect CST
    panache parse document.qmd

    # Parse and verify losslessness (input == CST text)
    panache parse --verify document.qmd

CONFIGURATION:

Panache looks for configuration files in this order:
  1. Explicit --config path
  2. panache.toml or .panache.toml in current/parent directories
  3. ~/.config/panache/config.toml (XDG)
  4. Built-in defaults

Example .panache.toml:

    flavor = \"quarto\"
    line-width = 80

    [extensions]
    hard-line-breaks = false
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
        formatting rules. By default, formats files in place. Use --check to verify formatting \
        without making changes. With --verify, panache runs parser/formatter invariants without \
        writing changes to disk. Stdin input always outputs to stdout."
    )]
    #[command(after_help = "\
EXAMPLES:

    # Format file in place (default)
    panache format document.qmd

    # Format multiple files
    panache format file1.md file2.md file3.qmd

    # Use glob patterns (expanded by shell)
    panache format **/*.{md,qmd}

    # Format entire directory recursively, all supported files
    panache format .
    panache format docs/

    # Format from stdin to stdout
    echo '# Heading' | panache format

    # Check formatting (exit code 1 if not formatted)
    panache format --check document.qmd

    # Verify parser/formatter invariants (does not write changes)
    panache format --verify document.qmd")]
    Format {
        /// Input file(s) (stdin if not provided)
        #[arg(help = "Input file path(s) or directories")]
        #[arg(
            long_help = "Path(s) to the input file(s) or directories to format. If not provided, reads from stdin. \
            Supports .qmd, .md, .Rmd, and other Markdown-based formats. When file paths are \
            provided, the files are formatted in place by default. Stdin input always outputs \
            to stdout. Supports glob patterns (e.g., *.md) and directories (e.g., . or docs/). \
            Directories are traversed recursively, respecting .gitignore files."
        )]
        files: Vec<PathBuf>,

        /// Check if files are formatted without making changes
        #[arg(long)]
        #[arg(help = "Check if file is formatted (exit code 1 if not)")]
        #[arg(
            long_help = "Check if the file is already formatted according to panache's rules \
            without making any changes. If the file is not formatted, displays a diff and exits \
            with code 1. If formatted, exits with code 0. Useful for CI/CD pipelines."
        )]
        check: bool,

        /// Format only a specific line range (1-indexed, inclusive)
        #[arg(long, value_name = "START:END")]
        #[arg(help = "Format only lines START:END (e.g., --range 5:10) [Experimental]")]
        #[arg(
            long_help = "Format only the specified line range. Lines are 1-indexed and inclusive. \
            The range will be expanded to complete block boundaries to ensure well-formed output. \
            For example, if you select part of a list, the entire list will be formatted. \
            Format: --range START:END (e.g., --range 5:10 formats lines 5 through 10). \
            \n\nNote: This feature is experimental. Range filtering may not work correctly in all cases."
        )]
        range: Option<String>,

        /// Verify parser losslessness and formatter idempotency
        #[arg(long)]
        #[arg(help = "[Deprecated] Verify losslessness and idempotency invariants")]
        #[arg(long_help = "Run smoke-check invariants: \
            (1) parser losslessness (input == parsed CST text) and \
            (2) formatter idempotency (format(format(x)) == format(x)). \
            When formatting files by path (including directories), --verify does not write any changes \
            to disk. Exits with code 1 when verification fails. \
            \n\nDeprecated: prefer `panache debug format --checks all`.")]
        verify: bool,
    },
    /// Parse and display the CST tree for debugging
    #[command(
        long_about = "Parse a document and display its Concrete Syntax Tree (CST) for debugging \
        and understanding how panache interprets the document structure. The CST shows all block \
        and inline elements detected by the parser."
    )]
    #[command(after_help = "\
EXAMPLES:

    # Parse a file and show CST
    panache parse document.qmd

    # Parse a file and write CST JSON to disk
    panache parse --json cst.json document.qmd

    # Parse from stdin
    echo '# Heading' | panache parse

    # Parse without printing CST output
    panache parse --quiet document.qmd

    # Parse with custom config (affects extension parsing)
    panache parse --config .panache.toml document.qmd

    # Verify parser losslessness while parsing
    panache parse --verify document.qmd")]
    Parse {
        /// Input file (stdin if not provided)
        #[arg(help = "Input file path")]
        #[arg(
            long_help = "Path to the input file to parse. If not provided, reads from stdin. \
            The parser respects extension flags from the configuration file."
        )]
        file: Option<PathBuf>,

        /// Write CST JSON output to the given file
        #[arg(long, value_name = "PATH")]
        #[arg(help = "Write CST JSON output to PATH")]
        #[arg(
            long_help = "Write the parsed CST to the given JSON file instead of printing the debug tree. \
            The output includes node kinds, text ranges, and token text."
        )]
        json: Option<PathBuf>,

        /// Suppress CST output to stdout
        #[arg(long)]
        #[arg(help = "Do not print CST output")]
        #[arg(
            long_help = "Suppress CST output to stdout. Useful with --verify for smoke-testing \
            large files without terminal spam. Verification failures still print diagnostics and exit non-zero."
        )]
        quiet: bool,

        /// Verify parser losslessness (input must equal CST text)
        #[arg(long)]
        #[arg(help = "[Deprecated] Verify parser losslessness invariant")]
        #[arg(
            long_help = "Run parser losslessness verification (input == parsed CST text). \
            Exits with code 1 when verification fails. \
            \n\nDeprecated: prefer `panache debug format --checks losslessness`."
        )]
        verify: bool,
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
    Lsp {
        /// Enable debug logging to ~/.local/state/panache/lsp-debug.log
        #[arg(long)]
        #[arg(help = "Enable LSP debug logging to ~/.local/state/panache/lsp-debug.log")]
        #[arg(
            long_help = "Enable verbose LSP debug logging to ~/.local/state/panache/lsp-debug.log \
            (or $XDG_STATE_HOME/panache/lsp-debug.log when XDG_STATE_HOME is set). \
            Logs are written to file to avoid interfering with the LSP protocol over stdout."
        )]
        debug: bool,
    },
    /// Lint a Quarto, Pandoc, or Markdown document
    #[command(
        long_about = "Lint a document to check for correctness issues and best practice \
        violations. Unlike the formatter which handles style, the linter catches semantic \
        problems like syntax errors, heading hierarchy issues, and broken references."
    )]
    #[command(after_help = "\
EXAMPLES:

    # Lint a file and show diagnostics
    panache lint document.qmd

    # Lint multiple files
    panache lint file1.md file2.qmd

    # Lint entire directory
    panache lint .

    # Lint from stdin
    echo '# H1\\n### H3' | panache lint

    # Check mode for CI (exit code 1 if violations found)
    panache lint --check document.qmd

    # Apply auto-fixes
    panache lint --fix document.qmd

LINT RULES:

  - Parser errors: Syntax errors detected during parsing
  - Heading hierarchy: Warns on skipped heading levels (e.g., h1 → h3)
  
Configure rules in .panache.toml with [lint] section.")]
    Lint {
        /// Input file(s) or directories (stdin if not provided)
        #[arg(help = "Input file path(s) or directories")]
        #[arg(
            long_help = "Path(s) to the input file(s) or directories to check. If not provided, reads from stdin. \
            Supports .qmd, .md, .Rmd, and other Markdown-based formats. Supports glob patterns \
            (e.g., *.md) and directories (e.g., . or docs/). Directories are traversed recursively, \
            respecting .gitignore files."
        )]
        files: Vec<PathBuf>,

        /// Check mode: exit with code 1 if violations found
        #[arg(long)]
        #[arg(help = "Exit with code 1 if violations found (CI mode)")]
        check: bool,

        /// Apply auto-fixes
        #[arg(long)]
        #[arg(help = "Automatically fix violations where possible")]
        fix: bool,
    },
    /// Debug utilities for parser/formatter diagnostics
    #[command(
        long_about = "Debugging utilities for parse/format workflows. These commands are intended \
        for diagnosing parser losslessness and formatter idempotency failures in repositories."
    )]
    Debug {
        #[command(subcommand)]
        command: DebugCommands,
    },
}

#[derive(Subcommand)]
pub enum DebugCommands {
    /// Run parser+formatter checks and emit diagnostics
    #[command(name = "format")]
    Format {
        /// Input file(s) or directories (stdin if not provided)
        #[arg(help = "Input file path(s) or directories")]
        files: Vec<PathBuf>,

        /// Which checks to run
        #[arg(long, value_enum, default_value = "all")]
        checks: DebugChecks,

        /// Emit JSON output for machine-readable tooling
        #[arg(long)]
        json: bool,

        /// Directory where failing artifacts are written
        #[arg(long, value_name = "DIR")]
        dump_dir: Option<PathBuf>,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum DebugChecks {
    Idempotency,
    Losslessness,
    All,
}
