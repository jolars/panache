---
applyTo: "crates/panache-parser/src/parser/**/*.rs,crates/panache-parser/src/parser.rs,crates/panache-parser/src/syntax/**/*.rs,crates/panache-parser/src/syntax.rs,src/parser.rs,src/syntax.rs"
---

Parser and syntax changes must preserve lossless CST behavior.

- Treat Pandoc as the behavioral reference for ambiguous Markdown/Quarto syntax.
- Preserve all structural markers and whitespace in CST nodes/tokens.
- Do not move formatter policy into parser code.
- Keep parser single-pass assumptions intact (block parsing with inline emission
  during parse).
- Prefer existing block dispatcher and inline parser utilities over introducing
  parallel parsing paths.
- Keep parser behavior independent of formatter/linter policy; parser should
  remain lossless syntax capture only.
- For bug fixes and new parsing behavior, add a focused test first (unit or
  golden case).
- If golden snapshots change, validate each case and ensure formatting
  idempotency still holds.
