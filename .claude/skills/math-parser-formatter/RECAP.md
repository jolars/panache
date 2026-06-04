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

--------------------------------------------------------------------------------

## Latest session

**Math formatter (Phases 2+3) + `math-syntax` → Error** (branch
`feat/math-content-cst`). Shipped the experimental content-aware math formatter
and promoted the lint severity.

Decisions locked with the user this session:
1. **Config shape is `[experimental] format-math` (default false)**, a *new
   top-level* host-config table — NOT `experimental_math_formatting` under
   `[format]` as the old plan suggested. Documented as opt-in / API-may-break.
   Mirrored onto formatter `Config::experimental_format_math`; plumbed in
   `to_formatter_config`. Schema regenerated (`ExperimentalConfig`).
2. **`math-syntax` diagnostics promoted Warning → Error.** The channel (a
   suppressible registry rule) was right, but warning *severity* undersold a
   build-breaking error (`quarto render` hard-fails). One-line change in the
   `err()` helper (`src/linter/rules/math_content.rs`); docs updated.
3. **Env-body indent hardcoded to 2 spaces** (opinionated; `math_indent` left
   untouched — it still flat-indents non-environment `$$` content). Promote to a
   knob later only on pushback.
4. **Full scope incl. `&`-alignment** (not deferred).
5. **Operators not being tokenized is NOT a blocker** — every transform is
   structural (keys off `&`/`\\`/env/group tokens); cells are opaque text
   measured by *source* char width. Operator spacing stays out of scope.

Architecture (`crates/panache-formatter/src/formatter/math/` + `math.rs`):
- **Re-parses the clean content string** (`parse_math_report`) like the YAML
  formatter does, NOT a walk of the host-embedded subtree — dissolves the
  blockquote-prefix problem. `format_math(content, opts) -> String`, delimiter-
  free in/out; bails to verbatim on gate-off, lone-`$`, or any parse diagnostic.
- `render.rs`: `MathContext::{Inline, Display, EnvironmentBody}`. Rows split on
  top-level `\\`/newline; cells on top-level `&`; **trim-before-measure +
  trailing-only padding** is the idempotency engine (see `STYLE.md`). Canonical
  separator ` & ` (latexindent style — `&=` becomes `& =`). **Trailing `\\` also
  align**: the last column is padded too, but only on rows that carry a `\\`
  (a final/soft-break row's last cell stays unpadded → no trailing whitespace).
  Cell internals (operator spacing) never touched.
- Gated at three call sites, OFF byte-identical: `core.rs` block DISPLAY_MATH
  (env + non-env), `inline.rs` INLINE_MATH (inline + display sub-form) and
  standalone DISPLAY_MATH. Gated inline read switched to `InlineMath::content()`
  (prefix-stripping).

Verified: 12 sub-formatter unit tests + 4 format-API integration tests + 3
golden cases (`math_align_experimental`, `math_blockquote_experimental`,
`math_format_off_default`); 284 host goldens green (OFF path byte-identical);
CLI `debug format --checks all` passes (idempotency + losslessness) on `$$`,
`\[`, and **blockquote** display math. Clippy + fmt clean.

Traps / scope notes:
- **Standalone `\begin{env}…\end{env}` TeX blocks parse as `TEX_BLOCK` with
  OPAQUE `TEXT` (no `MATH_CONTENT`)** — Phase 1 only embedded structure into
  `$$`/`$`/`\[`/`\(` spans. So the formatter does NOT reformat bare TeX blocks;
  embedding `MATH_CONTENT` into `TEX_BLOCK` is a parser change (future work).
- **R3 (blockquote re-prefixing) resolved**: the block emitter re-prefixes each
  formatted line with `> ` automatically; multi-line aligned bodies in `>` work.
- `MathContext::EnvironmentBody` (raw `\begin` *delimiters*) is wired + unit-
  tested but rarely hit at block level (those are `TEX_BLOCK`s); kept defensive.

### Suggested next sub-targets
1. **Phase 4** — dev-oracle (latexindent/KaTeX) cross-validation + idempotency
   corpus harness.
2. **Embed `MATH_CONTENT` into `TEX_BLOCK`** (parser) so bare `\begin{env}`
   blocks become formattable, then extend the formatter to them.
3. Representative TeX corpus (Phase 0 leftover).

--------------------------------------------------------------------------------

## Earlier sessions

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
