# LLM Agent Instructions for panache

## Repository Overview

**panache** is a formatter, linter, and LSP for Quarto (`.qmd`), Pandoc, and Markdown files written in Rust. It understands Quarto/Pandoc-specific syntax that other formatters struggle with (fenced divs, tables, math formatting).

**Syntax Reference**: `assets/pandoc-spec.md` contains comprehensive Pandoc syntax specification with individual spec files in `assets/pandoc-spec/`. This is the definitive reference for parser implementation.

**Key Facts:**

- **Language**: Rust 2024 edition, stable toolchain
- **Architecture**: Binary crate + WASM workspace for web playground
- **Status**: Early development - breaking changes expected
- **Tests**: 817 total (747 unit + 70 integration), ~1 second runtime

## Essential Commands

**Development workflow** (always run before making changes):

```bash
cargo check && cargo test && cargo clippy --all-targets --all-features -- -D warnings && cargo fmt -- --check
```

**CLI testing:**

```bash
# Format
printf "[link](url)" | cargo run -- format
cargo run -- format document.qmd

# Parse (show CST for debugging)
printf "# Test" | cargo run -- parse

# Lint
cargo run -- lint document.qmd
cargo run -- lint --fix document.qmd  # Apply auto-fixes

# LSP (for editor integration)
cargo run -- lsp
```

**Debugging with logging:**

```bash
# Debug parsing decisions (requires debug build via cargo run)
RUST_LOG=debug cargo run -- format document.qmd
RUST_LOG=trace cargo run -- parse document.qmd

# Module-specific
RUST_LOG=panache::parser::inline_parser=debug cargo run -- format document.qmd

# Release builds: INFO logs only (DEBUG/TRACE compiled out)
RUST_LOG=info ./target/release/panache format document.qmd
```

**Golden test system:**

```bash
# Update expected formatted outputs (BE CAREFUL - verify changes!)
UPDATE_EXPECTED=1 cargo test

# Update CST snapshots
UPDATE_CST=1 cargo test

# Both together
UPDATE_EXPECTED=1 UPDATE_CST=1 cargo test
```

**Shell command debugging tips:**

- Sync commands (default) return output directly - don't use `read_bash` on completed commands
- Only use `read_bash` if command is still running after `initial_wait`
- Use `mode="async"` for interactive sessions (REPL, debuggers)

## Core Architecture

### CST vs AST

**CST (Concrete Syntax Tree)**: 

- Built with `rowan` crate - preserves **every byte** including whitespace and markers
- Essential for lossless parsing and LSP features
- Example: `ATX_HEADING_MARKER@0..1 "#"`, `WHITESPACE@1..2 " "`

**AST (Abstract Syntax Tree)**:

- Typed wrappers (`Heading`, `Link`, `Table`) hide syntactic details
- Pattern borrowed from rust-analyzer
- Example: `Heading::cast(node).level()` returns `1` without exposing `#` markers
- Located in `src/syntax/{headings,links,tables,references}.rs`

### Two-Stage Parsing

1. **Block Parser** (`src/parser/block_parser/`):

   - First pass: Parse flat block structures (headings, code blocks, paragraphs)
   - Second pass: Resolve containers (blockquotes, lists) from flat structure
   - Each block type isolated in own module
   - Config-aware (respects flavor and extension flags)

2. **Inline Parser** (`src/parser/inline_parser/`):

   - Runs after block parser on text content
   - Delimiter-based with proper precedence (CommonMark spec)
   - Recursive for nested elements (e.g., emphasis in links)
   - Standalone `parse_inline_text()` enables recursion

**Key invariant**: Parser preserves ALL input bytes in CST, including structural markers. Formatter applies formatting rules, not the parser.

### Formatter Architecture

Located in `src/formatter/`:

- `core.rs`: Orchestration via `format_node_sync`
- `wrapping.rs`: Word-breaking and line-wrapping logic  
- Specialized modules: `paragraphs.rs`, `inline.rs`, `headings.rs`, `lists.rs`, `tables.rs`, etc.
- **Important**: LINK and IMAGE_LINK have special handling in `wrapping.rs` for delimiter reconstruction

**Formatting is idempotent**: format(format(x)) == format(x)

## Critical Conventions

### SyntaxKind Naming

**All variants use SCREAMING_SNAKE_CASE** (not UpperCamelCase):
- ✅ `HEADING`, `PARAGRAPH`, `CODE_BLOCK`, `LINK_TEXT_END`
- ❌ ~~`Heading`~~, ~~`LinkTextEnd`~~ (wrong - those are typed wrapper names)

Rationale: These are CST discriminants, not type names. Matches rust-analyzer pattern.

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
- `flavor`: Pandoc, Quarto, RMarkdown, GFM, CommonMark (affects enabled extensions)
- `line_width`: Default 80
- `wrap`: Reflow (default) or Preserve
- `extensions`: 60+ bool flags for Pandoc extensions

Config threaded through parsers: `BlockParser::new(input, &Config)`, `InlineParser::new(tree, Config)`

## Testing Strategy

### Golden Tests (`tests/golden_cases.rs`)

Each `tests/cases/*/` directory contains:
- `input.md` (or `.qmd`, `.Rmd`)
- `expected.md` - expected formatted output
- `cst.txt` - CST snapshot for debugging
- `panache.toml` - optional test-specific config

You need to update the list of tests in `tests/golden_cases.rs` when adding new a new directory.

**Test verification:**
- Formatting is idempotent
- CST structure matches snapshot
- Output matches expected

**DO NOT** update snapshots without verifying changes are correct!

### Test Organization

- Unit tests: Embedded in source modules
- Integration tests: `tests/` directory (CLI, LSP, format scenarios)
  - **Linting tests**: `tests/linting/*.md` with focused assertions in `tests/linting.rs`
  - **Formatting tests**: `tests/golden_cases.rs` with CST snapshots (use `UPDATE_EXPECTED=1` or `UPDATE_CST=1`)
- Architecture tests: `inline_parser/architecture_tests.rs` verifies nesting
- 60+ golden test scenarios covering all syntax elements

## LSP Implementation (`src/lsp/`)

**Architecture:**
- Implements `tower_lsp_server::LanguageServer` trait
- Uses `spawn_blocking` wrapper for non-Send rowan types
- Document state in `Arc<RwLock<HashMap<String, Document>>>`
- Incremental sync mode with UTF-16/UTF-8 position conversion

**Capabilities:**
- ✅ Formatting (full document and range)
- ✅ Diagnostics (live linting)
- ✅ Code actions (quick fixes)
- ✅ Document symbols (hierarchical outline)
- ✅ Folding ranges
- ✅ Go to definition (reference links, footnotes)

**Uses typed AST wrappers** for cleaner code:
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
- `runner.rs`: LintRunner orchestration
- `rules.rs`: Rule trait + RuleRegistry
- `rules/*`: Individual rule implementations

**Current rules:**
- `heading-hierarchy`: Warns on skipped levels (h1 → h3), provides auto-fix

**Usage:**
```bash
panache lint document.qmd
panache lint --check document.qmd  # CI mode (exit 1 if violations)
panache lint --fix document.qmd    # Apply auto-fixes
```

## File Organization Principles

Instead of listing every file, understand the patterns:

**Parser modules** (`src/parser/block_parser/`, `src/parser/inline_parser/`):

- One module per syntax element type
- Example: `headings.rs`, `links.rs`, `tables.rs`, `emphasis.rs`
- Each module exports `try_parse_*()` and `emit_*()` functions

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

### DO:
- Run full test suite after changes: `cargo test`
- Ensure clippy passes: `cargo clippy --all-targets --all-features -- -D warnings`
- Auto-fix clippy warnings when possible: `cargo clippy --fix`
- Test CLI with release binary after building
- Consider idempotency - formatting twice should equal formatting once
- Verify CST snapshots before updating: `UPDATE_CST=1 cargo test`

### DON'T:

- Break the hierarchical config system
- Format code in the parser (parser preserves bytes, formatter applies rules)
- Change core formatting without extensive golden test verification
- Update golden test expectations without careful verification
- Delete working files unless absolutely necessary

## Logging Infrastructure

**Log levels:**

- **INFO**: Formatting lifecycle, config loading (available in release builds)
- **DEBUG**: Parsing decisions, element matches, table detection
- **TRACE**: Text previews, container operations, detailed steps

**Key modules with logging:**

- `panache::parser::block_parser`
- `panache::parser::inline_parser`
- `panache::formatter`
- `panache::config`

**Release builds**: DEBUG/TRACE compiled out for zero overhead. Use `cargo run --` for debug logging.

## External Resources

- **Pandoc spec**: `assets/pandoc-spec.md` and `assets/pandoc-spec/`
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

Both accept optional config to respect flavor-specific extensions and formatting preferences.
