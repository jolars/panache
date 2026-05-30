---
paths:
  - "crates/panache-formatter/src/formatter/yaml.rs"
  - "crates/panache-formatter/src/formatter/yaml/**/*.rs"
  - "crates/panache-formatter/src/formatter/yaml/**/*.md"
  - "crates/panache-formatter/tests/yaml_shadow_parity.rs"
  - "crates/panache-formatter/tests/fixtures/yaml_corpus/**"
  - "crates/panache-formatter/tests/fixtures/yaml_divergences/**"
  - "crates/panache-formatter/src/yaml_engine.rs"
---

This rule applies to the staged in-tree YAML formatter rollout. See
[`yaml-formatter-cutover`](../skills/yaml-formatter-cutover/SKILL.md)
for the phased plan; this rule encodes the load-bearing invariants.

- **Shadow-first.** The in-tree YAML formatter is NOT wired into the
  live formatting pipeline until Phase 2 (joint cutover) lands. While
  in shadow, callers must continue to go through
  `yaml_engine.rs::format_text` (pretty_yaml). Don't introduce a
  partial cutover where some YAML output paths use the in-tree
  formatter and others use pretty_yaml — that desynchronizes the
  parity bar.
- **Parity bar is pretty_yaml minus an enumerated divergence list.**
  Every difference between `format_in_tree(x)` and `pretty_yaml(x)`
  is either a bug to fix in the in-tree formatter, a parser CST shape
  bug to fix in `panache-parser`, or an enumerated divergence with
  written rationale and a locking fixture under
  `tests/fixtures/yaml_divergences/`. No fourth option.
- **Divergence cases need rationale.** A new entry in
  `formatter/yaml/divergences.md` plus a fixture directory with
  `input.yaml`, `expected_in_tree.yaml`, `expected_pretty_yaml.yaml`,
  and `rationale.md`. Don't silently diverge.
- **Idempotency is asserted in the harness.** Every parity-corpus
  case must satisfy `format(format(x)) == format(x)`. This is not a
  separate test bucket — it's a second assertion per case. A new case
  that fails idempotency blocks merge until fixed.
- **Drive parser fixes through formatter symptoms.** A formatter
  parity failure often surfaces a parser CST shape gap (mis-attached
  trivia, off-by-one comment placement, wrong indent grouping). Before
  writing a formatter workaround, verify the in-tree parser CST
  matches what a correct formatter would expect. See the
  [`formatter`](formatter.md) rule on idempotency root-causing.
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
  parity regressions.
