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
    long_about = "Panache is a command-line formatter, linter, and language server \
    (implementing the Language Server Protocol, LSP) for Quarto (.qmd), Pandoc, and Markdown \
    files written in Rust. It understands Quarto/Pandoc-specific syntax that other formatters \
    like Prettier and mdformat struggle with, including fenced divs, tables, and math \
    formatting."
)]
#[command(after_help = "For help with a specific command, see: `panache help <command>`.")]
#[command(
    after_long_help = "For help with a specific command, see: `panache help <command>`.\n\n\
    Homepage and downloads (including .deb and .rpm packages): <https://panache.bz>"
)]
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
        from file extension (for example: `--stdin-filename doc.qmd`)."
    )]
    pub stdin_filename: Option<PathBuf>,

    /// Override the markdown flavor for this invocation
    #[arg(
        long,
        global = true,
        value_enum,
        value_name = "FLAVOR",
        help_heading = "Global options"
    )]
    #[arg(help = "Override the markdown flavor (overrides config and file extension)")]
    #[arg(
        long_help = "Override the markdown flavor for this invocation. Takes the highest \
        precedence: it overrides any value in panache.toml, the [flavor-overrides] glob \
        table, and the flavor inferred from the file extension. Extension overrides from \
        [extensions] in panache.toml still merge on top of the flavor's defaults. Useful \
        when the file extension is unknown (e.g. a .txt file containing markdown) or when \
        you want to force a one-off interpretation."
    )]
    pub flavor: Option<CliFlavor>,

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

    /// Suppress informational output
    #[arg(short = 'q', long, global = true, help_heading = "Global options")]
    #[arg(
        help = "Suppress informational and diagnostic output (errors and primary results still print)"
    )]
    #[arg(
        long_help = "Suppress informational status messages on stdout (e.g. \"Formatted X\", \
        \"N file left unchanged\", \"All files are correctly formatted\", \"No issues found\") \
        as well as per-violation lint diagnostics. Errors are still written to stderr, and \
        primary command output (such as formatted content when reading from stdin, JSON/Markdown \
        reports, or the parsed CST) continues to print so that pipelines keep working. The \
        process exit code still reflects whether issues were found in --check mode."
    )]
    pub quiet: bool,

    /// Print additional informational output
    #[arg(short = 'v', long, global = true, help_heading = "Global options")]
    #[arg(help = "Print additional informational output where supported")]
    #[arg(
        long_help = "Print additional informational output where supported. Currently used by \
        `panache clean` to include a summary of cache size and file count alongside the \
        \"Removed cache directory\" message. Conflicts with --quiet."
    )]
    pub verbose: bool,

    /// Ignore all discovered configuration files
    #[arg(long, global = true, help_heading = "Global options")]
    #[arg(help = "Ignore all discovered configuration files")]
    pub isolated: bool,

    /// Disable lint/format cache reads and writes
    #[arg(
        long,
        global = true,
        env = "PANACHE_NO_CACHE",
        help_heading = "Global options"
    )]
    #[arg(help = "Disable all lint/format cache reads and writes for this run")]
    #[arg(
        long_help = "Disable all lint/format cache reads and writes for this run. Can also be enabled with PANACHE_NO_CACHE."
    )]
    pub no_cache: bool,

    /// Path to cache directory override
    #[arg(
        long,
        global = true,
        value_name = "CACHE_DIR",
        env = "PANACHE_CACHE_DIR",
        help_heading = "Global options"
    )]
    #[arg(help = "Path to the cache directory (overrides config cache-dir)")]
    #[arg(
        long_help = "Path to the cache directory for this invocation. Overrides config `cache-dir`. Can also be set with PANACHE_CACHE_DIR."
    )]
    pub cache_dir: Option<PathBuf>,

    /// Number of worker threads for processing multiple files
    #[arg(
        short = 'j',
        long,
        global = true,
        value_name = "N",
        env = "PANACHE_JOBS",
        help_heading = "Global options"
    )]
    #[arg(help = "Worker threads for multi-file format/lint (0 = auto, 1 = serial)")]
    #[arg(
        long_help = "Number of worker threads to use when formatting or linting multiple files. \
        0 (the default) selects an automatic level based on available CPU cores. 1 forces serial \
        processing. Single-file invocations always run on one thread; the inner external-formatter \
        pool (see external-max-parallel) is only used when this value is 1 or when only one file is \
        being processed. Can also be set with PANACHE_JOBS."
    )]
    #[arg(default_value_t = 0)]
    pub jobs: usize,
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
        /// Input file(s) (stdin if not provided, or pass `-`)
        #[arg(help = "Input file path(s) or directories (use `-` for stdin)")]
        #[arg(
            long_help = "Path(s) to the input file(s) or directories to format. If not provided, or if \
            the single argument `-` is given, reads from stdin. \
            Supports .qmd, .md, .Rmd/.Rmarkdown, and other Markdown-based formats. When file paths are \
            provided, the files are formatted in place by default. Stdin input always outputs \
            to stdout. Supports glob patterns (e.g., *.md) and directories (e.g., . or docs/). \
            Directories are traversed recursively, respecting .gitignore files. \
            `-` cannot be combined with other paths."
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
            Format: `--range START:END` (e.g., --range 5:10 formats lines 5 through 10). \
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
        #[arg(
            long_help = "Apply exclude patterns from your configuration even to files \
            passed explicitly on the command line.\
            \n\nBy default, explicitly-named files bypass exclude patterns: the assumption is \
            that if you asked for a specific file, you want it processed. With --force-exclude, \
            those patterns are honored regardless.\
            \n\nThis is primarily intended for pre-commit hooks (and similar tooling), which \
            pass changed files explicitly but should still respect the project's exclude \
            configuration."
        )]
        force_exclude: bool,

        /// Override individual config options (repeatable)
        #[arg(short = 'o', long = "option", value_name = "KEY=VALUE")]
        #[arg(
            help = "Override an individual config option for this invocation (e.g. -o line-width=100)"
        )]
        #[arg(
            long_help = "Override an individual config option for this invocation, using the \
            kebab-case key names from panache.toml. May be repeated. Takes precedence over \
            both panache.toml and the built-in defaults. \
            \n\nSupported keys: \
            `line-width` (positive integer); \
            `wrap` (one of: reflow, sentence, semantic, preserve); and \
            `extensions.<name>=<bool>` to toggle any extension (booleans accept \
            true/false/1/0/yes/no/on/off). \
            \n\nExample: `--option line-width=100 -o wrap=sentence \
            -o extensions.east-asian-line-breaks=true`. \
            \n\nNote: this is an escape hatch for ad-hoc invocations. Prefer panache.toml so \
            that everyone formatting the repository gets the same result."
        )]
        option: Vec<String>,
    },
    /// Parse and display the CST tree for debugging
    #[command(
        long_about = "Parse a document and display its Concrete Syntax Tree (CST) for debugging \
        and understanding how Panache interprets the document structure. The CST shows all block \
        and inline elements detected by the parser."
    )]
    Parse {
        /// Input file (stdin if not provided, or pass `-`)
        #[arg(help = "Input file path (use `-` for stdin)")]
        #[arg(
            long_help = "Path to the input file to parse. If not provided, or if `-` is given, reads from stdin. \
            The parser respects extension flags from the configuration file."
        )]
        file: Option<PathBuf>,

        /// Output format printed to stdout
        #[arg(long, value_enum, default_value_t = ParseOutput::Cst, value_name = "FORMAT")]
        #[arg(help = "Output format: cst (default), pandoc-ast, or pandoc-json")]
        #[arg(long_help = "Choose what to print to stdout:\n\
            - cst (default): debug-format CST tree, useful for parser debugging.\n\
            - pandoc-ast: pandoc-native AST text, the same shape produced by \
              `pandoc -f markdown -t native`. Unsupported constructs emit visible \
              `Unsupported \"<KIND>\"` sentinels rather than being silently dropped.\n\
            - pandoc-json: the same AST encoded as JSON, matching \
              `pandoc -f markdown -t json`. UTF-8 round-trips cleanly through \
              `jq`/`ascii2uni` since strings use standard JSON escaping rather \
              than Haskell-show numeric escapes.")]
        to: ParseOutput,

        /// Write CST JSON output to the given file
        #[arg(long, value_name = "PATH")]
        #[arg(help = "Write CST JSON output to PATH")]
        #[arg(
            long_help = "Write the parsed CST to the given JSON file in addition to \
            printing the selected --to format to stdout. The JSON output is always \
            CST-shaped regardless of --to; it includes node kinds, text ranges, and \
            token text."
        )]
        json: Option<PathBuf>,

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
        /// Input file(s) or directories (stdin if not provided, or pass `-`)
        #[arg(help = "Input file path(s) or directories (use `-` for stdin)")]
        #[arg(
            long_help = "Path(s) to the input file(s) or directories to check. If not provided, or if \
            the single argument `-` is given, reads from stdin. \
            Supports .qmd, .md, .Rmd/.Rmarkdown, and other Markdown-based formats. Supports glob patterns \
            (e.g., *.md) and directories (e.g., . or docs/). Directories are traversed recursively, \
            respecting .gitignore files. `-` cannot be combined with other paths."
        )]
        files: Vec<PathBuf>,

        /// Deprecated: linting exits non-zero on violations by default
        #[arg(long)]
        #[arg(help = "Deprecated (no-op): lint now exits non-zero on violations by default")]
        #[arg(
            long_help = "Deprecated and now a no-op. `panache lint` exits with code 1 whenever \
            any violations are found, so this flag is no longer needed. It is still accepted for \
            backward compatibility but will be removed in a future release. Use `--fix` to apply \
            auto-fixes instead of reporting."
        )]
        check: bool,

        /// Apply auto-fixes
        #[arg(long)]
        #[arg(help = "Automatically fix violations where possible")]
        fix: bool,

        /// Also apply unsafe auto-fixes
        #[arg(long, requires = "fix")]
        #[arg(help = "Also apply unsafe auto-fixes (may change document meaning)")]
        #[arg(
            long_help = "Apply auto-fixes marked unsafe in addition to safe ones. \
            \n\nA fix is unsafe when applying it may change the document's meaning rather \
            than merely tidy its syntax (for example, the `empty-values` rule's fix deletes \
            the empty key). Unsafe fixes are skipped by a bare --fix; this flag opts into \
            them. Requires --fix."
        )]
        unsafe_fixes: bool,

        /// Diagnostic rendering format
        #[arg(long, value_enum, default_value = "human")]
        #[arg(help = "Diagnostic rendering format")]
        message_format: MessageFormat,

        /// Enforce exclude patterns even for explicitly provided files
        #[arg(long)]
        #[arg(help = "Apply exclude patterns to explicitly provided files")]
        #[arg(
            long_help = "Apply exclude patterns from your configuration even to files \
            passed explicitly on the command line. \
            \n\nBy default, explicitly-named files bypass exclude patterns: the assumption is \
            that if you asked for a specific file, you want it processed. With --force-exclude, \
            those patterns are honored regardless. \
            \n\nThis is primarily intended for pre-commit hooks (and similar tooling), which \
            pass changed files explicitly but should still respect the project's exclude \
            configuration."
        )]
        force_exclude: bool,
    },
    /// Delete cache data
    #[command(long_about = "Delete Panache's on-disk cache data.")]
    Clean {
        /// Remove all Panache cache buckets
        #[arg(long)]
        #[arg(help = "Remove all Panache cache buckets")]
        all: bool,

        /// Show what would be removed without deleting anything
        #[arg(long)]
        #[arg(help = "Show what would be removed without deleting anything")]
        #[arg(
            long_help = "Print the cache path(s) that would be deleted, along with a summary \
            of file count and size, but do not modify anything on disk. The size summary is \
            always shown in --dry-run mode (the global --verbose flag is not required). \
            Combine with --all to preview removal of every cache bucket."
        )]
        dry_run: bool,
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
        /// Input file(s) or directories (stdin if not provided, or pass `-`)
        #[arg(help = "Input file path(s) or directories (use `-` for stdin)")]
        files: Vec<PathBuf>,

        /// Which checks to run
        #[arg(long, value_enum, default_value = "all")]
        checks: DebugChecks,

        /// Emit JSON output for machine-readable tooling
        #[arg(long)]
        json: bool,

        /// Emit Markdown report output suitable for issue descriptions
        #[arg(long)]
        report: bool,

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
        #[arg(
            long_help = "Apply exclude patterns from your configuration even to files \
            passed explicitly on the command line. \
            \n\nBy default, explicitly-named files bypass exclude patterns: the assumption is \
            that if you asked for a specific file, you want it processed. With --force-exclude, \
            those patterns are honored regardless. \
            \n\nThis is primarily intended for pre-commit hooks (and similar tooling), which \
            pass changed files explicitly but should still respect the project's exclude \
            configuration."
        )]
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
pub enum ParseOutput {
    Cst,
    PandocAst,
    PandocJson,
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
pub enum CliFlavor {
    /// Pandoc's Markdown (the extended dialect described in `man pandoc`)
    Pandoc,
    /// Quarto's Markdown (Pandoc-based, with Quarto-specific extensions and shortcodes)
    Quarto,
    /// R Markdown (Pandoc-based, with knitr/Rmd code chunks)
    #[value(name = "rmarkdown")]
    RMarkdown,
    /// GitHub Flavored Markdown
    Gfm,
    /// CommonMark (the strict, standardized base dialect)
    #[value(name = "commonmark")]
    CommonMark,
    /// MultiMarkdown (Fletcher Penney's extended Markdown dialect)
    #[value(name = "multimarkdown")]
    MultiMarkdown,
    /// mdsvex (Svelte-flavored Markdown: CommonMark + Svelte template syntax)
    #[value(name = "mdsvex")]
    Mdsvex,
    /// MyST (CommonMark + Sphinx/MyST directives, roles, and targets)
    #[value(name = "myst")]
    Myst,
}
