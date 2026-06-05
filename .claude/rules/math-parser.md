---
paths:
  - "crates/panache-parser/src/parser/math.rs"
  - "crates/panache-parser/src/syntax/math.rs"
  - "crates/panache-parser/src/syntax/inlines.rs"
  - "crates/panache-parser/src/parser/inlines/math.rs"
  - "crates/panache-parser/tests/fixtures/cases/*math*/**"
---

This rule applies when editing the in-tree TeX **math content** parser, its
CST/syntax, or its embedding into the host tree. Skip it for unrelated parser
work even though it lives in the same crate. See the `math-parser-formatter`
skill for the multi-session roadmap and the per-session workflow.

The math parser (`crates/panache-parser/src/parser/math.rs`) parses the content
*between* math delimiters (the delimiters stay on the host `INLINE_MATH` /
`DISPLAY_MATH` nodes) into a lossless structural CST rooted at `MATH_CONTENT`,
which is spliced into the host document tree — exactly the embedding pattern the
YAML parser uses (`copy_green_node` from a sub-parser green node).

- **Lossless + never fails.** `tree.text() == content` for every input,
  including malformed math. The tokenizer has no error/abort path — the worst
  case is a single `MATH_TEXT` atom.
- **Errors ride a side-channel**, never the CST. `parse_math_report` returns a
  `MathParseReport { green, diagnostics }` (à la YAML's `YamlParseReport`):
  unclosed/stray brace, unclosed/mismatched/stray environment. Byte offsets are
  content-relative; the host offsets them when surfacing through linter/LSP.
- **texlab is the design reference, not KaTeX.** texlab is lossless and
  error-tolerant (an LSP can't reject input); KaTeX normalizes whitespace, drops
  comments, and throws — wrong model for a formatting CST.
- **The math parser is config-aware only where a Markdown flavor layers a
  construct onto TeX.** That is plumbed via `MathParseOptions` (currently just
  `bookdown_equation_labels`, gated on the `bookdown_equation_references`
  extension), derived in `inlines/math.rs::math_opts(config)`. Don't reach for
  global config inside the tokenizer; pass it through options.
- **Bookdown equation labels are parsed, not post-hoc text-scanned.**
  `(\#eq:label)` becomes a single `MATH_EQUATION_LABEL` token (reusing
  `try_parse_bookdown_equation_definition`); downstream consumers (salsa
  indexers, LSP) walk the token. A non-matching `(` must not fragment ordinary
  atom runs when the extension is off — keep that gate.
- **Every token the math parser emits is a `MATH_*` kind** (`MATH_SPACE` /
  `MATH_NEWLINE` included — do NOT reuse host `WHITESPACE`/`NEWLINE`). The host
  block machinery interleaves container prefixes (e.g. blockquote
  `BLOCK_QUOTE_MARKER` + bare `WHITESPACE`) into `MATH_CONTENT` on continuation
  lines; the `MATH_*`-only invariant lets `syntax::math::math_content_text()`
  strip those prefixes by whitelist. Read raw math content through that helper,
  never via `MATH_CONTENT.text()` directly — the latter leaks the `>` and breaks
  idempotency.
- **Operators are tokenized but NEVER classified in the CST.** `+ - * = < >`
  each emit a neutral `MATH_OPERATOR` token (one per char); the parser does not
  tag bin/rel or build precedence structure. Operator class is contextual (TeX
  coerces a Bin atom after Bin/Rel/Open/Punct to Ord — that *is* unary minus),
  override-able (`\mathbin`), and macro-dependent, so it is *interpretation* —
  the analog of YAML scalar cooking (`parser/yaml/cooking.rs`), belonging in a
  shared formatter/LSP module keyed on operator text + command name, never in
  CST kinds. Do NOT introduce `MATH_BIN_OP`/`MATH_REL_OP`. Command operators
  (`\cdot`, `\leq`, …) stay `MATH_COMMAND`. Remember to keep `MATH_OPERATOR` in
  the `math_content_text()` whitelist (`syntax/math.rs`) — dropping it breaks
  losslessness.
- **Single-pass.** The sub-parse happens once at emission in `inlines/math.rs`;
  don't add a re-parse/post-process pass.
- Keep parser policy separate from formatter policy. The formatter side is gated
  behind an experimental option (default off = verbatim); the parser is
  unconditional.
- Add focused, deterministic tests for new math behavior; parser golden fixtures
  live under `crates/panache-parser/tests/fixtures/cases/`.
