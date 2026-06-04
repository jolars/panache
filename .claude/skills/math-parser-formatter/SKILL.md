---
name: math-parser-formatter
description: Incrementally build Panache's math parser and formatter — a lossless
  structural TeX CST for inline/display math, then content-aware reformatting
  behind an experimental gate — one bounded phase at a time.
---

Use this skill when asked to advance Panache's math parsing/formatting, fix a
math-CST or math-formatting issue, or pick the next phase of this effort.

This is a **long-horizon, multi-session effort**. Each session moves one phase
or sub-task forward; do not attempt sweeping rewrites in one go. The standing
design plan is `~/.claude/plans/i-want-to-plan-foamy-pascal.md`.

## Scope boundaries

- Parser: `crates/panache-parser/src/parser/math.rs` (the TeX content parser),
  embedded via `crates/panache-parser/src/parser/inlines/math.rs`, with AST
  accessors in `crates/panache-parser/src/syntax/{math,inlines}.rs`.
- Formatter (future phases): a new `crates/panache-formatter/src/formatter/math/`
  mirroring `formatter/yaml/`, gated behind an experimental option.
- Goal: parse math *content* into a lossless structural CST (done, Phase 1) and
  reformat it **semantics-safely** (align `&` columns, indent environment
  bodies, normalize `\\`, collapse spaces) — never macro rewriting, never
  `\frac`/`\dfrac` canonicalization, never operator-spacing policy.
- **There is no pandoc oracle for math *formatting*** — pandoc passes math
  content through untouched. Use an external dev-only oracle (latexindent /
  KaTeX parser) for cross-validation, à la `pretty_yaml` for YAML.

## Locked-in design decisions (do not relitigate)

- **Parser is unconditional + lossless**; the experimental gate lives on the
  **formatter** side (default off = emit math verbatim, today's behavior). The
  gate is a formatter-config option, NOT a Pandoc `Extensions` flag.
- **texlab, not KaTeX**, is the parser model (lossless, error-tolerant vs.
  lossy, throwing).
- **Diagnostics ride a side-channel** (`MathParseReport`), to be surfaced via
  linter + LSP — not the CST.
- **Bookdown equation labels** (`(\#eq:label)`) are parsed into a
  `MATH_EQUATION_LABEL` token, gated on `bookdown_equation_references`.
- **`MATH_SPACE`/`MATH_NEWLINE` stay distinct** from host `WHITESPACE`/`NEWLINE`
  so `math_content_text()` can strip container prefixes the block machinery
  interleaves into `MATH_CONTENT` (blockquote `>` etc.). See `.claude/rules/math-parser.md`.

## Related rules to read first

- `.claude/rules/math-parser.md` — the math-parser invariants.
- `.claude/rules/parser.md` — single-pass + lossless CST.
- `.claude/rules/formatter.md` — idempotency; idempotency drift is often a
  parser-shape bug, not a formatter bug.

## Phased plan (status)

- **Phase 0 — scaffolding.** SyntaxKinds, this skill + rule, corpus. *Skill/rule
  DONE; representative TeX corpus still TODO.*
- **Phase 1 — TeX tokenizer + structural CST (parser).** *DONE* — committed on
  branch `feat/math-content-cst` (`feat(parser): parse math content into a
  structural CST`). Lossless `MATH_CONTENT` CST, diagnostics side-channel,
  bookdown labels, accessors/projector/indexers updated.
- **Phase 2 — formatter experimental gate + inline math.** Add
  `experimental_math_formatting` (default false) to the formatter config, mirror
  onto host `Config`, regenerate `panache.schema.json`. Off → verbatim; on →
  conservative inline spacing normalization.
- **Phase 3 — display math + environments.** `&`-column alignment,
  environment-body indentation (respect `math_indent`), `\\` normalization.
  Honor `has_unescaped_single_dollar_in_content()` preservation.
- **Phase 4 — dev-oracle cross-validation + idempotency corpus.**
- **Phase 5 — docs + stabilization** (`docs/guide/formatting.qmd`,
  `configuration.qmd`); consider flipping the gate per flavor (separate
  decision).
- **Surface math diagnostics via linter/LSP** — independent of the formatter
  phases; can land any time after Phase 1.

## Session workflow

1. Read `RECAP.md` (status, traps, next sub-targets) and the rules above.
2. Pick one bounded phase/sub-task.
3. TDD: add the failing test first (parser golden / formatter golden / unit).
4. Validate before landing:
   - `cargo test --workspace`
   - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
   - `cargo fmt -- --check`
   - For parser CST snapshot changes: review each diff (byte ranges must still
     reconstruct the input losslessly).
   - Flag-off regression: existing formatter goldens stay byte-identical.
5. Rewrite `RECAP.md`'s Latest-session entry; add a one-line Earlier-sessions
   note.

## Traps

- A background process (suspected pre-commit `git stash`) reverted tracked edits
  once mid-session; untracked files survived. If source edits vanish, re-apply.
- Don't read raw math content via `MATH_CONTENT.text()` — use
  `syntax::math::math_content_text()` (strips host container prefixes).
