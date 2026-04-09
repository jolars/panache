use clap::builder::Styles;
use clap::builder::styling::{AnsiColor, Effects};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .placeholder(AnsiColor::Cyan.on_default());

#[derive(Parser)]
#[command(name = "panache")]
#[command(author, version)]
#[command(
    about = "Panache: A language server, formatter, and linter for Pandoc, Quarto and R Markdown"
)]
#[command(styles = STYLES)]
#[command(
    long_about = "Panache is a CLI formatter and LSP for Quarto (.qmd), Pandoc, and Markdown files \
    written in Rust. It understands Quarto/Pandoc-specific syntax that other formatters like \
    Prettier and mdformat struggle with, including fenced divs, tables, and math formatting."
)]
#[command(after_help = "For help with a specific command, see: `panache help <command>`.")]
#[command(arg_required_else_help = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Path to config file
    #[arg(long, global = true, help_heading = "Global options")]
    #[arg(help = "Path to configuration file")]
    #[arg(
        long_help = "Path to a custom configuration file. If not specified, Panache will \
        search for .panache.toml or panache.toml in the current directory and its parents, \
        then fall back to ~/.config/panache/config.toml."
    )]
    pub config: Option<PathBuf>,

    /// Synthetic filename to use when reading from stdin
    #[arg(
        long,
        global = true,
        value_name = "PATH",
        help_heading = "Global options"
    )]
    #[arg(help = "Synthetic filename for stdin input (used for flavor detection)")]
    #[arg(
        long_help = "Synthetic filename to associate with stdin input. This is useful for editor \
        integrations that pipe content via stdin but still need Panache to infer flavor/extensions \
        from file extension (for example: --stdin-filename doc.qmd)."
    )]
    pub stdin_filename: Option<PathBuf>,

    /// Control when colored output is used
    #[arg(
        long,
        global = true,
        value_enum,
        default_value = "auto",
        value_name = "WHEN",
        help_heading = "Global options"
    )]
    #[arg(help = "Control when colored output is used")]
    pub color: ColorMode,

    /// Disable colored output
    #[arg(long, global = true, help_heading = "Global options")]
    #[arg(help = "Disable colored output (equivalent to --color never)")]
    pub no_color: bool,

    /// Ignore all discovered configuration files
    #[arg(long, global = true, help_heading = "Global options")]
    #[arg(help = "Ignore all discovered configuration files")]
    pub isolated: bool,

    /// Disable lint/format cache reads and writes
    #[arg(
        long,
        global = true,
        env = "RUFF_NO_CACHE",
        help_heading = "Global options"
    )]
    #[arg(help = "Disable all lint/format cache reads and writes for this run")]
    #[arg(
        long_help = "Disable all lint/format cache reads and writes for this run. Can also be enabled with RUFF_NO_CACHE."
    )]
    pub no_cache: bool,

    /// Path to cache directory override
    #[arg(
        long,
        global = true,
        value_name = "CACHE_DIR",
        env = "RUFF_CACHE_DIR",
        help_heading = "Global options"
    )]
    #[arg(help = "Path to the cache directory (overrides config cache-dir)")]
    #[arg(
        long_help = "Path to the cache directory for this invocation. Overrides config `cache-dir`. Can also be set with RUFF_CACHE_DIR."
    )]
    pub cache_dir: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Format a Quarto, Pandoc, or Markdown document
    #[command(
        long_about = "Format a Quarto, Pandoc, or R Markdown document according to Panache's \
        formatting rules. By default, formats files in place. Use --check to verify formatting \
        without making changes. With --verify, Panache runs parser/formatter invariants without \
        writing changes to disk. Stdin input always outputs to stdout."
    )]
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
            long_help = "Check if the file is already formatted according to Panache's rules \
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

        /// Enforce exclude patterns even for explicitly provided files
        #[arg(long)]
        #[arg(help = "Apply exclude patterns to explicitly provided files")]
        force_exclude: bool,
    },
    /// Parse and display the CST tree for debugging
    #[command(
        long_about = "Parse a document and display its Concrete Syntax Tree (CST) for debugging \
        and understanding how Panache interprets the document structure. The CST shows all block \
        and inline elements detected by the parser."
    )]
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
        long_about = "Start the Panache language server protocol (LSP) server for editor \
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
    #[command(after_help = "Configure rules in panache.toml with [lint] section.")]
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

        /// Diagnostic rendering format
        #[arg(long, value_enum, default_value = "human")]
        #[arg(help = "Diagnostic rendering format")]
        message_format: MessageFormat,

        /// Enforce exclude patterns even for explicitly provided files
        #[arg(long)]
        #[arg(help = "Apply exclude patterns to explicitly provided files")]
        force_exclude: bool,
    },
    /// Translate human-readable text while preserving document structure
    #[command(
        long_about = "Translate human-readable text in Quarto, Pandoc, or Markdown documents \
        while preserving syntax and structure. With a single input path, translates in place by default. \
        With two input paths, treats them as input and output paths (`in.md out.md`). \
        Stdin input always outputs to stdout."
    )]
    Translate {
        /// Input path(s) (stdin if not provided)
        #[arg(help = "Input path(s) or input/output file pair")]
        #[arg(
            long_help = "Path arguments for translation. If omitted, reads from stdin and writes to stdout. \
            If one path is provided and it is a file, translation is applied in place. \
            If two file paths are provided, the first is input and the second is output. \
            Directory traversal supports .qmd, .md, .Rmd, and other Markdown-based formats."
        )]
        files: Vec<PathBuf>,

        /// Translation provider backend
        #[arg(long, value_enum)]
        #[arg(help = "Translation provider to use")]
        provider: Option<TranslateProviderArg>,

        /// Source language code (e.g. en, fr, de). Use auto where supported.
        #[arg(long, value_name = "LANG")]
        #[arg(help = "Source language code")]
        source_lang: Option<String>,

        /// Target language code (e.g. en, fr, de)
        #[arg(long, value_name = "LANG")]
        #[arg(help = "Target language code")]
        target_lang: Option<String>,

        /// Provider API key (overrides config)
        #[arg(long, value_name = "KEY")]
        #[arg(help = "Provider API key")]
        api_key: Option<String>,

        /// Provider endpoint URL (overrides config)
        #[arg(long, value_name = "URL")]
        #[arg(help = "Provider endpoint URL")]
        endpoint: Option<String>,

        /// Print translated output to stdout instead of writing files in place
        #[arg(long)]
        #[arg(help = "Output translated content to stdout")]
        stdout: bool,

        /// Enforce exclude patterns even for explicitly provided files
        #[arg(long)]
        #[arg(help = "Apply exclude patterns to explicitly provided files")]
        force_exclude: bool,
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

        /// Dump intermediate check artifacts even when checks pass
        #[arg(long)]
        #[arg(help = "Write input/parse/format pass artifacts to --dump-dir for every file")]
        dump_passes: bool,

        /// Enforce exclude patterns even for explicitly provided files
        #[arg(long)]
        #[arg(help = "Apply exclude patterns to explicitly provided files")]
        force_exclude: bool,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum DebugChecks {
    Idempotency,
    Losslessness,
    All,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum MessageFormat {
    Human,
    Short,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum TranslateProviderArg {
    Deepl,
    Libretranslate,
}
