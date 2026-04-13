# Parser golden corpus ownership

This corpus is owned by `panache-parser` integration tests.

- Primary purpose: parser invariants (losslessness, CST regression shape, parse behavior contracts).
- Harness: `crates/panache-parser/tests/golden_parser_cases.rs`.
- Scope: parser-only behavior, not formatter policy.
- Snapshot format: `insta` snapshots in `crates/panache-parser/tests/snapshots/`.
- Optional case config file: `parser-options.toml` (`flavor` and `[extensions]` only).

This directory started as a one-time copy from the top-level formatter corpus.

From now on, this corpus is intentionally allowed to diverge from `tests/cases/`.
When adding or changing parser-specific coverage, update this corpus directly.

Parser corpus does not keep formatter `expected.*` files, `cst.txt`, or app-level `panache.toml` files.
