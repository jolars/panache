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
- Pandoc-native (`pandoc -f markdown -t native`) is the **behavioral
  reference** for Pandoc-dialect parsing changes, not the legacy parser's
  current output. The legacy recursive-descent paths (e.g.
  `try_parse_emphasis` and the `delimiter_stack` module) approximate
  pandoc but have their own quirks; an existing fixture matching the
  legacy output is NOT a guarantee of correctness. When the parser
  output, an existing fixture, and pandoc-native disagree, fix toward
  pandoc-native and update the fixture — don't preserve a legacy bug.
- **TEXT-token coalescence diffs are benign.** Two CSTs that differ only
  in whether a TEXT span is split (`TEXT@0..5 "foo" + TEXT@5..6 "*"`)
  versus coalesced (`TEXT@0..6 "foo*"`) over the same byte range and
  with no structural-element (Strong/Emph/Link/Image/etc.) differences
  are equivalent. Pandoc-native doesn't pin TEXT-token granularity, so
  these are not regressions; update the snapshot. **Structural diffs**
  (presence, absence, or changed nesting of typed wrapper nodes) DO
  require pandoc-native verification before accepting — those are the
  cases where dialect-divergence rules and fixture preservation matter.
- Preserve all structural markers and whitespace in CST nodes/tokens.
- **Bracket-shape patterns separate syntax from semantics.** Resolved links
  emit `LINK`/`IMAGE_LINK`. Bracket-shape patterns whose label doesn't
  resolve to a refdef under Pandoc emit `UNRESOLVED_REFERENCE` (a separate
  CST kind), with an `is_image()` accessor on the typed wrapper. Downstream
  tools (linter `undefined_references`, LSP heading-link conversion /
  goto-def / rename, salsa `heading_link_usages`, pandoc-ast projector,
  formatter) walk both wrappers as appropriate. Implicit-heading-id
  resolution lives downstream (in the projector / LSP handlers), not in
  the parser. Do not re-introduce shape-only `LINK` emission for
  unresolved references. Empty-component shapes (`[]`, `[][]`) are
  literal text under both dialects (no wrapper).
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
