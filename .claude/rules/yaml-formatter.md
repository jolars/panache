---
paths:
  - "crates/panache-formatter/src/formatter/yaml.rs"
  - "crates/panache-formatter/src/formatter/yaml/**/*.rs"
  - "crates/panache-formatter/src/formatter/yaml/**/*.md"
  - "crates/panache-formatter/tests/yaml_cross_validation.rs"
  - "crates/panache-formatter/tests/fixtures/yaml_corpus/**"
  - "crates/panache-formatter/src/yaml_engine.rs"
---

This rule applies to the staged in-tree YAML formatter rollout. See
[`yaml-formatter-cutover`](../skills/yaml-formatter-cutover/SKILL.md)
for the phased plan and the canonical 12-rule style spec; this rule
encodes the load-bearing invariants.

- **The style spec is the source of truth.** The 12-rule spec in
  `plan.md` (eventually `crates/panache-formatter/src/formatter/yaml/STYLE.md`)
  defines what the formatter emits. pretty_yaml is used as a
  cross-validation reference because it implements the same rules,
  not as a divergence target. Don't introduce "we differ from
  pretty_yaml here because…" comments — if we differ, one of us is
  wrong relative to the spec; fix that.
- **Shadow-first.** The in-tree YAML formatter is NOT wired into the
  live formatting pipeline until Phase 2 (joint cutover) lands. While
  in shadow, callers must continue to go through
  `yaml_engine.rs::format_text` (pretty_yaml). Don't introduce a
  partial cutover where some YAML output paths use the in-tree
  formatter and others use pretty_yaml.
- **Cross-validation disagreements are bugs, not divergences.** Every
  difference between `format_in_tree(x)` and `pretty_yaml(x)` is
  either (a) a bug in the in-tree formatter, (b) a parser CST shape
  bug in `panache-parser`, or (c) a bug in pretty_yaml. Diagnose in
  that order. Fix it; don't enumerate it.
- **Adding a 13th rule is a deliberate act.** If Phase 1 surfaces an
  edge case the 12 rules don't cover, the resolution is a new rule
  in STYLE.md (with a one-line rationale and a fixture) — not a
  special-case branch in the formatter. New rules need explicit
  decision: does pretty_yaml's behavior become the spec, or do we
  diverge? Document the decision.
- **Idempotency is asserted in the harness.** Every corpus case must
  satisfy `format(format(x)) == format(x)`. Second assertion per
  case, not a separate test bucket. A new case that fails idempotency
  blocks merge until fixed.
- **Drive parser fixes through formatter symptoms.** A formatter
  cross-validation failure often surfaces a parser CST shape gap
  (mis-attached trivia, off-by-one comment placement, wrong indent
  grouping). Before writing a formatter workaround, verify the
  in-tree parser CST matches what a spec-compliant formatter would
  expect. See the [`formatter`](formatter.md) rule on idempotency
  root-causing.
- **Formatter modules stay in `panache-formatter`.** No YAML
  formatter logic in `panache-parser`. The parser stays policy-free
  per [`yaml-parser`](yaml-parser.md) and [`parser`](parser.md).
- **No ad-hoc YAML output paths.** All YAML rendering goes through
  the in-tree formatter (or, until Phase 2, through
  `yaml_engine.rs::format_text`). No one-off `format_yaml_inline`
  helpers in feature modules.
- **Plain metadata before hashpipe.** Phase 3 (hashpipe) starts after
  Phase 2 cuts over. Hashpipe inherits the plain pipeline via
  `normalize_hashpipe_input`; building hashpipe paths first locks in
  behavior before the plain engine has fully stabilized.
- **Don't update host golden cases under `tests/fixtures/cases/*/`
  to match in-tree output until Phase 2.** Until the cutover, live
  output is still pretty_yaml; changing host expectations would mask
  cross-validation regressions.
