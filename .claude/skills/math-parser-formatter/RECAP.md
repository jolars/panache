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

**Math diagnostics surfaced via linter + LSP** (branch `feat/math-content-cst`).
The Phase-1 diagnostics (unclosed/mismatched braces + environments) were
computed at parse and discarded; they now reach CLI + LSP as the always-on
**`math-syntax`** lint rule.

Two design decisions (locked with the user):
1. **Registry lint rule**, NOT the parser's `SyntaxError` channel. YAML *must*
   validate at parse time because it changes CST shape on error; math **always**
   embeds losslessly, so the verdict is pure linter policy. A registry rule also
   gets `[lint.rules]` opt-out + ignore-directive filtering and keeps the parser
   lossless-only.
2. **Derive diagnostics from the embedded CST shape, NOT a re-parse.** The first
   cut reconstructed the content, re-ran `parse_math_report`, and mapped offsets
   through a `math_content_segments` helper — three passes over content already
   parsed once (smelly; half-violates `math-parser.md`'s "no re-parse" rule).
   The rule now reads the five conditions straight off the tree; the embedded
   `MATH_*` tokens already carry host ranges, so a span is just the offending
   token's `text_range()` — no reconstruction, no offset mapping, and the
   blockquote-prefix wrinkle vanishes. Key realization: the lossless CST is
   *designed* for downstream tools to recompute structural facts, so reading the
   tree is the intended pattern; the parser's inline-emission offset gap
   (`ParagraphBuffer` reconstructs inline text as a fresh `String`) is real but
   irrelevant here because the **final red tree already has correct host
   offsets** — not worth a refactor.

CST signatures the rule reads (`src/linter/rules/math_content.rs`):
- `math-unclosed-group` — `MATH_GROUP` with no `MATH_GROUP_CLOSE` child (→ `{`).
- `math-unexpected-close-brace` — `MATH_GROUP_CLOSE` whose parent ≠ `MATH_GROUP`.
- `math-unclosed-environment` — `MATH_ENVIRONMENT` with no `\end` command (→ `\begin`).
- `math-mismatched-environment` — env begin/end name groups differ (→ end name).
- `math-unexpected-end` — `\end` `MATH_COMMAND` whose parent ≠ `MATH_ENVIRONMENT`.

Shipped:
- `src/linter/rules/math_content.rs` — `MathContentRule` (`name = "math-syntax"`),
  pure CST reader; per-code kebab Warnings. No parser-crate changes (the
  earlier `math_content_segments` helper + `parser::math` re-export were
  removed). `pub mod` in `linter/rules.rs`; registered always-on in `linter.rs`
  (no math ext ⇒ no math nodes ⇒ no-op). No `lsp`/`salsa` changes.
- Docs: `docs/reference/linter-rules.qmd` umbrella `### math-syntax` + five
  `####` sub-anchors (so the renderer's `#{code}` help URL resolves, matching
  the `undefined-references` multi-code convention).
- Tests: 10 rule unit tests (5 codes + well-formed + nested envs + blockquote
  host-range guard); LSP
  `tests/lsp/test_diagnostics.rs::test_math_parse_error_in_built_in_lint_plan`.
- Workspace green / 0 failed; clippy + fmt clean. CLI smoke verified.

Traps noted:
- The renderer suggests `[lint.rules] {code} = false`, but disabling is gated on
  the umbrella **rule name** (`math-syntax`), not the code — same pre-existing
  behavior as `undefined-references`. Not a regression.
- The CST-reader couples to the parser's `MATH_*` shaping (begin/end as
  `MATH_COMMAND` children of `MATH_ENVIRONMENT`, stray close/end emitted into the
  enclosing node). That contract is locked by the parser golden snapshots; if it
  changes, update this rule in lockstep.

### Suggested next sub-targets
1. **Phase 2** — formatter `experimental_math_formatting` gate + verbatim/inline
   split (regenerate `panache.schema.json`).
2. **Representative TeX corpus** (Phase 0 leftover) to drive Phases 2–4.

--------------------------------------------------------------------------------

## Earlier sessions

- **Phase 1 (parser CST) + scaffolding** — branch `feat/math-content-cst`,
  `feat(parser): parse math content into a structural CST`. Lossless
  `MATH_CONTENT` CST (groups/envs/commands/`&`/`\\`/scripts/comments/ws) +
  `MathParseReport` side-channel + `MathParseOptions`; embedded in all 8
  `inlines/math.rs` emit paths; bookdown `(\#eq:label)` → `MATH_EQUATION_LABEL`;
  fixed bookdown-crossref indexing + blockquote idempotency drift; added the
  skill, `.claude/rules/math-parser.md`, `TODO.md` note.
