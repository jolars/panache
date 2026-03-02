# LLM Agent Instructions for panache

## Repository Overview

**panache** is a formatter, linter, and LSP for Quarto (`.qmd`), Pandoc, and
Markdown files written in Rust. It understands Quarto/Pandoc-specific syntax
that other formatters struggle with (fenced divs, tables, math formatting).

**Syntax Reference**: `assets/pandoc-spec.md` contains comprehensive Pandoc
syntax specification with individual spec files in `assets/pandoc-spec/`. This
is the definitive reference for parser implementation.

### Key Facts

- **Language**: Rust 2024 edition, stable toolchain
- **Architecture**: Binary crate + WASM workspace for web playground
- **Status**: Early development - breaking changes expected
- **Tests**: 1,200+ total (846+ unit + 169 integration + others),
- **Parser**: Integrated inline parsing (single-pass, Pandoc-style) - only mode

### Principles

- Test-driven development: if you find a bug, write a test that reproduces it
  before fixing. If you want to add a new feature, write a test first.
- Pandoc parser is the gold standard - if in doubt, see how Pandoc handles it.
- Parsing failures take priority over formatting issues - the parser must be
  robust and lossless.

## Essential Commands

**Development workflow** (always run before making changes):

```bash
cargo check && cargo test && cargo clippy --all-targets --all-features -- -D warnings && cargo fmt -- --check
```

**CLI testing:**

```bash
# Format in place
cargo run -- format document.qmd

# Format from file to stdout
cargo run -- format < document.qmd

# Format from stdin to stdout
cat document.qmd | cargo run -- format 

# Parse (show CST for debugging)
printf "# Test" | cargo run -- parse

# Targeted golden case
cargo test --test golden_cases <case_name>

# Lint
cargo run -- lint document.qmd
cargo run -- lint --fix document.qmd  # Apply auto-fixes

# LSP (for editor integration)
cargo run -- lsp
```

## Debugging with logging

```bash
# Debug parsing decisions (requires debug build via cargo run)
RUST_LOG=debug cargo run -- format document.qmd
RUST_LOG=trace cargo run -- parse document.qmd

# Module-specific
RUST_LOG=panache::parser::blocks=debug cargo run -- format document.qmd
RUST_LOG=panache::parser::inlines=debug cargo run -- format document.qmd

# Release builds: INFO logs only (DEBUG/TRACE compiled out)
RUST_LOG=info ./target/release/panache format document.qmd
```

**Shell command debugging tips:**

- Sync commands (default) return output directly - don't use `read_bash` on
  completed commands
- Only use `read_bash` if command is still running after `initial_wait`
- Use `mode="async"` for interactive sessions (REPL, debuggers)

## Core Architecture

### CST vs AST

**CST (Concrete Syntax Tree)**:

- Built with `rowan` crate - preserves **every byte** including whitespace and
  markers
- Essential for lossless parsing and LSP features
- Example: `ATX_HEADING_MARKER@0..1 "#"`, `WHITESPACE@1..2 " "`

**AST (Abstract Syntax Tree)**:

- Typed wrappers (`Heading`, `Link`, `Table`) hide syntactic details
- Pattern borrowed from rust-analyzer
- Example: `Heading::cast(node).level()` returns `1` without exposing `#`
  markers
- Located in `src/syntax/{headings,links,tables,references}.rs`

### Single-Pass Parsing Architecture

**Parser** (`src/parser/`):

1. **Parser** (`src/parser/core.rs`):
   - Main parsing implementation in `Parser` struct
   - Parses block structures (headings, code blocks, paragraphs, tables, lists,
     etc.) in a single forward pass
   - Emits inline structure during block parsing (single-pass, Pandoc-style)
   - Each block type isolated in `blocks/` directory
   - Config-aware (respects flavor and extension flags)

2. **Inline Parsing** (integrated, not a separate pass):
   - Inline elements parsed during block parsing via `inlines/core.rs`
   - Delimiter-based with proper precedence (CommonMark spec)
   - Recursive for nested elements (e.g., emphasis in links)
   - Available in `inlines/` directory for specific constructs

3. **Block Dispatcher** (`src/parser/block_dispatcher.rs`):
   - Centralized block detection via `detect_prepared()` and emission via
     `parse_prepared()` (legacy `can_parse/parse` removed)
   - Carries prepared payloads (e.g., blockquote markers, list metadata) to
     avoid re-parsing during emission
   - Parser core still owns continuation rules, blank-line handling, and list
     item buffering

**Key invariant**: Parser preserves ALL input bytes in CST, including structural
markers. Formatter applies formatting rules, not the parser.

### Formatter Architecture

Located in `src/formatter/`:

- `core.rs`: Orchestration via `format_node_sync`
- `wrapping.rs`: Word-breaking and line-wrapping logic
- Specialized modules: `paragraphs.rs`, `inline.rs`, `headings.rs`, `lists.rs`,
  `tables.rs`, etc.
- **Important**: LINK and IMAGE_LINK have special handling in `wrapping.rs` for
  delimiter reconstruction

**Formatting must be idempotent**: format(format(x)) == format(x)

## Critical Conventions

### SyntaxKind Naming

**Variants use SCREAMING_SNAKE_CASE** (not UpperCamelCase):

- ✅ `HEADING`, `PARAGRAPH`, `CODE_BLOCK`, `LINK_TEXT_END`
- ❌ ~~`Heading`~~, ~~`LinkTextEnd`~~ (wrong - those are typed wrapper names)

### Module Structure

- Modern Rust: `module.rs` instead of `module/mod.rs`
- Each feature in own file under parent module
- Re-export public API through parent module

### Configuration System

Hierarchical lookup:

1. Explicit `--config` path (errors if invalid)
2. `.panache.toml` or `panache.toml` in current/parent directories
3. `~/.config/panache/config.toml` (XDG)
4. Built-in defaults

**Key settings:**

- `flavor`: Pandoc, Quarto, RMarkdown, GFM, CommonMark (affects enabled
  extensions)
- `line-width`: Default 80 - `wrap`: Reflow (default) or Preserve
- `extensions`: 60+ bool flags for Pandoc extensions

Config threaded through parsers: `Parser::new(input, &Config)`

## Testing Strategy

- Unit tests: Embedded in source modules
- Integration tests: `tests/` directory (CLI, LSP, format scenarios)
  - **Linting tests**: `tests/linting/*.md` with focused assertions in
    `tests/linting.rs`
  - **Formatting tests**: `tests/golden_cases.rs` with CST snapshots (use
    `UPDATE_EXPECTED=1` or `UPDATE_CST=1`)
- Architecture tests: `blocks/tests/` verifies block parsing behavior
- 98 golden test scenarios covering all syntax elements

### Golden Tests (`tests/golden_cases.rs`)

Each `tests/cases/*/` directory contains:

- `input.md` (or `.qmd`, `.Rmd`)
- `expected.md`: expected formatted output
- `cst.txt` - CST snapshot for debugging
- `panache.toml`: optional test-specific config

You need to update the list of tests in `tests/golden_cases.rs` when adding a
new directory.

To update expected outputs or CST snapshots, set environment variables:

```bash
# Update expected formatted outputs (BE CAREFUL - verify changes!)
UPDATE_EXPECTED=1 cargo test

# Update CST snapshots
UPDATE_CST=1 cargo test

# Both together
UPDATE_EXPECTED=1 UPDATE_CST=1 cargo test
```

But be *very careful* when updating snapshots - always verify changes are
correct before committing!

### Test verification

- Formatting is idempotent
- CST structure matches snapshot
- Output matches expected

## LSP Implementation (`src/lsp/`)

### Architecture

- Implements `tower_lsp_server::LanguageServer` trait
- Uses `spawn_blocking` wrapper for non-Send rowan types
- Document state in `Arc<RwLock<HashMap<String, Document>>>`
- Incremental sync mode with UTF-16/UTF-8 position conversion

Uses typed AST wrappers for cleaner code:

```rust
// With wrapper (preferred in LSP)
if let Some(heading) = Heading::cast(node) {
    let text = heading.text();
    let level = heading.level();
}
```

## Linter (`src/linter/`)

**Components:**

- `diagnostics.rs`: Core types (Diagnostic, Severity, Fix, Edit)
- `runner.rs`: LintRunner orchestration - `rules.rs`: Rule trait + RuleRegistry
- `rules/*`: Individual rule implementations

**Current rules:** - `heading-hierarchy`: Warns on skipped levels (h1 → h3),
provides auto-fix

**Usage:**

```bash
panache lint document.qmd
panache lint --check document.qmd # CI mode (exit 1 if violations)
panache lint --fix document.qmd # Apply auto-fixes
```

## File Organization Principles

Instead of listing every file, understand the patterns:

**Parser modules** (`src/parser/`):

- `core.rs`: Main `Parser` struct and orchestration logic
- `blocks/`: Block-level constructs (headings, tables, lists, paragraphs, etc.)
  - One module per syntax element type
  - Each module exports `try_parse_*()` and `emit_*()` functions
- `inlines/`: Inline-level constructs (emphasis, links, code spans, math, etc.)
  - `core.rs`: Main inline parsing entry point
  - Recursive parsing for nested inline elements
- `utils/`: Shared utilities (attributes, text buffers, inline emission helpers)
- `list_postprocessor.rs`: Post-processing for list item content wrapping

**Formatter modules** (`src/formatter/`):

- Split by concern: wrapping, inline, paragraphs, headings, lists, tables, etc.
- Core orchestration in `core.rs`
- Public API limited to `format_tree()` and `format_tree_async()`

**Syntax modules** (`src/syntax/`):

- `kind.rs`: SyntaxKind enum
- `ast.rs`: AstNode trait
- Typed wrappers: `{headings,links,tables,references}.rs`

**Tests** (`tests/`):

- `cases/*/`: Golden test scenarios (use `view` to explore)
- `cli/`: CLI integration tests
- `format/`: Feature-specific unit tests

## Important Development Rules

### DO

- Run full test suite after changes: `cargo test`
- Add tests for bugs before fixing them (test-driven development)
- Ensure clippy passes:
  `cargo clippy --all-targets --all-features -- -D warnings`
- Auto-fix clippy warnings when possible: `cargo clippy --fix`
- Test CLI with release binary after building
- Consider idempotency - formatting twice should equal formatting once
- Verify CST snapshots before updating: `UPDATE_CST=1 cargo test`
- Update the docs in `docs/` when adding features or changing behavior.

### DON'T

- Break the hierarchical config system
- Format code in the parser (parser preserves bytes, formatter applies rules)
- Change core formatting without extensive golden test verification
- Update golden test expectations without careful verification
- Delete working files unless absolutely necessary
- Run `cargo format`/`panache format` directly on files just to check formatting
  **IT FORMATS IN PLACE**. Use `cargo  format < file.md` instead.
- Update `CHANGELOG.md`. It is handled automatically by semantic release.

## Logging Infrastructure

**Log levels:**

- **INFO**: Formatting lifecycle, config loading (available in release builds)
- **DEBUG**: Parsing decisions, element matches, table detection
- **TRACE**: Text previews, container operations, detailed steps

**Key modules with logging:**

- `panache::parser::blocks`
- `panache::parser::inlines`
- `panache::formatter`
- `panache::config`

**Release builds**: DEBUG/TRACE compiled out for zero overhead. Use
`cargo run --` for debug logging.

## External Resources

- **Pandoc spec**: `assets/pandoc-spec.md` and `assets/pandoc-spec/`
- **Pandoc repo submodule**: the entire Pandoc repository is added as a
  submodule at `pandoc/`. Most of the relevant syntax parsing reference files
  are in `pandoc/src/Text/Pandoc/Readers/Markdown.hs`.
- **Documentation site**: `docs/` (Quarto-based, published to GitHub Pages)
- **Playground**: `docs/playground/` (WASM-based web interface)
- **WASM crate**: `crates/panache-wasm/` for browser integration

## Public API (`src/lib.rs`)

```rust
// Format a document
pub fn format(input: &str, config: Option<Config>) -> String

// Parse to CST (for inspection/debugging)
pub fn parse(input: &str, config: Option<Config>) -> SyntaxNode
```

Both accept optional config to respect flavor-specific extensions and formatting
preferences.
