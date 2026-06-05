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

--------------------------------------------------------------------------------

## Latest session

**Parse operators into `MATH_OPERATOR` (parser-only).** Split `+ - * = < >` out
of the catch-all `MATH_TEXT` atom runs into a dedicated **neutral**
`MATH_OPERATOR` token — one token per char — so a future formatter phase can do
operator-aware spacing/precedence. NOT committed yet; sits on top of the existing
working tree (still effectively `feat/math-content-cst` lineage).

The session was mostly a **design debate the user opened twice** and we
converged; the conclusion is now a persistent invariant above (operator
class/precedence is interpretation, not CST — the `cooking.rs` analog). Net: the
parser only tokenizes; the eventual class/precedence table is a *shared
formatter/LSP module*, never `MATH_BIN_OP`/`MATH_REL_OP` kinds. The user
initially wanted a "fully-cooked CST" (YAML scanner→events→cook-once pattern);
the resolution is that YAML cooks *structure the grammar defines* into the tree
but cooks *scalar value interpretation* as a shared pure fn — and operator class
is the latter kind.

Changes (all parser-crate + one whitelist):
- `syntax/kind.rs`: new `MATH_OPERATOR` variant (comment block above it).
- `parser/math.rs`: dispatch arm `c if is_operator(c) => bump 1 MATH_OPERATOR`
  *before* the `parse_text` fallback; new `is_operator()` (`+ - * = < >`);
  `is_special()` now `is_operator(c) || matches!(…)` so text runs stop at ops.
- `syntax/math.rs`: added `MATH_OPERATOR` to the `is_math_content_token()`
  whitelist — **critical**, else `math_content_text()` (formatter re-parse,
  projector, salsa) drops operators and breaks losslessness.
- **Formatter unchanged**: `render.rs::push_token` already routes non-space
  tokens through `push_str(text)`, so OFF and ON paths stay byte-identical;
  operator-aware formatting is a separate future phase.

Scope notes:
- Command operators (`\cdot`, `\leq`, …) stay `MATH_COMMAND` (classify by name in
  the future shared module). `( ) [ ] / : | , ;` stay `MATH_TEXT` (out of scope).
- One token per char, no coalescing of adjacent ops (`a<=b` → `OP(<) OP(=)`);
  unary vs binary minus NOT distinguished (both `MATH_OPERATOR`).

Verified: math parser unit tests (79) incl. updated `plain_text_is_one_atom_run`
+ `line_break_alignment_and_scripts` and new operator tests; new golden fixture
`inline_math_operators`; **17 parser CST snapshots** regenerated — audited every
changed line = pure `MATH_TEXT`→`MATH_OPERATOR` retag / text-run split over
identical byte ranges, zero structural/nesting diffs; full `cargo test
--workspace` green (formatter math goldens byte-identical); clippy + fmt clean;
CLI `parse` shows the tokens and `debug format --checks all` passes (lossless +
idempotent) on inline + display `&`-aligned operator math.

### Suggested next sub-targets
1. **Shared `math` interpretation module** (the `cooking.rs` analog): operator
   table keyed on operator text + command name → class + break-priority; consumed
   by the formatter (class-based spacing) and later LSP. This is the gateway to
   "format with operator precedence" (the user's stated goal — both spacing AND
   precedence-aware line-breaking of long display math).
2. **Phase 4** — dev-oracle (latexindent/KaTeX) cross-validation + idempotency
   corpus harness.
3. **Embed `MATH_CONTENT` into `TEX_BLOCK`** (parser) so bare `\begin{env}`
   blocks become formattable, then extend the formatter to them.
4. Optional structural cooking (legit parser work, orthogonal to operators):
   script attachment, known-command argument grouping.

--------------------------------------------------------------------------------

## Earlier sessions

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
