---
paths:
  - "crates/panache-parser/src/parser/**/*.rs"
  - "crates/panache-parser/src/parser.rs"
  - "crates/panache-parser/src/syntax/**/*.rs"
  - "crates/panache-parser/src/syntax.rs"
  - "src/parser.rs"
  - "src/syntax.rs"
---

Parser and syntax changes must preserve lossless CST behavior.

- Treat Pandoc as the behavioral reference for ambiguous Markdown/Quarto syntax.
- CommonMark and Pandoc-markdown are **distinct dialects**, not flavor variants
  of one parser. When their tokenization rules disagree (e.g. backtick run
  matching, emphasis flanking, raw HTML recognition), branch on
  `config.dialect == Dialect::CommonMark` rather than introducing a per-feature
  extension flag or a `match config.flavor` against a hardcoded flavor list.
  `Dialect` lives on `ParserOptions` (see `crates/panache-parser/src/options.rs`)
  and is set by `Dialect::for_flavor(flavor)`. Per-feature toggles still belong
  on `Extensions`; reserve `Dialect` for structural parser-identity differences.
- Before changing parser behavior in a way that could affect both dialects,
  verify against pandoc:
  ```
  pandoc <case>.md -f commonmark -t native    # CommonMark dialect
  pandoc <case>.md -f markdown   -t native    # Pandoc dialect
  ```
  If the outputs differ, land paired parser fixtures (one per dialect) under
  `tests/fixtures/cases/` with `parser-options.toml` pinning the flavor.
- Preserve all structural markers and whitespace in CST nodes/tokens.
- Do not move formatter policy into parser code.
- Keep parser single-pass assumptions intact (block parsing with inline emission
  during parse).
- Prefer existing block dispatcher and inline parser utilities over introducing
  parallel parsing paths.
- Keep parser behavior independent of formatter/linter policy; parser should
  remain lossless syntax capture only.
- For bug fixes and new parsing behavior, add a focused test first (unit or
  golden case).
- Parser golden fixtures live under
  `crates/panache-parser/tests/fixtures/cases/`; keep parser-specific coverage
  there.
- If parser CST snapshots change
  (`crates/panache-parser/tests/snapshots/parser_cst_*.snap`), validate each
  case intentionally.
