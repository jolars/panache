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

**Phase 4 — dev-oracle cross-validation + idempotency corpus.** *DONE* (branch
`feat/math-oracle-corpus`, not yet committed). Pure test/validation
infrastructure; production formatter code untouched.

**The reframing that drove this** (answers the user's opening question — "what's
the best oracle, given latexindent doesn't do operator spacing/precedence?"):
math has **no output oracle**. Unlike YAML, where `pretty_yaml` *is* an output
oracle and byte-exact parity is the right assertion, nothing produces Panache's
eventual math output text (latexindent does `&`/`\\` alignment but no operator
spacing; KaTeX-class tools *render* rather than reformat). So we keep
pretty_yaml's *wiring* but flip the *assertion* from "byte-equal to oracle
output" to **"semantically equivalent to oracle parse"** —
`render(x) == render(format(x))` on normalized MathML. Invariance survives
Phases 5/6 because spacing/line-breaks are presentation a renderer collapses.

Three-tier strategy (decisions confirmed with user via AskUserQuestion):
- **Tier 1 (primary, no dep):** `tests/math_corpus_properties.rs` — idempotency +
  parser losslessness (`parse(x).text()==x`) + gate-off verbatim over the corpus.
- **Tier 2 (oracle):** `tests/math_cross_validation.rs` — dev-only
  **`pulldown-latex`** (pure-Rust LaTeX→MathML, ~95% KaTeX coverage, no JS
  engine; chosen over `katex` JS-engine crate and the immature `katex-rs` 0.2).
  Four-way rule: oracle-rejects-input → skip(counted); rejects `format(x)` only →
  **fail** (broke parseability); MathML differs → **fail** (meaning drift); equal
  → pass. Skip-fraction guard (>40% fails) keeps coverage honest.
- **Tier 3 (symbol→atom-class table):** **deferred to Phase 5** (lands with the
  interpretation module that consumes it — no orphan fixture).
- **latexindent: deferred entirely** (can't validate Phase 5/6 output; only
  duplicates Tier-1 idempotency; revisit as optional Phase 4b).

Key API notes for the oracle: `Parser::new(tex, &Storage::new())` yields
`Result<Event, ParserError>`; `push_mathml` renders parse *errors as inline error
nodes* rather than returning `Err`, so detect rejection by
`.collect::<Result<Vec<_>,_>>()` **first**, then render the validated events.
MathML (not HTML) is the surface — HTML's measured widths false-positive on
benign spacing. `MathContext`: `inline/` → Inline, everything else → Display
(`EnvironmentBody` not yet reachable; standalone `\begin{env}` still opaque
`TEX_BLOCK`).

Deliverables: `crates/panache-formatter/tests/fixtures/math_corpus/` (56 bare
`.tex` cases across inline/display/environments/groups/scripts/operators/
comments/escapes + `macro_dependent/` Tier-1-only + README), the two harness
files, and the `pulldown-latex` dev-dep (TEMPORARY note in `Cargo.toml`).

Verified: both harnesses green; `oracle_discriminates_meaning_from_spacing`
permanent test pins that the oracle isn't vacuous (`a+b`==`a + b`, `a+b`!=`a-b`);
confirmed **0/54 oracle skips** (full coverage) and that format_math is
non-identity on ~all cases (collapse/align/indent) so the invariance check
exercises real transforms; `cargo test --workspace` (29 binaries) + clippy + fmt
clean; CLI `debug format --checks all` passes on gate-on aligned math.

### Suggested next sub-targets
1. **Phase 5 — shared `math` interpretation module** (the `cooking.rs` analog):
   operator table keyed on operator text + command name → class + break-priority;
   consumed by the formatter (class-based spacing `a+b`→`a + b`) and later LSP.
   The gateway to "format with operator precedence". **Tier 3** (vendored
   symbol→atom-class table, asserted via `pulldown-latex`'s Event stream) lands
   here. Use the new corpus's `operators/` stressors (`unary_minus`,
   `double_minus`) as the regression bed.
2. **Phase 6 — semantic line-breaking + continuation indent.**
3. **Embed `MATH_CONTENT` into `TEX_BLOCK`** (parser) so bare `\begin{env}`
   blocks become formattable (would make `MathContext::EnvironmentBody` reachable).
4. Optional structural cooking (orthogonal to operators): script attachment,
   known-command argument grouping.

--------------------------------------------------------------------------------

## Earlier sessions

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
