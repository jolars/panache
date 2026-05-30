---
name: yaml-formatter-cutover
description: Drive the staged in-tree YAML formatter rollout — implement
  the rule-based style spec, cross-validate against pretty_yaml, joint
  parser+formatter cutover, then hashpipe extension. Sibling to
  yaml-shadow-expand (parser-coverage); invoke when the work is formatter-side
  or the joint cutover gate.
---

Use this skill when:

- Building, extending, or testing the in-tree YAML formatter under
  `crates/panache-formatter/src/formatter/yaml/`.
- Maintaining the cross-validation harness that compares formatter
  output to `pretty_yaml` on the corpus.
- Extending the style spec with a newly-discovered rule (rare —
  see [`plan.md`](plan.md) for the 12-rule spec and the process for
  adding a 13th).
- Preparing or executing the joint cutover that retires `yaml_parser` and
  `pretty_yaml` for plain metadata YAML in one commit.
- Extending the same pipeline to hashpipe YAML after the plain cutover lands.

For parser-side fixture nibbling, triage, allowlist work, or yaml-test-suite
maintenance, use [`yaml-shadow-expand`](../yaml-shadow-expand/SKILL.md)
instead. The two skills share parser code but own different concerns.

## Current state

Phase 1 not yet started. The live YAML path still goes through
`crates/panache-formatter/src/yaml_engine.rs` → `pretty_yaml::format_text`,
with the host CST carrying the legacy `yaml_parser` shape from
`crates/panache-parser/src/syntax/yaml.rs`. The in-tree parser
(`crates/panache-parser/src/parser/yaml/`) is fully event-parity green
against yaml-test-suite (308 passes_now, 94 error_contract_ok, both
`fails_needs_*` buckets empty) but has no formatter consumer.

The plan lives in [`plan.md`](plan.md) alongside this file. Treat it as
the authoritative phasing reference; update its "what landed" block as
work progresses.

## Scope boundaries

- **Formatter crate** (`crates/panache-formatter/`) owns the new YAML
  formatter modules. Don't put formatter logic in `panache-parser`; the
  parser stays policy-free per [`yaml-parser`](../../rules/yaml-parser.md)
  and [`formatter`](../../rules/formatter.md) rules.
- **Shadow-first.** Until the joint cutover lands, the in-tree formatter
  is NOT wired into the live formatting pipeline. It exists as a parallel
  implementation cross-validated against pretty_yaml output. See
  [`yaml-formatter`](../../rules/yaml-formatter.md) for the invariants.
- **Rule-based deterministic style.** The 12-rule spec in [`plan.md`](plan.md)
  (eventually `STYLE.md` in the formatter module) is the source of truth.
  pretty_yaml is a cross-validation reference because it implements the
  same rules — not a divergence target. If `format_in_tree(x) != pretty_yaml(x)`,
  it's a bug in the in-tree formatter, the in-tree parser CST shape, or
  pretty_yaml, in that diagnostic order.
- **Plain metadata first, hashpipe last.** Hashpipe inherits the plain
  engine via `normalize_hashpipe_input`. Doing hashpipe first means doing
  the plain work anyway plus locking in hashpipe-specific behavior before
  plain solidifies.

## Architecture trajectory

End state:

```
src/syntax/yaml.rs   →  panache-parser in-tree YAML CST
                           ↓
host CST            →  panache-formatter::formatter::yaml
                           ↓
                       formatted YAML text
```

`yaml_parser` and `pretty_yaml` both removed in the cutover commit.

Phased path:

1. **Shadow formatter (Phase 1).** Build
   `crates/panache-formatter/src/formatter/yaml/` consuming the in-tree
   parser's CST. No host wiring. Tests run `format_in_tree(text) ≈
   pretty_yaml(text)` over a corpus.
2. **Joint cutover (Phase 2).** When parity holds minus declared
   divergences, swap `src/syntax/yaml.rs` to use the in-tree parser AND
   replace `yaml_engine.rs::format_text` with the in-tree formatter in
   one commit. Both `yaml_parser` and `pretty_yaml` come out together.
3. **Hashpipe extension (Phase 3).** Wire the same parser+formatter
   through `normalize_hashpipe_input` for `#|`-prefixed YAML inside
   executable chunks. Hashpipe-specific edge cases (continuation lines,
   blank-line semantics) get their own golden fixtures.

## Key files

- `crates/panache-formatter/src/formatter/yaml/` — in-tree YAML formatter
  modules (created in Phase 1).
- `crates/panache-formatter/src/formatter/yaml/STYLE.md` — canonical
  style spec (12 rules), moved from `plan.md` once the module exists.
- `crates/panache-formatter/tests/yaml_cross_validation.rs` — Phase 1
  cross-validation harness. Walks a corpus, asserts
  `format_in_tree == pretty_yaml` and idempotency
  (`format(format(x)) == format(x)`). Disagreements are bugs to fix,
  not divergences to enumerate.
- `crates/panache-formatter/tests/fixtures/yaml_corpus/` — corpus of
  representative frontmatter cases (real `.qmd`/`.Rmd` frontmatter
  plus hand-picked stressors for flow overflow, comments, anchors).
- `tests/fixtures/cases/*/` — host-level golden cases (existing
  pattern). New cases that specifically exercise YAML formatting come
  here; the host config schema lives at this level.
- `crates/panache-formatter/src/yaml_engine.rs` — current pretty_yaml
  wrapper. Untouched until Phase 2.
- `crates/panache-parser/src/syntax/yaml.rs` — current `yaml_parser`
  bridge. Untouched until Phase 2.

## Workflow

1. **Confirm phase.** Read [`plan.md`](plan.md) for the current
   "what landed" annotations. Don't start Phase 2 work before Phase 1
   parity holds; don't start Phase 3 before Phase 2 has cut over.

2. **Add or extend divergence cases first when in doubt.** If a parity
   failure could plausibly be intentional, add the case to
   `yaml_divergences/` with a written rationale before fixing the
   formatter to match pretty_yaml. The divergence list is load-bearing.

3. **Drive parser fixes through formatter symptoms.** A formatter
   parity failure often surfaces a parser CST shape gap (mis-attached
   trivia, wrong indent grouping). Verify against the in-tree parser
   CST shape before reaching for a formatter workaround — see
   [`formatter`](../../rules/formatter.md) rule on idempotency
   divergence root-causing.

4. **Validate**:
   - `cargo test -p panache-formatter --test yaml_shadow_parity`
   - `cargo test -p panache-parser --test yaml` (no regression in parser parity)
   - `cargo test` (workspace)
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo fmt -- --check`

5. **Update `plan.md`** with a "what landed" annotation matching the
   `scanner-rewrite.md` precedent in the sibling skill.

## Dos and don'ts

- **Do** add a divergence case with rationale before adapting the
  formatter to match pretty_yaml on a debatable choice.
- **Do** keep formatter modules in `crates/panache-formatter/`; the
  parser stays policy-free.
- **Do** treat idempotency as a first-class invariant, asserted in the
  parity harness, not just verified ad-hoc.
- **Don't** wire the in-tree formatter into the live pipeline before
  Phase 2. The shadow invariant is what keeps the cutover honest.
- **Don't** add ad-hoc YAML output paths elsewhere (e.g. a
  one-off `format_yaml_inline` somewhere). Funnel everything through
  the in-tree formatter from Phase 1 forward.
- **Don't** start hashpipe work before plain metadata has cut over.
- **Don't** absorb parser-coverage triage work here — that lives in
  [`yaml-shadow-expand`](../yaml-shadow-expand/SKILL.md).

## Report-back format

1. Phase status before and after the session (what landed, what's
   blocking).
2. Divergence cases added or modified, with the choice each pins.
3. Parser changes driven by formatter-side symptoms (if any).
4. Cases unblocked but not yet landed (candidates for follow-up).
5. Suggested next target.
6. **Session continuation recommendation** — same three options as
   [`yaml-shadow-expand`](../yaml-shadow-expand/SKILL.md) (continue
   here / compact, then continue / new session).
