---
name: yaml-shadow-expand
description: Guard Panache's YAML shadow parser coverage — yaml-test-suite
  parity, allowlist nibbling, triage regen, and parser-side cluster fixes.
  Sibling to yaml-formatter-cutover (which owns the in-tree formatter
  rollout and joint cutover); invoke this one when the work is
  parser-coverage or test-suite triage.
---

Use this skill when:

- A scanner/validator/projection change moved a case out of `passes_now` or
  `error_contract_ok` and you need to investigate the regression.
- `scripts/update-yaml-test-suite-fixtures.sh` brought in new upstream cases
  that need triaging.
- A fresh `fails_needs_feature` or `fails_needs_error_path` entry appears in
  `triage.json` and you want to pick it up.

For the formatter cutover (joint retirement of `yaml_parser` + `pretty_yaml`)
or hashpipe extension, use
[`yaml-formatter-cutover`](../yaml-formatter-cutover/SKILL.md) instead.

**Current state (as of last triage regen):** every fixture is in a terminal
bucket and allowlisted (`passes_now`: 308, `error_contract_ok`: 94,
`fails_needs_feature`: 0, `fails_needs_error_path`: 0). The "one more case"
nibbling workflow has no queue right now — re-run the triage generator before
assuming there's a case to pick up.

## Scope boundaries

- Target is the incremental shadow YAML parser in
  `crates/panache-parser/src/parser/yaml/` and the event-parity harness in
  `crates/panache-parser/tests/yaml.rs`.
- This is a **long-horizon, staged replacement** of the existing
  `yaml_parser` dependency, not a forever-shadow. Don't promise near-term
  replacement, but don't read this as "we're keeping the current lexer
  indefinitely" either.
- Stay parser-crate scoped. Do not leak YAML parser changes into the formatter
  or CLI.
- Keep CST lossless (markers, whitespace, comments, scalar trivia preserved).

## Architecture trajectory

The streaming scanner rewrite has landed and the legacy line-based
lexer is gone. The live tree-building path is now:

1. `parser.rs::parse_yaml_report` — orchestrator. Calls the
   validator, then drives [`parser.rs::parse_stream`] which consumes
   the streaming scanner and emits the rowan green tree.
2. `validator.rs::validate_yaml` — structural validator.
   Each `check_*` function is one cluster of error contracts
   (directive ordering, trailing content, unterminated flow, flow
   comma anomalies, multi-line quoted indent, block indent anomalies,
   block-scalar header, doc-level/value-level mixed scalar+map, flow
   continuation indent, invalid double-quoted escapes, etc.). Runs
   the scanner internally for token-level checks.
3. `parser.rs::parse_stream` — consumes the streaming `scanner.rs`
   and emits the rowan green tree (was `parser_v2.rs::parse_v2`; merged
   in once the legacy line-based lexer was gone).

`scanner.rs` is the streaming, char-by-char scanner modeled on
libyaml / PyYAML / snakeyaml: position-tracked, indent-stack driven,
**simple-key-table** based, with a token queue and lookahead. Trivia
(whitespace, comments, newlines) is interleaved in the queue rather
than dropped, so the CST stays lossless. Key/value pairing,
multi-line scalars, and explicit-key (`?` / `:`) entries unify under
one mechanism.

Residual cutover work (deferred):

- The eventual `yaml_parser` live-path cutover. `crates/panache-parser/src/syntax/yaml.rs`
  still parses the embedded YAML region with the legacy `yaml_parser`
  crate; production document CSTs carry that shape, not the streaming
  parser's.

Scalar cooking now lives in `parser/yaml/cooking.rs` (`cook_plain`,
`cook_single_quoted`, `cook_double_quoted`, plus internal
`fold_quoted_inner` / `decode_double_quoted_inner` primitives). Event
projection delegates to those helpers; the formatter (when it lands)
should call them too rather than duplicating the fold/strip/decode
pipeline. The two CST-walking collectors
`events.rs::collect_doc_scalar_source` and
`collect_value_scalar_source` only assemble raw token source for the
quoted multi-line paths — they are not cooking and don't need
extraction.

Tag, anchor, and alias dispatch landed in the scanner — `!`, `&`, `*`
emit dedicated `Tag` / `Anchor` / `Alias` tokens that flow through
`parse_stream` to `YAML_TAG` / `YAML_ANCHOR` / `YAML_ALIAS`, and
`events.rs::resolve_long_tag` consults per-document `%TAG` handles for
the `<tag:...>` event annotation. The validator's `check_tag_handle_scope`
enforces YAML 1.2 §6.8.2 (handles are document-scoped) and emits
`PARSE_UNDEFINED_TAG_HANDLE` on undeclared use.

The concrete plan and design decisions for the rewrite — including
trivia model, token enum lifetime, scalar cooking, diagnostic channel,
and the step-by-step migration sequence — live in `scanner-rewrite.md`
alongside this file. Consult it for context on residual work and for
the rationale behind the validator-driven cutover.

## Key files

- `crates/panache-parser/src/parser/yaml/scanner.rs` — streaming
  char-by-char scanner with simple-key table. Emits the token stream
  consumed by `parser.rs::parse_stream`.
- `crates/panache-parser/src/parser/yaml/parser.rs` — orchestrator
  (`parse_yaml_report` / `parse_yaml_tree` / `parse_shadow`) plus the
  streaming `parse_stream` entry that drives the scanner and builds
  the rowan green tree. Tree-shape changes happen here.
- `crates/panache-parser/src/parser/yaml/validator.rs` — structural
  diagnostic validator. `validate_yaml(input)` composes per-cluster
  `check_*` functions in priority order. Add new diagnostic clusters
  here as `check_*` functions and wire them into `validate_yaml`.
- `crates/panache-parser/src/parser/yaml/cooking.rs` — pure scalar
  cooking (`cook_plain`, `cook_single_quoted`, `cook_double_quoted`
  plus their multi-line variants and internal primitives like
  `fold_quoted_inner` and `decode_double_quoted_inner`). Event
  projection delegates here; the formatter should too.
- `crates/panache-parser/src/parser/yaml/events.rs` — event projection
  (`project_events` plus `project_*` helpers). Walks the CST and
  produces a yaml-test-suite event stream. The CST-walking
  source-collectors `collect_doc_scalar_source` and
  `collect_value_scalar_source` aggregate raw token text for the
  multi-line quoted paths; cooking is delegated to
  `super::cooking`.
- `crates/panache-parser/src/parser/yaml/model.rs` — `YamlDiagnostic`,
  `diagnostic_codes`, `YamlParseReport`, shadow report shape.
- `crates/panache-parser/tests/yaml.rs` — fixture-driven tests, including:
  - `yaml_allowlist_cases_snapshot` — diagnostic/tree snapshot per case
  - `yaml_allowlist_cases_cst_snapshot` — full CST snapshot per case
  - `yaml_allowlist_losslessness_raw_input` — byte-exact round-trip
  - `yaml_allowlist_projected_event_parity` — event stream vs
    fixture `test.event`
  - `yaml_suite_generate_triage_report` (ignored) — regenerates
    `tests/yaml/triage.json` bucketing every fixture
- `crates/panache-parser/tests/yaml/allowlist.txt` — small, intentionally curated
  list of case IDs. One case per addition, with a short `#` comment explaining
  what the case exercises.
- `crates/panache-parser/tests/yaml/triage.json` — derived; do not hand-edit.
- `crates/panache-parser/tests/fixtures/yaml-test-suite/` — vendored fixtures,
  refreshed via `scripts/update-yaml-test-suite-fixtures.sh`.

## Triage buckets

`triage.json` splits every fixture into four buckets. Understand which bucket a
case is in before touching it:

- `passes_now` — tree parses AND projected events match `test.event`. Safe to
  allowlist if not already listed.
- `error_contract_ok` — case has an `error` file and we correctly reject it
  with at least one diagnostic. Do not allowlist unless the test harness
  explicitly models the expected error contract.
- `fails_needs_error_path` — case has an `error` file but we currently parse
  it successfully (no diagnostic). Needs parser work to detect the error.
- `fails_needs_feature` — no `error` file. Two sub-patterns:
  - `tree: true, event_parity: false` — parses OK, projection fails. Usually
    low-effort: fix `cst_yaml_projected_events` / helpers in `tests/yaml.rs`.
  - `tree: false` — parser rejects. Usually needs lexer/parser work.

## Workflow

1. **Regenerate triage** if stale:
   ```
   cargo test -p panache-parser --test yaml yaml_suite_generate_triage_report -- --ignored
   ```
   Then inspect counts:
   ```
   grep -E '"passes_now_count"|"fails_needs_feature_count"|"error_contract_ok_count"|"fails_needs_error_path_count"' \
     crates/panache-parser/tests/yaml/triage.json
   ```

2. **Pick a case** — prefer highest-leverage, lowest-risk:
   - First check: are `fails_needs_feature_count` and
     `fails_needs_error_path_count` both 0? If so, the nibbling queue is empty
     — there is nothing to pick. Stop and report back; don't manufacture work
     by allowlisting already-allowlisted cases or by cherry-picking from
     `error_contract_ok` without explicit error-contract modeling.
   - If the queue is non-empty, start with `fails_needs_feature` entries where
     `tree: true` — these only need projection fixes.
   - Skim `in.yaml` and `test.event` for a few candidates. Group cases that
     share a root cause so one fix unlocks several.
   - Do not allowlist a case that has an `error` file without modeling the
     error contract explicitly.

3. **Probe the gap** if not obvious. A throwaway `#[ignore]` test in
   `tests/yaml.rs` printing `parse_yaml_tree(input)` and
   `project_events(input)` is cheap and informative. Remove the probe before
   finishing.

4. **Classify the fix** before coding:
   - Projection-only → edit `parser/yaml/events.rs` helpers
     (`project_document`, `project_block_map_entries`,
     `project_block_sequence_items`, `project_flow_map_entries`,
     `scalar_document_value`).
   - Parser-shape issue (tree built doesn't match spec) → edit
     `parser/yaml/parser.rs::parse_stream`. The emitter is keyed on
     the scanner's token kinds (`BlockMappingStart` / `Key` / `Value`
     / `BlockEntry` / `BlockEnd` / flow indicators); trivia is
     consumed inline.
   - Tokenization gap (scanner doesn't recognize a construct) → edit
     `parser/yaml/scanner.rs`. Consider indent/flow/block-scalar/
     simple-key-table state interactions.
   - Structural-diagnostic gap (spec error not caught) → add a
     `check_*` function in `parser/yaml/validator.rs` and wire it
     into `validate_yaml`. Each check is one cluster of error
     contracts. New diagnostic codes go in
     `model.rs::diagnostic_codes` first.
   - Lex-level diagnostic gap (e.g. invalid escape, malformed
     directive) → push the diagnostic onto `Scanner::diagnostics`
     from `parser/yaml/scanner.rs` (use `push_diagnostic`), or, if
     it requires CST inspection, add a `check_*` cluster in
     `validator.rs`.

5. **Apply the smallest focused change.** Keep changes parser-crate scoped,
   CST-lossless, and don't regress already-allowlisted cases.

6. **Add the case(s) to `allowlist.txt`** with a one-line `#` comment capturing
   the pattern (not the case ID — the shape, e.g. "Block map with inline
   flow-map values"). One commit/session can add several if they share a root
   cause, but annotate each.

7. **Run the parity tests**:
   ```
   cargo test -p panache-parser --test yaml
   ```
   Expect snapshot tests to fail the first time with `.snap.new` files. Review
   each new snapshot before accepting:
   - `tests/snapshots/yaml__yaml_suite_<ID>.snap.new` — summary
   - `tests/snapshots/yaml__yaml_cst_suite_<ID>.snap.new` — CST tree
   Accept by renaming (`mv ...snap.new ...snap`) only after confirming the CST
   shape matches the fixture semantics. Note: `insta` stops on the first
   snapshot failure, so you may need to iterate (accept, re-run, accept…).

8. **Check for unlocked cases.** A single projection or parser fix can flip
   several cases to passing. After regenerating triage, diff `passes_now` vs
   the allowlist and allowlist the cleanly-unlocked ones with their own
   rationale comments.

9. **Validate**:
   - `cargo test -p panache-parser --test yaml`
   - `cargo clippy -p panache-parser --all-targets -- -D warnings`
   - `cargo fmt -p panache-parser -- --check`
   - Regenerate `triage.json` a final time so it reflects the new state.

## Dos and don'ts

- **Do** keep `allowlist.txt` intentionally small. One case per addition, with
  an explanatory comment.
- **Do** prefer fixing the underlying projection/parser gap over papering over
  a single case — shared-root fixes are the main source of leverage.
- **Do** verify losslessness visually in the CST snapshot (byte ranges
  contiguous, all trivia captured).
- **Don't** allowlist error-contract cases without explicit error-path
  coverage.
- **Don't** hand-edit `triage.json` — it is derived output.
- **Don't** drift into formatter territory. Parser/CST only.
- **Don't** introduce parser styles that hide indentation or recovery state.
  The scanner is explicitly indentation-aware by design.

## Report-back format

When done, report:

1. Triage counts before and after (`passes_now`, `fails_needs_feature`,
   `error_contract_ok`, `fails_needs_error_path`).
2. Cases allowlisted this session and the shared pattern behind them.
3. Files changed and the root cause addressed.
4. Any cases unlocked but not yet allowlisted (candidates for follow-up).
5. Suggested next targets grouped by shared root cause.
6. **Session continuation recommendation** — close with one of:
   - **Continue here** — when the next target builds directly on this
     session's fix (same code paths, same mental model still loaded) and
     the conversation hasn't accumulated much one-off scratch state. Also
     fine when the user has explicitly queued follow-up targets.
   - **Compact, then continue** — when the next target is in the same
     skill but the conversation has accumulated long tool outputs (full
     CST dumps, multi-file reads, large diffs) that would crowd context.
     Compaction preserves the cluster knowledge but drops the noise.
   - **New session** — when the next target shifts to an unrelated root
     cause (e.g. lexer indent state vs. projection helpers), or when the
     current session ended on a structural decision worth re-grounding
     against fresh triage. Also recommend this if the user is pausing and
     the work won't resume within the prompt-cache window.

   Don't default to one answer; pick based on what the next target needs.
