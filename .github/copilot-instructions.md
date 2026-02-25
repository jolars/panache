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
- **Tests**: 817 total (747 unit + 70 integration), ~1 second runtime

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
RUST_LOG=panache::parser::inline_parser=debug cargo run -- format document.qmd

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

**Key invariant**: Parser preserves ALL input bytes in CST, including structural
markers. Formatter applies formatting rules, not the parser.

### Long-term refactor plan: inline parsing during block parsing (Pandoc-style)

We have decided to incrementally move from the current “block parse → inline
pass” pipeline to a Pandoc-like approach where blocks emit inline structure
directly.

#### Goals

- Preserve losslessness and stable `SyntaxKind` structure (formatter/LSP depend
  on it).
- Avoid backtracking on `GreenNodeBuilder` by using “detect/collect first, emit
  once.”
- Keep changes incremental: maintain an A/B-able path until golden tests and
  formatter are updated.

#### Incremental phases

1. **Shared inline emission helper**
   - Introduce a small helper (e.g. `emit_inlines(builder, text, config)`) that
     calls the existing inline parsing core.
   - Block modules should call this helper instead of emitting a single `TEXT`
     token where appropriate.

2. **Convert low-risk blocks first (stringy content)**
   - Headings (`HEADING_CONTENT`), table captions/cells, definition list terms,
     figure captions, etc.
   - These already have the relevant text slice available at emission time.

3. **Convert paragraphs/plain via buffering-at-close**
   - While scanning lines, accumulate raw paragraph/plain bytes (including
     newlines) in the block parser container state.
   - When the paragraph/plain closes, emit `PARAGRAPH`/`PLAIN` and run inline
     parsing once on the buffered chunk.
   - This supports multi-line inline constructs (e.g. display math) without
     needing rollback.

4. **Transition plumbing and compatibility**
   - Add a temporary flag/config or internal switch to skip the separate
     `InlineParser` pass when blocks already emit inline structure.
   - Keep both paths temporarily to A/B in tests and isolate formatter/linter
     assumptions.

5. **Finalize and delete the legacy pass**
   - Once golden cases, formatter, and linter all match expectations, remove the
     separate inline traversal.
   - Prefer small, feature-by-feature migrations over snapshot-wide updates.

### Formatter Architecture

Located in `src/formatter/`:

- `core.rs`: Orchestration via `format_node_sync`
- `wrapping.rs`: Word-breaking and line-wrapping logic
- Specialized modules: `paragraphs.rs`, `inline.rs`, `headings.rs`, `lists.rs`,
  `tables.rs`, etc.
- **Important**: LINK and IMAGE_LINK have special handling in `wrapping.rs` for
  delimiter reconstruction

**Formatting is idempotent**: format(format(x)) == format(x)

## Critical Conventions

### SyntaxKind Naming

**All variants use SCREAMING_SNAKE_CASE** (not UpperCamelCase): - ✅ `HEADING`,
`PARAGRAPH`, `CODE_BLOCK`, `LINK_TEXT_END` - ❌ ~~`Heading`~~, ~~`LinkTextEnd`~~
(wrong - those are typed wrapper names)

Rationale: These are CST discriminants, not type names. Matches rust-analyzer
pattern.

### Module Structure

- Modern Rust: `module.rs` instead of `module/mod.rs`
- Each feature in own file under parent module
- Re-export public API through parent module

### Configuration System

Hierarchical lookup: 1. Explicit `--config` path (errors if invalid)
2. `.panache.toml` or `panache.toml` in current/parent directories 3.
`~/.config/panache/config.toml` (XDG) 4. Built-in defaults

**Key settings:** - `flavor`: Pandoc, Quarto, RMarkdown, GFM, CommonMark
(affects enabled extensions) - `line_width`: Default 80 - `wrap`: Reflow
(default) or Preserve - `extensions`: 60+ bool flags for Pandoc extensions

Config threaded through parsers: `BlockParser::new(input, &Config)`,
`InlineParser::new(tree, Config)`

## Testing Strategy

- Unit tests: Embedded in source modules
- Integration tests: `tests/` directory (CLI, LSP, format scenarios)
  - **Linting tests**: `tests/linting/*.md` with focused assertions in
    `tests/linting.rs`
  - **Formatting tests**: `tests/golden_cases.rs` with CST snapshots (use
    `UPDATE_EXPECTED=1` or `UPDATE_CST=1`)
- Architecture tests: `inline_parser/architecture_tests.rs` verifies nesting
- 60+ golden test scenarios covering all syntax elements

### Golden Tests (`tests/golden_cases.rs`)

Each `tests/cases/*/` directory contains: - `input.md` (or `.qmd`, `.Rmd`)
- `expected.md` - expected formatted output - `cst.txt` - CST snapshot for
debugging - `panache.toml` - optional test-specific config

You need to update the list of tests in `tests/golden_cases.rs` when adding new
a new directory.

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

**Components:** - `diagnostics.rs`: Core types (Diagnostic, Severity, Fix, Edit)
- `runner.rs`: LintRunner orchestration - `rules.rs`: Rule trait + RuleRegistry
- `rules/*`: Individual rule implementations

**Current rules:** - `heading-hierarchy`: Warns on skipped levels (h1 → h3),
provides auto-fix

**Usage:** ```bash panache lint document.qmd panache lint --check document.qmd
# CI mode (exit 1 if violations) panache lint --fix document.qmd # Apply
auto-fixes ```

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

### DO

- Run full test suite after changes: `cargo test`
- Ensure clippy passes:
  `cargo clippy --all-targets --all-features -- -D warnings`
- Auto-fix clippy warnings when possible: `cargo clippy --fix`
- Test CLI with release binary after building
- Consider idempotency - formatting twice should equal formatting once
- Verify CST snapshots before updating: `UPDATE_CST=1 cargo test`

### DON'T

- Break the hierarchical config system
- Format code in the parser (parser preserves bytes, formatter applies rules)
- Change core formatting without extensive golden test verification
- Update golden test expectations without careful verification
- Delete working files unless absolutely necessary
- Run `cargo format`/`panache format` directly on files just to check formatting
  - **IT FORMATS IN PLACE**. Use `cargo  format < file.md` instead.

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
