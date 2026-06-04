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

**Phase 1 (parser CST) landed** — branch `feat/math-content-cst`, commit
`feat(parser): parse math content into a structural CST`. Plus this session
added the in-repo scaffolding (this skill, `.claude/rules/math-parser.md`,
`TODO.md` note).

Shipped:
- `crates/panache-parser/src/parser/math.rs` — lossless structural TeX CST under
  `MATH_CONTENT` (groups, environments, commands, `&`, `\\`, scripts, comments,
  whitespace) + `MathParseReport` diagnostics side-channel + `MathParseOptions`.
- Embedded unconditionally in all 8 `inlines/math.rs` emit paths (replaces the
  opaque content `TEXT` token); AST accessors + pandoc projector read via the
  shared `math_content_text()`.
- Bookdown `(\#eq:label)` → `MATH_EQUATION_LABEL` token (gated on
  `bookdown_equation_references`); both salsa indexers walk it.
- Fixed two bugs: bookdown crossref indexing (now CST-driven) and
  display-math-in-blockquote idempotency drift.
- 17 parser CST snapshots updated. Workspace: 3726 passed / 0 failed; clippy +
  fmt clean.

### Suggested next sub-targets
1. **Surface math diagnostics via linter + LSP** (independent of formatter work).
2. **Phase 2** — formatter `experimental_math_formatting` gate + verbatim/inline
   split (regenerate `panache.schema.json`).
3. **Representative TeX corpus** (Phase 0 leftover) to drive Phases 2–4.

--------------------------------------------------------------------------------

## Earlier sessions

- (none — Phase 1 + scaffolding was the first session)
