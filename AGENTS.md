# Panache agent guide

Use this file for durable, high-signal guidance.

## Project overview

- Panache is a Rust 2024 workspace with a root `panache` crate (library + CLI).
- Workspace crates are `crates/panache-parser`, `crates/panache-formatter`, and
  `crates/panache-wasm`.
- `editors/zed` is intentionally outside the workspace.

## Project-wide guidance

### Architecture and ownership

- The parser captures a lossless CST; the formatter owns style policy.
- Keep parser policy separate from formatter and linter policy.
- Use typed syntax wrappers in downstream code, including the linter and
  language server, rather than raw kind matching where wrappers exist.

### Validation

Run these commands before and after non-trivial changes:

```bash
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt -- --check
```

Useful targeted commands:

```bash
cargo test --test golden_cases <case_name>
cargo test -p panache-parser --test golden_parser_cases <case_name>
cargo run -- debug format --checks all document.qmd
```

### Performance investigations

Use the [performance guide](benches/PERFORMANCE.md) to select the parser or
formatter harness. Time the release CLI for end-to-end changes and a persistent
language server for editor latency. Preserve CST losslessness and formatter
bytes, and run the corresponding golden cases, parser snapshots, and CommonMark
conformance checks.

### Tests and fixtures

- Assert one user-visible behavior at a time with minimal brittleness.
- Use existing fixture layouts rather than ad-hoc test directories.
- Keep expected-output updates intentional and reviewed.
- Prefer stable substring and ordering assertions for CLI diagnostics.

### Generated artifacts

Do not hand-edit generated files. Regenerate them using existing project
commands. Important generated outputs include:

- `docs/reference/cli.qmd`
- `docs/reference/_formatter-presets-details.qmd`
- `docs/reference/_linter-presets-details.qmd`
- `docs/reference/_linter-rules.qmd` (`task docs:rules`)
- `panache.schema.json`

### Commits and releases

- Use Conventional Commits per `CONTRIBUTING.md`.
- Do not push or open pull requests unless explicitly asked.
- Do not skip hooks with `--no-verify`.
- Do not hand-edit `CHANGELOG.md`.
- Only `v*` CLI releases should publish GitHub release assets in this
  repository.

## Parser

### Core invariants

- Keep parsing single-pass: block parsing followed by inline emission.
- Preserve every source byte, including structural markers, whitespace, and
  trivia, in the CST.
- Reuse existing dispatchers and parser utilities; do not introduce post-parse
  repair passes.
- CommonMark and Pandoc are different dialects. Use `Dialect` for structural
  parser differences and `Extensions` for feature toggles.
- Treat Pandoc-native output as the behavioral reference for parser ambiguity.
- Add focused parser tests or fixtures before changing behavior, and review CST
  snapshot diffs intentionally.

### Math

- Keep math parsing lossless and tolerant; parsing must not hard-fail.
- Report diagnostics through side channels rather than encoding errors as CST
  structure.
- Emit only `MATH_*` token kinds for math-content internals.
- Pass flavor-specific behavior through parser options, never global state.
- Keep formatting policy out of the math parser, and add focused parser fixtures
  for new behavior.

### YAML

- Keep YAML parsing CST-first, lossless, indentation-aware, and
  trivia-preserving.
- Use one core parser model for plain and hashpipe-prefixed YAML.
- Keep host-to-embedded range mapping explicit and deterministic.
- Guard behavior with yaml-test-suite parity, losslessness checks, and focused
  deterministic tests.

### Conformance

- Keep conformance work incremental; avoid broad rewrites in one session.
- Never add allowlist entries without rerunning the relevant report and
  confirming that the example or case passes.
- Parser behavior changes require focused parser fixtures; conformance
  allowlists are regression guards, not primary behavior specifications.
- If parser structure changes user-visible formatting, add or update formatter
  golden cases.

#### CommonMark

- Run conformance only under `Flavor::CommonMark`; keep the HTML renderer
  test-only.
- Treat `spec.txt` and byte-equal HTML after shared `<li>` whitespace
  normalization as the source of truth.
- Do not hand-edit generated reports or vendored spec fixtures.

#### Pandoc

- Run conformance only under `Flavor::Pandoc`, and treat
  `pandoc -f markdown -t native` as the behavioral reference.
- Do not hand-edit generated reports or `expected.native` corpus outputs.
- Never fix Pandoc divergence in the projector when the CST is wrong; fix the
  parser shape first.

#### YAML

- Treat each yaml-test-suite case directory as the source of truth; check both
  `test.event` and `error` contracts before allowlisting it.
- Keep triage and regression reports reproducible and harness-generated.

### Tests

- Parser golden fixtures live in `crates/panache-parser/tests/fixtures/cases/`.
- Run a focused golden parser case with
  `cargo test -p panache-parser --test golden_parser_cases <case_name>`.

## Formatter

### Core invariants

- Enforce idempotency: `format(format(x)) == format(x)`.
- When idempotency fails, inspect the parser CST shape before adding a formatter
  workaround.
- Reuse existing wrapping, list, table, and related helpers.
- Keep formatter core logic in `crates/panache-formatter`; keep host runtime and
  process integration, including external tools and CLI/language-server wiring,
  in top-level `src/`.
- `crates/panache-formatter` must remain dependency-lean. Do not move external
  process execution into it.
- Add or update the smallest relevant formatter golden and documentation for
  user-visible formatting changes.

### YAML

- Treat `crates/panache-formatter/src/formatter/yaml/STYLE.md` as the source of
  truth for YAML formatting rules.
- Keep one YAML output path through `format_yaml`, shared by plain metadata and
  hashpipe formatting.
- Treat cross-validation mismatches as formatter, parser, or oracle bugs to
  diagnose, not accepted divergence.
- Keep `pretty_yaml` as a development-only reference with no runtime dependency.
- Ensure every YAML corpus case is idempotent.

### Tests and debugging

- Formatter golden fixtures live in `tests/fixtures/cases/` and must be
  registered in `tests/golden_cases.rs`.
- Run a focused golden formatter case with
  `cargo test --test golden_cases <case_name>`.
- `cargo run -- format <file>` edits files in place. Use stdin or `--check` for
  inspection.

## Linter

### Diagnostics and fixes

- Prioritize accurate rule codes, severity, spans, and precise diagnostics.
- Add auto-fixes only when replacements preserve document intent.
- Rules produce structured diagnostics; shared CLI and language-server paths own
  presentation and position mapping.
- Keep CLI diagnostics concise without regressing language-server mappings.
- Reuse shared linter orchestration instead of duplicating flows.
- Add focused lint tests, and synchronize documentation for user-visible
  changes.
- Keep rule explanations in `src/linter/rules/docs/` and executable examples in
  each rule's `examples()`. Regenerate the reference with `task docs:rules`.

### Tests and debugging

- The linter documentation parity gate is `tests/linter_rules_docs.rs`.
- The lint cache can mask linter changes. Use `cargo run -- clean --all` or
  `--no-cache` when debugging.

## Language server

### Document state and protocol

- Preserve open, change, save, and close behavior and stable document state.
- Keep UTF-16/UTF-8 position conversion correct.
- The main thread is the sole writer of `GlobalState` and the Salsa database;
  workers operate on read-only snapshots.
- Treat Salsa cancellation as the concurrency boundary; keep Salsa mutation on
  the main thread.
- Prefer typed syntax wrappers and shared conversion and state helpers.
- Make state transitions explicit; avoid silent failure paths.
- Add targeted protocol-visible tests, and update the language-server guide for
  user-visible behavior changes.

### Editor integrations

- Keep settings aligned across implementation, schema, and README.
- Preserve activation behavior for supported languages and workspaces.
- Reuse existing process, download, and configuration helpers.
- Keep `panache lsp` startup explicit and predictable.
- Validate changes with `npm run compile` in `editors/code/`.

## Configuration

### Discovery and merging

- Preserve configuration discovery precedence and explicit `--config` failure
  behavior.
- Merge deterministically: flavor defaults first, then user overrides.
- Keep canonical keys kebab-case; aliases are compatibility shims.
- Make deprecations explicit and actionable.

### Schema and tests

- Add focused tests for parsing, precedence, and merge changes.
- Update documentation when keys, defaults, or enums change.
- Regenerate `panache.schema.json` with
  `UPDATE_EXPECTED=1 cargo test config_schema`, and review the diff.
- The configuration schema parity gate is `tests/config_schema.rs`.
