# Math parser/formatter — running session recap

Rolling, terse handoff between sessions of the `math-parser-formatter` skill.
Read at the start of a session for phase status, persistent traps, and the
latest "Suggested next sub-targets". At the end of a session, **rewrite** the
Latest session entry, add a one-line Earlier-sessions note, and merge any
still-relevant trap into Persistent traps. Keep it short.

--------------------------------------------------------------------------------

## Persistent traps & invariants (cross-session)

- **Read math content via `syntax::math::math_content_text()`**, never
  `MATH_CONTENT.text()`. The block machinery interleaves container prefixes
  (blockquote `BLOCK_QUOTE_MARKER` + bare `WHITESPACE`) into `MATH_CONTENT` on
  continuation lines; the helper strips them by whitelisting `MATH_*` tokens.
  Reading `.text()` directly leaks the `>` and re-accumulates it every format
  pass (a real idempotency bug that was fixed in Phase 1).
- **`MATH_SPACE`/`MATH_NEWLINE` are intentionally distinct** from host
  `WHITESPACE`/`NEWLINE` — that distinction is what makes the helper above work.
  `MATH_SPACE` is load-bearing (collides with blockquote-prefix `WHITESPACE`
  otherwise); `MATH_NEWLINE` is kept for symmetry.
- **Parser is unconditional; the experimental gate is formatter-side only.**
- **No pandoc oracle for math formatting** — pandoc passes math through. Lean on
  golden tests + idempotency/losslessness, plus a dev-only latexindent/KaTeX
  oracle (Phase 4).
- **Background revert trap**: a process (suspected pre-commit `git stash`)
  reverted tracked edits once mid-session; untracked files survived. Re-apply if
  source edits vanish.
- **Operator class/precedence is NOT a CST concern — settled, do not
  relitigate.** The parser emits a *neutral* `MATH_OPERATOR` token (one per char,
  `+ - * = < >`); it does NOT tag bin/rel or build a precedence tree. Rationale:
  TeX assigns atom class contextually during mlist→hlist (Appendix G coerces a
  Bin atom after Bin/Rel/Open/Punct to Ord — that *is* unary minus), it's
  override-able (`\mathbin`) and macro-dependent, and there is no
  operator-precedence grammar in TeX (the math list is flat). Class/precedence is
  the analog of YAML's *scalar cooking* (`parser/yaml/cooking.rs`): a pure
  interpretation shared between consumers, NOT a tree shape. So when the
  formatting phase needs class+precedence, build a **shared `math` interpretation
  module** (operator table keyed on operator text *and* command name → class +
  break-priority) consumed by formatter + LSP — never `MATH_BIN_OP`/`MATH_REL_OP`
  kinds. (Structural cooking that *would* be legit future parser work: script
  attachment, known-command argument grouping — orthogonal to operators.)
  **That module now exists** (Phase 5):
  `crates/panache-formatter/src/formatter/math/operators.rs`, `pub` for LSP
  reuse; break-priority column still TODO (Phase 6).
- **Splitting a `MATH_OPERATOR` run: rel chars merge, sign chars split.** A run
  of adjacent operator chars is NOT one atom. Adjacent relation chars (`= < >`)
  merge (`<=`), but each sign char (`+ - *`) is its own atom so it can be unary —
  `=-` is `=` then unary `-` (`x = -y`), not a composite `=-` (`x =- y`). See
  `operators::split_operator_atoms`.
- **CST grain vs interpretation — the line to hold.** A *fact* (unambiguous from
  the bytes, no macro escape) belongs in the CST grain; a *guess* (fallible
  without macro expansion, which we don't do) belongs in the interpretation
  layer (`operators.rs`), never the CST. Operator **class** (bin/rel/unary) is a
  guess — `\mathbin`/`\def`/`\mathcode` can override it — so it stays neutral in
  the CST even though the parser *has* the same context the formatter does (no
  information asymmetry; it's a principle, not a capability limit). Delimiters
  and punctuation (`( ) [ ] , ;`) are the opposite: their category is
  unambiguous at the character level, so they belong in the CST grain. **Done
  (Phase 6 commit 1):** the parser tokenizes them into `MATH_OPEN`/`MATH_CLOSE`/
  `MATH_PUNCT`, and the old `text_tail_class` (which re-lexed a `MATH_TEXT` tail)
  is gone — the formatter reads the token kind via `operators::delimiter_class`.
  `| . /` stay `MATH_TEXT` (their class needs macro context).

--------------------------------------------------------------------------------

## Latest session

**Phase 6 commit 1 — tokenize delimiters/punctuation (parser) + delete
`text_tail_class` (formatter).** *DONE* (not yet committed). The one re-lexing
smell is gone: the formatter no longer peeks at a `MATH_TEXT` tail to recover a
`(`/`,` class.

- **Parser** (`parser/math.rs`): new `is_delimiter` helper + dispatch arms split
  `( [` → `MATH_OPEN`, `) ]` → `MATH_CLOSE`, `, ;` → `MATH_PUNCT` (one token per
  char). The ambiguous `| . /` deliberately stay `MATH_TEXT`. `is_special` gained
  them so they bound text runs; `parse_text` lost its bookdown-`(` special case
  (a `(` is now always a boundary, so the equation-label check still sees every
  `(`). Three new `SyntaxKind`s after `MATH_OPERATOR`. New unit tests
  (`delimiters_and_punctuation_split_atom_runs`, reworked `plain_text_*` /
  `plain_parens_*`).
- **Losslessness**: added the three kinds to `is_math_content_token`
  (`syntax/math.rs`) — the critical whitelist that keeps `math_content_text` (and
  thus the pandoc-ast projector + format roundtrip) lossless.
- **Formatter**: `operators::text_tail_class` replaced by
  `pub fn delimiter_class(kind) -> Option<AtomClass>` (kind-keyed, not char-keyed
  — no duplication of the parser's char→kind grouping). `render.rs::atom_prev_class`
  delegates to it; `MATH_TEXT` is now unconditionally `Ord`. New tokens hit the
  existing `_ => Plain` arm in `space_operators`, so spacing is **byte-identical**.
- **Tier-3 fixture**: `char_class` now parses the delimiter char and maps the
  resulting kind through `delimiter_class` (validates the *full* parser→formatter
  path + the pulldown oracle, not a standalone char fn). tsv/README comments
  updated.

**No behavior change** — confirmed end-to-end: `$f(-x) = (a)-b + [1,2]$` →
`$f(-x) = (a) - b + [1,2]$` (unary `-` after `(` tight, binary after `)` spaced,
`,` tight). 4 parser CST snapshots regenerated (parens split out; ranges still
contiguous/lossless; no `MATH_EQUATION_LABEL` touched). The `math-syntax` linter
rule needed **no** change (it only walks groups/environments; new kinds fall
through its `_`). Verified: `cargo test --workspace` (31 binaries) clean, clippy
`-D warnings` clean, `cargo fmt --check` clean.

### Suggested next sub-targets
1. **Phase 6 — semantic line-breaking + continuation indent** (add the
   break-priority column to `operators.rs`; use `operators/` corpus stressors).
   Walk the *structured* CST — do NOT flatten as the spacing pass does; flattening
   then relinearizing fights bracket-matching / nesting depth.
2. **Embed `MATH_CONTENT` into `TEX_BLOCK`** (parser) so bare `\begin{env}`
   blocks become formattable (would make `MathContext::EnvironmentBody` reachable).
3. Optional structural cooking (orthogonal to operators): script attachment,
   known-command argument grouping.

**Placement note (deferred, YAGNI):** `operators.rs` lives in the formatter
crate (`pub`), but the `cooking.rs` analog it mirrors lives in the *parser*
crate. Only the formatter consumes it today, so leave it; **move it to the parser
crate if/when a second consumer appears** (linter wanting atom info, LSP semantic
tokens) so formatter + linter + LSP share one interpretation.

--------------------------------------------------------------------------------

## Earlier sessions

- **Phase 5b leftover — Tier-3 symbol-class fixture** (committed `9e10d943`).
  Dev-only vendored `symbol-classes.tsv` (token/atom_class/oracle) + harness
  cross-checking `operators` against `pulldown-latex` Events; catches class drift
  both ways (retyped class **and** deleted command). `\lim`/`\asymp` divergences
  recorded, not corrected. (Its `char_class` delimiter path was rebased onto
  `delimiter_class` in Phase 6 commit 1.)
- **Phase 5b — command-operator spacing** (committed `1e43f251`). Formatter-only
  `render.rs` `MATH_COMMAND` arm: a command whose `command_class` is `Bin`/`Rel`
  (after `coerce`) demands `SpacedOp` (`a\cdot b`→`a \cdot b`); command ops are
  NEVER `TightOp` (the control word's terminating `MATH_SPACE` is mandatory).
- **Phase 5 — operator interpretation module + precedence-aware spacing**
  (committed `adbebe06`). New `formatter/math/operators.rs` (the `cooking.rs`
  analog, `pub` for LSP): `AtomClass`, `split_operator_atoms`,
  `classify_operator`, `command_class` table, `text_tail_class`, `coerce` (TeX
  Bin→Ord unary rule), `is_spaced`. `render.rs::render_inline` became a gap-based
  re-spacer over the flat token stream: fold adjacent `MATH_OPERATOR` into a run,
  split into atoms (rel chars `= < >` merge → `<=`; each sign `+ - *` stands
  alone so it can be unary), classify+coerce vs running prev-class, emit gap-by-
  gap. `a+b`→`a + b`, unary `-x`/`f(-x)`/`e^{-t}` tight, `x=-y`→`x = -y`,
  `a--b`→`a - -b`. **The `=-` trap:** merging a whole run gave `x =- y`; the
  split rule (rel merge, sign split) fixes it — relation vs sign are different
  atoms.
- **Phase 4 — dev-oracle cross-validation + idempotency corpus** — math has *no
  output oracle*, so flip the assertion to invariance: `render(x) ==
  render(format(x))` on normalized MathML. Tier 1 `tests/math_corpus_properties.rs`
  (idempotency + losslessness + gate-off), Tier 2 `tests/math_cross_validation.rs`
  (dev-only `pulldown-latex` LaTeX→MathML, four-way accept/skip/fail rule,
  `oracle_discriminates_meaning_from_spacing` pins it non-vacuous). 56-case bare
  `.tex` corpus under `tests/fixtures/math_corpus/`. Tier 3 deferred.
- **Phase 1b — operators into `MATH_OPERATOR`** — split `+ - * = < >` out of
  `MATH_TEXT` into a dedicated *neutral* `MATH_OPERATOR` token (one per char), so
  a future phase can do operator-aware spacing/precedence. Committed `303e05bd`.
  Parser only tokenizes; class/precedence is *interpretation* (a shared
  formatter/LSP module, the `cooking.rs` analog), never `MATH_BIN_OP`/`MATH_REL_OP`
  kinds — see the persistent invariant above. `MATH_OPERATOR` added to the
  `is_math_content_token()` whitelist (critical for losslessness). Command
  operators (`\cdot`, `\leq`) stay `MATH_COMMAND`; `( ) [ ] / : | , ;` stay
  `MATH_TEXT`; unary vs binary minus not distinguished.

- **Math formatter (Phases 2+3) + `math-syntax` → Error** — shipped the
  experimental content-aware formatter behind `[experimental] format-math`
  (default false), mirrored onto `Config::experimental_format_math`. Re-parses
  the clean content string (`parse_math_report`) like the YAML formatter;
  `format_math(content, opts)` in `crates/panache-formatter/src/formatter/math/`
  + `math.rs`, `MathContext::{Inline,Display,EnvironmentBody}`; rows split on
  top-level `\\`/newline, cells on top-level `&`; **trim-before-measure +
  trailing-only padding** = idempotency engine (see `STYLE.md`), canonical ` & `
  separator. Bails to verbatim on gate-off / lone-`$` / any parse diagnostic.
  Gated at 3 call sites (`core.rs`, `inline.rs`), OFF byte-identical. Promoted
  `math-syntax` diagnostics Warning→Error (`src/linter/rules/math_content.rs`).
  Standalone `\begin…\end` blocks parse as `TEX_BLOCK` with opaque `TEXT` (no
  `MATH_CONTENT`) — not reformatted; embedding is future parser work.
- **Math diagnostics surfaced via linter + LSP** — Phase-1 diagnostics now reach
  CLI + LSP as the always-on `math-syntax` registry rule
  (`src/linter/rules/math_content.rs`), a pure CST reader (no re-parse) deriving
  the five codes off the embedded tree shape; spans are the offending tokens'
  host ranges. (This session promoted those five from Warning → Error.) The rule
  couples to the parser's `MATH_*` shaping (begin/end as `MATH_COMMAND` children
  of `MATH_ENVIRONMENT`; stray close/end in the enclosing node) — locked by
  parser golden snapshots; update in lockstep if it changes.
- **Phase 1 (parser CST) + scaffolding** — branch `feat/math-content-cst`,
  `feat(parser): parse math content into a structural CST`. Lossless
  `MATH_CONTENT` CST (groups/envs/commands/`&`/`\\`/scripts/comments/ws) +
  `MathParseReport` side-channel + `MathParseOptions`; embedded in all 8
  `inlines/math.rs` emit paths; bookdown `(\#eq:label)` → `MATH_EQUATION_LABEL`;
  fixed bookdown-crossref indexing + blockquote idempotency drift; added the
  skill, `.claude/rules/math-parser.md`, `TODO.md` note.
