---
paths:
  - "crates/panache-formatter/src/formatter/yaml.rs"
  - "crates/panache-formatter/src/formatter/yaml/**/*.rs"
  - "crates/panache-formatter/src/formatter/yaml/**/*.md"
  - "crates/panache-formatter/tests/yaml_cross_validation.rs"
  - "crates/panache-formatter/tests/fixtures/yaml_corpus/**"
  - "crates/panache-formatter/src/yaml_engine.rs"
---

This rule applies to the in-tree YAML formatter — the live YAML
formatting path for both plain frontmatter and hashpipe option bodies.
The canonical 14-rule style spec lives in
[`STYLE.md`](../../crates/panache-formatter/src/formatter/yaml/STYLE.md)
next to the code; this rule encodes the load-bearing invariants.

- **The style spec is the source of truth.** The 14-rule spec in
  `STYLE.md` defines what the formatter emits. pretty_yaml is used as a
  cross-validation reference because it implements the same rules, not
  as a divergence target. Don't introduce "we differ from pretty_yaml
  here because…" comments — if we differ, one of us is wrong relative to
  the spec; fix that.
- **`format_yaml` is the only YAML output path.** All YAML rendering
  goes through `formatter::yaml::format_yaml` via
  `yaml_engine.rs::format_yaml_with_config`. No one-off
  `format_yaml_inline` helpers in feature modules, and no partial path
  that routes some YAML output around the in-tree formatter.
- **Cross-validation disagreements are bugs, not divergences.** Every
  difference between `format_yaml(x)` and `pretty_yaml(x)` is either (a)
  a bug in the in-tree formatter, (b) a parser CST shape bug in
  `panache-parser`, or (c) a bug in pretty_yaml. Diagnose in that order.
  Fix it; don't enumerate it. The corpus is calibration data for the
  spec, not a divergence registry.
- **pretty_yaml is a temporary dev-only oracle.** It is a
  `[dev-dependencies]` entry used solely by
  `tests/yaml_cross_validation.rs`, slated for removal once the in-tree
  formatter has been stable in releases for a few months (revisit
  ~2026-09; see the `TEMPORARY` note in
  `crates/panache-formatter/Cargo.toml`). Don't reintroduce it as a
  normal dependency or wire it back into any runtime path.
- **Adding a 15th rule is a deliberate act.** If an edge case the
  current rules don't cover surfaces, the resolution is a new rule in
  `STYLE.md` (with a one-line rationale and a fixture under
  `crates/panache-formatter/tests/fixtures/yaml_corpus/`) — not a
  special-case branch in the formatter. New rules need explicit
  decision: does pretty_yaml's behavior become the spec, or do we
  diverge? Document the decision.
- **Idempotency is asserted in the harness.** Every corpus case must
  satisfy `format(format(x)) == format(x)`. Second assertion per case,
  not a separate test bucket. A new case that fails idempotency blocks
  merge until fixed.
- **Drive parser fixes through formatter symptoms.** A formatter
  cross-validation failure often surfaces a parser CST shape gap
  (mis-attached trivia, off-by-one comment placement, wrong indent
  grouping). Before writing a formatter workaround, verify the in-tree
  parser CST matches what a spec-compliant formatter would expect. See
  the [`formatter`](formatter.md) rule on idempotency root-causing.
- **Formatter modules stay in `panache-formatter`.** No YAML formatter
  logic in `panache-parser`. The parser stays policy-free per
  [`yaml-parser`](yaml-parser.md) and [`parser`](parser.md).
- **Plain metadata and hashpipe share one path.** Hashpipe option bodies
  format through the same `format_yaml` chokepoint as plain frontmatter;
  the prefix-aware parser carries the `#|` prefixes as trivia. Don't
  build a separate hashpipe formatting path.
