# CommonMark conformance — running session recap

This file is the rolling, terse handoff between sessions of the
`commonmark-conformance-expand` skill. Read it at the start of a session for
suggested next targets and known follow-ups; rewrite the **Latest session**
entry at the end with what changed and what to look at next. Remove and replace
the "Latest session" entry with a new one at the end of each session, but 
check if there is something from the prior session that should be
carried forward.

Keep entries short. The full triage data lives in
`crates/panache-parser/tests/commonmark/report.txt` and
`docs/development/commonmark-report.json`; this file is for the *judgment calls*
a fresh session can't reconstruct from those artifacts (why a target was picked,
what was deliberately skipped, which fix unlocked which group).

--------------------------------------------------------------------------------

## Latest session — 2026-04-29 (xxv)

**Pass count: 634 → 635 / 652 (97.4%, +1)**

Took the carried-over target #278 (empty-marker list item
followed by indented content). Two related parser fixes,
both apply across dialects (verified via `pandoc -f commonmark`
and `pandoc -f markdown`).

### Targets unlocked

- **#278** `-\n  foo\n-\n  \`\`\`...\n-\n      baz\n` — three
  list items: a paragraph item, a fenced-code item, and an
  indented-code item.

### Root cause + fix

Two parser-shape gaps:

1. **Setext eagerly absorbed the next list-item's marker.**
   For `-\n  foo\n-\n…`, when the dispatcher processed the `  foo`
   line it looked ahead at the bare `-` on line 3 and matched
   `try_parse_setext_heading(["foo", "-"])`, emitting an h2 with
   "foo" — collapsing item 1 + item 2 into one item.

2. **Indented code didn't fire on the line *after* an empty
   marker.** For the third item `-\n      baz\n`, the dispatcher
   gates indented code on `has_blank_before || at_document_start`
   (CommonMark) or `has_blank_before_strict` (Pandoc). Both are
   false when the prior line is the empty marker line, so the
   line was buffered as PLAIN text.

**Fixes in
`crates/panache-parser/src/parser/block_dispatcher.rs`:**

- `SetextHeadingParser::detect_prepared`: after the existing
  blockquote same-container check, also reject when
  `ctx.list_indent_info` is `Some(list_info)` and
  `leading_indent(next_line).0 < list_info.content_col` — the
  underline is at shallower indent than the list item's content
  column, so it would close the list item rather than continue
  it. No dialect gate (both dialects agree).
- New `BlockContext::in_marker_only_list_item` flag (mirrors
  `Container::ListItem.marker_only`) plumbed through every
  `BlockContext` constructor in `core.rs` and the unit-test
  contexts in `blocks/tests/blockquotes.rs`.
- `IndentedCodeBlockParser::detect_prepared`: when
  `ctx.in_marker_only_list_item` is true, allow regardless of
  blank-before, AND return `BlockDetectionResult::YesCanInterrupt`
  so the parser core's YesCanInterrupt path runs
  `emit_list_item_buffer_if_needed` *before* `parse_prepared`.
  Without the YesCanInterrupt swap, the buffered post-marker
  newline (`\n` from `-\n`) flushes *after* the CODE_BLOCK is
  emitted, breaking byte-order losslessness (verified directly).

### Files changed

- **Parser-shape gap** (both dialects):
  - `crates/panache-parser/src/parser/block_dispatcher.rs`:
    new `BlockContext.in_marker_only_list_item` field; setext
    list-item-content-col guard; indented-code marker-only allow
    branch returning YesCanInterrupt.
  - `crates/panache-parser/src/parser/core.rs`: set
    `in_marker_only_list_item` from
    `containers.last() == Some(ListItem { marker_only: true, .. })`
    in all three `BlockContext` construction sites.
  - `crates/panache-parser/src/parser/blocks/tests/blockquotes.rs`:
    `in_marker_only_list_item: false` added to seven test
    `BlockContext` literals.
- **Parser fixtures** (both Pandoc-default since CSTs match):
  - `list_item_empty_marker_indented_code_next_line/input.md` —
    pins `-\n      baz\n` → LIST_ITEM with
    PLAIN(NEWLINE) + CODE_BLOCK, exercising the indented-code
    fix. No `parser-options.toml` (Pandoc default).
  - `list_item_empty_marker_setext_blocked_commonmark/{input.md,
    parser-options.toml = "commonmark"}` — pins
    `-\n  foo\n-\n` → two LIST_ITEMs (no setext absorbing the
    second marker). Pandoc-default already produced this shape
    via `blank_before_header`, so a paired Pandoc fixture is
    redundant; existing top-level fixture suite covers it.
  - Both wired in `golden_parser_cases.rs`. Snapshots accepted
    via `INSTA_UPDATE=always`.
- **Allowlist additions**: #278 (List items).

No formatter golden case: the new CST under CommonMark is
structurally identical to the Pandoc path (existing top-level
fixture coverage applies). `cargo run -- debug format --checks
all` on `-\n      baz\n` passes (losslessness + idempotency).

### Don't redo

- **Don't drop the `BlockDetectionResult::YesCanInterrupt`
  return for the marker-only indented-code case.** Returning
  plain `Yes` makes the parser core skip
  `emit_list_item_buffer_if_needed`, leaving the post-marker
  newline buffered until end-of-item — it then flushes *after*
  the CODE_BLOCK and breaks byte-order losslessness. Verified
  directly: with plain `Yes` the CST text is `-      baz\n\n`
  vs source `-\n      baz\n`.
- **Don't thread setext-in-buffered-list-item via the existing
  `SetextHeadingParser`.** The dispatcher's setext detection
  fires on the *text* line with next_line as underline. For
  #300's case (`- Bar\n  ---\n  baz`), "Bar" is buffered on
  line 2 (no dispatch) and `  ---` arrives as the *current*
  line on line 3 — by then setext can't see "Bar". A separate
  fold pass over the list-item buffer is needed. Tracked as
  next session.
- **Don't simplify the setext list-item indent check by reusing
  `try_parse_list_marker`.** A line of `-` at column 0 is a
  list marker only when it would actually open/continue a list
  at that column; the simpler "is the underline at a shallower
  indent than the current list item's content_col?" check
  captures the precedence rule directly without re-invoking
  the marker parser.
- **Don't generalize `in_marker_only_list_item` to
  `BlockContext` users beyond IndentedCode without re-checking
  what each parser does on a marker-only first line.** ATX,
  fenced code, HRs already handle this through other paths
  (post-marker text on the marker line). The flag is narrowly
  needed for "block on the line *after* an empty marker."
- **Don't lift the `dialect_allows_nested` gate** on the
  blockquote-in-list-item branch without also fixing the
  formatter's nested-only LIST_ITEM round-trip. (Carried from
  session xxiv.)

### Suggested next targets, ranked

1. **#300 setext-in-list-item** — `- # Foo\n- Bar\n  ---\n  baz`.
   Item 2's "Bar" buffers as text; `  ---` arrives on the next
   line and the dispatcher fires HR. The `\n` after "Bar" is
   already in the buffer when `  ---` reaches the dispatcher,
   so a fold-on-flush approach in
   `list_item_buffer.rs::emit_as_block` (or a new pre-dispatcher
   hook in `core.rs`'s YesCanInterrupt path that intercepts HR
   when the buffered text could be setext-content) is the next
   move. Likely both-dialect (pandoc agrees on the shape).
2. **Formatter fix for nested-only outer LIST_ITEM** — still
   the #1 unblocker for lifting the same-line nested-LIST and
   blockquote-in-list-item dialect gates. The formatter today
   drops the LIST_MARKER when LIST_ITEM's first structural
   child is a BLOCK_QUOTE or LIST. Likely lives in
   `crates/panache-formatter/src/formatter/lists.rs`'s LIST_ITEM
   emission path. (Carried.)
3. **Proper delimiter-stack for emphasis (#402, #408, #412, #417,
   #426, #445, #457, #464, #465, #466, #468)** — rewrite emphasis
   to use CMark's process_emphasis algorithm. Largest single fix;
   substantial; gate on `Dialect::CommonMark`. (Carried.)
4. **#472 `*foo *bar baz*`** — CommonMark expects `*foo <em>bar
   baz</em>`. Likely needs delimiter-stack work. (Carried.)
5. **Reference-link nesting (#569, #571)** — `[foo][bar][baz]`
   with only `[baz]` defined should parse as `[foo]` + ref-link
   `[bar][baz]`. CMark left-bracket scanner stack with
   refdef-aware resolution. (Carried; #533 stays in this group.)
6. **#533** — inline link with emphasis closing inside the
   bracket text plus a trailing reference link. (Carried;
   companion to #569/#571.)
7. **#523 `*foo [bar* baz]`** — emphasis closes inside link
   bracket text mid-flight. Likely needs delimiter-stack work +
   bracket scanner integration. (Carried.)

### Carry-forward from prior sessions

- The "Don't redo" notes from session (ix) about emphasis split-runs
  ("tail-end only" heuristic, dialect gate, not widening to
  #402/#408/#412/#426) still apply. Don't touch those paths without
  first reading session (ix)'s recap.
- The session (x) / (xi) link-scanner skip pattern (autolink/raw-HTML
  opacity for emphasis closer + link bracket close) is load-bearing
  for #524/#526/#536/#538. Don't unify the autolink and raw-HTML
  skip flags — Pandoc treats them differently.
- Session (xii)'s lazy paragraph continuation across reduced
  blockquote depth (`bq_depth < current_bq_depth` branch) is the
  same site session (xx) extended for fence interrupts. Don't
  conflate it with session (xiii)'s list-marker lazy continuation
  and the bq_depth=0 list-continuation gate — the three paths
  share the "lazy continuation" name but operate on different
  state. Don't try to unify them.
- Session (xiii)'s `try_lazy_list_continuation` only fires for
  `BlockEffect::OpenList` with `indent_cols ≥ 4`. Other
  interrupting-block effects (HR, ATX, fence) at deep indent
  inside lists go through the existing CommonMark §5.2 close path
  at `core.rs:2447+` (`close_lists_above_indent`); don't touch
  that path on its account.
- Session (xvii)'s HTML block #148 fix (`</pre>` rejection in the
  VERBATIM_TAGS branch) remains active under CommonMark and is
  Pandoc-unreachable via `extract_block_tag_name(_, false)`.
- Session (xviii)'s `disallow_inner_links` flag and
  `link_text_contains_inner_link` helper are scoped to inline
  links only. Reference-link nesting (#569/#571 + #533) needs a
  different pass with refdef resolution; do not retrofit the
  helper.
- Session (xix)'s column-aware indented-code logic
  (`is_indented_code_line` via `leading_indent`,
  `consume_leading_cols` in the renderer) was extended in (xxi)
  to handle blockquote tab-expansion via
  `consume_leading_cols_from(body, start_col, target)`. Session
  (xxii) closed the parser-side gap: list-item markers now
  apply CommonMark §5.2 rule #2 via `marker_spaces_after`, with
  a parallel `virtual_marker_space` flag mirroring the bq
  `virtual_absorbed` bookkeeping. The bq and list-item virtual
  spaces are *separate* concepts — don't try to unify their
  fields. They contribute additively in the renderer's
  `code_block_content` when both are present (rare; not yet
  exercised by a passing example).
- Session (xxiv)'s same-line blockquote-in-list-item branch in
  `finish_list_item_with_optional_nested` is dialect-gated to
  CommonMark via `dialect_allows_nested`. Don't lift the gate
  without first fixing the formatter's nested-only LIST_ITEM
  round-trip (the formatter currently drops the LIST_MARKER on
  re-format when LIST_ITEM's first structural child is a
  BLOCK_QUOTE).
