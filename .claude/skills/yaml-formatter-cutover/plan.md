# In-tree YAML formatter cutover plan

Staged plan for retiring `yaml_parser` and `pretty_yaml` in favor of an
in-tree YAML formatter driven by the in-tree YAML CST. Sibling document
to `SKILL.md`. Annotate the **What landed** block as work progresses,
matching the `scanner-rewrite.md` precedent in `yaml-shadow-expand/`.

## Status

- **Phase 1 (shadow formatter):** not started.
- **Phase 2 (joint cutover):** not started, blocked on Phase 1.
- **Phase 3 (hashpipe extension):** not started, blocked on Phase 2.

## What landed since drafting

_(Update as phases complete. Earliest entries on top.)_

- _Nothing yet._

## Context

The in-tree streaming YAML parser is event-parity complete against
yaml-test-suite (`crates/panache-parser/tests/yaml/triage.json`:
308 passes_now, 94 error_contract_ok, both `fails_needs_*` buckets
empty). It has a lossless CST and a delegated scalar-cooking module.

It has no formatter consumer. The live pipeline still uses the legacy
`yaml_parser` crate via `crates/panache-parser/src/syntax/yaml.rs` for
the CST, and `pretty_yaml::format_text` via
`crates/panache-formatter/src/yaml_engine.rs` for output. The in-tree
parser is therefore unproven on the dimensions a formatter would
exercise — CST shape (trivia attachment, comment placement, indent
grouping) rather than event stream.

A pure parser cutover would swap internals with no user-visible
payoff; its parity bar is too weak to catch shape gaps. A formatter
gives the cutover a downstream consumer and a real parity bar.

## Goals

- One pipeline end-to-end: in-tree parser → in-tree formatter.
- `yaml_parser` and `pretty_yaml` both retired in the cutover commit.
- Parity bar: `format_in_tree(x) ≈ pretty_yaml(x)` on a real corpus,
  minus an enumerated divergence list.
- Strong idempotency invariant: `format(format(x)) == format(x)`
  asserted in the parity harness, not as a separate test.
- Plain metadata first; hashpipe inherits via existing
  `normalize_hashpipe_input` once Phase 2 lands.

## Non-goals

- Replacing yaml-test-suite event parity. That bar stays.
- Adopting pretty_yaml's defaults wholesale. The divergence list is
  where deliberate differences live.
- Wiring the in-tree formatter into the live path before Phase 2.

## Phase 1 — Shadow in-tree formatter (plain metadata)

Build `crates/panache-formatter/src/formatter/yaml/` consuming the
in-tree parser CST. Not wired to the live pipeline.

### 1.1 — Module skeleton

- New module `crates/panache-formatter/src/formatter/yaml/`:
  - `mod.rs` — public entry: `format_yaml(text: &str, opts: &YamlFormatOptions) -> String`.
  - `document.rs` — top-level document orchestration.
  - `block_map.rs`, `block_sequence.rs`, `flow.rs`, `scalar.rs` —
    per-CST-node rendering.
  - `options.rs` — `YamlFormatOptions` (line-width, wrap mode, quote
    style preference, …).
- Initial entry calls into in-tree parser via
  `panache_parser::parser::yaml::parse_yaml_tree(text)`, walks the
  returned CST, emits text.

### 1.2 — Divergence enumeration

Stub `crates/panache-formatter/src/formatter/yaml/divergences.md`
seeded with the known points of disagreement (fill in from session
context — quote style? key ordering? blank-line policy?). Each entry:

```markdown
### Divergence: <short name>

**Choice:** what the in-tree formatter does.
**pretty_yaml does:** what pretty_yaml emits.
**Rationale:** why we differ.
**Fixture:** path under `tests/fixtures/yaml_divergences/`.
```

Locking fixtures live at
`crates/panache-formatter/tests/fixtures/yaml_divergences/<name>/`
with `input.yaml`, `expected_in_tree.yaml`,
`expected_pretty_yaml.yaml`, and `rationale.md`.

### 1.3 — Shadow parity harness

New test file
`crates/panache-formatter/tests/yaml_shadow_parity.rs`. For each case
in the parity corpus:

1. Read `input.yaml`.
2. `let pretty = pretty_yaml::format_text(input, &opts)?;`
3. `let in_tree = panache_formatter::formatter::yaml::format_yaml(input, &opts);`
4. Assert `in_tree == pretty` UNLESS the case is in the divergence
   list, in which case assert `in_tree == expected_in_tree.yaml`.
5. Assert `format_yaml(in_tree, ...) == in_tree` (idempotency).

Corpus seeding:
- Pull real frontmatter from existing
  `tests/fixtures/cases/*/input.{md,qmd,Rmd}` (extract the YAML
  region).
- Add `crates/panache-formatter/tests/fixtures/yaml_corpus/` with
  hand-picked cases that stress comments, multi-line scalars,
  anchors, tags.
- Optionally cycle in a slice of the yaml-test-suite plain cases that
  pretty_yaml handles cleanly.

### 1.4 — CST shape gaps surfaced by the harness

Expected outcome of Phase 1 is a list of parser-side fixes driven by
formatter symptoms. Track each fix as a separate parser commit (per
[`formatter`](../../rules/formatter.md) rule on idempotency
root-causing).

### Exit criteria for Phase 1

- All parity-corpus cases pass either pretty_yaml-match or
  divergence-fixture match.
- Idempotency assertion green on every case.
- Divergence list is closed (no surprise divergences; every difference
  is either a fixed bug or an enumerated choice).

## Phase 2 — Joint cutover

When Phase 1 exits, swap parser and formatter in one commit.

### 2.1 — Parser side

- Update `crates/panache-parser/src/syntax/yaml.rs` to call the
  in-tree parser (`parse_yaml_report`) and surface its CST shape into
  the host CST.
- Audit downstream consumers of the YAML CST shape: linter rules,
  LSP, anything that walks
  `SyntaxKind::YAML_*` nodes. The in-tree parser's `YAML_*` kinds
  must already be the host CST's kinds for this to be a no-op (verify
  before cutover).

### 2.2 — Formatter side

- Replace `crates/panache-formatter/src/yaml_engine.rs::format_text`
  call with `formatter::yaml::format_yaml`.
- Remove the `pretty_yaml` dependency from
  `crates/panache-formatter/Cargo.toml`.
- Remove the `yaml_parser` dependency from `Cargo.toml` (root).

### 2.3 — Golden case regen

Expect host-level golden cases under `tests/fixtures/cases/*/` to
shift on YAML-affecting cases. Each delta must:
- Match a divergence-list entry, or
- Be a fix for a known bug captured by a `tests/yaml_corpus/` case, or
- Be challenged before accepting.

### Exit criteria for Phase 2

- `yaml_parser` and `pretty_yaml` removed from `Cargo.lock`.
- All host golden cases green; deltas annotated.
- `cargo test` workspace green.
- Triage of parser-side regressions (if any) — should be zero per the
  shape audit, but verify.

## Phase 3 — Hashpipe extension

Same parser + formatter, exercised through the existing hashpipe
normalization path.

### 3.1 — Wire-up

- `crates/panache-formatter/src/formatter/hashpipe.rs` already calls
  the YAML engine for option bodies. Re-point it to
  `formatter::yaml::format_yaml` with hashpipe normalization.
- Confirm `normalize_hashpipe_input` behaviour matches what the
  formatter expects (it strips `#|`; the formatter re-prefixes).

### 3.2 — Hashpipe-specific fixtures

Add cases under
`crates/panache-formatter/tests/fixtures/yaml_corpus/hashpipe/` for:
- Continuation lines (`#| key: value\n#|   continued`).
- Blank-line semantics inside `#|`.
- Anchors / tags in chunk options.
- The existing `issue_*_hashpipe_*` host fixtures should drop their
  pretty_yaml-specific quirks at this point — re-check each.

### Exit criteria for Phase 3

- Hashpipe and plain metadata share one formatter path with one
  divergence list.
- All host hashpipe golden cases green; pretty_yaml-specific
  workarounds in `crates/panache-formatter/src/formatter/hashpipe.rs`
  removed.

## Open questions

- **Divergence seed.** What pretty_yaml choices do we already know we
  want to diverge from? (Owner decision; needed before Phase 1.3.)
- **YamlFormatOptions surface.** Mirror pretty_yaml's option surface
  in the in-tree formatter, or design our own from scratch? Mirroring
  eases the cutover; designing fresh avoids inheriting quirks.
- **Salsa integration.** Does the formatter need its own salsa input,
  or piggyback on the parser's `YamlInput` from
  `crates/panache-parser/src/parser/yaml/model.rs`?
- **Style-as-CST-kind promotion.** Deferred in `scanner-rewrite.md`,
  but the formatter may force it (rendering `>` vs `|` vs `'…'` vs
  `"…"` cleanly probably needs the styled `SyntaxKind` variants).
  Decide before Phase 1.1 lands whether to do this preemptively or
  reactively.
