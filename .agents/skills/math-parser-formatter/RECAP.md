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
  (`LINE_PREFIX`, and sometimes host `NEWLINE`) into `MATH_CONTENT` on
  continuation lines; the helper strips them by whitelisting `MATH_*` tokens.
  Reading `.text()` directly leaks the `>` and re-accumulates it every format
  pass (a real idempotency bug that was fixed in Phase 1).
- **`MATH_SPACE`/`MATH_NEWLINE` are intentionally distinct** from host
  `WHITESPACE`/`NEWLINE` — that distinction is what makes the helper above work.
  `MATH_SPACE` is load-bearing (collides with blockquote-prefix `WHITESPACE`
  otherwise); `MATH_NEWLINE` is kept for symmetry.
- **Parser is unconditional; the experimental gate is formatter-side only.**
- **Panache owns its complete production math stack.** Runtime code must not
  depend on Badness, retain a Badness CST, share a math crate, delegate
  formatting, or project between runtime trees. Exact, pinned Badness crates are
  permitted as development-only parser and formatter oracles.
- **Badness defines the target math CST and formatting style.** Panache's math
  subtree is structurally isomorphic after mechanical `MATH_*` renaming and
  removal of host-only trivia. Formatter output matches Badness byte-for-byte,
  except that Panache aligns trailing `\\` markers in alignment-capable
  environments.
- **Test projectors are deliberately weak.** They may rename kinds, adjust
  wrapper offsets, and discard documented host trivia, but never infer command
  arguments, attach scripts, repair recovery, or otherwise parse TeX.
- **Background revert trap**: a process (suspected pre-commit `git stash`)
  reverted tracked edits once mid-session; untracked files survived. Re-apply if
  source edits vanish.
- **Operator class/precedence is NOT a CST concern.** The target parser emits a
  Badness-equivalent `MATH_WORD` run, not the old `MATH_OPERATOR`, `MATH_OPEN`,
  `MATH_CLOSE`, or `MATH_PUNCT` grain. A semantic atom iterator slices Unicode
  scalars and classifies them. Rationale:
  TeX assigns atom class contextually during mlist→hlist (Appendix G coerces a
  Bin atom after Bin/Rel/Open/Punct to Ord — that *is* unary minus), it's
  override-able (`\mathbin`) and macro-dependent, and there is no
  operator-precedence grammar in TeX (the math list is flat). Class/precedence is
  the analog of YAML's *scalar cooking* (`parser/yaml/cooking.rs`): a pure
  interpretation shared between consumers, NOT a tree shape. So when the
  formatting phase needs class+precedence, build a **shared `math` interpretation
  module** (operator table keyed on operator text *and* command name → class +
  break-priority) consumed by formatter + LSP — never `MATH_BIN_OP`/`MATH_REL_OP`
  kinds. The existing formatter-local `operators.rs` and fine-grained tokens are
  migration residue; replace them with the Panache-owned Badness-parity semantic
  model.
- **CST grain vs interpretation — the line to hold.** A *fact* (unambiguous from
  the bytes, no macro escape) belongs in the CST grain; a *guess* (fallible
  without macro expansion, which we don't do) belongs in the interpretation
  layer (`operators.rs`), never the CST. Operator **class** (bin/rel/unary) is a
  guess — `\mathbin`/`\def`/`\mathcode` can override it — so it stays neutral in
  the semantic layer even though the parser has the same local bytes. Match
  Badness's lexical grain rather than encoding a partial character-class table
  in Panache's token kinds.
- **Scripts are native CST structure.** `MATH_SCRIPTED` owns one base atom and
  `MATH_SUBSCRIPT`/`MATH_SUPERSCRIPT` children. Unbraced text bases and
  arguments split at Unicode-scalar boundaries; comments and blank lines stop
  attachment. Formatter interpretation must inherit the base atom's class
  across the script—especially for scripted relation and assignment breaks.

--------------------------------------------------------------------------------

## Latest session

**Badness parser oracle—first parity slice.** Added the exact, development-only
`badness-parser = "=0.4.0"` oracle and mechanical canonical projectors for
Badness `MATH` and Panache `MATH_CONTENT` subtrees.

- The projectors compare kind names, ownership, body-relative byte ranges,
  token text, and exact source gaps. They only shift the `$...$` wrapper and
  document Panache's host-trivia omission; legacy kinds remain visible as
  divergences rather than being repaired.
- Four shared-corpus cases now form the mandatory passing slice (nested groups
  and scripts). The reproducible ignored report records 4/58 passing and 54/58
  divergent cases with both canonical trees.
- No production parser or formatter code changed.

### Suggested next sub-targets
1. Lock the one-to-one kind map and migrate the lexical grain.
2. Give commands Badness-equivalent argument ownership, then grow the mandatory
   parity corpus from the report's passing candidates.
3. Add the pinned formatter oracle only when starting formatter-output parity.

--------------------------------------------------------------------------------

## Earlier sessions

- **Badness-parity roadmap revision.** Made Badness the normative development
  oracle while retaining Panache-owned production parsing and formatting.
- **Native script CST—first roadmap slice.** Added scripted/subscript/superscript
  nodes, typed wrappers, Unicode-scalar attachment, and temporary legacy-renderer
  support; full parity still depends on Badness-equivalent command atoms.
- **Earlier native math-stack roadmap.** Recorded the first independent-stack
  plan in `TODO.md`; the Badness-parity roadmap now supersedes it.
- **Composable embedded math environments.** Added the math-local Wadler-style
  document model, structured mixed-environment layout, and focused idempotency
  and MathML coverage; unsupported shapes remain verbatim.
- **`MATH_DELIMITED`-aware line-break scanning.** Delimited nodes are opaque
  operands; `left`/`right` left the command-class table, with nested/asymmetric
  delimiter and following-binary regression coverage.
- **Phase 6 indent budget; greedy packing rejected.** The flat `math-indent` is
  charged against `line-width`; binary breaks remain one operator per line
  after a greedy line-fill experiment produced semantically arbitrary ragging.
- **Phase 6 commit 1 — tokenize delimiters/punctuation** (`7249710c`). Parser
  splits `( [`→`MATH_OPEN`, `) ]`→`MATH_CLOSE`, `, ;`→`MATH_PUNCT` (`| . /` stay
  text); formatter's `text_tail_class` replaced by kind-keyed
  `operators::delimiter_class`; all three kinds added to the `math_content_text`
  whitelist. No behavior change.
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
  skill and `TODO.md` note.
