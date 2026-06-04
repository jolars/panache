# In-tree YAML formatter cutover plan

Staged plan for retiring `yaml_parser` and `pretty_yaml` in favor of an
in-tree YAML formatter driven by the in-tree YAML CST. Sibling document
to `SKILL.md`. Annotate the **What landed** block as work progresses,
matching the `scanner-rewrite.md` precedent in `yaml-shadow-expand/`.

## Status

- **Phase 1 (shadow formatter):** in progress. 1.1–1.15b as
  previously recorded; 1.15b adds the plain-scalar overflow analog of
  rule 6 — when a single-line plain scalar in a block-map value
  pushes its line past `line_width`, greedy word-wrap onto
  continuation lines indented at `depth * 2` (the value column,
  matching rule 1's multi-line continuation indent so wrap output
  round-trips). Quoted/block/decorated/seq-item scalars skip. Probe
  results post-fix: 17/17 input + 16/16 expected frontmatter, 35/35
  expected hashpipe, **20/21 input hashpipe** — the remaining gap
  (`issue_194_idempotency_lsj_tbl_cap`) is the deliberate
  trailing-space-vs-strip tradeoff governed by rule 10 (we strip a
  one-char trailing space pretty_yaml keeps for fold-semantic
  preservation), which already aligns with the host fixture's
  expected output.
- **Phase 2 (joint cutover):** consumer audit done (see "what landed"
  below). Headline: the two crates have **independent** removal
  blockers and should be sequenced, not removed in one commit as the
  original framing assumed.
  - `pretty_yaml` (formatting only) is the small, clean cutover —
    swap the two `yaml_engine.rs::format_yaml_with_config` bodies
    (`src/yaml_engine.rs` **and** `crates/panache-formatter/src/yaml_engine.rs`,
    both live) from `pretty_yaml::format_text` to
    `formatter::yaml::format_yaml`. It is a text→text swap; the
    in-tree formatter modules have **zero** runtime `pretty_yaml`
    references (all comment-only), and the validator's `pretty_yaml`
    mentions are comment/test-name only too.
  - `yaml_parser` removal is the large blocker the original plan
    under-scoped. Beyond the CST/diagnostics bridge
    (`crates/panache-parser/src/syntax/yaml.rs`, Category B), it is
    the **value-extraction AST** for semantic metadata — `src/metadata/yaml.rs`,
    `src/metadata/project.rs`, `src/bib/csl_yaml.rs`, `src/includes.rs`,
    and `crates/panache-formatter/src/formatter/hashpipe.rs` all walk
    `yaml_parser::ast::{Root, BlockMap, BlockMapEntry, BlockMapValue,
    Flow, FlowSeq, BlockSeq}` and `SyntaxKind::{SINGLE,DOUBLE}_QUOTED_SCALAR`
    to read titles/authors/bibliography/includes/project-config/chunk-options.
    The in-tree parser exposes only a raw rowan `SyntaxNode`
    (`parse_yaml_tree`/`parse_yaml_report`) — **no typed AST**. So
    `yaml_parser` removal needs either typed accessors over the
    in-tree CST or a serde-based value reader; that's its own
    multi-file workstream, not part of the formatter cutover.
  - **CST embedding is the end goal, not a non-issue** (correcting the
    original framing). Today `blocks/metadata.rs` emits frontmatter as
    opaque **raw line tokens** under `YAML_METADATA_CONTENT` (the
    hashpipe preamble is the same), and YAML *structure* is recovered
    only by **re-parsing the content string on demand**. That was true
    of `yaml_parser` too (it was always `parse(&content)` on demand,
    never embedded), so retiring `yaml_parser` did **not** require
    embedding — which is the only sense in which it was a "non-issue":
    the *dependency cutover* doesn't depend on it. But the whole point
    of the in-tree parser is to put the YAML tokens (`YAML_STREAM` /
    `YAML_DOCUMENT` / `YAML_BLOCK_MAP` / …) **inside the full document
    CST**, so the frontmatter and hashpipe bodies stop being opaque
    text. That gives one parse instead of parse-then-reparse, and lets
    the linter/LSP/formatter navigate YAML structure in the host tree
    (key goto, folding, semantic tokens, hover) instead of re-parsing a
    substring with offset remapping. This is now its own phase (2c).
  - Recommended sequencing: (2a) `pretty_yaml` formatter swap first
    (small, no host-golden shift expected) — **DONE**; (2b)
    `yaml_parser` value-extraction migration (typed AST wrappers over
    the in-tree CST) + diagnostics repoint + drop the `yaml_parser` dep
    — **DONE** (re-parse-on-demand parity swap; host CST shape
    unchanged); (2c) **embed the in-tree YAML CST into the host
    document CST** so frontmatter/hashpipe carry real YAML structure
    — **OUTSTANDING** (the actual end goal; its own workstream, see
    Phase 2c below); (2d) drop `pretty_yaml` (retire the
    cross-validation test) — **OUTSTANDING** (`pretty_yaml` is
    runtime-unused since 2a; only `yaml_cross_validation.rs` references
    it, and it still pulls `yaml_parser` in transitively).
- **Phase 3 (hashpipe extension):** functionally already on the in-tree
  stack — hashpipe *value extraction* migrated to the in-tree wrappers
  in 2b, and hashpipe *option-body formatting* has gone through
  `yaml_engine::format_yaml_with_config` (the in-tree formatter) since
  2a. **3.2 DONE** (see "what landed"): dedicated hashpipe corpus
  fixtures landed under `tests/fixtures/yaml_corpus/hashpipe/`, the lone
  stale `pretty_yaml`-era comment in `hashpipe.rs` de-staled, and the
  host `issue_*_hashpipe_*` goldens audited (no `pretty_yaml` quirks
  remain — they were reconciled to in-tree output when 2a went live).
  Remaining: folding hashpipe into the 2c CST embedding (the preamble
  content node) — deferred **with 2c**, not part of Phase 3 proper.

## What landed since drafting

_(Update as phases complete. Earliest entries on top.)_

- **Phase 2d (partial) — `pretty_yaml` recategorized as a dev-dependency.**
  `pretty_yaml` has been runtime-unused since 2a, yet sat under
  `[dependencies]` in both the root `panache` crate (where nothing —
  src/tests/benches — referenced it; a dead edge) and `panache-formatter`
  (where only `tests/yaml_cross_validation.rs` uses it; `src/` mentions it
  in comments only). Fixed the categorization: removed the dead entry from
  root `Cargo.toml` and moved the formatter's entry from `[dependencies]`
  to `[dev-dependencies]`. **No test/source change** — the cross-validation
  test keeps its `format_yaml == pretty_yaml::format_text` parity oracle and
  idempotency check (the parity oracle is the only per-case correctness bar
  for the 112-case corpus, so it stays live). The `pretty_yaml`-referencing
  doc comments across `formatter/yaml*.rs`, `formatter.rs`, and parser
  `validator.rs` remain accurate (we still cross-validate), so none were
  touched. **Payoff:** both published crates stop shipping `pretty_yaml`
  (and transitively `yaml_parser`) as normal dependencies — downstream
  consumers get a slimmer tree; only dev/test builds in this repo pull them
  now. Verified: `cargo tree -e normal` shows no `pretty_yaml` under either
  crate, `-e dev` shows it under `panache-formatter`; the sole `Cargo.lock`
  delta is dropping the `pretty_yaml` edge from the `panache` package block
  (both packages remain, as accepted). Full workspace `cargo test`, clippy
  `-D warnings`, fmt clean. **Deliberately deferred:** the *full* drop (and
  thus evicting `pretty_yaml` + `yaml_parser` from `Cargo.lock`) needs the
  parity oracle retired/re-based first — the Phase 2 exit criterion is
  knowingly not yet met.
- **Phase 2c step 2 — prefix-aware scanner + builder.** Landed as two
  parser-crate commits. (1) `refactor(parser): fragment multi-line YAML
  scalars at line breaks` — `emit_scalar_node` now splits a multi-line
  scalar into per-line `YAML_SCALAR_TEXT` leaves interleaved with
  `NEWLINE` (the seam the `#|` prefix interleaves into); node text range
  unchanged so the parse stays byte-lossless and yaml-test-suite event
  parity holds. Surfaced three event-projection consumers that had
  assumed the single-embedded-newline leaf: the block-map-value and
  block-sequence-item `value_text` assemblies dropped `NEWLINE` (folding
  `e\n  f` → `e  f` instead of `e f`, A984/UV7Q — fixed by keeping
  `NEWLINE`), and `collect_scalar_source`/`fold_plain_document_lines`
  carried a now-stale `%`-line filter that dropped a `%YAML 1.2`
  plain-scalar continuation (XLQ9 — removed; real directives are
  `YAML_DIRECTIVE`, already excluded by kind). The formatter's
  `canonical_indent_depth` (`formatter/yaml/document.rs`) was repointed to
  read the parent `YAML_SCALAR` node (multi-line check, start offset,
  `|`/`>` probe) instead of a single leaf — fixed 4 host hashpipe goldens
  (issue_172/201, nested-list-indent, code_blocks_executable). 297
  yaml-test-suite CST snapshots re-blessed (pure scalar-leaf splitting).
  (2) `feat(parser): prefix-aware YAML scanner and builder` — new
  `parse_stream_with_prefix` / `validate_yaml_with_prefix`. The scanner
  carries a `line_prefix` (`Box<str>`) and recognizes the marker (+ ≤1
  trailing space, mirroring `strip_hashpipe_prefix`) at each physical line
  start: in the main fetch loop it is consumed as a `Trivia(LinePrefix)`
  token (`YAML_LINE_PREFIX`) inside `scan_newline` (+ a first-line
  bootstrap in `scan_trivia`) **before** the `#` reaches `scan_comment`,
  resetting `cursor.column` so all ~15 column sites are prefix-excluded
  for free. The raw-byte forward scanners
  (`auto_detect_block_scalar_indent`, the block-scalar content loop) skip
  the marker via `prefix_byte_len_at`; the plain
  (`try_consume_plain_line_break`) and quoted (`fetch_flow_scalar`)
  multi-line continuation paths call `skip_embedded_line_prefix` (advance
  past the marker, reset column, leave bytes embedded). `emit_scalar_node`
  peels the embedded continuation marker into a `YAML_LINE_PREFIX` leaf.
  Result is the directive's "no offsets" shape: the YAML CST token ranges
  are host ranges directly (verified on a block scalar — `YAML_SCALAR@12..62`
  over raw `#|`-prefixed bytes, blank `#|` line peeled as a 2-byte
  `YAML_LINE_PREFIX`). New `yaml_prefix_parity.rs` harness asserts
  losslessness, structural parity vs. the prefix-stripped baseline
  (projected events; the projector already skips `YAML_LINE_PREFIX`), and
  validator agreement across block scalars, quoted/plain multi-line
  continuation, flow, blank `#|`, dotted keys, and tags; the empty-prefix
  path is pinned identical to the plain parse (frontmatter callers
  unaffected). Full workspace `cargo test`, clippy `-D warnings`, fmt
  clean. Remaining 2c: steps 3 (cook over prefixed scalars), 4 (host
  embedding), 5 (consumer rewire + drop offset layer).
- **Phase 2c step 1 — `YAML_SCALAR` promoted from token to node.** The
  enabling CST reshape for hashpipe embedding (and the long-term shape the
  formatter/LSP want). A scalar value is now a `YAML_SCALAR` **node**
  wrapping a single `YAML_SCALAR_TEXT` content leaf, instead of a single
  `YAML_SCALAR` token. The wrapper is the seam a later step uses to
  interleave hashpipe `#|` line-prefix leaves as clean tokens (the
  "no offsets" requirement). To make `YAML_SCALAR` unambiguously a node,
  the two other things the parser had been emitting as `YAML_SCALAR`
  *tokens* got their own leaf kinds: flow punctuation (`[ ] { } ,`) →
  `YAML_FLOW_INDICATOR`, and directive lines (`%YAML`/`%TAG`) →
  `YAML_DIRECTIVE`. Builder change is `emit_scalar_node` in
  `parser/yaml/parser.rs`; the typed `YamlScalar` wrapper
  (`syntax/yaml_ast.rs`) is now node-based (`.raw()` returns `String`).
  **Single-leaf, not fragmented:** the leaf still carries embedded
  newlines for multi-line scalars, so `events.rs` flat-token
  reconstruction stays byte-identical — per-line fragmentation is
  deferred to step 2 where the `#|` prefix actually needs to interleave.
  Blast radius was the two parity-gated files (`validator.rs`,
  `events.rs`, ~40 `YAML_SCALAR` sites each — direct-child scans switched
  to node lookups; content descendant-filters switched to
  `YAML_SCALAR_TEXT`; flow/directive checks switched to the new kinds),
  plus the live formatter (`formatter/yaml/document.rs`). Guardrails held:
  yaml-test-suite **event parity unchanged** (the projection produces
  identical events from the new shape), **losslessness unchanged**, and
  the 297 yaml-test-suite CST snapshots + the YAML golden parser cases
  re-blessed (audited in aggregate: every removed `YAML_SCALAR` token maps
  1:1 to `YAML_SCALAR`+`YAML_SCALAR_TEXT` / `YAML_FLOW_INDICATOR` /
  `YAML_DIRECTIVE`, no other kinds touched). Full workspace `cargo test`,
  clippy `-D warnings`, and fmt clean. Resolves the open questions
  "Block-scalar interior re-indent" (option a, parser-side token lift) and
  "Style-as-CST-kind promotion" directionally — scalars are now navigable
  structure. Remaining 2c steps below.
- **Phase 3.2 — hashpipe corpus + finishing.** Closes the 2c-independent
  half of Phase 3. (1) Added a dedicated hashpipe corpus under
  `crates/panache-formatter/tests/fixtures/yaml_corpus/hashpipe/` —
  five plain-YAML payload cases mirroring the shapes the host
  `issue_*_hashpipe_*` fixtures emit after `#|` stripping:
  `fig_subcap_block_sequence` (quoted captions in a block sequence,
  issue #172/#181), `dotted_key` (`cache.extra:` + a `cache-vars` block
  sequence, issue #280), `yaml_tag_value` (`!expr` tag, issue #280),
  `blank_line_between_keys` (interior single blank, issue #190), and
  `continuation_lines` (multi-line plain scalar, issue #189). The
  cross-validation harness (`yaml_cross_validation.rs`) auto-discovers
  them recursively — no harness change — and all five passed
  `format_yaml == pretty_yaml` parity **and** idempotency on the first
  run (no parser/formatter fix or 14th rule needed; tag + dotted key
  verified to round-trip end-to-end through the live formatter). (2)
  De-staled the only `pretty_yaml`-era comment left in
  `crates/panache-formatter/src/formatter/hashpipe.rs` (the print-width
  note above the `line_width.saturating_sub(prefix.len() + 1)` math) to
  reference the in-tree formatter; `grep pretty_yaml hashpipe.rs` is now
  empty. The width subtraction itself and the issue-#172 block-value
  preservation notes are load-bearing and untouched. (3) Audited the
  host hashpipe goldens — all nine registered cases green and reflecting
  in-tree output; `issue_194_idempotency_lsj_tbl_cap`'s trailing-space
  strip is the intentional rule-10 trade, not a `pretty_yaml` quirk; no
  fixture edits. **Note:** the on-disk
  `issue_179_hashpipe_one_space_list_idempotency` case is **not**
  registered in `tests/golden_cases.rs` (pre-existing wiring gap,
  unrelated to the cutover) — flagged, not fixed. Remaining Phase 3 work
  (nest the hashpipe preamble's YAML structure into the host CST) folds
  into the still-outstanding Phase 2c. STYLE.md unchanged (no new rule).
- **Phase 2b — `yaml_parser` retirement via in-tree typed AST
  wrappers.** Built `crates/panache-parser/src/syntax/yaml_ast.rs`: a
  typed wrapper layer over the in-tree YAML CST in the house
  rust-analyzer/rowan style (`YamlDocument`, `YamlBlockMap`,
  `YamlBlockMapEntry`/`Key`/`Value`, block/flow sequences & maps, a
  token-backed `YamlScalar` with `raw()` / cooked `value()` / `style()`
  / `text_range()`, a `YamlNode` union, and a public `YamlScalarStyle`),
  plus `parse_yaml_document` / `parse_yaml_documents` that descend the
  `DOCUMENT > YAML_METADATA_CONTENT > YAML_STREAM` envelope. Cooking
  reuses `crate::parser::yaml::cook` (re-exported `pub(crate)`). The
  colon-in-key gotcha (`YAML_BLOCK_MAP_KEY` includes `:`) is handled by
  reading the scalar token child. Migrated all five value-extraction
  consumers off `yaml_parser::ast` onto the wrappers — `src/includes.rs`,
  `src/metadata/project.rs`, `src/metadata/yaml.rs`, `src/bib/csl_yaml.rs`
  (metadata/bib/includes use cooked `value()`, fixing the old greedy
  `trim_matches` bug; pinned by a new `metadata::yaml` test), and
  `crates/panache-formatter/src/formatter/hashpipe.rs` (uses `raw()` +
  `text_range()` + `value.tag()` to preserve `#|` round-trip bytes).
  Repointed the diagnostics/validation infra in
  `crates/panache-parser/src/syntax/yaml.rs` (`validate_yaml_text`,
  `ParsedYamlRegion`, `YamlAstRoot`, `document_shape_summary`) onto
  `parse_yaml_report`; offset + `"Root docs=N first=Kind"` parity held.
  Removed `yaml_parser` from all three `Cargo.toml`s (root,
  `panache-parser`, `panache-formatter`); it survives in `Cargo.lock`
  only as a transitive dep of `pretty_yaml` (2d). **Architecture note:
  this is a re-parse-on-demand *parity* swap** — like `yaml_parser`
  before it, structure is recovered by re-parsing the frontmatter
  content string; the host document CST is **unchanged** (still opaque
  `YAML_METADATA_CONTENT` line tokens), which is why every losslessness
  / CST-snapshot test passed untouched. Embedding the YAML tokens into
  the host CST is the separate, still-outstanding end goal (Phase 2c
  below). Full workspace suite green (incl. 280 golden cases +
  `yaml_cross_validation`), clippy `-D warnings` clean, fmt clean,
  `debug format --checks all` green on frontmatter-rich and hashpipe
  docs.
- **Phase 2a — `pretty_yaml` formatter swap (live cutover).** The
  in-tree formatter is now the live YAML formatting path. Swapped both
  `format_yaml_with_config` bodies (`src/yaml_engine.rs` and
  `crates/panache-formatter/src/yaml_engine.rs`, byte-identical copies)
  from `pretty_yaml::format_text` to
  `panache_formatter::formatter::yaml::format_yaml`, bridging
  `config.line_width` → `YamlFormatOptions.line_width` and
  `config.wrap` → `YamlFormatOptions.wrap` via a new
  `yaml_wrap_for_config` (Preserve → `WrapMode::Preserve`, all else →
  `Always`, mirroring the old `prose_wrap_for_config` mapping). The
  `validate_yaml` gate (still `yaml_parser`-backed, Phase 2b) is kept
  in front, so behavior on invalid YAML is unchanged. Both plain
  frontmatter and hashpipe option bodies cut over together since they
  share the `format_yaml_with_config` chokepoint — no partial cutover.
  **Behavior gap found + fixed:** the in-tree wrap passes consulted
  only `line_width`, never `opts.wrap`, so `WrapMode::Preserve` was
  ignored (the corpus harness only ever ran the default `Always`).
  Probed pretty_yaml to pin the correct semantics: `ProseWrap::Preserve`
  leaves overflowing *plain scalars* on their line but still wraps
  overflowing *flow collections* (flow wrapping is a print-width
  concern, not prose). Fix: gate `apply_plain_scalar_wrap` on
  `WrapMode::Always` in `document.rs`; leave `apply_flow_wrap`
  unconditional. New unit test
  `preserve_wrap_mode_leaves_plain_scalar_unwrapped` pins both
  directions (plain scalar preserved; flow still wraps under Preserve).
  Renamed the two `prose_wrap_follows_panache_wrap_mode` engine tests to
  `wrap_mode_follows_panache_wrap_mode` asserting against
  `formatter::yaml::WrapMode`. Updated stale "shadow / not wired" doc
  comments in `formatter/yaml.rs`, `formatter/yaml/options.rs`, and
  `formatter.rs`. **No host golden fixture changed** — the full
  workspace suite (incl. `golden_cases`) is green unmodified,
  confirming byte parity with the retired pretty_yaml path across the
  fixture set. `pretty_yaml` is now used only by the cross-validation
  test (`yaml_cross_validation.rs`); the `Cargo.toml` deps are left in
  place for 2c. clippy + fmt clean.
- **Phase 2.0 — consumer/dependency audit (no code).** Mapped every
  `yaml_parser` and `pretty_yaml` touch point ahead of the cutover.
  Findings (full detail in the Phase 2 status bullet above):
  `pretty_yaml` is formatting-only and lives in two live
  `yaml_engine.rs` copies (`src/` host + `crates/panache-formatter/`),
  both a clean text→text swap to `formatter::yaml::format_yaml`; the
  in-tree formatter modules and the parser validator reference
  `pretty_yaml` in comments/test-names only (no runtime dep).
  `yaml_parser` is the real blocker — besides the CST/diagnostics
  bridge it backs typed value extraction in `src/metadata/yaml.rs`,
  `src/metadata/project.rs`, `src/bib/csl_yaml.rs`, `src/includes.rs`,
  and `formatter/hashpipe.rs`, none of which have an in-tree
  equivalent (the in-tree parser yields a raw rowan CST, no typed
  AST). The narrow "CST-kind no-op swap" the original plan worried
  about is a non-issue: host consumers only touch the wrapper kinds
  (`YAML_METADATA`/`_CONTENT`/`_DELIM`, `YAML_CONTENT`,
  `YAML_PREAMBLE`) via `.text()`, never interior structural kinds.
  Conclusion: split Phase 2 into 2a (`pretty_yaml` formatter swap,
  small) and 2b (`yaml_parser` value-extraction migration, its own
  workstream); drop both deps in 2c. No source changes this session.

- **Phase 1.15b — plain-scalar overflow wrap (rule 6's block-map
  analog).** Closes the last functional gap from the Phase 2 readiness
  probe (18/21 → 20/21 input hashpipe parity). Added
  `apply_plain_scalar_wrap` to
  `crates/panache-formatter/src/formatter/yaml/document.rs::render`,
  inserted between rule 6's flow wrap and rule 10's trailing-WS strip.
  Strategy: re-parse the post-indent buffer, walk
  `YAML_BLOCK_MAP_VALUE` nodes; for each value whose direct child is a
  single-line plain `YAML_SCALAR` (skip quoted `'…'`/`"…"`, block
  `|`/`>`, multi-line, or values decorated with tags / anchors /
  aliases / inline comments / inside a block sequence), greedy
  word-wrap the scalar onto continuation lines at `depth * 2`
  (matching rule 1's multi-line continuation column so wrap output
  round-trips). Multi-space runs that aren't break points stay
  verbatim; a multi-space run that IS the break point is consumed
  entirely by `\n + indent` (pretty_yaml leaves the leading char as a
  trailing space to preserve YAML's plain-scalar fold semantics, but
  rule 10 would strip it anyway, so consuming it here keeps pass-2
  byte-stable — same family of trades against pretty_yaml's semantic
  preservation as rule 10's stance). Block-sequence values are
  deliberately deferred: pretty_yaml's wrap continuation there
  (`parent_content_col + 2`) disagrees with rule 1's multi-line
  continuation (`depth * 2`), so pretty_yaml itself fails idempotency
  on that shape — picking one column without breaking pass-2
  stability needs a spec decision we don't need today (no host fixture
  exercises a long single-line plain scalar in a block sequence).
  Seven new corpus cases under
  `tests/fixtures/yaml_corpus/plain_wrap/`:
  `simple_block_map_overflow` (top-level depth 1),
  `nested_block_map_overflow` (depth 2 with col-4 continuation),
  `multiple_entries_one_overflows` (two-entry map; only the long
  entry wraps), `already_wrapped_round_trip` (pretty_yaml output fed
  back; sticks unchanged), `non_overflow_stays_single_line`
  (no wrap at width 80), `quoted_value_preserved` (long `"…"` stays),
  `block_scalar_value_preserved` (long `|` stays). Four new unit
  tests in `yaml.rs` (`rule_6_plain_scalar_wraps_at_block_map_value`,
  `rule_6_plain_scalar_wrap_skips_non_plain_and_short`,
  `rule_6_plain_scalar_wrap_skips_inline_comment_and_decoration`,
  `rule_6_plain_scalar_wrap_skips_block_sequence_value`). STYLE.md
  rule 6 amended with the plain-scalar overflow paragraph (greedy
  wrap, depth*2 continuation, scope restrictions, multi-space
  semantics) and the header note bumped to reference 1.15b. yaml.rs
  status block bumped. No live-pipeline changes. The one remaining
  input-hashpipe gap (`issue_194_idempotency_lsj_tbl_cap`,
  `*clinicaltrial.csv*  data`) is the deliberate rule 10 strip vs
  pretty_yaml fold-semantic preserve trade — host fixture already
  expects our shape, so the cutover commit is now clear.
- **Phase 1.15 — multi-line scalar continuation canonicalization
  (rule 1 extension).** Probe-driven: a one-shot survey under
  `crates/panache-formatter/tests/yaml_fixture_survey.rs` (now
  removed) ran `format_yaml` and `pretty_yaml::format_text` over the
  YAML frontmatter and hashpipe payloads of every fixture under
  `tests/fixtures/cases/`. Result: 7/35 expected-hashpipe and 8/21
  input-hashpipe divergences clustered around multi-line plain /
  single-quoted / double-quoted scalars whose continuation lines lost
  their indent because rule 1's depth formula
  (`entry/item ancestors − 1`) returned 0 for the continuation line's
  containing entry. Fix: extend `canonical_indent_depth` in
  `crates/panache-formatter/src/formatter/yaml/document.rs` to handle
  multi-line `YAML_SCALAR` continuation lines explicitly — block
  scalars (`|`/`>`) still preserve verbatim (no real renderer yet);
  plain / single- / double-quoted continuation lines indent at
  `entry/item ancestors * 2` (one level deeper than the default; the
  scalar belongs to the value side of the entry, so its column is the
  value column rather than the key column). Five new corpus cases
  under `tests/fixtures/yaml_corpus/multiline_scalars/`
  (`plain_continuation_canonical`, `double_quoted_continuation_canonical`,
  `single_quoted_continuation_canonical`,
  `double_quoted_continuation_one_space`,
  `nested_value_continuation`). Two new unit tests in `yaml.rs`
  (`rule_1_canonicalizes_multiline_plain_scalar_continuation`,
  `rule_1_canonicalizes_multiline_quoted_scalar_continuation`).
  STYLE.md rule 1 amended with the value-column note. The probe
  results after this fix: 14/14 expected frontmatter, 15/15 input
  frontmatter, 35/35 expected hashpipe, 18/21 input hashpipe parity.
  The remaining 3 input-hashpipe gaps are all long single-line plain
  scalars that pretty_yaml wraps and the in-tree formatter leaves
  untouched — these are the Phase 2 blocker (plain-scalar overflow
  wrap, rule 6's analog for block-map scalar values). The probe and
  CST shape probes were one-shot tools and were removed after the fix
  landed. No live-pipeline changes.
- **Phase 1.14 — multi-line flow round-trip (parser + formatter).**
  Two coupled changes that unblock the "multi-line flow input is
  sticky" behavior parked in Phase 1.10. Parser-side: relaxed
  `check_flow_continuation_indent` in
  `crates/panache-parser/src/parser/yaml/validator.rs` so that a
  continuation line whose first non-whitespace byte is the flow's
  matching closing indicator (`]` for `YAML_FLOW_SEQUENCE`, `}` for
  `YAML_FLOW_MAP`) is exempt from the strict `col > threshold` rule.
  YAML 1.2 §7.1 reads the spec stricter than mainstream parsers — but
  pretty_yaml, libyaml (via pandoc), and yaml.v3 (via yq) all emit
  and accept the closing bracket on its own line at the parent
  block-map's indent column, and the `parser` rule names pandoc as
  the behavioral reference. Verified via probes (pandoc `-t native`
  + yq) on depth-0 closing-`]`, depth-0 closing-`}`, and depth-1
  nested cases. Five new unit tests in
  `parser/yaml/validator.rs::tests` cover accept (depth-0 seq /
  depth-1 seq / depth-0 map) and reject (CML9 comment line at parent
  indent, 9C9N content lines at parent indent) directions; the three
  existing yaml-test-suite snapshots (CML9, 9C9N, VJP3/00) still
  carry their `LEX_WRONG_INDENTED_FLOW` diagnostics because the
  carve-out only spares the closing indicator, not content/comment
  lines at the threshold. Triage regen produced no bucket changes
  (308 passes_now / 94 error_contract_ok unchanged).

  Formatter-side: rule 1's
  `canonical_indent_depth` returns `None` when the offset lands on a
  continuation line of an enclosing multi-line `YAML_FLOW_SEQUENCE` /
  `YAML_FLOW_MAP` (the ancestor flow's text contains a `\n` between
  its start and the offset). Without this carve-out, rule 1 would
  re-indent multi-line flow content as if it were block-map keys at
  depth 0 (column 0), destroying the wrap that rule 6 produced and
  breaking idempotency on every `flow_wrap/*.yaml` corpus case once
  the parser stopped rejecting their pass-1 outputs. Three new
  corpus cases under
  `tests/fixtures/yaml_corpus/flow_wrap/`
  (`sticky_multiline_depth_0`, `sticky_multiline_depth_1`,
  `sticky_multiline_map`) feed pre-wrapped pretty_yaml output back
  through the harness — they must parity-match pretty_yaml and
  round-trip unchanged. One new unit test in
  `formatter/yaml.rs::tests::rule_6_wrap_round_trips_multiline_input`
  pins the depth-0 seq case at the API level. STYLE.md unchanged
  (the spec already documented rule 6's wrap shape; this fixes
  implementation). No live-pipeline changes — still shadow.
- **Phase 1.13 — real-frontmatter harvest + rule 14
  (block-structural spacing).** Pulled six representative frontmatter
  blocks into `tests/fixtures/yaml_corpus/real/`:
  `quarto_frontmatter_keywords` (title/author/date/keywords sequence
  from `tests/fixtures/cases/yaml_metadata/`),
  `whitespace_normalization` (the `echo:    false` / `-  a` /
  `-     b` stressor from `yaml_metadata_normalization/`),
  `non_ascii_scalar` (`smörgås` from `umlauts/`),
  `quarto_landing_page` (the full `docs/index.qmd` frontmatter —
  folded scalars, nested `open-graph` / `twitter-card` / `format`
  sub-maps), `folded_description_performance` (folded `description: >`
  + `engine: knitr` from `docs/guide/performance.qmd`), and
  `folded_description_short` (short folded description from
  `docs/guide/formatting.qmd`). Skipped single-line `title: x` cases
  (already covered by `mappings/simple_mapping`) and the
  `yaml_metadata_opening_blank_not_metadata` case (already covered
  by `blank_lines/leading_blank_run`). The `whitespace_normalization`
  case immediately surfaced a spec gap pretty_yaml normalizes but
  rules 1, 5, and 8 didn't reach: runs of whitespace between block
  structural indicators (`:` after a key, `-` after a sequence
  marker) and their inline value. Resolution: a 14th rule, added per
  the `yaml-formatter` rule on deliberate spec extensions.

  Rule 14 implementation: a `WHITESPACE` token whose `prev_token()`
  is `YAML_COLON` or `YAML_BLOCK_SEQ_ENTRY` and whose `next_token()`
  is not `NEWLINE` collapses to a single space. Composed into
  `emit_token` via an OR with rule 8's
  `is_ws_before_inline_comment` — both want the same output for the
  shared `key:    # comment` shape, so no precedence conflict. Three
  new corpus cases under `tests/fixtures/yaml_corpus/structural_spacing/`
  (`multiple_spaces_after_colon`, `multiple_spaces_after_dash`,
  `tab_after_colon`) plus the harvested `real/whitespace_normalization`
  case lock the behavior. Two new unit tests in `yaml.rs`
  (`rule_14_collapses_run_after_colon`,
  `rule_14_collapses_run_after_dash`) cover the trailing-WS carve-out
  (`key:   \n  inner: v` keeps the trailing WS untouched for rule 10
  to strip) and the bare-`-` case (`-   \n  - foo` keeps the dash
  alone, not `- ` + nothing). STYLE.md amended: header notes rules
  1–12 + 14 share a 15-case cross-validation battery with Prettier,
  rule 13 + 14 were cross-validated against pretty_yaml later in the
  corpus harness rollout. yaml.rs module doc-comment bumped to 1.13.
  No live-pipeline changes.
- **Phase 1.12 — preserve-rule lockdown (rules 4, 9, 11, 12).** No
  formatter code; locks in the four spec rules that explicitly decline
  to canonicalize a semantically-meaningful user choice by giving each
  corpus + unit coverage that cross-validates against pretty_yaml.
  Eleven new corpus cases. Under
  `tests/fixtures/yaml_corpus/block_scalars/`: `literal_preserved`,
  `folded_preserved`, `literal_strip`, `folded_keep`, `literal_in_seq`,
  `folded_then_literal` — exercise `|`, `>`, `|-`, `>+`, and mixed
  literal/folded usage in both block-map values and block-sequence
  items. Under `comments/`: `between_keys`, `between_seq_items`,
  `trailing_doc_comment`, `blank_separated_section` — exercise rule
  9's position-preservation at the doc-end, between map keys, between
  sequence items, and across a blank-line section boundary. Under
  `empty_values/`: `bare_empty`, `multiple_empties`,
  `empty_with_inline_comment`, `empty_in_sequence` — exercise rule
  11's no-`null` canonicalization across bare, stacked, comment-
  trailed, and sequence-position empties. Under `key_order/`:
  `reverse_alpha_preserved`, `numeric_like_keys_preserved`,
  `deep_order_preserved` — exercise rule 12 with reverse-alphabetic
  top-level keys, quoted numeric keys (avoids stringification
  surprises), and reverse order at two nesting levels. Four new unit
  tests in `yaml.rs` (`rule_4_block_scalar_style_preserved`,
  `rule_9_comment_positions_preserved`,
  `rule_11_empty_scalars_preserved`, `rule_12_key_order_preserved`)
  lock the behavior at the API level so a future regression doesn't
  ride along with a pretty_yaml regression silently. yaml.rs module
  doc-comment bumped to 1.12 with the preserve-rule note. No
  live-pipeline changes.
- **Phase 1.11 — rule 3 (quote-style preference).** Added
  `try_convert_single_to_double` to
  `crates/panache-formatter/src/formatter/yaml/document.rs::emit_token`.
  Strategy: for any token whose text starts and ends with `'` (length
  ≥ 2), strip outer quotes and de-escape (`''` → `'`), then check the
  content for any of `\`, `'`, `"`, or ASCII control char (< 0x20 or
  0x7F). If found, emit verbatim (keep single). Else emit `"<content>"`
  (convert to double). Brackets/commas inside flow containers are also
  `YAML_SCALAR` tokens but their text never starts with `'`, so the
  prefix check filters them out. Plain and double-quoted scalars pass
  through unchanged — never up-quote plain to double or down-quote
  double to single, matching pretty_yaml's "preserve user choice except
  for the one safe direction" behavior. Conservative on control chars:
  pretty_yaml escapes literal `\t` / `\n` into double-quoted form when
  converting, but we keep single in those cases (frontmatter rarely has
  literal control characters in quoted scalars; the escape logic adds
  complexity for little real-world benefit). Eleven new corpus cases
  under `tests/fixtures/yaml_corpus/quotes/`:
  `single_to_double_simple`, `single_to_double_with_space`,
  `single_to_double_with_colon`, `single_keeps_with_backslash`,
  `single_keeps_with_apostrophe`, `single_keeps_with_doublequote`,
  `double_stays_double`, `plain_stays_plain`,
  `empty_single_becomes_double`, `single_key_converts`,
  `flow_singles_convert`, `seq_singles_convert`. Three new unit tests in
  `yaml.rs` covering the single→double conversion paths, the
  conservative-keep paths, and key/flow-context coverage. STYLE.md
  rule 3 amended with the operational rule (the spec's preference
  order doesn't strip quotes from plain or down-quote double; it's
  applied at the single→double conversion boundary) and the
  control-char carve-out. yaml.rs and document.rs status blocks bumped
  to 1.11. No live-pipeline changes.
- **Phase 1.10 — rule 6 (overflow wrap).** Added `apply_flow_wrap` to
  `crates/panache-formatter/src/formatter/yaml/document.rs::render`,
  inserted between rule 1 (indent canonicalization) and rule 10
  (trailing-WS strip). Strategy: re-parse the post-rule-1 buffer with
  the in-tree YAML parser, walk top-level (`!has_flow_ancestor`)
  `YAML_FLOW_SEQUENCE` / `YAML_FLOW_MAP` nodes in reverse byte order,
  and replace any whose canonical single-line form + the container's
  containing-line context exceeds `opts.line_width`. Wrap layout
  follows pretty_yaml: opening bracket stays on the key line; each
  item indented at `parent_content_column + 2`; trailing comma after
  every item; closing bracket on its own line at
  `parent_content_column`. `parent_content_column` =
  `2 * (entry/item depth − 1)` for a flow in a block-map value, and
  the same +2 for a flow in a block-sequence item — the `- ` prefix
  shifts the content column right by two. Nested flow containers
  inside a wrapped item stay in their canonical rule-5 single-line
  form (matches pretty_yaml's seq-of-maps output). The wrap threshold
  is strict `>`: lines exactly at `line_width` (default 80) stay
  single-line; lines at `line_width + 1` wrap. Re-parsing on rule 6
  is bounded by the in-tree parser's known limitation: multi-line
  flow containers (a flow with `\n` between brackets) currently fail
  to parse, so `format_yaml` already passed the input through
  verbatim before `render` was reached — the "multi-line input is
  sticky" behavior pretty_yaml shows is parked on parser support for
  those inputs. Idempotency holds because run 2 of a wrapped output
  hits the multi-line-flow parser-rejection path and passes through
  verbatim. Seven new corpus cases under
  `tests/fixtures/yaml_corpus/flow_wrap/`: `overflow_depth_0`,
  `overflow_depth_1`, `overflow_depth_2`, `overflow_in_block_seq`,
  `overflow_map`, `overflow_seq_of_maps`, `exactly_80_no_wrap`. Four
  new unit tests in `yaml.rs` (depth-0 wrap with at-80 / over-80
  boundary, depth-1 wrap alignment, block-sequence parent +4 shift,
  nested flow stays canonical). STYLE.md rule 6 amended with the
  wrap-decision formula, the parent-content-column math, the nested
  flow rule, and the multi-line-input deferral. yaml.rs status block
  bumped to 1.10. No live-pipeline changes.
- **Phase 1.9 — rule 5 (canonical flow spacing) + recursive walker.**
  Refactored the token walk into a recursive node walk
  (`walk_with_normalization` → `emit_node` → `emit_token`) so flow
  containers can take over emission for their subtree.
  `YAML_FLOW_SEQUENCE` emits `[item, item, ...]` (no inner space, one
  space after `,`); `YAML_FLOW_MAP` emits `{ k: v, ... }` (one inner
  space, one space after `,`, one space after `:`). When the parser
  couldn't structure a flow map's content into `YAML_FLOW_MAP_ENTRY`
  children (e.g. `{key:value}` — no space to disambiguate `:`), the
  inner bytes are emitted verbatim between `{ ` and ` }` — matches
  pretty_yaml's "normalize spacing around structure, don't re-parse
  content" behavior. Multi-line flow containers and flow containers
  with embedded `YAML_COMMENT` tokens fall through to the generic
  recursive path and emit verbatim (rule 6 will own multi-line wrap;
  in-flow comments are too rare to justify their own canonical path).
  Rule 8 (inline comment WS normalization) was re-anchored to
  `SyntaxToken::prev_token()` / `next_token()` so it works during the
  recursive walk without an array index. Nine new corpus cases under
  `tests/fixtures/yaml_corpus/flow/`: `canonical_sequence`,
  `canonical_map`, `empty_sequence`, `empty_map`,
  `sequence_no_comma_space`, `sequence_extra_space`,
  `map_no_inner_space`, `map_extra_inner_space`, `map_no_comma_space`,
  `map_pathological_no_spaces`, `nested_seq_of_maps`, `nested_maps`,
  `sequence_inside_block_sequence`. Two new unit tests
  (`rule_5_flow_spacing_canonicalized` and
  `rule_5_multiline_flow_preserved_verbatim`). STYLE.md rule 5
  amended with the in-flow-comment / multi-line scope and the
  unparseable-content pass-through behavior. yaml.rs status block
  bumped to 1.9. No live-pipeline changes.
- **Phase 1.8 — rule 8 (inline comment spacing) + pipeline refactor.**
  Added `walk_with_inline_comment_normalization` and
  `is_ws_before_inline_comment` to
  `crates/panache-formatter/src/formatter/yaml/document.rs`. During the
  token walk, when a `WHITESPACE` token's contiguous-WS run ends with
  a `YAML_COMMENT` AND the previous non-WHITESPACE token is not
  `NEWLINE`, the WS is emitted as a single space. Standalone
  comments (line-start) keep original surrounding WS. Rule 8 had to
  run inside the token walk because line-level passes can't reliably
  distinguish `#` inside quoted scalars from a comment indicator.
  Since rule 8 changes byte counts after a line's first non-WS byte
  (collapsing `   ` → ` `), the existing rule-1 implementation's
  CST-offset lookup (`line_start + trimmed_start`) would no longer
  map to CST. Refactored: `precompute_line_depths` walks
  `root.text()` line-by-line and computes the canonical depth per
  CST line up front; `apply_canonical_indents` iterates the
  (post-rule-8) buffer in lockstep — rule 8 preserves `\n` positions,
  so the line index alignment holds. Five new corpus cases under
  `tests/fixtures/yaml_corpus/comments/`:
  `inline_loose_spacing`, `inline_tight_spacing`, `multiple_inline`,
  `nested_inline`, `standalone_above_key`. One new unit test in
  `yaml.rs`. STYLE.md rule 8 amended with the inline/standalone
  distinction and the in-walk implementation note. yaml.rs status
  block bumped to 1.8. No live-pipeline changes.
- **Phase 1.7 — rule 7 (blank-line collapse) + rule 2 verification.**
  Added `collapse_blank_line_runs` to
  `crates/panache-formatter/src/formatter/yaml/document.rs::render`,
  applied after rule 10 (so whitespace-only "blank" lines participate)
  and before rule 13 (so trailing residue gets finalized to one `\n`).
  Interior runs of multiple blank lines collapse to one; leading
  blank lines are stripped entirely — symmetric with rule 13's
  no-trailing-blank-lines invariant. Probed pretty_yaml first: it
  also strips leading blanks (not just collapses), so STYLE.md
  rule 7 was extended to call that out explicitly rather than
  leaving "one max" ambiguous. Rule 2 (sequence items indented +2
  from parent key) verified: rule 1's depth formula
  (`2 * (entry/item ancestors − 1)`) already canonicalizes
  same-column inputs (`categories:\n- foo` → `categories:\n  - foo`)
  because the `-` sits inside two entry/item ancestors. No new code
  for rule 2 — three corpus cases plus a unit test lock the
  behavior. Four new corpus cases under
  `tests/fixtures/yaml_corpus/blank_lines/`
  (`triple_blank_collapses`, `multiple_runs`, `single_blank_preserved`,
  `whitespace_only_blanks_collapse`, `leading_blank_run`) and three
  under `sequences/` (`parent_column_dashes`, `nested_parent_column`,
  `sequence_of_mappings_parent_column`). Two new unit tests in
  `yaml.rs`. yaml.rs status block bumped to 1.7. No live-pipeline
  changes.
- **Phase 1.6 — rule 1 (canonical 2-space indent).** Added
  `canonicalize_line_indents` + `canonical_indent_depth` to
  `crates/panache-formatter/src/formatter/yaml/document.rs`. Strategy:
  walk tokens to build a raw output buffer (byte-lossless), then
  line-rewrite leading whitespace per `2 * (entry/item ancestor
  count − 1)` for each line's first non-WS byte (looked up against
  the CST via `token_at_offset`). Run before rule 10 + rule 13.
  Tab-indented input is rejected by the parser outright — no
  formatter concern. Block scalar (`|`/`>`) interior lines are
  detected (offset > scalar_start, multi-line scalar text starting
  with the indicator) and pass through verbatim because the scalar
  is one multi-line `YAML_SCALAR` token; proper canonicalization
  needs a real block-scalar renderer (deferred — added as an open
  question below and noted in STYLE.md rule 1). Four new corpus
  cases under `tests/fixtures/yaml_corpus/indent/`:
  `nested_mapping_4sp`, `triple_nested_4sp`, `sequence_in_mapping_4sp`,
  `sequence_of_mappings_canonical` (the canonical sequence-of-mappings
  case earns its keep as a structural shape stressor even though it
  doesn't reshape indent). Two new unit tests covering the nested
  collapse cases and the block-scalar passthrough. STYLE.md rule 1
  amended with the depth formula and the block-scalar limitation;
  yaml.rs status block bumped to 1.6. No live-pipeline changes.
- **Phase 1.5 — rule 10 (strip trailing whitespace per line).** Added
  `strip_trailing_whitespace_per_line` to
  `crates/panache-formatter/src/formatter/yaml/document.rs::render`,
  applied before rule 13. Strips ASCII space + tab from every line;
  leaves `\r` so CRLF round-trips. Applies uniformly — including
  inside `|`/`>` block scalars, where YAML semantically pins trailing
  spaces as content. Matches pretty_yaml's behavior (probed before
  implementing); STYLE.md rule 10 amended to note the deliberate
  semantic trade. Six new corpus cases under
  `tests/fixtures/yaml_corpus/`: `whitespace/{trailing_spaces_on_value,
  whitespace_only_blank_line, comment_trailing_spaces, trailing_tab,
  literal_block_trailing}.yaml` plus `document/whitespace_only.yaml`
  (3 ASCII spaces, no newline — resolves the rule-13 era divergence
  for whitespace-only input). Files written via `printf` because
  the Write tool's hook strips per-line trailing whitespace. One new
  unit test in `yaml.rs` covering the four shapes. Workspace test
  suite still green. No live-pipeline changes.
- **Phase 1.4 — rule 13 (trailing document newline).** Added
  `normalize_trailing_newline` to
  `crates/panache-formatter/src/formatter/yaml/document.rs::render`:
  every successfully-parsed document now ends with exactly one `\n`
  (zero → add; many → collapse). Verified the in-tree parser
  preserves trailing newlines byte-for-byte across the
  zero/one/many cases — resolved the
  "lossless parser preservation of trailing newline" open question
  below. Added three corpus cases under
  `tests/fixtures/yaml_corpus/document/`
  (`empty.yaml` (0 bytes), `missing_trailing_newline.yaml`,
  `multiple_trailing_newlines.yaml`) plus three new unit tests in
  `yaml.rs`. Whitespace-only inputs (e.g. `"   "`) are still a
  divergence — pretty_yaml canonicalizes those to `"\n"`; resolves
  once rule 10 (strip per-line trailing whitespace) lands.
  STYLE.md rule 13 footnote updated to note cross-validation;
  yaml.rs status block bumped to 1.4. No live-pipeline changes.
- **Phase 1.3 — cross-validation harness.** Added
  `crates/panache-formatter/tests/yaml_cross_validation.rs`, which
  discovers every `*.yaml` under
  `crates/panache-formatter/tests/fixtures/yaml_corpus/` and, per
  case, asserts (a) `format_yaml(input) == pretty_yaml::format_text(input)`
  with options bridged the same way `yaml_engine.rs` bridges them
  (`print_width` ← `line_width`, `prose_wrap` ← `wrap`, everything
  else at pretty_yaml defaults) and (b) `format_yaml(format_yaml(x)) ==
  format_yaml(x)`. Failures accumulate into one panic so a batch of
  red cases is visible at once. Seeded the corpus with 8 trivially-
  canonical inputs (simple/two-key/nested mappings, top-level + nested
  sequences, leading comment, short flow sequence, doc-start marker)
  that round-trip through pretty_yaml's defaults — chosen so the
  Phase 1.1 byte-passthrough stub passes parity and idempotency
  today. The plan's Phase 1.3 "corpus seeding" intent (real
  frontmatter extracts, hand-picked stressors for flow overflow /
  anchors / multi-line scalars) deferred to land alongside the rule
  implementations that make each case pass — adding them now would
  just enumerate divergences, which is exactly what the
  yaml-formatter rule forbids. yaml.rs module doc-comment updated to
  reflect the 1.3 status. No live-pipeline changes.
- **Phase 1.2 — STYLE.md relocation.** Moved the 13-rule style spec
  out of this plan into
  `crates/panache-formatter/src/formatter/yaml/STYLE.md` (canonical
  home). Added a pointer from `docs/guide/formatting.qmd` in the
  YAML frontmatter section so user-facing docs reach the spec.
  Updated the `crates/panache-formatter/src/formatter/yaml.rs`
  module doc-comment to cite `STYLE.md` instead of the now-relocated
  plan-side spec. No behavior change; the formatter module is still
  the Phase 1.1 byte-passthrough stub. Plan retains rollout context
  and references STYLE.md from the spec section below.
- **Phase 1.1 — module skeleton.** Added
  `crates/panache-formatter/src/formatter/yaml.rs` (parent) and the
  six submodule files (`options.rs`, `document.rs`, `block_map.rs`,
  `block_sequence.rs`, `flow.rs`, `scalar.rs`) under
  `crates/panache-formatter/src/formatter/yaml/`. Public entry
  `format_yaml(text, &YamlFormatOptions) -> String` calls
  `panache_parser::parser::yaml::parse_yaml_tree`, walks the CST, and
  emits tokens verbatim (byte-lossless stub — applies no style rules
  yet). Module wired into `formatter.rs` as `pub mod yaml;` behind an
  `#[allow(dead_code)]` shadow marker; not reachable from the live
  pipeline. Compiles clean; clippy clean; two unit-test smokes pass.
  Plan amended to spell out the no-`mod.rs` layout rule, matching the
  project convention from AGENTS.md.

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

The canonical 13-rule style spec lives in
[`crates/panache-formatter/src/formatter/yaml/STYLE.md`](../../../crates/panache-formatter/src/formatter/yaml/STYLE.md).
That file is the source of truth for what the in-tree formatter
emits; this plan tracks rollout, not the spec itself.

The spec is deterministic (same input → same output) and was
cross-validated against pretty_yaml 0.6.0 and Prettier 3.6.2 on a
15-case battery of representative frontmatter — both agree on rules
1–12; rule 6's bracket placement is the one point where they differ,
and the rule pins pretty_yaml's choice. Rule 13 (trailing document
newline) is not yet cross-validated; that gets done as part of the
Phase 1.3 corpus harness.

Adding a 14th rule is a deliberate act and follows the process
documented in [`yaml-formatter`](../../rules/yaml-formatter.md): a new
rule in STYLE.md with a one-line rationale and a fixture under
`crates/panache-formatter/tests/fixtures/yaml_corpus/`, plus an
explicit decision when it conflicts with pretty_yaml's behavior.

## Phase 1 — Shadow in-tree formatter (plain metadata)

Build `crates/panache-formatter/src/formatter/yaml/` consuming the
in-tree parser CST. Not wired to the live pipeline.

### 1.1 — Module skeleton

Follow the project's modern-Rust layout convention: a parent `yaml.rs`
file declares the submodules; per-feature code sits in sibling files
under `yaml/`. **No `mod.rs`** anywhere in the tree (see AGENTS.md).

- `crates/panache-formatter/src/formatter/yaml.rs` — parent module.
  Public entry: `format_yaml(text: &str, opts: &YamlFormatOptions) -> String`.
  Declares the submodules below.
- `crates/panache-formatter/src/formatter/yaml/` — submodule files:
  - `document.rs` — top-level document orchestration.
  - `block_map.rs`, `block_sequence.rs`, `flow.rs`, `scalar.rs` —
    per-CST-node rendering.
  - `options.rs` — `YamlFormatOptions` (line-width, wrap mode, quote
    style preference, …).
- Wire into the formatter crate by adding `pub mod yaml;` to
  `crates/panache-formatter/src/formatter.rs`.
- Initial entry calls into in-tree parser via
  `panache_parser::parser::yaml::parse_yaml_tree(text)`, walks the
  returned CST, emits text.

### 1.2 — Move style spec into the module

Landed: the 13-rule spec lives in
`crates/panache-formatter/src/formatter/yaml/STYLE.md`, with a
pointer from `docs/guide/formatting.qmd` (YAML frontmatter section).
This plan no longer carries the spec; it tracks rollout only.

If Phase 1 development discovers a 14th rule (an edge case neither
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

## Phase 2 — Cutover

Sequenced, not one commit (the original "joint cutover" framing was
wrong — the pieces have independent blockers).

### 2a — `pretty_yaml` formatter swap — DONE

Both `yaml_engine.rs::format_yaml_with_config` bodies now call
`formatter::yaml::format_yaml`. See "what landed."

### 2b — `yaml_parser` value-extraction migration + dep drop — DONE

Typed AST wrappers (`syntax/yaml_ast.rs`) over the in-tree CST; all five
consumers + the diagnostics infra migrated; `yaml_parser` removed from
the three manifests. **Re-parse-on-demand parity swap — host CST shape
unchanged.** See "what landed."

### 2c — Embed the in-tree YAML CST into the host document CST — IN PROGRESS

End goal: the YAML tokens (`YAML_STREAM` / `YAML_DOCUMENT` /
`YAML_BLOCK_MAP` / … / `YAML_SCALAR`) live **inside the full document
CST**, so frontmatter and hashpipe bodies are real structure, not opaque
text re-parsed on demand. Frontmatter embedding already landed
(`emit_yaml_block` in `blocks/metadata.rs` splices the `parse_stream`
subtree under `YAML_METADATA_CONTENT`). Hashpipe is the remaining target,
and the user directive for it is **"handle the `#|` prefixes as part of
the YAML regions — no offsets"**: the YAML CST's token ranges must be host
ranges directly (prefixes included as trivia), retiring the
`normalize_hashpipe_header` offset-remapping layer entirely.

Staged (each landable independently; the offset layer dies last):

- **Step 1 — `YAML_SCALAR` as a node — DONE** (see "what landed"). A
  scalar is now a `YAML_SCALAR` node wrapping a `YAML_SCALAR_TEXT` leaf;
  flow punctuation/directives got `YAML_FLOW_INDICATOR`/`YAML_DIRECTIVE`.
  This is the wrapper that lets `#|` prefixes (and per-line content) live
  as clean child tokens. Single-leaf for now (no fragmentation).
- **Step 2 — prefix-aware scanner + builder — DONE** (see "what landed").
  Added `line_prefix` to the scanner for prefix-excluded column/indent
  accounting (`auto_detect_block_scalar_indent` + the block-scalar content
  loop skip the marker via `prefix_byte_len_at`; the plain/quoted
  continuation paths use `skip_embedded_line_prefix`); added
  `parse_stream_with_prefix` / `validate_yaml_with_prefix`; `emit_scalar_node`
  fragments multi-line scalars at line breaks (landed as its own commit
  first) and peels a `YAML_LINE_PREFIX` leaf off each continuation line.
  Parity-tested against the `normalize_hashpipe_input` baseline in
  `yaml_prefix_parity.rs`.
- **Step 3 — cook/value over prefixed scalars — DONE.**
  `YamlScalar::value()` cooks a prefix-stripped reassembly of the content
  leaves (skipping `YAML_LINE_PREFIX`); `raw()` stays byte-exact for
  losslessness. Test: `value_skips_embedded_line_prefix` in `yaml_ast.rs`.
- **Step 4 — host embedding — DONE.** `parse_fenced_code_block`
  (`blocks/code_blocks.rs`) now validates (`validate_yaml_with_prefix`)
  then splices a `parse_stream_with_prefix` subtree into
  `HASHPIPE_YAML_CONTENT` (validate→splice→opaque-fallback, mirroring
  `emit_yaml_block`); `compute_hashpipe_preamble_line_count` still does
  region detection; the `HASHPIPE_PREFIX` token + `emit_hashpipe_option_line/
  continuation` are retired (shared `CHUNK_OPTION*` kinds kept for inline
  fence options).
  - **No scanner/builder generalization was needed** (the planned closure
    over a "host per-line recipe"). Within a preamble the container prefix
    is uniform per line, so a *composite* marker — container prefix + `#|`,
    computed by `hashpipe_composite_marker` — matches via length-agnostic
    `strip_prefix` and splices nested (list-/blockquote-indented) cells
    through the *same* path as top-level, peeling the whole prefix into one
    opaque `YAML_LINE_PREFIX` leaf. Verified end-to-end: parser golden
    `quarto_hashpipe_list_item` (4-space list-nested cell) + parity case
    `composite_prefix_matches_stripped_baseline`.
  - **Forced consumer rewire (was assumed Step 5):** `CodeBlock`'s
    `hashpipe_chunk_option_entries` and `ChunkOptionEntry` read `CHUNK_OPTION`
    *structurally*, so the splice broke them. `ChunkOptionEntry` is now
    decoupled from the `CHUNK_OPTION` node (eager key/value/ranges +
    `declaration_range`) and hashpipe options are read from the embedded
    YAML block map. Linter consumers (`figure_crossref_captions`,
    `duplicate_references`, `chunk_label_spaces`) only use the decoupled
    views, so no linter changes were needed.
  - **html-entities exclusion not needed:** the rule only flags `TEXT`
    tokens and already excludes `CODE_BLOCK`/`CODE_CONTENT` ancestors;
    hashpipe YAML is `YAML_SCALAR_TEXT` under `CODE_CONTENT`.
  - **Semantic note for Step 5/downstream:** hashpipe option `value` is now
    the *cooked* scalar (quotes stripped, multi-line folded) and
    `value_range` for a quoted scalar spans the quotes.
- **Step 5 — consumer rewire + drop offset layer — DONE.** Done via a
  **parser syntax-error channel** rather than the planned offset-remap:
  digging in showed the parser already validates the embedded YAML (to pick
  CST shape) and *discarded* the verdict, forcing the linter to re-parse +
  map offsets just to recover it. Now `parse_with_errors` returns the tree
  plus host-ranged `SyntaxError`s (rust-analyzer `Parse { green, errors }`
  style); the two validation sites (`parse_fenced_code_block`,
  `emit_yaml_block`) map the diagnostic to host via a lockstep strip+offset
  pass (`locate_yaml_diagnostic`) and push into an `Rc`-backed `Diagnostics`
  sink on `BlockContext`. salsa caches `(green, errors)` once
  (`parsed_document`) and `built_in_lint_plan` emits `yaml-parse-error`
  straight from the channel. With errors off the channel, `syntax/yaml.rs`
  dropped `yaml_to_host_offsets` / `parse_error_host_offset` / the
  `parse_region_yaml` re-parse — region validity/shape now derive from the
  embedded subtree (`is_valid` = parser spliced; empty-vs-opaque told apart
  by the absence of raw `TEXT`). The formatter reads the embedded
  `HASHPIPE_YAML_CONTENT` (prefix-stripped reconstruction + preamble
  line-count split) and `hashpipe_normalizer.rs` is deleted. A latent parser
  bug surfaced and was fixed: a blank `#|` line inside a literal block scalar
  truncated the preamble scan (`compute_hashpipe_preamble_line_count` now
  keeps it when followed by another prefixed line — `issue_201`). The parser
  stays diagnostics-only-for-validatable-sublanguages; Markdown emits none.

**Consumer audit.** Linter rules, LSP, salsa indexers, pandoc-ast
projector — anything that walks `YAML_METADATA_CONTENT`/
`HASHPIPE_YAML_CONTENT` via `.text()` keeps working (the content node
still exists), but new features (key goto, folding, semantic tokens,
hover) become possible by walking the nested `YAML_*` structure.

### 2d — Drop `pretty_yaml` — PARTIAL (recategorized; full drop deferred)

`pretty_yaml` is runtime-unused since 2a; only
`crates/panache-formatter/tests/yaml_cross_validation.rs` references it,
and it transitively pulls `yaml_parser` into `Cargo.lock`.

**Done (this slice):** recategorized the dependency to match its actual
use — dropped the dead entry from the root `panache` `[dependencies]` and
moved it to `panache-formatter` `[dev-dependencies]`. The cross-validation
parity oracle (and idempotency check) stays live, so no test rebase was
needed. Both published crates (`panache`, `panache-formatter`) now stop
shipping `pretty_yaml`/`yaml_parser` as normal dependencies; only dev/test
builds in this repo pull them. See "what landed."

**Deferred (full drop) — trigger: a few months of stable releases.** The
dev-dependency is explicitly temporary. Once the in-tree YAML formatter has
shipped without YAML-formatting regressions for ~3 months (cutover landed
mid-2026; **revisit ~2026-09**), retire or re-base the cross-validation test
(the in-tree formatter's own corpus + idempotency harness is the durable
bar; capture golden expected outputs in place of the parity oracle), then
remove `pretty_yaml` from the dev-dependencies too. Only after that are both
`yaml_parser` and `pretty_yaml` gone from `Cargo.lock` — the Phase 2 exit
criterion is **knowingly not yet met** by the partial slice. The trigger is
also recorded as a `TEMPORARY` comment on the dev-dependency in
`crates/panache-formatter/Cargo.toml`.

### Exit criteria for Phase 2

- `yaml_parser` and `pretty_yaml` removed from `Cargo.lock` (2d).
- Host document CST carries nested YAML structure for frontmatter and
  hashpipe; no on-demand re-parse of frontmatter content remains (2c).
- All host golden + parser CST snapshots green; deltas annotated.
- `cargo test` workspace green; losslessness holds across the embedding.

## Phase 3 — Hashpipe extension

Same parser + formatter, exercised through the existing hashpipe
normalization path.

### 3.1 — Wire-up — effectively DONE via 2a + 2b

`hashpipe.rs` formats option bodies through
`yaml_engine::format_yaml_with_config` (line ~770), which 2a repointed
to `formatter::yaml::format_yaml` — so hashpipe formatting already runs
on the in-tree formatter; there was no separate hashpipe formatting path
to cut over. Hashpipe *value extraction* migrated to the in-tree
wrappers in 2b. `normalize_hashpipe_input` behaviour is unchanged
(strips `#|`; the formatter re-prefixes). Remaining wire-up work folds
into 2c (nest the hashpipe preamble's YAML structure into the host CST).

### 3.2 — Hashpipe-specific fixtures

Add cases under
`crates/panache-formatter/tests/fixtures/yaml_corpus/hashpipe/` for:
- Continuation lines (`#| key: value\n#|   continued`).
- Blank-line semantics inside `#|`.
- Anchors / tags in chunk options.
- The existing `issue_*_hashpipe_*` host fixtures should drop their
  pretty_yaml-specific quirks at this point — re-check each.

### Exit criteria for Phase 3

- Hashpipe and plain metadata share one formatter path governed by
  the same style spec.
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
- ~~**Lossless parser preservation of trailing newline.**~~
  Resolved in Phase 1.4. The parser round-trips zero/one/many
  trailing newlines byte-for-byte (verified by probe; the formatter
  applies rule 13 on top in `document::render`).
- **Block-scalar interior re-indent.** Rule 1's line-rewrite
  approach treats each block scalar (`|`/`>`) as one multi-line
  `YAML_SCALAR` token and preserves its interior verbatim. That
  keeps parity on already-canonical block scalars but diverges from
  pretty_yaml when the input uses non-canonical indent (e.g. 4-space
  inside a literal block re-flows to 2-space under pretty_yaml). Two
  paths to fix: (a) lift the indent-indicator and content lines into
  separate CST tokens parser-side (cleanest, but a real parser
  change), or (b) keep the token shape and have the formatter
  re-indent the scalar text bytes during rule 1, using the
  block-scalar header to compute the canonical indent. Option (b) is
  smaller and likely the right Phase 1.7+ move. Picked up when the
  formatter starts caring about non-canonical block-scalar inputs
  (no urgent corpus pressure yet).
