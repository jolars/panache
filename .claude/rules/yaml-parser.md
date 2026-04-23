---
paths:
  - "crates/panache-parser/src/parser/yaml.rs"
  - "crates/panache-parser/src/parser/yaml/**/*.rs"
  - "crates/panache-parser/src/parser/**/*.rs"
  - "crates/panache-parser/src/syntax/**/*.rs"
  - "crates/panache-parser/tests/**/*yaml*"
---

YAML parser work is incremental and parser-crate scoped.

- Keep YAML parsing lossless and CST-first (markers, whitespace, comments,
  scalar trivia).
- Prefer explicit, indentation-aware lexer + parser design; avoid parser styles
  that hide indentation/recovery state.
- Support both plain YAML and hashpipe-prefixed YAML through one core model.
- Keep host↔embedded range mapping explicit and deterministic.
- Introduce new behavior in shadow/parity mode before replacing existing YAML
  pipeline paths.
- Keep parser policy separate from formatter policy.
- Add focused, deterministic tests for new YAML behavior and mapping rules.
