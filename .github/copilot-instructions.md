# LLM Agent Instructions for panache

## Repository Overview

**panache** is a formatter, linter, and LSP for Quarto (`.qmd`),
Pandoc, and Markdown files written in Rust. It's designed to understand
Quarto/Pandoc-specific syntax that other formatters like Prettier and mdformat
struggle with, including fenced divs, tables, and math formatting.

**Syntax Reference**: See <a>`assets/pandoc-spec.md`</a> for comprehensive
Pandoc syntax specification. This index document links to individual
specification files organized by syntax element type (paragraphs, headings,
lists, tables, etc.) in the <a>`assets/pandoc-spec/`</a> directory. These
documents represent the definitive reference for elements that the parser
should understand and handle correctly. This specification is essential for
understanding formatting requirements, implementing parser logic, and ensuring
spec compliance. panache aims to support the full suite of Pandoc syntax,
including all extensions. It will also support all the Quarto-specific syntax
extensions.

**Key Facts:**

- **Language**: Rust (2024 edition), stable toolchain
- **Size**: ~30k lines across 100+ files
- **Architecture**: Binary crate with workspace containing WASM crate for web
  playground
- **Status**: Early development - expect bugs and breaking changes

## Build and Validation Instructions

### Prerequisites

```bash
# Install Rust components (required for CI checks)
rustup component add rustfmt clippy
```

### Essential Commands (in order of typical workflow)

1. **Check compilation** (fastest validation):

```bash
cargo check
```

2. **Run tests**:

```bash
cargo test
```

3. **Build release** (for CLI testing):

```bash
cargo build --release
```

4. **Lint code**:

```bash
cargo clippy -- -D warnings
```

5. **Check formatting**:

```bash
cargo fmt -- --check
```

### Development Workflow

**ALWAYS run this sequence before making changes to understand baseline:**

```bash
cargo check && cargo test && cargo clippy -- -D warnings && cargo fmt -- --check
```

### CLI Testing

```bash
# Basic functionality test (format subcommand required)
printf "# Test\n\nThis is a very long line that should be wrapped." | ./target/release/panache format

# Expected: Line wrapping at ~80 characters with proper Markdown formatting
```

### Parsing the CST for Debugging

```bash
# Parse subcommand shows CST structure
printf "# Heading\n\nParagraph with *emphasis* and `code`." | ./target/release/panache parse

# Parse with config to respect extension flags
printf "Math: \\(x^2\\)" | ./target/release/panache parse --config .panache.toml
```

### CLI Subcommands

panache requires explicit subcommands:

```bash
panache format document.qmd   # writes file in place, support globbing
panache format < document.qmd # writes to stdout

# Format with options
panache format --check document.qmd       # Check if formatted
panache format --config cfg.toml file.qmd # Custom config

# Parse (show CST for debugging)
panache parse document.qmd
panache parse --config cfg.toml file.qmd  # Config affects parsing

# Lint (check for issues)
panache lint document.qmd
panache lint --check document.qmd         # Exit code 1 if issues found (CI mode)
panache lint --fix document.qmd           # Apply auto-fixes

# LSP (Language Server Protocol)
panache lsp  # Starts LSP server on stdin/stdout for editor integration
```

### Debugging with Logging

panache has comprehensive logging infrastructure for debugging:

```bash
# Development: See all parsing decisions (requires debug build via cargo run)
# Works with any subcommand: format, parse, lint, lsp
RUST_LOG=debug cargo run -- format document.qmd
RUST_LOG=debug cargo run -- parse document.qmd
RUST_LOG=debug cargo run -- lint document.qmd

# Detailed debugging: Every parsing step (requires debug build)
RUST_LOG=trace cargo run -- parse document.qmd

# Detailed debugging: Every formatting + parsing step (requires debug build)
RUST_LOG=trace cargo run -- format document.qmd

# Module-specific: Only inline parser debug logs
RUST_LOG=panache::parser::inline_parser=debug cargo run -- format document.qmd

# Multiple modules with different levels
RUST_LOG=panache::parser::block_parser=trace,panache::formatter=debug cargo run -- format document.qmd

# Reading from stdin during development
printf "# Test\n\nParagraph." | RUST_LOG=debug cargo run -- format

# Using release build: INFO logs only (DEBUG/TRACE compiled out for performance)
RUST_LOG=info ./target/release/panache format document.qmd
```

**Log levels and content:**

- **INFO**: Formatting lifecycle, config loading (available in release)
- **DEBUG**: All parsing decisions, element matches, table detection
- **TRACE**: Text previews, container operations, detailed steps

**Modules with logging:**

- `panache::parser::block_parser` - Block element detection (headings, tables,
  code blocks, etc.)
- `panache::parser::inline_parser` - Inline element matching (emphasis, code,
  math, links, footnotes)
- `panache::formatter` - Formatting decisions and node traversal
- `panache::config` - Config file loading and resolution

### Timing Notes

- `cargo test`: ~1 second (817 total tests: 747 unit + 70 integration)
- `cargo build --release`: ~25 seconds
- `cargo check`: ~1 second

### Debugging with Shell Commands

When using the bash tool for testing and debugging:

**For quick commands (compile, test, format):**
- Use `mode="sync"` (default) with appropriate `initial_wait` (30-60 seconds)
- Output is returned directly in the response
- **Don't use `read_bash` on completed sync commands** - they don't create persistent sessions
- Only use `read_bash` if the command is still running after `initial_wait`

**Example - correct usage:**
```bash
# Sync command returns output directly
cargo run -- format test.md  # Output appears in response, no read_bash needed

# If a long command needs more time, use read_bash only while it's running
cargo build --release  # initial_wait=60, then read_bash if still running
```

**For interactive debugging:**
- Use `mode="async"` for REPL sessions, debuggers, or interactive tools
- Use `write_bash` to send input to the async session
- Use `read_bash` to get more output from the async session

## Project Architecture and Layout

The project is designed to first parse the document into a concrete syntax tree
(CST) using a block parser, then run an inline parser/lexer to handle inline
elements. The CST is represented using the `rowan` crate, which provides a
red-green tree structure for efficient syntax tree manipulation. It is vital
that the parser preserves **every byte** of the input in the syntax tree,
including structural markers like `>` for blockquotes, to ensure lossless
parsing so that LSP and linting features can accurately map source locations.

**CST vs AST Distinction**:

- **CST (Concrete Syntax Tree)**: The rowan tree preserves every byte including
  whitespace, markers, and structural tokens. This is what `SyntaxNode` provides
  and what the `cst.txt` snapshot files contain. Example: includes
  `ATX_HEADING_MARKER@0..1 "#"`, `WHITESPACE@1..2 " "`, etc.
- **AST (Abstract Syntax Tree)**: The typed wrappers provide an abstract view
  that hides syntactic details. Example: `Heading::cast(node).level()` returns
  `1` without exposing the `#` markers.

The parser builds the CST, and typed wrappers (see **Typed AST Wrappers**
section below) provide convenient AST-like access for LSP and other features.

**Typed AST Wrappers**: The CST provides low-level access via `SyntaxNode`, but
for ergonomic use (especially in LSP), panache provides typed wrapper structs
that implement the `AstNode` trait. These wrappers (e.g., `Heading`, `Link`,
`Table`) provide type-safe access with convenient methods like `heading.level()`
and `link.dest()`. This pattern is borrowed from rust-analyzer and significantly
improves code readability in LSP handlers. The formatter works directly with
`SyntaxNode` for maximum flexibility, but can use wrappers where beneficial.

### Source Structure

```
src/
├── main.rs              # CLI entry point with subcommands (format, parse, lint, lsp)
├── lib.rs               # Public API with format() and parse() functions
├── cli.rs               # CLI argument definitions with clap
├── config.rs            # Configuration handling (.panache.toml, flavor, extensions)
├── syntax.rs            # Syntax module entry point (re-exports, type aliases)
├── syntax/
│   ├── kind.rs              # SyntaxKind enum (SCREAMING_SNAKE_CASE) + QuartoLanguage
│   ├── ast.rs               # AstNode trait + support helpers for typed wrappers
│   ├── headings.rs          # Heading, HeadingContent typed wrappers
│   ├── links.rs             # Link, ImageLink, Figure typed wrappers
│   ├── tables.rs            # Table typed wrappers (PipeTable, GridTable, etc.)
│   └── references.rs        # ReferenceDefinition, Footnote typed wrappers
├── utils.rs             # General utility functions
├── range_utils.rs       # Range manipulation utilities
├── external_formatters.rs       # Async external formatter integration
├── external_formatters_sync.rs  # Sync external formatter integration
├── external_formatters_common.rs # Shared formatter utilities
├── formatter.rs         # Formatter module entry point (public API)
├── formatter/
│   ├── core.rs             # Formatter struct + format_node_sync orchestration
│   ├── wrapping.rs         # Word-breaking and line-wrapping logic
│   ├── code_blocks.rs      # Code block collection + external formatters
│   ├── paragraphs.rs       # Paragraph + display math formatting
│   ├── inline.rs           # Inline element formatting (emphasis, code, links)
│   ├── headings.rs         # Heading formatting
│   ├── utils.rs            # Helper functions (is_block_element)
│   ├── indent_utils.rs     # Indentation utilities
│   ├── blockquotes.rs      # Blockquote formatting logic
│   ├── lists.rs            # List formatting logic (ordered/unordered/task)
│   ├── tables.rs           # Table formatting logic (grid tables, pipe tables, simple tables)
│   ├── fenced_divs.rs      # Fenced div formatting logic (Quarto/Pandoc)
│   ├── metadata.rs         # Frontmatter formatting logic (YAML, TOML, Pandoc)
│   ├── hashpipe.rs         # Hashpipe formatting (Quarto-specific)
│   └── shortcodes.rs       # Shortcode formatting (Quarto-specific)
├── parser.rs            # Parser module entry point with parse() function
├── parser/
│   ├── block_parser.rs      # Block parser module entry point
│   ├── block_parser/
│   │   ├── attributes.rs        # Attribute parsing ({#id .class key=value})
│   │   ├── blockquotes.rs       # Blockquote parsing and resolution
│   │   ├── chunk_options.rs     # Chunk option parsing (Quarto/RMarkdown)
│   │   ├── code_blocks.rs       # Fenced code block parsing
│   │   ├── container_stack.rs   # Container block stack management
│   │   ├── definition_lists.rs  # Definition list parsing
│   │   ├── fenced_divs.rs       # Quarto/Pandoc fenced div parsing (:::)
│   │   ├── figures.rs           # Figure parsing (![alt](img))
│   │   ├── headings.rs          # ATX heading parsing (#)
│   │   ├── horizontal_rules.rs  # Horizontal rule parsing (---)
│   │   ├── html_blocks.rs       # HTML block parsing
│   │   ├── indented_code.rs     # Indented code block parsing
│   │   ├── latex_envs.rs        # LaTeX environment parsing (\begin{} \end{})
│   │   ├── line_blocks.rs       # Line block parsing (|)
│   │   ├── lists.rs             # List parsing (ordered/unordered/task/definition)
│   │   ├── marker_utils.rs      # List marker utilities
│   │   ├── metadata.rs          # Frontmatter parsing (YAML, TOML, Pandoc title block)
│   │   ├── paragraphs.rs        # Paragraph parsing
│   │   ├── reference_definitions.rs # Reference link/footnote definition parsing
│   │   ├── tables.rs            # Table parsing (grid, pipe, simple)
│   │   ├── utils.rs             # Helper functions (strip_leading_spaces, etc.)
│   │   └── tests/               # Block parser unit tests
│   ├── inline_parser.rs     # Inline parser module entry point
│   ├── inline_parser/
│   │   ├── architecture_tests.rs # Tests for nested inline structures
│   │   ├── bracketed_spans.rs    # Bracketed span parsing ([text]{.class})
│   │   ├── citations.rs          # Citation parsing (@key, [@key])
│   │   ├── code_spans.rs         # Code span parsing (`code`)
│   │   ├── emphasis.rs           # Emphasis/strong parsing (*em* **strong**)
│   │   ├── escapes.rs            # Escape sequence parsing (\*)
│   │   ├── inline_footnotes.rs   # Inline footnote parsing (^[text])
│   │   ├── latex.rs              # Inline LaTeX command parsing (\command)
│   │   ├── links.rs              # Link and image parsing ([text](url))
│   │   ├── math.rs               # Inline math parsing ($x^2$)
│   │   ├── native_spans.rs       # Native span parsing
│   │   ├── raw_inline.rs         # Raw inline parsing (`code`{=format})
│   │   ├── shortcodes.rs         # Shortcode parsing (Quarto-specific)
│   │   ├── strikeout.rs          # Strikeout parsing (~~text~~)
│   │   ├── subscript.rs          # Subscript parsing (~text~)
│   │   ├── superscript.rs        # Superscript parsing (^text^)
│   │   ├── tests/                # Inline parser test modules
│   │   └── tests.rs              # Integration tests
│   └── list_postprocessor.rs # List post-processing utilities
├── linter.rs            # Linter module entry point (public API)
├── linter/
│   ├── code_block_collector.rs  # Code block collection for linting
│   ├── diagnostics.rs       # Diagnostic types (Location, Severity, Fix, Edit)
│   ├── external_linters.rs  # Async external linter integration
│   ├── external_linters_sync.rs # Sync external linter integration
│   ├── runner.rs            # LintRunner orchestration
│   ├── rules.rs             # Rule trait and RuleRegistry
│   └── rules/
│       └── heading_hierarchy.rs # Heading hierarchy rule (h1 → h3 warns)
├── lsp.rs               # LSP module entry point
├── lsp/
│   ├── config.rs            # Config loading and management
│   ├── conversions.rs       # Position/offset conversion utilities
│   ├── documents.rs         # Document state management
│   ├── handlers.rs          # LSP handler trait and implementations
│   ├── handlers/            # Individual handler modules
│   │   ├── code_actions.rs      # Code action implementations
│   │   ├── diagnostics.rs       # Diagnostic publishing
│   │   ├── document_symbols.rs  # Document outline/symbols
│   │   ├── folding_ranges.rs    # Code folding ranges
│   │   ├── formatting.rs        # Document/range formatting
│   │   └── goto_definition.rs   # Go to definition support
│   ├── helpers.rs           # Helper functions for LSP
│   └── server.rs            # Backend server implementation
├── metadata.rs          # Metadata module entry point
└── metadata/
    ├── bibliography.rs      # Bibliography/citation handling
    └── yaml.rs              # YAML frontmatter utilities
```

### Configuration System

panache uses a hierarchical config lookup:

1. Explicit `--config` path (errors if invalid)
2. `.panache.toml` or `panache.toml` in current/parent directories
3. `~/.config/panache/config.toml` (XDG)
4. Built-in defaults (80 char width, auto line endings, reflow wrap)

**Extension Configuration**: The config system includes:

- `flavor` field: Choose Markdown flavor (Pandoc, Quarto, RMarkdown, GFM,
  CommonMark)
- `extensions` section: 60+ bool flags for individual Pandoc extensions
- Each flavor has sensible defaults that can be overridden

Example `.panache.toml`:

```toml
flavor = "quarto"
line_width = 80

[extensions]
# Override flavor defaults:
hard_line_breaks = false
citations = true
```

### SyntaxKind Naming Convention

All `SyntaxKind` enum variants follow **SCREAMING_SNAKE_CASE** convention,
matching rust-analyzer and other rowan-based parsers:

- ✅ `HEADING`, `PARAGRAPH`, `CODE_BLOCK`, `LINK`, `IMAGE_LINK`
- ✅ `ATX_HEADING_MARKER`, `FOOTNOTE_REFERENCE`, `TABLE_CAPTION`
- ❌ ~~`Heading`~~, ~~`CodeBlock`~~, ~~`ImageLink`~~ (old UpperCamelCase -
  removed)

**Rationale**: These are CST discriminants, not type names. The
`#[allow(non_camel_case_types)]` attribute suppresses Rust's lint warning. Typed
wrappers use UpperCamelCase (`Heading`, `Link`, etc.) to distinguish them from
the raw discriminants.

### Test Architecture

```
tests/
├── golden_cases.rs      # Main integration tests using test case directories
├── cases/              # Input/expected output pairs (60+ test scenarios)
│   ├── wrap_paragraph/
│   │   ├── cst.txt      # CST snapshot of input for debugging
│   │   ├── panache.toml # Optional config for this test case
│   │   ├── input.md     # Raw input (can be .md, .qmd, .Rmd)
│   │   └── expected.md  # Expected formatted output (can be .md, .qmd, .Rmd)
│   └── ...
├── cli/                # CLI integration tests
│   ├── main.rs         # CLI test harness entry point
│   ├── common.rs       # Shared test utilities
│   ├── format.rs       # Format subcommand tests
│   ├── parse.rs        # Parse subcommand tests
│   ├── lint.rs         # Lint subcommand tests
│   ├── lsp.rs          # LSP subcommand tests
│   └── fixtures/       # Test fixtures for CLI tests
├── format/             # Unit tests organized by feature
│   ├── code_chunks.rs
│   ├── headings.rs
│   ├── yaml_frontmatter.rs
│   └── ...
└── external_linters.rs # External linter integration tests
```

### Workspace Structure

```
crates/
└── panache-wasm/   # WebAssembly bindings for web playground
    ├── Cargo.toml
    └── src/
```

### Build Configuration Files

- `Cargo.toml`: Main project config, dependencies, binary definition
- `rust-toolchain.toml`: Pins to stable Rust toolchain
- `Taskfile.yml`: Task runner commands (go-task) - NOT available in CI
- `devenv.nix`: Nix development environment setup

### Documentation Site

The project has a Quarto-based documentation site in the `docs/` directory:

```
docs/
├── _quarto.yml          # Quarto configuration (site metadata, navigation)
├── index.qmd            # Homepage with project overview
├── getting-started.qmd  # Installation and basic usage
├── cli.qmd              # CLI subcommands reference (format, parse, lint, lsp)
├── lsp.qmd              # Language Server setup for editors
├── configuration.qmd    # .panache.toml reference
├── formatting.qmd       # Supported syntax and formatting rules
└── playground/          # WASM-based web playground for live formatting
    └── index.html       # Uses TypeScript bindings from panache-wasm
```

**Structure:**

- **User guides**: Installation, CLI usage, LSP setup, configuration, feature
  showcase
- **playground/**: Interactive WASM-based formatter demo
- **Published**: GitHub Pages via `docs.yml` workflow at
  https://jolars.github.io/panache/

**Building the docs:**

```bash
# Requires Quarto installed
cd docs
quarto preview  # Live preview
quarto render   # Build to _site/
```

**Note:** The README.md in the repo root contains the canonical documentation
content.

## CI/CD and Validation Pipeline

### GitHub Actions Workflows

1. **build-and-test.yml**: Main CI (Ubuntu/Windows/macOS, Rust stable)
   - cargo build --verbose
   - cargo test --verbose
   - cargo clippy -- -D warnings
   - cargo fmt -- --check
   - Security audit via rustsec/audit-check

2. **release.yml**: Semantic release workflow
   - Triggered manually
   - Uses semantic-release with conventional commits

3. **docs.yml**: Quarto documentation publishing to GitHub Pages

### Pre-commit Validation

The CI exactly replicates these commands - ensure all pass locally:

```bash
cargo build --verbose
cargo test --verbose
cargo clippy -- -D warnings
cargo fmt -- --check
```

You can just run `cargo fmt` directly to fix formatting issues and some clippy
warnings can be fixed with `cargo clippy --fix` (but review changes carefully).

### Golden Test System

The project uses snapshot testing via `tests/golden_cases.rs`:

- Each `tests/cases/*` directory contains `input.qmd` and `expected.qmd`
- Tests verify formatting is idempotent (format twice = format once)
- Use `UPDATE_EXPECTED=1 cargo test` to update expected formatted outputs (BE
  CAREFUL)
- Use `UPDATE_CST=1 cargo test` to update expected CST outputs (BE CAREFUL)
- Use both flags together to update both:
  `UPDATE_EXPECTED=1 UPDATE_CST=1 cargo test`
- New features should have corresponding test cases added to cover new
  formatting scenarios.
- **DO NOT** update expected outputs without verifying that the change is
  correct and intended.

## Key Development Facts

### Dependencies

- **clap**: CLI argument parsing with derive macros
- **rowan**: Red-green tree for CST (Concrete Syntax Tree)
- **regex**: Pattern matching for lexing
- **textwrap**: Text wrapping functionality
- **toml**: Configuration file parsing
- **serde**: Serialization for config structs
- **tokio**: Async runtime (features: process, rt-multi-thread, io-util, io-std,
  time, macros, fs)
- **tower-lsp-server**: Community-maintained LSP framework (v0.23)
- **similar**: Text diffing library for diagnostics
- **serde-saphyr**: YAML parsing for frontmatter
- **unicode-width**: Unicode character width calculations
- **uuid**: UUID generation for LSP
- **tempfile**: Temporary file handling for external formatters
- **log** + **env_logger**: Logging infrastructure (debug builds have
  DEBUG/TRACE, release builds have INFO only)

### Linter

panache includes a built-in linter accessible via `panache lint`:

**Architecture:**

- Linter code lives in `src/linter.rs` with submodules for diagnostics, rules,
  and runner
- Provides diagnostic detection and auto-fixes
- Uses modern Rust module structure (`linter.rs` instead of `linter/mod.rs`)

**Components:**

- **diagnostics.rs**: Core types (`Diagnostic`, `Location`, `Severity`, `Fix`,
  `Edit`)
- **runner.rs**: `LintRunner` that orchestrates rule execution
- **rules.rs**: `Rule` trait and `RuleRegistry` for managing lint rules
- **rules/**: Individual rule implementations

**Current Rules:**

- ✅ **heading-hierarchy**: Warns on skipped heading levels (e.g., h1 → h3),
  provides auto-fix to correct level

**Usage:**

```bash
# Lint a document
panache lint document.qmd

# Check mode for CI (exit code 1 if violations)
panache lint --check document.qmd

# Apply auto-fixes
panache lint --fix document.qmd
```

**Adding New Rules:**

1. Create rule file in `src/linter/rules/` implementing `Rule` trait
2. Register in `linter.rs` `default_registry()` function
3. Add tests in rule module

**Diagnostic Structure:**

- **Severity**: Error, Warning, Info
- **Location**: Line, column, text range
- **Code**: Rule identifier (e.g., "heading-hierarchy")
- **Fix**: Optional auto-fix with one or more `Edit` operations

### Language Server Protocol (LSP)

panache includes a built-in LSP implementation accessible via `panache lsp`:

**Architecture:**

- LSP code organized in `src/lsp/` modules (server, handlers, documents,
  conversions)
- Implements `tower_lsp_server::LanguageServer` trait
- Communicates via stdin/stdout using standard LSP JSON-RPC protocol
- Document symbols built synchronously (SyntaxNode is not Send)
- Hierarchical outline: headings contain tables/figures as children

**Current Capabilities:**

- ✅ `textDocument/formatting` - Full document formatting
- ✅ `textDocument/rangeFormatting` - Range formatting
- ✅ `textDocument/didOpen/didChange/didClose` - Document tracking (INCREMENTAL
  sync mode)
- ✅ `textDocument/publishDiagnostics` - Live linting with diagnostics
- ✅ `textDocument/codeAction` - Quick fixes for lint issues (heading hierarchy)
- ✅ `textDocument/documentSymbol` - Document outline with headings, tables,
  and figures
- ✅ `textDocument/foldingRange` - Code folding support for headings, code
  blocks, lists, tables, and more
- ✅ `textDocument/definition` - Go to definition for reference links and
  footnotes
- ✅ Config discovery from workspace root (`.panache.toml`)
- ✅ Thread-safe document state management with Arc
- ✅ UTF-16 to UTF-8 position conversion for proper incremental edits

**Linting Integration:**

- Diagnostics published on document open/change
- Auto-fix suggestions via code actions (QUICKFIX kind)
- Diagnostics cleared on document close
- Uses same linter infrastructure as CLI `lint` subcommand

**Document Outline Implementation:**

- Hierarchical structure: H1 contains H2, H2 contains H3, etc.
- Tables and figures appear as children under their parent heading
- Symbol extraction from syntax tree:
  - Headings: Extract text from HeadingContent node
  - Tables: Extract caption from TableCaption if present
  - Figures: Extract alt text from ImageAlt node
- Proper range calculation using `conversions::offset_to_position`
- Edge case handling: empty headings, documents without headings

**Implementation Details:**

- Document URIs stored as strings (Uri type doesn't implement Send)
- Workspace root captured from `InitializeParams.workspace_folders` or
  deprecated `root_uri`
- Config loaded per formatting request (no caching yet)
- Document symbols built synchronously (SyntaxNode is not Send)
- INCREMENTAL sync mode with proper UTF-16/UTF-8 position conversion
- Full document reparsing (incremental parsing deferred for performance need)
- **Uses typed AST wrappers** for cleaner code: `Heading::cast(node)` provides
  type-safe access with methods like `.level()` and `.text()` instead of manual
  tree traversal

**Typed AST Wrappers**:

The `syntax` module provides typed wrappers following rust-analyzer's pattern:

- **Location**: `src/syntax/ast.rs` defines `AstNode` trait
- **Wrappers**: Implemented in `src/syntax/{headings,links,tables,references}.rs`
- **Pattern**: Each wrapper wraps `SyntaxNode` and provides `cast()` method
- **Benefits**: Type safety, ergonomic APIs, self-documenting code
- **Usage**: Prefer wrappers in LSP handlers, optional in formatter
- **Tier 1 implemented**: Heading, Link, Image, Table, Reference/Footnote types
- **Future**: Tier 2 (CodeBlock, List, BlockQuote) and Tier 3 (inline
  formatting)

Example: ```rust // Without wrapper (manual tree traversal) if node.kind()
== SyntaxKind::HEADING { for child in node.children() { if child.kind() ==
SyntaxKind::HEADING_CONTENT { let text = child.text().to_string(); } } }

// With wrapper (type-safe and clean) if let Some(heading) = Heading::cast(node)
{ let text = heading.text(); let level = heading.level(); } ```

**Testing:**

- Comprehensive unit tests for LSP handlers (symbols, folding, goto definition,
  etc.)
- Unit tests for conversion functions (offset_to_position, convert_diagnostic,
  etc.)
- Tests cover UTF-16 edge cases (emoji, accented characters)
- Integration test documents in `tests/` directory
- Manual testing via editor integration (see README.md for editor configs)

**Testing LSP Manually:**

```bash
# Start the server (for manual editor testing)
./target/release/panache lsp

# Editor configuration examples in README.md (Neovim, VS Code, Helix)
```

### Logging Infrastructure

panache has comprehensive logging (~50 strategic log statements):

- **Release builds**: INFO logs only (formatting metrics, config loading) - zero
  overhead for DEBUG/TRACE
- **Debug builds**: Full DEBUG and TRACE logging available (via `cargo run`)
- **Modules logged**: parser::block_parser, parser::inline_parser, formatter,
  config
- **Usage**: `RUST_LOG=debug cargo run -- format <file>` or
  `RUST_LOG=panache::parser::inline_parser=trace cargo run -- format <file>`
- **Purpose**: Debug parsing decisions, understand element matching, trace
  formatter behavior
- **Important**: Must use `cargo run --` (not installed binary) for DEBUG/TRACE
  logs

Example log output (DEBUG level):

```
[DEBUG] Parsed ATX heading at line 0: level 1
[DEBUG] Matched emphasis at pos 10: level=2, delim=*
[DEBUG] Parsed grid table at line 8 (5 lines)
[INFO] Formatting document with config: line_width=80, wrap=Some(Reflow)
```

### Testing Approach

- Unit tests embedded in source modules (inline and block parser tests)
- Integration tests for inline elements (architecture_tests.rs verifies nesting)
- Golden tests comparing input/expected pairs (1 comprehensive test covering
  60+ scenarios)
- Format tests organized by feature (tests for specific formatting scenarios)
- Property: formatting is idempotent
- Test helpers abstract Config creation (parse_blocks(), parse_inline())

### Public API

The library exposes two main functions in `src/lib.rs`:

**`format(input: &str, config: Option<Config>) -> String`**

- Formats a Quarto/Markdown document
- Takes optional config (uses default if None)
- Returns formatted string

**`parse(input: &str, config: Option<Config>) -> SyntaxNode`**

- Parses a document into a concrete syntax tree (CST)
- Takes optional config (affects which extensions are enabled)
- Returns rowan SyntaxNode for inspection/debugging

Both functions accept an optional config to respect flavor-specific extensions
and formatting preferences.

### Formatting Rules

- Default 80 character line width (configurable)
- **Most formatting behavior will be configurable through panache.toml**
- Preserves frontmatter and code blocks
- Converts setext headings to ATX format
- Handles Quarto-specific syntax (fenced divs, math blocks)
- **Tables will be auto-formatted for consistency**
- **Lists will be formatted to avoid lazy list style**
- **Block quotes will be properly formatted**
- Wraps paragraphs but preserves inline code/math whitespace

## Configuration Files and Settings

- `.panache.toml`: Project-specific config (flavor, line_width, line-ending,
  wrap mode, extensions)
- `.envrc`: direnv configuration for Nix environment
- `.gitignore`: Excludes target/, devenv artifacts, Nix build outputs
- `devenv.nix`: Development environment with go-task, quarto, wasm-pack

**Config is threaded through parsers**:

- `BlockParser::new(input, &Config)` - borrows config
- `InlineParser::new(block_tree, Config)` - owns config
- Test helpers use `Config::default()` to simplify test code

## Web Playground

The `docs/playground/` contains a WASM-based web interface:

- Built via `wasm-pack build --target web` in `crates/panache-wasm/`
- Uses TypeScript bindings for browser integration
- Served via local Python HTTP server for development

## Important Notes for Code Changes

### DO:

- Run full test suite after every change: `cargo test`
- Ensure clippy passes:
  `cargo clippy --all-targets --all-features -- -D warnings`
- Ensure formatting passes: `cargo fmt -- --check` (or just run `cargo fmt` to
  format automatically)
- Test CLI functionality after building release binary
- Consider idempotency - formatting twice should equal formatting once
- Update golden test expectations (CAREFULLY!):
  - `UPDATE_EXPECTED=1 cargo test` for formatted outputs
  - `UPDATE_CST=1 cargo test` for CST snapshots
  - Both flags can be combined if needed

### DON'T:

- Assume task runner is available - use direct cargo commands
- Break the hierarchical config system (explicit > local > XDG > default)
- Change core formatting without extensive golden test verification
- Format code in the parser - the parser should preserve all input bytes,
  including whitespace and structural markers. The formatter is responsible for
  applying formatting rules, not the parser.

### Architecture Dependencies

- Block parser captures block structures (including nested ones) using a
  two-pass approach:
  1. First pass: Parse flat block structures (headings, code blocks, paragraphs,
     etc.)
  2. Second pass: Resolve container blocks (blockquotes, lists) from flat
     structure
- Each block type is isolated in its own module under `src/parser/block_parser/`
- Inline parser runs after block parser to handle inline syntax within blocks
  - Uses delimiter-based parsing with proper precedence (CommonMark spec)
  - Recursive parsing for nested inline elements (e.g., code/emphasis in links)
  - Standalone `parse_inline_text()` function enables recursive calls
- `parser::parse()` function provides clean API that hides two-stage
  implementation
- Parser builds rowan CST consumed by formatter
- Formatter is split into focused modules under `src/formatter/`:
  - Each module has clear responsibilities (wrapping, inline, paragraphs,
    headings, code blocks)
  - Core orchestration in `core.rs` with `format_node_sync` delegating to
    modules
  - Placeholder modules exist for future extraction of complex logic (lists,
    tables, blockquotes)
  - Public API limited to `format_tree()` and `format_tree_async()`
- LSP implementation in `src/lsp/`:
  - Uses `spawn_blocking` wrapper to handle non-Send rowan types
  - Document state stored in Arc<Mutex<HashMap<String, String>>>
  - Config loaded per request from workspace root
  - Multiple handler modules for different LSP capabilities (formatting,
    symbols, folding, diagnostics, goto definition, code actions)
  - Helper functions in `helpers.rs` for common LSP operations
- Config system provides extension flags to enable/disable features
  - Config fields marked `#[allow(dead_code)]` until features use them
- Test helpers abstract Config creation to keep tests clean
- WASM crate depends on main crate - changes affect both
- Metadata module (`src/metadata/`) handles bibliography and YAML frontmatter
  parsing
- Range utilities (`src/range_utils.rs`) for range manipulation and validation

**Trust these instructions and search only when information is incomplete or
incorrect.**
