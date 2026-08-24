---
name: math-parser-formatter
description: Incrementally build Panache's math parser and formatter — a lossless
  structural TeX CST for inline/display math, then content-aware reformatting
  behind an experimental gate — one bounded phase at a time.
---

Use this skill when asked to advance Panache's math parsing/formatting, fix a
math-CST or math-formatting issue, or pick the next phase of this effort.

This is a **long-horizon, multi-session effort**. Each session moves one phase
or sub-task forward; do not attempt sweeping rewrites in one go. Current state
and design decisions live in this skill and `RECAP.md`.

## Scope boundaries

- Parser: `crates/panache-parser/src/parser/math.rs` (the TeX content parser),
  embedded via `crates/panache-parser/src/parser/inlines/math.rs`, with AST
  accessors in `crates/panache-parser/src/syntax/{math,inlines}.rs`.
- Formatter (future phases): a new `crates/panache-formatter/src/formatter/math/`
  mirroring `formatter/yaml/`, gated behind an experimental option.
- Goal: make Panache's math-content CST structurally isomorphic to Badness's and
  reproduce Badness's math formatting style, while retaining a wholly native
  production parser, semantic model, CST, and formatter. The sole intentional
  style extension is alignment of trailing `\\` markers in alignment-capable
  environments. *Out of scope*: macro rewriting, `\frac`/`\dfrac`
  canonicalization, and anything that needs macro expansion.
- **There is no pandoc oracle for math *formatting*** — pandoc passes math
  content through untouched. Use exact, pinned `badness-parser` and
  `badness-formatter` development dependencies as the structural and output
  oracles. Retain independent MathML and TeX/PDF checks for meaning preservation.

## Locked-in design decisions (do not relitigate)

- **Parser is unconditional + lossless**; the experimental gate lives on the
  **formatter** side (default off = emit math verbatim, today's behavior). The
  gate is a formatter-config option, NOT a Pandoc `Extensions` flag.
- **Badness is the parser model.** Match its lossless, error-tolerant CST and
  recovery through the pinned development-only oracle; do not substitute a
  lossy or throwing math parser.
- **Diagnostics ride a side-channel** (`MathParseReport`), to be surfaced via
  linter + LSP — not the CST.
- **Host-only constructs stay outside TeX math where possible.** Markdown
  delimiters, Bookdown equation labels (`(\#eq:label)`), Pandoc attributes, and
  container prefixes belong to the host layer. Existing embedded label tokens
  are migration residue, not the target CST.
- **`MATH_SPACE`/`MATH_NEWLINE` stay distinct** from host `WHITESPACE`/`NEWLINE`
  so `math_content_text()` can strip container prefixes the block machinery
  interleaves into `MATH_CONTENT` (blockquote `>` etc.).
- **The math CST follows Badness's lexical grain.** Ordinary characters live in
  a Badness-equivalent `MATH_WORD` run; `MATH_OPERATOR`, `MATH_OPEN`,
  `MATH_CLOSE`, and `MATH_PUNCT` are migration residue from the old formatter.
  A Panache-owned semantic atom iterator slices Unicode scalars and derives
  operator class, delimiter role, unary coercion, and break priority exactly as
  Badness does.
- **Badness is test-only.** Production code must not depend on Badness, retain a
  Badness CST, delegate formatting, or project between runtime trees. Test-only
  projectors may mechanically rename kinds, remove wrapper offsets, and discard
  documented host trivia; they must never parse, infer attachment, or repair a
  tree.

Follow the math-parser, parser, and formatter invariants in the repository's
root `AGENTS.md`.

## Phased plan (status)

- **Phase 0 — scaffolding.** SyntaxKinds, this skill + rule, corpus. *Skill/rule
  DONE; representative TeX corpus still TODO.*
- **Phase 1 — TeX tokenizer + structural CST (parser).** *DONE*. Lossless
  `MATH_CONTENT` CST, diagnostics side-channel, bookdown labels,
  accessors/projector/indexers.
- **Phase 1b — operator atoms (parser).** *LEGACY; TO BE REPLACED*. The old
  formatter introduced `MATH_OPERATOR` and related fine-grained tokens. The
  Badness-compatible CST returns ordinary characters to `MATH_WORD` and exposes
  atoms through the semantic layer.
- **Phase 2 — formatter experimental gate + inline math.** *DONE*. Gate is
  `[experimental] format-math` (default false), mirrored onto
  `Config::experimental_format_math`, schema regenerated. Off → verbatim; on →
  inline spacing normalization.
- **Phase 3 — display math + environments.** *DONE*. `&`-column alignment,
  environment-body indentation, `\\` normalization; honors
  `has_unescaped_single_dollar_in_content()`.
- **Phase 4 — dev-oracle cross-validation + idempotency corpus.** *DONE*.
  Tier-1 corpus props + Tier-2 `pulldown-latex` MathML invariance oracle.
- **Phase 5 — legacy operator interpretation + precedence-aware spacing.**
  *DONE ON THE OLD PATH; TO BE REPLACED*. `formatter/math/operators.rs`:
  classify char operators + curated command table → class; TeX Bin→Ord coercion;
  gap-based re-spacer (`a+b`→`a + b`, unary `-x`/`f(-x)` tight, `x=-y`→`x = -y`).
  Char operators only; **command-operator spacing + Tier 3 → Phase 5b**;
  break-priority column → Phase 6.
- **Phase 5b — legacy command-operator spacing + Tier 3.** *DONE ON THE OLD
  PATH; TO BE REPLACED*. Re-spaced
  `\leq`/`\cdot` (command-terminating space handled, never `TightOp`); landed the
  dev-only vendored symbol→atom-class fixture (`tests/fixtures/math_symbol_classes/`)
  cross-checked against `pulldown-latex` Events. `\lim`/`\asymp` divergences
  recorded, not corrected.
- **Phase 6 — legacy semantic line-breaking + indenting.** *DONE IN PART ON THE
  OLD PATH; TO BE REPLACED BY BADNESS-PARITY LOWERING.*
  - *Commit 1 DONE*: parser tokenizes delimiters/punctuation (`( [` →
    `MATH_OPEN`, `) ]` → `MATH_CLOSE`, `, ;` → `MATH_PUNCT`; `| . /` stay text);
    formatter's `text_tail_class` replaced by kind-keyed `operators::delimiter_class`.
    No behavior change.
  - *Commit 2 DONE* (`9d7c2e5b`): `operators::break_priority` (Rel > Bin > 0) +
    new `formatter/math/linebreak.rs`. Over-width display **free rows** break at
    depth-0 relations (≥2), continuations align under the first relation; depth
    tracked via open/close counter (`(`/`[`/`\left` vs `)`/`]`/`\right`), brace
    groups opaque. `line_width` threaded onto `MathFormatOptions`. Idempotency:
    `render.rs::split_logical_rows` joins soft newlines into one logical row
    (only `\\` splits) — except a `%`-comment-terminating newline (significant,
    or the next line is absorbed into the comment).
  - *Commit 3 DONE*: nested **binary** breaking inside an over-width relation
    segment — each `+ term` nests one indent step deeper (under the relation
    RHS). `linebreak.rs` now uses `spaced_operator_breaks` (depth-0, coerced, so
    unary signs excluded) + `break_binary_segment`; `render_inline_seeded(_,
    Some(Close))` keeps a leading-`+` continuation binary (not unary) in
    isolation. **Scope:** binary breaking only WITHIN a relation chain (≥2 rels);
    standalone binary chains / single-relation / no-relation rows stay one line.
    Remaining: binary breaking outside a relation chain, environment-body
    breaking, min-breaks-to-fit.
- **Phase 7 — docs + stabilization** (`docs/guide/formatting.qmd`,
  `configuration.qmd`); consider flipping the gate per flavor (separate
  decision).
- **Surface math diagnostics via linter/LSP** — *DONE* (promoted Warning→Error).
- **Current redesign — Badness parity.** Add pinned dev-only oracles and minimal
  projectors; align the lexical and structural CST; port the signature and math
  semantics; then replace the legacy renderer with Badness-parity typed lowering
  and layout. See `TODO.md` for the authoritative sequence.

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
   - Run the focused Badness parser and formatter parity suites for every
     migrated slice.
   - Flag-off regression: existing formatter goldens stay byte-identical.
5. Rewrite `RECAP.md`'s Latest-session entry; add a one-line Earlier-sessions
   note.

## Traps

- A background process (suspected pre-commit `git stash`) reverted tracked edits
  once mid-session; untracked files survived. If source edits vanish, re-apply.
- Don't read raw math content via `MATH_CONTENT.text()` — use
  `syntax::math::math_content_text()` (strips host container prefixes).
