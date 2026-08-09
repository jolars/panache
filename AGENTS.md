# LLM Agent Instructions for Panache

Contributor-facing process (setup, PR expectations, editor debugging) lives in
`CONTRIBUTING.md`, which is published on the docs site. This file is the
agent-facing companion: architecture, invariants, file layout, and traps. Where
the two overlap (validation commands, commit conventions, release hygiene), they
must agree, so update both.

## Repository Overview

Panache is a formatter, linter, and LSP for Quarto (`.qmd`), Pandoc, and
Markdown files written in Rust. It understands Quarto/Pandoc-specific syntax
that other formatters struggle with (fenced divs, tables, math formatting).

### Key Facts

- **Language**: Rust 2024 edition. The toolchain is *pinned* by
  `rust-toolchain.toml` (currently 1.94.1); rustup installs it automatically.
- **Architecture**: Cargo workspace. The root `panache` crate is both a library
  and the `panache` binary (the binary needs the `cli` feature). Workspace
  members: `crates/panache-parser`, `crates/panache-formatter`,
  `crates/panache-wasm`. `editors/zed` is a separate crate, deliberately outside
  the workspace.
- **Tests**: \~4,800 across the workspace (unit + integration + fixture-driven
  conformance suites).

## Essential Commands

Validation gate (run before and after changes). `Taskfile.yml` wraps these ---
`task dev` is check + test + clippy; `task --list` for the rest.

```bash
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt -- --check
```

```bash
cargo run -- format document.qmd          # IN PLACE - mutates the file
cargo run -- format < document.qmd        # to stdout, file untouched
cargo run -- format --check document.qmd  # exit 1 + diff if unformatted

# Losslessness + idempotency checks (preferred over --verify flags)
cargo run -- debug format --checks all document.qmd
cargo run -- debug format --checks losslessness document.qmd

printf "# Test" | cargo run -- parse      # CST
cargo run -- parse --to pandoc-ast doc.qmd   # diff against pandoc -t native
cargo run -- parse --to pandoc-json doc.qmd

cargo run -- lint document.qmd
cargo run -- lint --fix document.qmd
cargo run -- clean --all                  # clear on-disk caches
cargo run -- lsp

# Targeted golden cases
cargo test --test golden_cases <case_name>
cargo test -p panache-parser --test golden_parser_cases <case_name>
```

## Debugging with logging

Levels: **INFO** = formatting lifecycle and config loading (kept in release
builds); **DEBUG** = parsing decisions, element matches, table detection;
**TRACE** = text previews, container operations. DEBUG/TRACE are compiled out of
release builds, so debug logging needs `cargo run --`. Modules that log:
`panache::parser::blocks`, `panache::parser::inlines`, `panache::formatter`,
`panache::config`.

```bash
RUST_LOG=debug cargo run -- format document.qmd
RUST_LOG=trace cargo run -- parse document.qmd
RUST_LOG=panache::parser::blocks=debug cargo run -- format document.qmd
RUST_LOG=info ./target/release/panache format document.qmd # INFO only
```

**Disk lint cache trap (`~/.cache/panache/`):**

When debugging linter changes (rules, salsa indexers, anchor resolution), the
CLI reads cached lint output keyed on a tool fingerprint that is only
`panache@<version>`, so it does NOT invalidate when behavior changes under a
fixed in-development version. Symptoms: unit tests for the rule pass,
`cargo build` succeeds, but `panache lint` keeps emitting the OLD diagnostic and
`eprintln!` from your changed code never fires.

Fix: run `cargo run -- clean --all` (or `rm -rf ~/.cache/panache/`) before
re-running the CLI, set `cache = false` in `panache.toml`, or pass `--no-cache`
(env `PANACHE_NO_CACHE=true`). The repo's own `panache.toml` already sets
`cache = false` for this reason. Always validate the rule via unit tests first;
treat CLI diagnostics as a downstream sanity check, not the primary signal.

## Core Architecture

### CST vs AST

The **CST** is built with `rowan` and must preserve **every byte** ---
whitespace and structural markers included (`ATX_HEADING_MARKER@0..1 "#"`,
`WHITESPACE@1..2 " "`). This is what makes parsing lossless and the LSP
possible.

The **AST** is a set of typed wrappers over that CST (rust-analyzer's pattern),
one module per construct family in `crates/panache-parser/src/syntax/`.
`Heading::cast(node).level()` returns `1` without exposing `#` markers.
Consumers (linter, LSP) should use wrappers, never raw `SyntaxKind` sequences.

**Key invariant**: the parser preserves all input bytes; the formatter applies
formatting rules. Never format in the parser.

### Single-pass parsing

`parser/core.rs` walks the document in a single forward pass, parsing block
structure and **emitting inline structure as it goes** (Pandoc-style), rather
than running a second inline pass. Block types are isolated in `blocks/`; inline
constructs in `inlines/`, entered via `inlines/core.rs` and delimiter-based with
CommonMark precedence, recursing for nested elements.

`parser/block_dispatcher.rs` centralizes detection (`detect_prepared()`) and
emission (`parse_prepared()`). `detect_prepared` may hand a payload
(`Box<dyn Any>`) to `parse_prepared` so emission never re-parses the line.
Parser core retains continuation rules, blank-line handling, and list item
buffering.

`inlines/inline_ir.rs` is the unified inline IR. **It currently backs CommonMark
only**; the Pandoc dialect is being migrated onto it incrementally. Do not
assume a construct is on the IR without checking (`pandoc-ir-migrate` skill).

### Formatter

Formatter core is `crates/panache-formatter/` (`formatter/` for the per-concern
implementation, `formatter.rs` for orchestration and YAML frontmatter,
`config.rs` for the dependency-lean config surface). The host `panache` crate
keeps `src/formatter.rs` as the CLI/LSP/public-API bridge, owns all external
process execution, and owns `lsp` feature gating --- none of that belongs in the
formatter crate, which is published and consumed by
`jolars/dprint-plugin-panache`.

**Formatting must be idempotent**: `format(format(x)) == format(x)`.

## Critical Conventions

- **SyntaxKind variants are SCREAMING_SNAKE_CASE**: `HEADING`, `CODE_BLOCK`,
  `LINK_TEXT_END`. UpperCamelCase names (`Heading`) are typed wrappers, not
  kinds.
- **Modern module style**: `module.rs`, not `module/mod.rs`; one feature per
  file; re-export the public API through the parent.
- **Config keys are kebab-case** (`line-width`, not `line_width`).

### Configuration

Lookup order: explicit `--config` (errors if invalid) -> `panache.toml` or
`.panache.toml` in current/parent dirs -> `~/.config/panache/config.toml` ->
built-in defaults. Individual keys override per-invocation with repeatable
`-o key=value`.

`panache.schema.json` is the generated schema for the whole surface and the
authoritative key list; `tests/config_schema.rs` regression-tests it. Read it
rather than enumerating keys here. The parts worth knowing up front: `flavor` is
one of `pandoc` (default), `quarto`, `rmarkdown`, `gfm`, `commonmark`,
`multimarkdown`, `mdsvex`, `myst`, with per-path `flavor-overrides`; sections
are `[format]`, `[lint]`, `[formatters]`, `[linters]`, `[experimental]`.

Threading: the host `Config` (`src/config.rs`) converts into the dependency-lean
`ParserOptions` (`crates/panache-parser/src/options.rs`) and the formatter's own
`Config`. Parsers take `Parser::new(input, &ParserOptions)`.

## Task Playbooks

Multi-step procedures live in `.claude/skills/` so they load on demand instead
of costing context every session: `add-syntax-construct`, `add-lint-rule`,
`external-tools`, `commonmark-conformance`, `html-conformance`,
`pandoc-ir-migrate`, `math-parser-formatter`, `perf-investigation`,
`linter-investigation`, `smoke-test-triage`. Load the relevant one rather than
improvising the procedure.

Three facts from those playbooks are global enough to repeat here, because
violating them damages code well outside the task at hand:

- **Block parser registry order is precedence.** The vector in
  `BlockParserRegistry::new()` is aligned with pandoc's reader order. Inserting
  a parser at the wrong slot changes parsing globally, so `commonmark.rs` and
  `pandoc.rs` are the regression signal, not a targeted golden case.
- **New syntax must be extension- or flavor-gated.** A construct that is
  unconditionally live changes behavior for `commonmark` and breaks that suite.
- **`crates/panache-parser/src/pandoc_ast.rs` is a test-only diagnostic.** When
  it diverges from pandoc, the CST is wrong. "Fixing" the divergence inside the
  projector hides the defect from the linter, LSP, and formatter, which all read
  the CST.

External formatter/linter delegation is a **host concern**:
`crates/panache-formatter` never spawns a process. See the `external-tools`
skill.

## Conformance Suites

Three vendored corpora guard parser behavior, each with an **allowlist** (a
regression there fails the build), a **blocked** list of deliberate gaps, and an
`#[ignore]`d full-report test for triage.

  | Suite                 | Harness                                     | Allowlist / blocked                        |
  | --------------------- | ------------------------------------------- | ------------------------------------------ |
  | CommonMark `spec.txt` | `crates/panache-parser/tests/commonmark.rs` | `tests/commonmark/{allowlist,blocked}.txt` |
  | Pandoc native AST     | `crates/panache-parser/tests/pandoc.rs`     | `tests/pandoc/{allowlist,blocked}.txt`     |
  | MyST spec             | `tests/myst_corpus.rs`                      | `EXPECTED_FAILURES` in the file            |

Never add an ID to an allowlist you have not watched pass. A weekly
`smoke-test.yml` workflow additionally scans real-world repositories with
`panache debug format` and files regression issues automatically.

## Generated Files

Never hand-edit these; regenerate them. `build.rs` reruns on `src/cli.rs`,
`src/config/formatter_presets.rs`, `src/linter/external_linters.rs`, and the
vendored Quarto schema.

  | File                                            | Produced by                                                                              |
  | ----------------------------------------------- | ---------------------------------------------------------------------------------------- |
  | `docs/reference/cli.qmd`                        | `build.rs` from the clap defs in `src/cli.rs`                                            |
  | `docs/reference/_formatter-presets-details.qmd` | `build.rs` from `src/config/formatter_presets.rs`                                        |
  | `docs/reference/_linter-presets-details.qmd`    | `build.rs` from `src/linter/external_linters.rs`                                         |
  | `panache.schema.json`                           | `UPDATE_EXPECTED=1 cargo test config_schema` (`docs/panache.schema.json` symlinks to it) |
  | `docs/guide/benchmarks.qmd`                     | `benches/generate_docs.sh` (`check_docs.sh` fails if stale)                              |
  | `CHANGELOG.md`                                  | versionary                                                                               |
  | man page, shell completions                     | `build.rs` into `OUT_DIR`                                                                |

Vendored corpora are refreshed by the `update-*.sh` scripts under
`crates/panache-parser/scripts/`, plus `scripts/update-quarto-schema.sh` and
`scripts/update-uri-schemes.sh`.

## Code and Test Layout

`ls` gives you the module lists; this section covers only what the tree does not
tell you.

**Where code goes.** Parser constructs live one-per-module under
`parser/blocks/` (each exporting `try_parse_*()` / `emit_*()`) and
`parser/inlines/`; shared machinery is in `parser/utils/` (container stack,
continuation rules, list item buffering, attributes, text buffers). Typed AST
wrappers are one module per construct family in `syntax/`. Formatter modules are
split by concern under `crates/panache-formatter/src/formatter/`. Keep
formatter-core logic in that crate and host-only process/runtime paths in
top-level `src/`.

**Golden tests come in two independent suites.** Both must be fed when adding a
construct:

- Formatter: `tests/fixtures/cases/<name>/` with `input.*`, `expected.*` (same
  extension), optional `panache.toml`. **Register the directory in the
  `golden_test_cases!` macro at the bottom of `tests/golden_cases.rs` or it
  silently never runs.** Regenerate with
  `UPDATE_EXPECTED=1 cargo test --test golden_cases` and verify every diff.
- Parser: `crates/panache-parser/tests/fixtures/cases/<name>/` with `input.*`
  and optional `parser-options.toml`. Validates losslessness and pins the CST
  via `insta` snapshots under `crates/panache-parser/tests/snapshots/`.

**Gates that fail on omission**, not just on wrong behavior:

- `tests/linter_rules_docs.rs` --- `docs/reference/linter-rules.qmd` must match
  the registry's `RuleMeta` (codes, severities, fixability, default state), so
  an undocumented rule fails the build.
- `tests/config_schema.rs` --- `panache.schema.json` must match the host
  `Config`, and every fixture `panache.toml` must validate against it.

**Why some tests live where they do:**

- Formatter fixtures are top-level, not in the formatter crate, because each
  case's `panache.toml` uses the *host* config schema (`[format]`,
  `[code-blocks]`, `[formatters.python]`), which the dependency-lean formatter
  `Config` cannot parse alone.
- `tests/external_formatters.rs` / `external_linters.rs` are top-level because
  spawning processes is a host concern. They need the real binaries on `PATH`
  and skip when absent --- so they can pass for the wrong reason outside the
  devenv shell.
- `crates/panache-formatter/tests/format/` holds programmatic-Config unit tests;
  wire new modules into `format/main.rs`.
- Architecture tests for block parsing (including a dedicated `losslessness.rs`)
  live in `crates/panache-parser/src/parser/blocks/tests/`.

**YAML.** The in-tree parser (`crates/panache-parser/src/parser/yaml.rs`) is the
production one; it retired the external `yaml_parser` crate, embeds a lossless
CST into the host tree for frontmatter and hashpipe bodies, and backs the
in-tree YAML formatter. `pretty_yaml` survives only as a dev-only
cross-validation oracle (revisit \~2026-09). Refresh fixtures with
`crates/panache-parser/scripts/update-yaml-test-suite-fixtures.sh` --- the
identically named script under top-level `scripts/` targets a directory that no
longer exists; don't use it.

**Editors.** `editors/code/` (VS Code, `npm run compile` / `npm run package`)
and `editors/zed/` --- the latter is a separate crate outside the workspace;
read the release asset hygiene invariant before touching release workflows.

## LSP Implementation (`src/lsp/`)

### Architecture

- Synchronous `lsp-server` (`Connection::stdio`) with a crossbeam `select!` main
  loop (`src/lsp.rs`) --- no tokio, no async runtime. This mirrors
  rust-analyzer's threading model.
- Single-writer `GlobalState` (`src/lsp/global_state.rs`) owns the only mutable
  `SalsaDb` handle; all salsa input mutation happens on the main thread. Worker
  threads (a `TaskPool`, plus a dedicated single-thread format pool) run
  read-only requests against cloned salsa handles carried in `StateSnapshot`.
- Shared state uses copy-on-write `Arc` (`Arc::make_mut`), not a coarse `Mutex`:
  `document_map` is `Arc<DocumentMap>` and diagnostics live in
  `DiagnosticCollection`. Salsa's `Cancelled` unwind (`catch_cancelled` in
  `src/lsp/helpers.rs`) is the concurrency fence --- a main-thread write cancels
  in-flight worker reads.
- Per-document state is represented by `DocumentState`, which is *only* salsa
  input handles (`file_id`, `salsa_file`, `salsa_config`). It holds no tree: the
  notification handlers write inputs and never parse, and every consumer derives
  the tree from `crate::salsa::parsed_document`. Salsa caches `GreenNode`
  (`Send + Sync`) rather than the cursor-carrying `SyntaxNode`, which workers
  materialize per request.
- Incremental reparsing lives inside `parsed_document`, splicing off a base kept
  in a side channel (`src/incremental.rs`) that only LSP-admitted documents
  enter. See `docs/development/lsp.qmd`; with `experimental.incrementalParsing`
  off, nothing is admitted and every parse is a full parse.
- Incremental sync mode with UTF-16/UTF-8 position conversion
  (`src/lsp/conversions.rs`, `src/lsp/line_index.rs`).
- Request handlers live in `src/lsp/handlers/`, routed by `src/lsp/dispatch.rs`.

Uses typed AST wrappers for cleaner code:

```rust
// With wrapper (preferred in LSP)
if let Some(heading) = Heading::cast(node) {
    let text = heading.text();
    let level = heading.level();
}
```

User-facing LSP behavior and editor setup are documented in `docs/guide/lsp.qmd`
and `docs/development/lsp.qmd`.

## Linter (`src/linter/`)

`diagnostics.rs` holds the core types (Diagnostic, Severity, Fix, Edit),
`runner.rs` the orchestration, `rules.rs` the Rule trait + `RuleMeta` +
registry, and `rules/*` the \~30 built-in rules. Cross-document reference and
anchor rules are backed by `index.rs`, `project_index.rs`, `yaml_anchors.rs`,
and `yaml_resolve.rs`; Quarto metadata validation by `quarto_schema.rs`.

**The registry in `rules.rs` is authoritative.**
`docs/reference/linter-rules.qmd` is the human-facing catalogue, held in sync by
`tests/linter_rules_docs.rs` --- so an undocumented rule fails the build. Never
hand-maintain a rule list anywhere else, including here.

Adding a rule: use the `add-lint-rule` skill (registry wiring, extension/flavor
gating, fixtures, and the docs entry the parity test requires).

## Important Development Rules

Four principles, in priority order when they conflict:

1. **Losslessness beats formatting.** A parse failure is more serious than an
   ugly output. The parser preserves bytes; the formatter applies rules. Never
   format in the parser.
2. **Pandoc is the gold standard.** When in doubt, check what pandoc does ---
   `cargo run -- parse --to pandoc-ast` diffs directly against
   `pandoc -f markdown -t native`.
3. **Test-driven.** Reproduce a bug with a failing test before fixing it; write
   the test first for new features.
4. **Single-pass stays single-pass.** If a classification depends on surrounding
   state, plumb that state into detection as context --- do not re-classify or
   post-process the parsed result. Two passes is a regression, not a refactor.

### DO

- Run `cargo test --workspace` after changes, and
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  (`--fix` auto-fixes much of it).
- Check idempotency: formatting twice must equal formatting once.
- Verify CST snapshots and expected outputs before updating them.
- Update `docs/` when adding features or changing behavior.

### DON'T

- Run `cargo run -- format <file>` just to inspect output --- **IT FORMATS IN
  PLACE**. Use `cargo run -- format < file.md` (stdin) or
  `cargo run -- format --check file.md`.
- Change core formatting, or update golden expectations, without verifying every
  diff.
- Hand-edit generated files (see "Generated Files") --- regenerate them.
- Add an ID to a conformance allowlist without watching it pass.
- Fix a pandoc divergence inside `pandoc_ast.rs` instead of in the CST.
- Touch `CHANGELOG.md` --- versionary owns it.
- Delete working files unless absolutely necessary.

## Commits

Conventional Commits: `<type>(<scope>): <subject>`. **CONTRIBUTING.md is the
canonical reference** for the type list, the scope list, and which type produces
which version bump --- versionary reads the type, so it matters. Subjects are
imperative, lowercase, no trailing period, <=50 chars (72 hard cap).

Panache-specific conventions not covered there:

- Wrap inline code, flags, identifiers, and SyntaxKinds in backticks
  (``add `{lang}` placeholder``, `` retag `HTML_BLOCK_DIV` ``).
- For pandoc-conformance work, put the pass-rate delta in the body (e.g.
  `html 226 -> 235 (+9), total 419 -> 428`) so the recap matches what landed.
- Explain the *why* and the user-visible effect; the diff shows the *what*. Wrap
  the body at \~72 chars and pass it via heredoc to preserve formatting.

**Don't**:

- Push commits or open pull requests unless the user explicitly asks. Local
  commits are fine; anything that publishes to the remote (`git push`,
  `gh pr create`, `gh pr merge`) needs explicit authorization for that specific
  action.
- Skip hooks (`--no-verify`) --- pre-commit runs `rustfmt`, `panache format`,
  and `eslint`. Fix the underlying issue if a hook fails.
- Amend a published commit; create a new one.
- Touch `CHANGELOG.md` --- versionary owns it.

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

**AUR**: `packaging/aur/PKGBUILD` is the source of truth for the `panache-bin`
package; `publish-aur.yml` (chained from `build-and-test.yml`, gated on `v*`)
rewrites `pkgver`/`pkgrel`/checksums and pushes to the AUR, which is a pure
deploy target. There is no `jolars/panache-aur` mirror any more. Details and the
`task aur:push` fallback are in `packaging/aur/README.md`.

## External Resources

- **Pandoc spec** (definitive reference for parser work):
  `assets/pandoc-spec.md` and `assets/pandoc-spec/`
- **Docs site**: `docs/` (Quarto, published to GitHub Pages); playground in
  `docs/playground/`, backed by `crates/panache-wasm/`
- **Benchmarks**: `benches/` (criterion + comparison scripts; `generate_docs.sh`
  regenerates the benchmarks page, `check_docs.sh` fails if stale)

## Public API (`src/lib.rs`)

```rust
// Format a document; `range` is an optional 1-indexed inclusive line range
pub fn format(input: &str, config: Option<Config>, range: Option<(usize, usize)>) -> String

// Format with defaults (no config, whole document)
pub fn format_with_defaults(input: &str) -> String

// Parse to CST (for inspection/debugging)
pub fn parse(input: &str, config: Option<Config>) -> SyntaxNode
```

`format` and `parse` accept optional config to respect flavor-specific
extensions and formatting preferences. Lower-level entry points (`format_tree`,
`format_with_tree`, `parse_with_refdefs`, `reparse_with_refdefs`) exist for the
LSP and incremental paths. `reparse_with_refdefs` is refusal-first: `None` means
"no reuse available", and the caller full-parses.
