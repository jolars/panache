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
- **Rule-based deterministic style** — output follows the style spec
  below, not a tool's whims. pretty_yaml is used as a cross-validation
  reference because it implements the same rules; it is not the
  source of truth.
- Strong idempotency invariant: `format(format(x)) == format(x)`
  asserted in the corpus harness, not as a separate test.
- Plain metadata first; hashpipe inherits via existing
  `normalize_hashpipe_input` once Phase 2 lands.

## Non-goals

- Replacing yaml-test-suite event parity. That bar stays.
- Tracking pretty_yaml's choices when they conflict with the style
  spec. If pretty_yaml ever drifts from the spec on an edge case, we
  follow the spec and either fix pretty_yaml upstream or work around
  in the corpus harness.
- Wiring the in-tree formatter into the live path before Phase 2.

## Style spec

The in-tree YAML formatter follows these rules. They are deterministic
(same input → same output) and small enough to fit in one table.
Cross-validated against pretty_yaml 0.6.0 and Prettier 3.6.2 on a
15-case battery of representative frontmatter — both agree with all 12
rules; rule 6's bracket placement is the one point where they differ
and the rule pins pretty_yaml's choice.

This spec lives here during the planning phase. After Phase 1.1
creates `crates/panache-formatter/src/formatter/yaml/`, move it to
`STYLE.md` in that directory and cross-reference from the user-facing
formatter docs.

1. **Indent.** 2 spaces, fully canonicalized regardless of input shape.
2. **Sequence items** indented +2 from the parent key
   (`categories:\n  - foo`, never `- foo` at parent column).
3. **Quote style preference:** plain → double-quoted → single-quoted only
   when content contains characters that would need backslash-escaping
   in double-quoted form (e.g. `'C:\Users\test'`).
4. **Block scalar style** (literal `|` vs folded `>`): preserved from
   input. They carry different YAML semantics and are not
   interchangeable.
5. **Flow spacing:** `{ key: value }` with spaces inside braces;
   `[a, b, c]` with a space after each comma.
6. **Flow wrap on line-width overflow:** each item on its own line,
   trailing comma, **opening bracket stays on the key line**
   (`keywords: [\n  first,\n  ...\n]`). This is the one point of
   disagreement between pretty_yaml and Prettier — we follow
   pretty_yaml.
7. **Blank lines:** runs of multiple blank lines collapse to one max.
8. **Inline comments:** exactly one space before `#`.
9. **Comment positions** (above key, inline, between keys): preserved.
   Comments are user-authored content.
10. **Trailing whitespace** on every line: stripped.
11. **Empty scalars:** `key:` stays `key:`, never canonicalized to
    `key: null` or `key: ""`.
12. **Key order:** preserved. Frontmatter is content the user wrote;
    reordering would surprise.

Rules 4, 9, and 12 are "preserve" rules: they don't add a new
behavior, they explicitly decline to canonicalize a
semantically-meaningful user choice. They're still deterministic.

Rule 3 is the only spec rule with semantic-content awareness. The
escape-required test is decidable from the scalar's bytes alone (no
context dependence), so it remains rule-based.

Open style decisions deferred until Phase 1 surfaces real cases:

- **Plain-scalar wrapping** at line-width overflow. Prettier's
  `proseWrap: always` will wrap a long plain scalar; pretty_yaml's
  default leaves it alone. YAML plain-scalar semantics depend on
  indentation context, so wrapping is fragile. Default position:
  never wrap plain scalars; if users want wrapping they should use
  block scalars (`>` for folded or `\|` for literal).
- **Trailing document newline** (single `\n` at EOF). Both tools
  emit one; need to verify the in-tree parser preserves
  losslessly so the formatter has a deterministic input signal.

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

### 1.2 — Move style spec into the module

When `crates/panache-formatter/src/formatter/yaml/` exists, move the
style-spec table from this plan into `STYLE.md` in that directory and
link to it from the user-facing formatter docs (`docs/guide/`). The
spec then has one canonical home; this plan tracks rollout, not the
spec itself.

If Phase 1 development discovers a 13th rule (an edge case neither
the spec nor pretty_yaml currently covers), add it to STYLE.md with
a fixture and a one-line rationale. New rules need cross-validation
against pretty_yaml before landing — if they conflict, decide
explicitly which is right and document the decision.

### 1.3 — Cross-validation harness

New test file
`crates/panache-formatter/tests/yaml_cross_validation.rs`. For each
case in the corpus:

1. Read `input.yaml`.
2. `let in_tree = panache_formatter::formatter::yaml::format_yaml(input, &opts);`
3. `let pretty = pretty_yaml::format_text(input, &opts)?;`
4. Assert `in_tree == pretty` (rule 6's bracket placement matches
   pretty_yaml, so this should hold across the corpus).
5. Assert `format_yaml(in_tree, ...) == in_tree` (idempotency).
6. If `in_tree != pretty`: it's a bug in (a) the in-tree formatter,
   (b) the in-tree parser CST shape, or (c) pretty_yaml. Diagnose
   and fix — do NOT add the case to a divergence list. The corpus
   is calibration data for the spec, not a divergence registry.

Corpus seeding:
- Pull real frontmatter from existing
  `tests/fixtures/cases/*/input.{md,qmd,Rmd}` (extract the YAML
  region).
- Add `crates/panache-formatter/tests/fixtures/yaml_corpus/` with
  hand-picked cases that stress comments, multi-line scalars,
  anchors, tags, and flow overflow (rule 6).
- Optionally cycle in a slice of the yaml-test-suite plain cases that
  pretty_yaml handles cleanly.

### 1.4 — CST shape gaps surfaced by the harness

Expected outcome of Phase 1 is a list of parser-side fixes driven by
formatter symptoms. Track each fix as a separate parser commit (per
[`formatter`](../../rules/formatter.md) rule on idempotency
root-causing).

### Exit criteria for Phase 1

- Every corpus case satisfies `in_tree == pretty` and idempotency.
- STYLE.md is the canonical spec; this plan no longer carries it.
- Any parser CST shape gaps surfaced by the harness are fixed in
  `panache-parser` (separate commits).

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
- Match the style spec (and pretty_yaml's output, by construction), or
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

- **YamlFormatOptions surface.** Mirror pretty_yaml's option surface
  in the in-tree formatter, or design our own from scratch? Mirroring
  eases the cutover; designing fresh avoids inheriting quirks. Note:
  the spec is fixed; options control orthogonal knobs like
  `line-width` and `prose-wrap`, not style choices.
- **Salsa integration.** Does the formatter need its own salsa input,
  or piggyback on the parser's `YamlInput` from
  `crates/panache-parser/src/parser/yaml/model.rs`?
- **Style-as-CST-kind promotion.** Deferred in `scanner-rewrite.md`,
  but the formatter may force it (rule 4 requires distinguishing
  `|` / `>` / `'…'` / `"…"` styles per-scalar). Decide before Phase
  1.1 lands whether to do this preemptively or reactively.
- **Plain-scalar wrapping policy.** See style spec — current
  position is "never". Confirm before Phase 1.1.
- **Trailing document newline.** Single `\n` at EOF is the universal
  convention; verify in-tree parser preserves it losslessly so the
  formatter can emit deterministically.
