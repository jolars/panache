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
- Goal: parse math *content* into a lossless structural CST (Phase 1 done;
  operator atoms now tokenized) and reformat it **semantics-safely**. *In scope*:
  align `&` columns, indent environment bodies, normalize `\\`, collapse spaces
  (done); **operator-precedence-aware spacing** (`a+b` → `a + b`, class-based);
  and **semantic line-breaking + indenting of long display math** (wrap at the
  lowest-precedence operators, indent continuations). *Out of scope*: macro
  rewriting, `\frac`/`\dfrac` canonicalization, and anything that needs macro
  expansion.
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
- **Operators are tokenized but never classified in the CST.** `+ - * = < >`
  emit a neutral `MATH_OPERATOR` token (one per char); bin/rel/precedence is
  *interpretation* (contextual unary minus, `\mathbin`, macro-dependent) — the
  analog of YAML scalar cooking. It lives in a **shared formatter/LSP module**
  keyed on operator text + command name (class + break-priority), never
  `MATH_BIN_OP`/`MATH_REL_OP` kinds. That module is the gateway to both
  precedence-aware spacing and semantic line-breaking.

## Related rules to read first

- `.claude/rules/math-parser.md` — the math-parser invariants.
- `.claude/rules/parser.md` — single-pass + lossless CST.
- `.claude/rules/formatter.md` — idempotency; idempotency drift is often a
  parser-shape bug, not a formatter bug.

## Phased plan (status)

- **Phase 0 — scaffolding.** SyntaxKinds, this skill + rule, corpus. *Skill/rule
  DONE; representative TeX corpus still TODO.*
- **Phase 1 — TeX tokenizer + structural CST (parser).** *DONE*. Lossless
  `MATH_CONTENT` CST, diagnostics side-channel, bookdown labels,
  accessors/projector/indexers.
- **Phase 1b — operator atoms (parser).** *DONE* (`feat(parser): tokenize math
  operators into MATH_OPERATOR`). Neutral `MATH_OPERATOR` token, no class.
- **Phase 2 — formatter experimental gate + inline math.** *DONE*. Gate is
  `[experimental] format-math` (default false), mirrored onto
  `Config::experimental_format_math`, schema regenerated. Off → verbatim; on →
  inline spacing normalization.
- **Phase 3 — display math + environments.** *DONE*. `&`-column alignment,
  environment-body indentation, `\\` normalization; honors
  `has_unescaped_single_dollar_in_content()`.
- **Phase 4 — dev-oracle cross-validation + idempotency corpus.** *DONE*.
  Tier-1 corpus props + Tier-2 `pulldown-latex` MathML invariance oracle.
- **Phase 5 — operator interpretation module + precedence-aware spacing.**
  *DONE*. `formatter/math/operators.rs` (`cooking.rs` analog, `pub` for LSP):
  classify char operators + curated command table → class; TeX Bin→Ord coercion;
  gap-based re-spacer (`a+b`→`a + b`, unary `-x`/`f(-x)` tight, `x=-y`→`x = -y`).
  Char operators only; **command-operator spacing + Tier 3 → Phase 5b**;
  break-priority column → Phase 6.
- **Phase 5b — command-operator spacing + Tier 3.** Re-space `\leq`/`\cdot`
  (handle command-terminating space); vendored symbol→atom-class fixture vs
  `pulldown-latex` Events.
- **Phase 6 — semantic line-breaking + indenting.** Wrap long display math at
  lowest-precedence operators, indent continuations (uses Phase 5 priorities).
- **Phase 7 — docs + stabilization** (`docs/guide/formatting.qmd`,
  `configuration.qmd`); consider flipping the gate per flavor (separate
  decision).
- **Surface math diagnostics via linter/LSP** — *DONE* (promoted Warning→Error).
- **Optional structural cooking (parser, orthogonal to operators):** script
  attachment, known-command argument grouping — legit future CST work if a
  formatting phase needs the structure.

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
