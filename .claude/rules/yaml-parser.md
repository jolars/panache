---
paths:
  - "crates/panache-parser/src/parser/yaml.rs"
  - "crates/panache-parser/src/parser/yaml/**/*.rs"
  - "crates/panache-parser/src/parser/blocks/metadata.rs"
  - "crates/panache-parser/src/syntax/yaml.rs"
  - "crates/panache-parser/tests/**/*yaml*"
  - "crates/panache-parser/tests/fixtures/yaml-test-suite/**"
  - "crates/panache-parser/tests/fixtures/cases/*yaml*/**"
  - "crates/panache-parser/tests/fixtures/cases/crlf_yaml_metadata/**"
---

This rule applies only when editing the YAML parser, its CST/syntax,
its test harness, or YAML-tagged fixtures. Skip it for non-YAML parser
work (other block/inline parsers, formatter, linter, conformance harness)
even though they live in the same crate.

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
