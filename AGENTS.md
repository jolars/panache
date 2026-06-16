# LLM Agent Instructions for Panache

## Repository Overview

Panache is a formatter, linter, and LSP for Quarto (`.qmd`), Pandoc, and
Markdown files written in Rust. It understands Quarto/Pandoc-specific syntax
that other formatters struggle with (fenced divs, tables, math formatting).

**Syntax Reference**: `assets/pandoc-spec.md` contains comprehensive Pandoc
syntax specification with individual spec files in `assets/pandoc-spec/`. This
is the definitive reference for parser implementation.

### Key Facts

- **Language**: Rust 2024 edition, stable toolchain
- **Architecture**: Binary crate + WASM workspace for web playground
- **Status**: Early development
- **Tests**: 1,200+ total (846+ unit + 169 integration + others),
- **Parser**: Integrated inline parsing (single-pass, Pandoc-style) - only mode

### Principles

- Test-driven development: if you find a bug, write a test that reproduces it
  before fixing. If you want to add a new feature, write a test first.
- Pandoc parser is the gold standard - if in doubt, see how Pandoc handles it.
- Parsing failures take priority over formatting issues - the parser must be
  robust and lossless.
- Preserve single-pass parsing when adding features. If a classification depends
  on surrounding state, plumb that state into detection as context rather than
  re-classifying or post-processing the parsed result. Two passes is a
  regression, not a refactor.

## Essential Commands

**Development workflow** (always run before making changes):

```bash
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt -- --check
```

**CLI testing:**

```bash
# Format in place
cargo run -- format document.qmd

# Format from file to stdout
cargo run -- format < document.qmd

# Format from stdin to stdout
cat document.qmd | cargo run -- format 

# Run parser+formatter debug checks (preferred over --verify flags)
cargo run -- debug format --checks all document.qmd

# Only run losslessness check
cargo run -- debug format --checks losslessness document.qmd

# Parse (show CST for debugging)
printf "# Test" | cargo run -- parse

# Targeted formatter golden case
cargo test --test golden_cases <case_name>

# Targeted parser CST/losslessness case
cargo test -p panache-parser --test golden_parser_cases <case_name>

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

**Disk lint cache trap (`~/.cache/panache/`):**

When debugging linter changes (rules, salsa indexers, anchor resolution), the
CLI reads cached lint output keyed on a tool fingerprint that does NOT
invalidate on every code change. Symptoms: unit tests for the rule pass,
`cargo build` succeeds, but `panache lint` keeps emitting the OLD diagnostic and
`eprintln!` from your changed code never fires.

Fix: `rm -rf ~/.cache/panache/` before re-running the CLI, set `cache = false`
in `panache.toml`, or pass `--no-cache` (env `PANACHE_NO_CACHE=true`). The
repo's own `panache.toml` already sets `cache = false` for this reason. Always
validate the rule via unit tests first; treat CLI diagnostics as a downstream
sanity check, not the primary signal.

## Core Architecture

### CST vs AST

**CST (Concrete Syntax Tree)**:

- Built with `rowan` crate - must preserve **every byte**, including whitespace
  and markers
- Essential for lossless parsing and LSP features
- Example: `ATX_HEADING_MARKER@0..1 "#"`, `WHITESPACE@1..2 " "`

**AST (Abstract Syntax Tree)**:

- Typed wrappers (`Heading`, `Link`, `Table`) hide syntactic details
- Pattern borrowed from rust-analyzer
- Example: `Heading::cast(node).level()` returns `1` without exposing `#`
  markers
- Located in
  `crates/panache-parser/src/syntax/{headings,links,tables,references}.rs`

### Single-Pass Parsing Architecture

**Parser** (`crates/panache-parser/src/parser/`):

1. **Parser** (`crates/panache-parser/src/parser/core.rs`):
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

3. **Block Dispatcher**
   (`crates/panache-parser/src/parser/block_dispatcher.rs`):
   - Centralized block detection via `detect_prepared()` and emission via
     `parse_prepared()` (legacy `can_parse/parse` removed)
   - Carries prepared payloads (e.g., blockquote markers, list metadata) to
     avoid re-parsing during emission
   - Parser core still owns continuation rules, blank-line handling, and list
     item buffering

**Key invariant**: Parser preserves ALL input bytes in CST, including structural
markers. Formatter applies formatting rules, not the parser.

### Formatter Architecture

Formatter core now lives in `crates/panache-formatter/`:

- `crates/panache-formatter/src/formatter/`: Core formatting implementation
  (paragraphs, inline, headings, lists, tables, etc.)
- `crates/panache-formatter/src/formatter.rs`: Core orchestration and YAML
  frontmatter formatting integration
- `crates/panache-formatter/src/config.rs`: Formatter-local config surface
  (dependency-lean, no config file parsing concerns)

Host/runtime integrations remain in top-level `panache` crate:

- `src/formatter.rs`: host-facing bridge used by CLI/LSP/public API
- external formatter process execution and runtime orchestration remain in
  top-level modules
- top-level `lsp` feature gating is intentional and should stay there, not in
  `crates/panache-formatter`

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
2. `panache.toml` or `.panache.toml` in current/parent directories
3. `~/.config/panache/config.toml` (XDG)
4. Built-in defaults

**Key settings:**

- `flavor`: Pandoc, Quarto, RMarkdown, GFM, CommonMark
- `line-width`: Default 80 - `wrap`: reflow (default), preserve, or sentence.
- `extensions`: many bool flags for Pandoc extensions
- `[format]` section for formatting preferences
- `[lint]` section for linter rule configuration
- `[formatters]` section for external formatter integration (e.g., black for
  python code blocks)
- `[linters]` section for external linter integration (e.g., flake8 for python
  code blocks)

Config threaded through parsers: `Parser::new(input, &Config)`

Use kebab-case for config keys (e.g., `line-width`, not `line_width`).

## Testing Strategy

- Unit tests: Embedded in source modules
- Integration tests: `tests/` directory (CLI, LSP, format scenarios)
  - **Linting tests**: `tests/linting/*.md` with focused assertions in
    `tests/linting.rs`
  - **Formatting tests**: `tests/golden_cases.rs` with fixture-based expected
    output assertions (use `UPDATE_EXPECTED=1`)
- Parser crate integration tests: `crates/panache-parser/tests/`
  - **Parser golden tests**: `golden_parser_cases.rs` validates losslessness +
    CST snapshots (`insta`) from `crates/panache-parser/tests/fixtures/cases/*/`
  - **YAML tests**: `yaml.rs` uses fixture-driven parity checks against
    `crates/panache-parser/tests/fixtures/yaml-test-suite/`
- Architecture tests: `blocks/tests/` verifies block parsing behavior
- 98 golden test scenarios covering all syntax elements

### Golden Tests (`tests/golden_cases.rs`)

Each `tests/fixtures/cases/*/` directory contains:

- `input.md` (or `.qmd`, `.Rmd`)
- `expected.md`: expected formatted output
- `panache.toml`: optional test-specific config

You need to update the list of tests in `tests/golden_cases.rs` when adding a
new directory.

To update expected outputs, set environment variable:

```bash
# Update expected formatted outputs (BE CAREFUL - verify changes!)
UPDATE_EXPECTED=1 cargo test

# Update expected formatted outputs
UPDATE_EXPECTED=1 cargo test
```

But be *very careful* when updating expected outputs - always verify diffs are
correct before committing!

### Parser Golden Tests (`crates/panache-parser/tests/golden_parser_cases.rs`)

Each `crates/panache-parser/tests/fixtures/cases/*/` directory contains:

- `input.md` (or `.qmd`, `.Rmd`)
- `parser-options.toml`: optional parser-only options for the case

CST expectations are reviewed via `insta` snapshots under
`crates/panache-parser/tests/snapshots/`.

### Test verification

- Formatting is idempotent
- Output matches expected

## LSP Implementation (`src/lsp/`)

### Architecture

- Implements `tower_lsp_server::LanguageServer` trait
- Uses `tokio::sync::Mutex`-guarded shared state (`Arc<Mutex<...>>`) for
  `document_map`, `workspace_root`, and `salsa_db`
- Per-document state is represented by `DocumentState` (path, salsa inputs, CST
  `GreenNode`)
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
panache lint --fix document.qmd   # Apply auto-fixes
```

## File Organization Principles

Instead of listing every file, understand the patterns:

**Parser modules** (`crates/panache-parser/src/parser/`):

- `core.rs`: Main `Parser` struct and orchestration logic
- `blocks/`: Block-level constructs (headings, tables, lists, paragraphs, etc.)
  - One module per syntax element type
  - Each module exports `try_parse_*()` and `emit_*()` functions
- `inlines/`: Inline-level constructs (emphasis, links, code spans, math, etc.)
  - `core.rs`: Main inline parsing entry point
  - Recursive parsing for nested inline elements
- `utils/`: Shared utilities (attributes, text buffers, inline emission helpers)
- `list_postprocessor.rs`: Post-processing for list item content wrapping

**Formatter modules** (`crates/panache-formatter/src/formatter/`):

- Split by concern: wrapping, inline, paragraphs, headings, lists, tables, etc.
- Core orchestration in `crates/panache-formatter/src/formatter.rs`
- Keep formatter-core logic in this crate; keep host-only process/runtime paths
  in top-level `src/` modules

**Syntax modules** (`crates/panache-parser/src/syntax/`):

- `kind.rs`: SyntaxKind enum
- `ast.rs`: AstNode trait
- Typed wrappers: `{headings,links,tables,references}.rs`

**Top-level tests** (`tests/`):

- `fixtures/cases/*/`: Formatter golden scenarios + `golden_cases.rs` runner.
  Lives at top-level (rather than in the formatter crate) because each case's
  optional `panache.toml` uses the host config schema (sub-tables like
  `[format]`, `[code-blocks]`, `[formatters.python]`) which the dependency-lean
  formatter `Config` doesn't parse on its own.
- `cli/`: CLI integration tests
- `lsp/` + `lsp.rs`: LSP integration tests
- `linting/` + `linting.rs`: Linter integration tests
- `external_formatters.rs`, `external_linters.rs`: Tests that exercise external
  tool execution (shfmt etc.) --- kept here because the formatter crate doesn't
  invoke external tools; that's a host concern.
- The YAML test suite uses vendored fixtures under
  `crates/panache-parser/tests/fixtures/yaml-test-suite/` refreshed via
  `scripts/update-yaml-test-suite-fixtures.sh`; allowlisted coverage is tracked
  by `crates/panache-parser/tests/yaml/allowlist.txt`.
- The in-tree YAML parser (`crates/panache-parser/src/parser/yaml.rs`) is the
  production YAML parser. It retired the external `yaml_parser` crate; its
  lossless CST is embedded into the host document tree for frontmatter and
  hashpipe option bodies, and it backs the in-tree YAML formatter
  (`crates/panache-formatter/src/formatter/yaml/`). `pretty_yaml` survives only
  as a temporary dev-only cross-validation oracle (revisit \~2026-09).

**Parser-crate tests** (`crates/panache-parser/tests/`):

- `fixtures/cases/*/` + `golden_parser_cases.rs`: parser golden scenarios
  (losslessness + CST snapshots), separate from formatter golden cases.

**Formatter-crate tests** (`crates/panache-formatter/tests/`):

- `format/`: Feature-specific formatter unit tests (programmatic Config, no TOML
  parsing) --- wire new modules into `format/main.rs`.

**Editor extension** (`editors/code/`):

- VS Code extension that launches `panache lsp`
- Includes binary download/install logic and extension settings wiring
- Build/package via `npm run compile` and `npm run package` in `editors/code/`

## Important Development Rules

### DO

- Run full test suite after changes: `cargo test`
- Add tests for bugs before fixing them (test-driven development)
- Ensure clippy passes:
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- Auto-fix clippy warnings when possible: `cargo clippy --workspace --fix`
- Consider idempotency - formatting twice should equal formatting once
- Verify CST snapshots and expected outputs before updating them.
- Update the docs in `docs/` when adding features or changing behavior.

### DON'T

- Fix a bug without a test that reproduces that bug first
- Format code in the parser (parser preserves bytes, formatter applies rules)
- Change core formatting without extensive golden test verification
- Update golden test expectations without careful verification
- Delete working files unless absolutely necessary
- Run `cargo format`/`panache format` directly on files just to check formatting
  **IT FORMATS IN PLACE**. Use `cargo  format < file.md` instead.
- Update `CHANGELOG.md`. It is handled automatically by versionary.

## Commits

Use [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/):
`<type>(<scope>): <subject>`. versionary reads the type to decide the next
version bump, so picking the right one matters.

**Types**: `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `chore`, `ci`,
`build`, `style`. Use `feat!` or include `BREAKING CHANGE:` in the body for
breaking changes.

**Scopes** (most common, derived from the workspace layout): `parser`,
`formatter`, `linter`, `lsp`, `cli`, `wasm`, `deps`. Omit the scope only when a
change genuinely spans the whole workspace.

**Subject style**:

- Imperative mood, lowercase, no trailing period (`lift trailing text`, not
  `Lifts trailing text.`). Imperative isn't required by the CC spec, but it's
  the broader git convention and matches existing log.
- Wrap inline code, flags, identifiers, and SyntaxKinds in backticks
  (``add `{lang}` placeholder``, `` retag `HTML_BLOCK_DIV` ``).
- Aim for ≤50 chars; 72 is the hard cap (the `<type>(<scope>):` prefix eats
  10--15 chars, so dependabot-style subjects sometimes spill past 50). Push
  detail into the body.

**Body** (when warranted):

- Keep it short and to the point. One paragraph or a single sentence is enough.
  If the subject says it all, skip the body entirely.
- Explain the *why* and the user-visible effect---the diff already shows the
  *what*.
- Reference the failure mode or pandoc-native shape being fixed when it isn't
  obvious from the subject.
- For pandoc-conformance work, include the pass-rate delta (e.g.
  `html 226 → 235 (+9), total 419 → 428`) so the recap matches what landed.
- Wrap at \~72 chars; pass via heredoc to preserve formatting.

**Don't**:

- Push commits or open pull requests unless the user explicitly asks. Local
  commits are fine; anything that publishes to the remote (`git push`,
  `gh pr create`, `gh pr merge`) needs explicit authorization for that specific
  action.
- Skip hooks (`--no-verify`) --- pre-commit runs `rustfmt` and `panache format`.
  Fix the underlying issue if a hook fails.
- Amend a published commit; create a new one.
- (See "DON'T" above for `CHANGELOG.md` --- versionary owns it.)

## Release Management

**Asset hygiene invariant**: Only the primary CLI release stream (`v*` tags on
`jolars/panache`) may carry GitHub release assets. Every other tag stream the
monorepo produces --- `panache-parser-v*`, `panache-formatter-v*`,
`panache-code-v*`, `panache-zed-v*` --- **must not** upload assets.

The Zed extension resolves its binary download with
`zed::latest_github_release("jolars/panache", { require_assets: true })`, which
returns the newest release that *has assets* and cannot filter by tag prefix.
Any extra asset-bearing release shadows the CLI stream, so the extension grabs
the wrong release and fails to find a platform binary. See
`editors/zed/src/lib.rs`.

For this reason the dprint Wasm plugin no longer lives here: it was relocated to
`jolars/dprint-plugin-panache`, where it uploads `panache.wasm` on its own
release stream and depends on the published `panache-formatter` crate. Do not
reintroduce an asset-uploading release workflow into this repo for anything but
the CLI.

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
- **Documentation site**: `docs/` (Quarto-based, published to GitHub Pages)
- **Playground**: `docs/playground/` (WASM-based web interface)
- **WASM crate**: `crates/panache-wasm/` for browser integration
- **VS Code extension**: `editors/code/` (publishes editor integration that runs
  `panache lsp`)

## Public API (`src/lib.rs`)

```rust
// Format a document
pub fn format(input: &str, config: Option<Config>) -> String

// Parse to CST (for inspection/debugging)
pub fn parse(input: &str, config: Option<Config>) -> SyntaxNode
```

Both accept optional config to respect flavor-specific extensions and formatting
preferences.
