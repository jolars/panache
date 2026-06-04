---
paths:
  - "crates/panache-parser/tests/yaml.rs"
  - "crates/panache-parser/tests/yaml/**/*.txt"
  - "crates/panache-parser/tests/fixtures/yaml-test-suite/**"
---

YAML test-suite harness changes must stay fixture-driven and parity-oriented.

- Treat each yaml-test-suite case directory as the source of truth.
- Use `test.event` as the expected parse-event contract for successful parse
  behavior; use `error` as an expected-failure contract.
- Never move a case into `crates/panache-parser/tests/yaml/allowlist.txt`
  without checking both `test.event` and `error`.
- If an `error` file exists for a case, do not allowlist it unless tests
  explicitly model the expected error behavior.
- The allowlist now covers the suite at event parity. When adding or
  regenerating cases, still verify each against both `test.event` and `error`
  rather than bulk-adding, and regenerate `triage.json` (the `#[ignore]`d
  `yaml_suite_generate_triage_report` harness) to confirm no bucket regresses.
- Prefer structured snapshots for CST/parity data (rowan's `{:#?}` debug tree
  via `insta`, or projected event streams from
  `parser::yaml::project_events`) over ad-hoc freeform text.
