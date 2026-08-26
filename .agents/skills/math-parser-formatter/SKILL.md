---
name: math-parser-formatter
description: Advance Panache's math parser and formatter one bounded phase at a
  time. Use when implementing or debugging the TeX math CST, semantic model,
  Badness parity, math formatting, or the next math-roadmap task.
---

This is a multi-session effort. Project source paths below are relative to the
repository root. Skill resources linked from this file are relative to this
`SKILL.md` file.

## Scope boundaries

- Parser: `crates/panache-parser/src/parser/math.rs` (the TeX content parser),
  embedded via `crates/panache-parser/src/parser/inlines/math.rs`, with AST
  accessors in `crates/panache-parser/src/syntax/{math,inlines}.rs`.
- Formatter: `crates/panache-formatter/src/formatter/math/`, gated behind an
  experimental option.
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
- **Diagnostics ride a side-channel.** Derive `MathDiagnostic` values through
  `math_diagnostics()` for the formatter, linter, and LSP; do not encode errors
  as CST structure.
- **Host-only constructs stay outside TeX math where possible.** Markdown
  delimiters, Bookdown equation labels (`(\#eq:label)`), Pandoc attributes, and
  container prefixes belong to the host layer. Equation labels are host tokens
  between ordered `MATH_CONTENT` segments, never children of those segments.
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
- **Known pinned Badness formatter defect:** it still splits a non-colon
  relation head from its CST-separated scripted tail (`<=_i` → `< =_i`, with
  the same problem for `>=_i` and `==_i`). Panache preserves these composite
  relations. Keep those cases on the compatibility path and outside mandatory
  byte parity until the pinned oracle is corrected. Definition relations,
  including `:=_i`, now have byte parity.
- **Raise issues with Badness to the user:** if you see a Badness parser 
  or formatter defect, record it and elevate it to the user so that
  it can be fixed in the Badness repository.

Follow the math-parser, parser, and formatter invariants in the repository's
root `AGENTS.md`.

## Session workflow

1. Read the repository roadmap at `TODO.md` in the root. Follow its checkboxes
   for the authoritative sequence, then read the skill-local
   [RECAP.md](RECAP.md) for the latest completed slice and suggested next
   sub-targets. Resolve that link relative to this `SKILL.md`, not the
   repository root.
2. Pick one bounded sub-task. If the roadmap, recap, and repository disagree,
   verify the implementation and correct the stale document before proceeding.
3. TDD: add the failing test first (parser golden / formatter golden / unit).
4. Validate during development and before landing:
   - During development, run the focused suite relevant to the changed layer:
     - Parser CST: `cargo test -p panache-parser --test math_badness_parity`
     - Semantic model:
       `cargo test -p panache-parser --test math_semantic_parity`
     - Formatter output:
       `cargo test -p panache-formatter --test math_badness_oracle`
   - If the formatter corpus or its parity classification changes, regenerate
     and review the committed report with:

     ```bash
     cargo test -p panache-formatter --test math_badness_oracle \
       math_badness_full_report -- --ignored --nocapture
     ```

   - Before landing, run the workspace validation required by root `AGENTS.md`.
     Its `cargo test --workspace` gate subsumes the focused suites; do not rerun
     a focused suite afterward when both exercised the same tree state.
   - For parser CST snapshot changes: review each diff (byte ranges must still
     reconstruct the input losslessly).
   - Flag-off regression: existing formatter goldens stay byte-identical.
5. Rewrite `RECAP.md` with the latest result and suggested next sub-targets.
   Update roadmap checkboxes when the completed work changes them.
