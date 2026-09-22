# Contributing to Panache

Thanks for your interest in contributing to Panache.

## Getting Started

1. Fork and clone the repository.

2. Install the Rust toolchain. The version is pinned in `rust-toolchain.toml`,
   so `rustup` picks it up automatically:

   ```bash
   rustup toolchain install
   rustup target add wasm32-unknown-unknown # only needed for the WASM crate
   ```

   Panache uses the Rust 2024 edition.

3. Optionally, use the provided [devenv](https://devenv.sh) shell. It pins the
   toolchain, installs the external tools the test suite exercises (`shfmt`,
   `ruff`, `stylua`, `shellcheck`, `quarto`, `wasm-pack`, and others), and
   installs the git pre-commit hooks:

   ```bash
   devenv shell
   ```

   Without devenv you can still work on the project; a handful of external
   formatter and linter tests will be skipped or fail if the corresponding tool
   is missing from `PATH`.

4. Run the full validation command before and after changes:

   ```bash
   cargo check --workspace && cargo test --workspace && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo fmt -- --check
   ```

   [Task](https://taskfile.dev) shortcuts are available for the common steps:
   `task dev` (check + test + clippy), `task test`, `task lint`,
   `task coverage`. Run `task --list` to see everything.

## Development Workflow

- Prefer test-first changes:
  - For bug fixes, add a failing test first, then fix.
  - For new features, add focused tests with the change.
- Keep edits minimal and targeted to the requested behavior.
- Follow existing module and naming patterns in the area you touch.
- Update the documentation under `docs/` when you change user-visible behavior.
- Small, focused PRs are easier to review and merge; avoid large refactors or
  multiple behavior changes in one PR.
- Before spending time on a change, consider opening an issue to discuss the
  approach and ensure alignment with project goals.

### Generated files

Several files in the repository are generated. Don't edit them by hand; change
the source and regenerate.

  | File                                            | Regenerate with                                           |
  | ----------------------------------------------- | --------------------------------------------------------- |
  | `CHANGELOG.md`                                  | Nothing --- versionary owns it entirely                   |
  | `docs/reference/cli.qmd`                        | `cargo build` (from the clap definitions in `src/cli.rs`) |
  | `docs/reference/_formatter-presets-details.qmd` | `cargo build` (from `src/config/formatter_presets.rs`)    |
  | `docs/reference/_linter-presets-details.qmd`    | `cargo build` (from `src/linter/external_linters.rs`)     |
  | `panache.schema.json`                           | `UPDATE_EXPECTED=1 cargo test config_schema`              |
  | `docs/guide/benchmarks.qmd`                     | `./benches/generate_docs.sh`                              |

Vendored test corpora (CommonMark `spec.txt`, the pandoc conformance corpus, the
MyST spec examples, the YAML test suite) are refreshed by the `update-*.sh`
scripts under `crates/panache-parser/scripts/` and `scripts/`.

### Git hooks

The pre-commit hooks run `rustfmt`, `eslint`, and Panache's own formatter
(`cargo run -- format`) over changed Markdown files. Don't bypass them with
`--no-verify`; fix the underlying issue instead. If you are not using the devenv
shell, run the equivalent checks manually before committing.

## Submitting Changes

Panache uses trunk-based development with a linear history on `main`, so keep
commits atomic and rebase rather than merge when your branch falls behind.

1. **Open an issue first** for anything beyond a small fix. Bug reports and
   feature requests have templates; for a bug, the most useful thing you can
   attach is a minimal input document plus the output of
   `panache debug format --checks all --report <file>`, which produces a
   Markdown report suitable for pasting into the issue.
2. **Branch from `main`** and keep the branch focused on one behavior change.
3. **Fill in the PR template checklist.** It is not decorative --- reviewers use
   it to know which gates you have already run locally.
4. **Reference the issue** in the commit body (`Closes #123`) so the release
   notes link back.

### What CI runs

A pull request must pass:

- **Build and Test** --- `cargo test --workspace` on Linux, macOS, and Windows,
  plus a `wasm32-unknown-unknown` build of `panache-wasm` and a type-check and
  bundle of the VS Code extension. It also builds the WASM playground and
  renders the documentation site without publishing. Releases require this
  documentation build to pass.
- **Lint** --- `cargo clippy -- -D warnings`, `cargo fmt -- --check`, and
  Panache formatting and linting its own `docs/` tree with the in-tree binary.
- **Cargo Deny** --- license and advisory checks over the dependency tree.

The local validation command in [Getting Started](#getting-started) is
deliberately *stricter* than CI: it adds
`--workspace --all-targets --all-features` to clippy. Running it locally catches
warnings that would otherwise only appear later.

Because tests run on Windows too, be careful with path separators and line
endings; CRLF handling in the parser is load-bearing.

Note that `cargo test --workspace` exercises the CLI and LSP integration tests
because `cli` and `lsp` are default features. A few external formatter and
linter tests additionally require the corresponding tool on `PATH`.

## Working in Core Areas

### Parser + Formatter

- Keep parser behavior lossless (preserve all input bytes/markers in CST).
- Keep formatting policy in formatter code, not parser code.
- Preserve single-pass parsing: thread state into block/inline detection as
  context rather than adding a post-processing pass.
- Prefer existing parser/formatter helpers over introducing parallel logic
  paths.
- Pandoc is the reference implementation. `panache parse --to pandoc-ast` prints
  the same shape as `pandoc -f markdown -t native`, which makes divergences easy
  to diff.
- Use debug checks when validating formatting behavior:
  - `cargo run -- debug format --checks all document.qmd` (idempotency +
    losslessness)
- For formatting behavior changes, validate idempotency:
  - `format(format(x)) == format(x)`
- Note that `cargo run -- format <file>` rewrites the file in place. To inspect
  output without modifying anything, pipe via stdin
  (`cargo run -- format < file.md`) or use `--check`.

### Adding a new syntax construct

This touches more places than it first appears. In order:

1. Check the shape pandoc produces (`pandoc -f markdown -t native`) before
   designing anything. Pandoc's AST decides what the CST has to express.
2. Add `SyntaxKind` variants in `crates/panache-parser/src/syntax/kind.rs` ---
   one for every byte category you need to round-trip, including markers and
   delimiters.
3. Gate the construct behind an extension or flavor flag in
   `crates/panache-parser/src/options.rs`. Syntax that is unconditionally live
   changes behavior for `commonmark` and breaks that conformance suite.
4. Add a parser module under `parser/blocks/` or `parser/inlines/`. Block
   parsers must also be registered in `BlockParserRegistry::new()` in
   `parser/block_dispatcher.rs`, where **list order is precedence** and mirrors
   pandoc's reader order. Put a comment next to your entry explaining why it
   sits where it does; a wrong slot is the most common source of subtly wrong
   output.
5. Add a typed AST wrapper under `crates/panache-parser/src/syntax/` if the
   linter or LSP needs to inspect the node.
6. Teach the formatter: block dispatch is the `match` in
   `crates/panache-formatter/src/formatter/core.rs`, inlines in
   `formatter/inline.rs`.
7. Add fixtures to **both** golden suites (see below), including the construct
   nested in a list item and in a blockquote --- container prefixes are where
   most constructs break.
8. Run the conformance suites. A new block parser changes precedence globally,
   so those are the real regression signal.

### Fixture-based tests

Two golden suites cover most behavior:

- Formatter cases in `tests/fixtures/cases/*/` (`input.*`, `expected.*`, and an
  optional `panache.toml`), driven by `tests/golden_cases.rs`. New case
  directories must also be listed in the `golden_test_cases!` macro at the
  bottom of that file, or they never run.
- Parser cases in `crates/panache-parser/tests/fixtures/cases/*/`, driven by
  `crates/panache-parser/tests/golden_parser_cases.rs`, which checks
  losslessness and reviews CST snapshots with `insta`.

Expected formatter outputs can be regenerated with
`UPDATE_EXPECTED=1 cargo test --test golden_cases`, but review every diff before
committing; a wrong regeneration silently bakes in a regression.

### Conformance suites

Three vendored corpora guard parser behavior against upstream references. Each
has an *allowlist* of cases guaranteed to pass (a regression there fails the
build), a *blocked* list of cases not targeted yet, and an `#[ignore]`d
full-report test for triage.

  | Suite                 | Harness                                     |
  | --------------------- | ------------------------------------------- |
  | CommonMark `spec.txt` | `crates/panache-parser/tests/commonmark.rs` |
  | Pandoc native AST     | `crates/panache-parser/tests/pandoc.rs`     |
  | MyST spec             | `tests/myst_corpus.rs`                      |

```bash
# Guarded run (what CI enforces)
cargo test -p panache-parser --test commonmark

# Full triage report, written to tests/commonmark/report.txt
cargo test -p panache-parser --test commonmark -- --ignored --nocapture
```

To advance a suite, fix the underlying parse and then move the newly passing IDs
from `blocked.txt` to `allowlist.txt` in the same commit. Never allowlist an ID
you have not watched pass.

`crates/panache-parser/src/pandoc_ast.rs` projects the CST into pandoc's native
shape. It is a test-only diagnostic: if it disagrees with pandoc, the CST is
wrong. Don't paper over a divergence inside the projector --- the linter, LSP,
and formatter all read the CST, so the defect would persist for them.

### External formatters and linters

Panache delegates embedded code blocks to third-party tools such as `ruff`,
`shfmt`, and `shellcheck`. This is entirely a host-crate concern; the formatter
crate never spawns a process.

To add a tool preset, add an entry to the table in
`src/config/formatter_presets.rs` (or `src/linter/external_linters.rs` for
linters). `{}` in `args` is the filename placeholder, and `stdin: false` means
the tool rewrites a temp file in place rather than reading stdin. The
documentation pages for both preset tables are generated from these files at
build time.

The subtle part is offset mapping: the external tool sees dedented code with no
fence or container prefix, so its reported line and column numbers must be
translated back to document positions (`src/linter/offsets.rs`). Tests in
`tests/external_formatters.rs` and `tests/external_linters.rs` need the real
binaries on `PATH`; the devenv shell provides them.

### Linter

- Add or adjust rules as focused, user-visible diagnostics.
- Keep fixes explicit and safe; avoid silent behavior changes. A fix must never
  change the meaning of the document.
- Add tests for rule behavior and autofix output.
- Every rule must be documented in `docs/reference/linter-rules.qmd`.
  `tests/linter_rules_docs.rs` asserts the catalogue matches the rule registry
  (codes, severities, fixability, default state), so an undocumented rule fails
  the test suite.
- When debugging the linter from the CLI, remember that lint results are cached
  on disk under `~/.cache/panache/` and the cache key does not invalidate on
  every code change. Use `cargo run -- clean --all`, `--no-cache`, or
  `cache = false` if you see stale diagnostics.

### LSP

- Preserve protocol-correct document lifecycle behavior
  (`didOpen`/`didChange`/`didClose`).
- Be careful with UTF-16/UTF-8 position/range conversions.
- The server is synchronous (`lsp-server` + crossbeam), with a single-writer
  `GlobalState` and read-only worker snapshots. Salsa cancellation is the
  concurrency fence; keep salsa mutation on the main thread.
- Prefer typed syntax wrappers for feature implementations when available.
- Run targeted LSP tests before full-suite revalidation.

## Debugging LSP (VS Code + Neovim)

Start the server manually for debugging:

```bash
panache lsp
```

Useful logging examples:

```bash
RUST_LOG=debug cargo run -- lsp
RUST_LOG=info ./target/release/panache lsp
```

### VS Code

- Install extension: `jolars.panache`.
- Confirm the extension can resolve the `panache` binary (see
  `panache.executableStrategy` and `panache.executablePath`, which supersede the
  deprecated `panache.commandPath` and `panache.downloadBinary`).
- If needed, set `panache.trace.server` to inspect client/server traffic.
- To build the extension locally: `npm ci && npm run compile` in
  `editors/code/`; `npm run package` produces a `.vsix`.

### Neovim

- Confirm your LSP config uses `cmd = { "panache", "lsp" }`.
- Ensure root markers include one of: `.panache.toml`, `panache.toml`, `.git`.
- Verify `panache` is available in your shell `PATH` as seen by Neovim.

Full editor setup instructions for these, plus Helix, Emacs, Sublime Text, and
Kate, are in the [LSP guide](https://panache.bz/guide/lsp.html).

## Commits and Releases

This project uses [Conventional Commits](https://www.conventionalcommits.org/)
and versionary. The commit type determines the version bump, so pick it
deliberately.

**Types**: `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `chore`, `ci`,
`build`, `style`.

- `feat:` typically results in a **minor** release bump.
- `fix:` and `perf:` typically result in a **patch** bump.
- `feat!:` / `fix!:` or a `BREAKING CHANGE:` body trigger a **major** bump.
- The rest (`refactor`, `docs`, `test`, `chore`, `ci`, `build`, `style`) are
  maintenance types for non-runtime or internal changes.

**Scopes** (derived from the workspace layout): `parser`, `formatter`, `linter`,
`lsp`, `cli`, `wasm`, `deps`. Omit the scope only when a change genuinely spans
the whole workspace. Note that versionary versions the root crate,
`crates/panache-parser`, `crates/panache-formatter`, `editors/code`, and
`editors/zed` independently based on the paths a commit touches, so keeping
commits atomic per area keeps the changelogs clean.

Write subjects in the imperative mood, lowercase, with no trailing period, and
aim for 50 characters or fewer (72 is the hard cap). Push explanation into the
body, wrapped at about 72 characters.

Examples:

- `feat(parser): support fenced div attributes in nested blocks`
- `fix(lsp): correct utf16 range conversion for diagnostics`
- `docs: clarify formatter idempotency checks`
- `chore(ci): tighten clippy gate in workflow`

**Release asset hygiene**: only the primary CLI release stream (`v*` tags on
`jolars/panache`) may upload GitHub release assets. The other tag streams the
monorepo produces (`panache-parser-v*`, `panache-formatter-v*`,
`panache-code-v*`, `panache-zed-v*`) must stay asset-free. The Zed extension
resolves its binary with `latest_github_release(..., require_assets: true)`,
which cannot filter by tag prefix, so any extra asset-bearing release shadows
the CLI stream and breaks the download.
