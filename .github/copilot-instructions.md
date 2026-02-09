# LLM Agent Instructions for panache

## Repository Overview

**panache** is a CLI formatter for Quarto (`.qmd`), Pandoc, and Markdown
files written in Rust. It's designed to understand Quarto/Pandoc-specific
syntax that other formatters like Prettier and mdformat struggle with,
including fenced divs, tables, and math formatting.

**Syntax Reference**: See [`docs/pandoc-spec.md`](docs/pandoc-spec.md) for
comprehensive Pandoc syntax specification. This index document links to
individual specification files organized by syntax element type (paragraphs,
headings, lists, tables, etc.) in the [`docs/pandoc-spec/`](docs/pandoc-spec/)
directory. These documents represent the definitive reference for elements
that the parser should understand and handle correctly. This specification
is essential for understanding formatting requirements, implementing parser
logic, and ensuring spec compliance. panache aims to support the full
suite of Pandoc syntax, including all extensions. It will also support all
the Quarto-specific syntax extensions.

**Key Facts:**

- **Language**: Rust (2024 edition), stable toolchain
- **Size**: ~15k lines across 62 files
- **Architecture**: Binary crate with workspace containing WASM crate for web playground
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
# Basic functionality test
printf "# Test\n\nThis is a very long line that should be wrapped." | ./target/release/panache

# Expected: Line wrapping at ~80 characters with proper Markdown formatting
```

### Parsing the AST for Debugging

```bash
printf "# Heading\n\nParagraph with *emphasis* and `code`." | ./target/release/panache parse
```

### Debugging with Logging

panache has comprehensive logging infrastructure for debugging:

```bash
# Production: High-level metrics only (available in release builds)
RUST_LOG=info panache document.qmd

# Development: See all parsing decisions (debug builds only)
RUST_LOG=debug panache document.qmd

# Detailed debugging: Every parsing step (debug builds only)
RUST_LOG=trace panache document.qmd

# Module-specific: Only inline parser debug logs
RUST_LOG=panache::inline_parser=debug panache document.qmd

# Multiple modules with different levels
RUST_LOG=panache::block_parser=trace,panache::formatter=debug panache document.qmd
```

**Log levels and content:**
- **INFO**: Formatting lifecycle, config loading (available in release)
- **DEBUG**: All parsing decisions, element matches, table detection
- **TRACE**: Text previews, container operations, detailed steps

**Modules with logging:**
- `panache::block_parser` - Block element detection (headings, tables, code blocks, etc.)
- `panache::inline_parser` - Inline element matching (emphasis, code, math, links, footnotes)
- `panache::formatter` - Formatting decisions and node traversal
- `panache::config` - Config file loading and resolution

### Timing Notes

- `cargo test`: ~1 second (603 tests total across lib, inline parser, block parser, format tests, golden tests, and doc tests)
- `cargo build --release`: ~25 seconds
- `cargo check`: ~1 second

## Project Architecture and Layout

The project is designed to first parse the document into a concrete syntax tree (CST)
using a block parser, then run an inline parser/lexer to handle inline elements. The CST is
represented using the `rowan` crate, which provides a red-green tree structure
for efficient syntax tree manipulation. The formatter then traverses this tree to apply
the formatting rules.

### Source Structure

```
src/
├── main.rs              # CLI entry point with clap argument parsing
├── lib.rs               # Public API with format() and parse() functions
├── config.rs            # Configuration handling (.panache.toml, flavor, extensions)
├── formatter.rs         # Formatter module entry point (public API)
├── formatter/
│   ├── core.rs             # Formatter struct + format_node_sync orchestration
│   ├── wrapping.rs         # Word-breaking and line-wrapping logic
│   ├── code_blocks.rs      # Code block collection + external formatters
│   ├── paragraphs.rs       # Paragraph + display math formatting
│   ├── inline.rs           # Inline element formatting (emphasis, code, links)
│   ├── headings.rs         # Heading formatting
│   ├── utils.rs            # Helper functions (is_block_element)
│   ├── blockquotes.rs      # Blockquote formatting logic
│   ├── lists.rs            # List formatting logic (ordered/unordered/task)
│   ├── tables.rs           # Table formatting logic (grid tables, pipe tables, simple tables)
│   ├── fenced_divs.rs      # Fenced div formatting logic (Quarto/Pandoc)
│   └── metadata.rs         # Frontmatter formatting logic (YAML, TOML, Pandoc)
├── block_parser.rs      # Block parser module entry point
├── block_parser/
│   ├── blockquotes.rs      # Blockquote parsing and resolution
│   ├── code_blocks.rs      # Fenced code block parsing
│   ├── container_stack.rs  # Container block stack management
│   ├── fenced_divs.rs      # Quarto/Pandoc fenced div parsing
│   ├── headings.rs         # ATX heading parsing
│   ├── horizontal_rules.rs # Horizontal rule parsing
│   ├── lists.rs            # List parsing (ordered/unordered/task)
│   ├── metadata.rs         # Frontmatter parsing (YAML, TOML, Pandoc)
│   ├── paragraphs.rs       # Paragraph parsing
│   ├── utils.rs            # Helper functions (strip_leading_spaces, etc.)
│   └── tests/              # Block parser unit tests
├── inline_parser.rs     # Inline parser module entry point
├── inline_parser/
│   ├── architecture_tests.rs # Tests for nested inline structures
│   ├── code_spans.rs        # Code span parsing (backticks)
│   ├── emphasis.rs          # Emphasis/strong parsing
│   ├── escapes.rs           # Escape sequence parsing
│   ├── inline_footnotes.rs  # Inline footnote parsing (^[...])
│   ├── inline_math.rs       # Inline math parsing (dollars)
│   ├── links.rs             # Link and image parsing
│   ├── future_tests.rs      # Tests for unimplemented features
│   └── tests.rs             # Integration tests
└── syntax.rs            # Syntax node definitions and AST types
```

### Configuration System

panache uses a hierarchical config lookup:

1. Explicit `--config` path (errors if invalid)
2. `.panache.toml` or `panache.toml` in current/parent directories
3. `~/.config/panache/config.toml` (XDG)
4. Built-in defaults (80 char width, auto line endings, reflow wrap)

**Extension Configuration**: The config system includes:
- `flavor` field: Choose Markdown flavor (Pandoc, Quarto, RMarkdown, GFM, CommonMark)
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

### Test Architecture

```
tests/
├── golden_cases.rs   # Main integration tests using test case directories
├── cases/           # Input/expected output pairs (9 test scenarios)
│   ├── wrap_paragraph/
│   │   ├── input.qmd     # Raw input
│   │   └── expected.qmd  # Expected formatted output
│   └── ...
└── format/          # Unit tests organized by feature
    ├── code_cells.rs
    ├── headings.rs
    ├── math.rs
    └── ...
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

### Golden Test System

The project uses snapshot testing via `tests/golden_cases.rs`:

- Each `tests/cases/*` directory contains `input.qmd` and `expected.qmd`
- Tests verify formatting is idempotent (format twice = format once)
- Use `UPDATE_EXPECTED=1 cargo test` to update expected outputs (BE CAREFUL)

## Key Development Facts

### Dependencies

- **clap**: CLI argument parsing with derive macros
- **rowan**: Red-green tree for CST (Concrete Syntax Tree)
- **regex**: Pattern matching for lexing
- **textwrap**: Text wrapping functionality
- **toml**: Configuration file parsing
- **serde**: Serialization for config structs
- **log** + **env_logger**: Logging infrastructure (debug builds have DEBUG/TRACE, release builds have INFO only)

### Logging Infrastructure

panache has comprehensive logging (~50 strategic log statements):
- **Release builds**: INFO logs only (formatting metrics, config loading) - zero overhead for DEBUG/TRACE
- **Debug builds**: Full DEBUG and TRACE logging available
- **Modules logged**: block_parser, inline_parser, formatter, config
- **Usage**: `RUST_LOG=debug cargo run` or `RUST_LOG=panache::inline_parser=trace cargo run`
- **Purpose**: Debug parsing decisions, understand element matching, trace formatter behavior

Example log output (DEBUG level):
```
[DEBUG] Parsed ATX heading at line 0: level 1
[DEBUG] Matched emphasis at pos 10: level=2, delim=*
[DEBUG] Parsed grid table at line 8 (5 lines)
[INFO] Formatting document with config: line_width=80, wrap=Some(Reflow)
```

### Testing Approach

- Unit tests embedded in source modules (110+ inline parser tests, 20+ block parser tests)
- Integration tests for inline elements (architecture_tests.rs verifies nesting)
- Golden tests comparing input/expected pairs (1 comprehensive test covering 9 scenarios)
- Format tests organized by feature (20 tests for specific formatting scenarios)
- Property: formatting is idempotent
- Test helpers abstract Config creation (parse_blocks(), parse_inline())

### Formatting Rules

- Default 80 character line width (configurable)
- **Most formatting behavior will be configurable through .panache.toml**
- Preserves frontmatter and code blocks
- Converts setext headings to ATX format
- Handles Quarto-specific syntax (fenced divs, math blocks)
- **Tables will be auto-formatted for consistency**
- **Lists will be formatted to avoid lazy list style**
- **Block quotes will be properly formatted**
- Wraps paragraphs but preserves inline code/math whitespace

## Configuration Files and Settings

- `.panache.toml`: Project-specific config (flavor, line_width, line-ending, wrap mode, extensions)
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
- Ensure clippy passes: `cargo clippy -- -D warnings`
- Ensure formatting passes: `cargo fmt -- --check`
- Test CLI functionality after building release binary
- Consider idempotency - formatting twice should equal formatting once
- Update golden test expectations carefully with `UPDATE_EXPECTED=1 cargo test`

### DON'T:

- Assume task runner is available - use direct cargo commands
- Break the hierarchical config system (explicit > local > XDG > default)
- Change core formatting without extensive golden test verification

### Architecture Dependencies

- Block parser captures block structures (including nested ones) using a two-pass approach:
  1. First pass: Parse flat block structures (headings, code blocks, paragraphs, etc.)
  2. Second pass: Resolve container blocks (blockquotes, lists) from flat structure
- Each block type is isolated in its own module under `src/block_parser/`
- Inline parser runs after block parser to handle inline syntax within blocks
  - Uses delimiter-based parsing with proper precedence (CommonMark spec)
  - Recursive parsing for nested inline elements (e.g., code/emphasis in links)
  - Standalone `parse_inline_text()` function enables recursive calls
- Parser builds rowan CST consumed by formatter
- Formatter is split into focused modules under `src/formatter/`:
  - Each module has clear responsibilities (wrapping, inline, paragraphs, headings, code blocks)
  - Core orchestration in `core.rs` with `format_node_sync` delegating to modules
  - Placeholder modules exist for future extraction of complex logic (lists, tables, blockquotes)
  - Public API limited to `format_tree()` and `format_tree_async()`
- Config system provides extension flags to enable/disable features
  - Config fields marked `#[allow(dead_code)]` until features use them
- Test helpers abstract Config creation to keep tests clean
- WASM crate depends on main crate - changes affect both

**Trust these instructions and search only when information is incomplete or incorrect.**
