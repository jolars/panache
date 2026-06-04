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

The in-tree YAML parser is the production parser: its lossless CST is
embedded directly into the host document tree (frontmatter and hashpipe
option bodies) and consumed by the in-tree YAML formatter. Work here is
incremental and parser-crate scoped.

- Keep YAML parsing lossless and CST-first (markers, whitespace, comments,
  scalar trivia).
- Prefer explicit, indentation-aware lexer + parser design; avoid parser styles
  that hide indentation/recovery state.
- Support both plain YAML and hashpipe-prefixed YAML through one core model
  (`parse_stream` / `parse_stream_with_prefix`); carry the `#|` prefix as
  `YAML_LINE_PREFIX` trivia so YAML token ranges are host ranges directly.
- Keep host↔embedded range mapping explicit and deterministic.
- Guard new behavior with yaml-test-suite event parity plus losslessness over
  the allowlisted fixtures before landing; don't regress either.
- Keep parser policy separate from formatter policy.
- Add focused, deterministic tests for new YAML behavior and mapping rules.
