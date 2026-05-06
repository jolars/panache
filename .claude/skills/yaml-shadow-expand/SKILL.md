---
name: yaml-shadow-expand
description: Incrementally expand the Panache YAML shadow parser by triaging
  yaml-test-suite fixtures one (or a few) at a time and allowlisting cases as
  parser/projection support grows.
---

Use this skill when asked to extend YAML shadow parser coverage, add a new
yaml-test-suite case to the allowlist, or pick "the next best case" to work on.

## Scope boundaries

- Target is the incremental shadow YAML parser in
  `crates/panache-parser/src/parser/yaml/` and the event-parity harness in
  `crates/panache-parser/tests/yaml.rs`.
- This is a **long-horizon effort**. Do not frame it as a near-term replacement
  for the existing `yaml_parser` dependency.
- Stay parser-crate scoped. Do not leak YAML parser changes into the formatter
  or CLI.
- Keep CST lossless (markers, whitespace, comments, scalar trivia preserved).

## Key files

- `crates/panache-parser/src/parser/yaml/lexer.rs` — indentation-aware lexer,
  block/flow token emission, block-scalar handling.
- `crates/panache-parser/src/parser/yaml/parser.rs` — rowan CST builder. Outer
  `parse_stream` / `emit_document` produce `YAML_STREAM > YAML_DOCUMENT*`;
  `emit_block_map` / `emit_block_seq` / `emit_scalar_document` build per-doc
  bodies and break on `---` / `...` boundaries.
- `crates/panache-parser/src/parser/yaml/events.rs` — event projection
  (`project_events` plus `project_*` helpers). Walks the CST and produces a
  yaml-test-suite event stream.
- `crates/panache-parser/src/parser/yaml/model.rs` — token enum, diagnostic
  codes, shadow report shape.
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
   - Start with `fails_needs_feature` entries where `tree: true` — these only
     need projection fixes.
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
   - Parser-shape issue → edit `parser/yaml/parser.rs` emitters. Outer:
     `parse_stream`, `emit_document`. Bodies: `emit_block_map`
     (+ `emit_block_map_entry` / `_key` / `_value` / `consume_block_scalar`),
     `emit_block_seq` (+ `emit_block_seq_item`), `emit_flow_map`,
     `emit_flow_sequence`, `emit_scalar_document`. Body emitters break on
     `DocumentStart` / `DocumentEnd`; the stream loop owns boundaries.
   - Lexer gap → edit `lexer.rs`; consider indent/flow/block-scalar state
     interactions.
   - Diagnostic gap → add a code in `model.rs::diagnostic_codes` and surface
     it at the right point.

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
  The lexer is explicitly indentation-aware by design.

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
