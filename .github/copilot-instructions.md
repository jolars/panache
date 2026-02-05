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
- **Size**: ~15k lines across 35+ files
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

### Timing Notes

- `cargo test`: ~1 second (75 tests)
- `cargo build --release`: ~25 seconds
- `cargo check`: ~1 second

## Project Architecture and Layout

The project is designed to first parse the document into a concrete syntax tree (CST)
using a block parser, then run an inline parser/lexer to handle inline elements. The CST is
represented using the `rowan` crate, which provides a red-green tree structure
for efficient syntax tree manipulation. The formatter then traverses this tree to apply
the formatting rules.

**IMPORTANT**: The block parser is currently in active development with support for:
- ATX headings
- Fenced code blocks (backtick and tilde)
- Paragraphs
- Blockquotes (with nesting support)
- Blank lines

The inline parser is a WIP placeholder that currently passes through block content unchanged.

### Source Structure

```
src/
├── main.rs              # CLI entry point with clap argument parsing
├── lib.rs               # Public API with format() and parse() functions
├── config.rs            # Configuration handling (.panache.toml, XDG paths)
├── formatter.rs         # Main formatting logic and AST traversal
├── block_parser.rs      # Block parser module entry point
├── block_parser/
│   ├── blockquotes.rs   # Blockquote parsing and resolution
│   ├── code_blocks.rs   # Fenced code block parsing
│   ├── headings.rs      # ATX heading parsing
│   ├── paragraphs.rs    # Paragraph parsing
│   ├── resolvers.rs     # Container block resolution (2nd pass)
│   ├── utils.rs         # Helper functions (strip_leading_spaces, etc.)
│   └── tests/           # Block parser unit tests
├── inline_parser.rs     # WIP inline parser module (placeholder)
├── inline_parser/
│   └── tests.rs         # Inline parser tests
└── syntax.rs            # Syntax node definitions and AST types
```

### Configuration System

panache uses a hierarchical config lookup:

1. Explicit `--config` path (errors if invalid)
2. `.panache.toml` or `panache.toml` in current/parent directories
3. `~/.config/panache/config.toml` (XDG)
4. Built-in defaults (80 char width, auto line endings, reflow wrap)

### Test Architecture

```
tests/
├── golden_cases.rs   # Main integration tests using test case directories
├── cases/           # Input/expected output pairs (14 test scenarios)
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

### Testing Approach

- Unit tests embedded in source modules (34 tests)
- Integration tests in `tests/format/` (39 tests)
- Golden tests comparing input/expected pairs (1 comprehensive test covering 14 scenarios)
- Property: formatting is idempotent

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

- `.panache.toml`: Project-specific config (line_width, line-ending, wrap mode)
- `.envrc`: direnv configuration for Nix environment
- `.gitignore`: Excludes target/, devenv artifacts, Nix build outputs
- `devenv.nix`: Development environment with go-task, quarto, wasm-pack

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
- Parser builds rowan CST consumed by formatter
- Config system must maintain backward compatibility
- WASM crate depends on main crate - changes affect both

**Trust these instructions and search only when information is incomplete or incorrect.**
