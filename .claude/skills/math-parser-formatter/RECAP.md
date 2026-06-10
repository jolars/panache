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
  unambiguous at the character level, so they're fair game for the CST grain
  (see next-sub-target #1) — that's why `text_tail_class` peeking inside
  `MATH_TEXT` is a smell, not the class logic.

--------------------------------------------------------------------------------

## Latest session

**Phase 5 — operator interpretation module + precedence-aware spacing.** *DONE*
(not yet committed). Formatter-only + new module; **parser untouched**.

New module `crates/panache-formatter/src/formatter/math/operators.rs` (the
`cooking.rs` analog, `pub` so LSP can reuse it later): `AtomClass`
(Ord/Bin/Rel/Open/Close/Punct/Op), `split_operator_atoms`, `classify_operator`,
`command_class` (curated `\leq`/`\cdot`/`\sum`/… table → class), `text_tail_class`
(MATH_TEXT last char → Open/Close/Punct/Ord), `coerce` (TeX Bin→Ord unary rule),
`is_spaced`. Pure, in-module unit-tested.

`render.rs::render_inline` rewritten from flatten+collapse to a **gap-based
re-spacer**: flatten to `(kind,text)`, fold adjacent `MATH_OPERATOR` into a run,
**split the run into atoms** (adjacent rel chars merge — `<=`; each sign char
`+ - *` stands alone), classify+coerce each against the running prev-atom class,
then emit gap-by-gap. Gap rule: a *spaced* op (Bin/Rel) forces one space and wins
over a neighbor; a *tight* (unary) op strips adjacent space; otherwise preserve
author whitespace (so `\alpha x` and `\text{ a }` survive). Result: `a+b`→`a + b`,
`a<=b`→`a <= b`, unary `-x`/`f(-x)`/`e^{-t}` stay tight, `x=-y`→`x = -y`,
`a--b`→`a - -b`.

**The `=-` trap (caught by the new golden, not unit tests):** an early
"merge-the-whole-run" design turned `x=-y` into `x =- y`. Fix = split rule above
(rel chars merge, sign chars split so they can be unary). Lesson: relation vs
sign are different atoms; don't classify a mixed operator run as one unit.

**Scope decisions confirmed with user (AskUserQuestion):** char operators only
this slice; **command-operator spacing (`\leq`/`\cdot`) → Phase 5b** (the table is
built and used for prev-class, just not yet re-spaced); **Tier 3 symbol-class
fixture → Phase 5b**; **unary = canonicalize tight** (strip author spaces, e.g.
`- x`→`-x`), the one choice that forced the gap-based rewrite over insert-only.

Deliverables also: rewrote `cell_internal_spacing_is_preserved` →
`cell_operator_spacing_applied` + new unit tests; STYLE.md Rule 6 + idempotency
bullet (dropped the old "never operator spacing"); `format_math` config doc +
`panache.schema.json` (regen via `UPDATE_EXPECTED=1 cargo test --test
config_schema` — note: the test *name* doesn't contain "config_schema", so
`cargo test config_schema` is a no-op; use `--test`); docs/guide
{configuration,formatting}.qmd; new golden case
`tests/fixtures/cases/math_operator_spacing_experimental` (+ wired into
`tests/golden_cases.rs`).

Verified: `cargo test --workspace` (30 binaries) + clippy + fmt clean; Tier-1
corpus + Tier-2 `pulldown-latex` MathML oracle green (spacing is
meaning-preserving — `oracle_discriminates_meaning_from_spacing` still pins it
non-vacuous); gate-off goldens byte-identical; CLI `debug format --checks all`
passes on tight-operator inline + aligned display math.

### Suggested next sub-targets
1. **Parser: tokenize unambiguous delimiters/punctuation** (`( ) [ ] , ;`) out
   of `MATH_TEXT` into neutral kinds (e.g. `MATH_OPEN`/`MATH_CLOSE`/`MATH_PUNCT`;
   leave the ambiguous `| . /` as text). Lets the interpretation layer read token
   *kinds* and **deletes `operators::text_tail_class`** (the one re-lexing smell —
   today the formatter peeks at the last char of a `MATH_TEXT` run to spot `(`/`,`).
   Lossless; touches `is_special`/dispatch in `parser/math.rs`, the
   `is_math_content_token` whitelist (`syntax/math.rs`), every parser golden with
   parens in math, and must move **in lockstep with the `math_content` linter
   rule**. **Do this as the first commit of the Phase 6 slice, with the consumer**
   (line-breaking walks the structured tree and wants clean atom grain) — not
   speculatively, so the new kinds are validated against a real use. Decision
   recorded with the user 2026-06-10; deferred from Phase 5 (clean checkpoint,
   modest-but-not-zero churn). See the "CST grain vs interpretation" invariant.
2. **Phase 5b — command-operator spacing + Tier 3.** Re-space command operators
   (`a\leq b`→`a \leq b`) — handle the command-terminating space carefully — and
   land the vendored symbol→atom-class fixture validated against `pulldown-latex`
   Events. The `operators::command_class` table is already there to drive both.
3. **Phase 6 — semantic line-breaking + continuation indent** (add the
   break-priority column to `operators.rs`; use `operators/` corpus stressors).
   Walk the *structured* CST — do NOT flatten as the spacing pass does; flattening
   then relinearizing fights bracket-matching / nesting depth.
4. **Embed `MATH_CONTENT` into `TEX_BLOCK`** (parser) so bare `\begin{env}`
   blocks become formattable (would make `MathContext::EnvironmentBody` reachable).
5. Optional structural cooking (orthogonal to operators): script attachment,
   known-command argument grouping.

**Placement note (deferred, YAGNI):** `operators.rs` lives in the formatter
crate (`pub`), but the `cooking.rs` analog it mirrors lives in the *parser*
crate. Only the formatter consumes it today, so leave it; **move it to the parser
crate if/when a second consumer appears** (linter wanting atom info, LSP semantic
tokens) so formatter + linter + LSP share one interpretation.

--------------------------------------------------------------------------------

## Earlier sessions

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
