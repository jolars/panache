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

## Latest session — 2026-04-29 (xxiv)

**Pass count: 632 → 634 / 652 (97.2%, +2)**

Took the carried-over target #292/#293 (same-line blockquote
inside list item). Both dialects agree on the shape (verified
via `pandoc -f commonmark` and `pandoc -f markdown`), but the
fix is **dialect-gated to CommonMark** because the formatter
drops the LIST_MARKER on round-trip when LIST_ITEM's first
structural child is a BLOCK_QUOTE — exactly the same formatter
constraint that blocks the same-line nested-LIST recursion at
the same site. Same gate (`dialect_allows_nested`).

### Targets unlocked

- **#292** `> 1. > Blockquote\ncontinued here.` — outer
  blockquote, list item, inner blockquote, paragraph with lazy
  continuation across both blockquote layers.
- **#293** `> 1. > Blockquote\n> continued here.` — same shape
  but with explicit `>` continuation marker on line 2.

### Root cause + fix

Parser-shape gap. `finish_list_item_with_optional_nested`
buffered `> Blockquote\n` as PLAIN text instead of recognizing
the inner `>` as a structural blockquote opener.

**Fix in `crates/panache-parser/src/parser/blocks/lists.rs`**
(after the existing same-line nested-LIST detection): when
`text_to_buffer.starts_with('>')` (and isn't `>>`, isn't a
thematic break), emit `BLOCK_QUOTE_MARKER` + optional
`WHITESPACE`, push `Container::ListItem` (empty buffer,
`marker_only: false`) and `Container::BlockQuote {}`, then if
there's content after `> ` start a paragraph and call
`paragraphs::append_paragraph_line` so subsequent lines flow in
via the parser's existing lazy-continuation path. Reused
because the standard blockquote-opening path in `core.rs`
already does the same dance (start BLOCK_QUOTE node → push
container → start_paragraph_if_needed → append).

The gate matches the same-line nested-LIST gate
(`dialect_allows_nested = config.dialect == Dialect::CommonMark`)
so Pandoc-default formatting is unaffected. Under Pandoc the
same input still flows through the PLAIN path (CST verified
unchanged via paired fixture).

### Files changed

- **Parser-shape gap** (CommonMark dialect only):
  - `crates/panache-parser/src/parser/blocks/lists.rs`:
    `finish_list_item_with_optional_nested` gains a
    `dialect_allows_nested && text_to_buffer.starts_with('>')`
    branch that opens an inline BLOCK_QUOTE inside the LIST_ITEM
    and buffers the post-marker content into a paragraph.
- **Parser fixtures** (paired, since the dialect gate makes
  CommonMark and Pandoc CSTs differ):
  - `list_item_same_line_blockquote_marker_commonmark/{input.md,
    parser-options.toml}` — pins the new LIST_ITEM > BLOCK_QUOTE
    > PARAGRAPH shape under `flavor = "commonmark"`.
  - `list_item_same_line_blockquote_marker_pandoc/{input.md,
    parser-options.toml}` — pins the gated-off (PLAIN-text)
    shape under `flavor = "pandoc"` so the dialect divergence
    is regression-tracked.
  - Both wired in `golden_parser_cases.rs`. Snapshots accepted
    via `INSTA_UPDATE=always`.
- **Allowlist additions**: #292, #293 (List items).
- **Lib test update**:
  `parser::blocks::tests::blockquotes::definition_list_list_blockquote_continuation_stays_structural`
  uses `parse_blocks` (Pandoc-default), so under the dialect
  gate the test's pre-fix behavior is preserved. No assertion
  change needed — restored to `marker_count == 2`. (Earlier
  push of this test to `marker_count == 3` was reverted when the
  dialect gate landed.)

No formatter golden case under CommonMark flavor: the formatter
currently drops the LIST_MARKER on round-trip for this CST shape
(verified — `1. > foo` re-formats to `   > foo`, losing the list
entirely). Adding a CommonMark formatter case would fail the
idempotency assertion. Tracked as carry-forward (see Suggested
next targets).

### Don't redo

- **Don't lift the `dialect_allows_nested` gate** on the
  blockquote-in-list-item branch without also fixing the
  formatter's nested-only LIST_ITEM round-trip. The gate is
  load-bearing: under Pandoc-default, the formatter would
  silently drop the `1.` LIST_MARKER on re-format because
  LIST_ITEM's first structural child is a BLOCK_QUOTE.
  Verified by reproducing on `1. > Blockquote\n` directly.
- **Don't try to add a CommonMark formatter golden case yet.**
  The output today is `>    > > Blockquote continued here.`
  which round-trips to nested blockquotes only (LIST stripped).
  The case lands together with the formatter fix.
- **Don't merge this branch with the same-line nested-LIST
  branch above it** — the inner-content classification is
  mutually exclusive (a list marker isn't a blockquote marker
  and vice versa). Sharing `dialect_allows_nested` is enough;
  the bodies do different work (recurse into emit_list_item vs
  open BLOCK_QUOTE + paragraph) and conflating them obscures
  intent.
- **Don't simplify `text_to_buffer.starts_with('>')` to use
  `try_parse_blockquote_marker`.** That helper accepts up to 3
  leading spaces; for our same-line path we want strict
  byte-0 `>` because leading whitespace was already consumed by
  `emit_list_item` into the post-marker WHITESPACE token, and
  re-detecting it here would double-count.
- **The `>>` short-circuit (`!text_to_buffer.starts_with(">>")`)**
  is intentional. Letting `> > x` through this branch would
  open one inner blockquote and leave the second `>` as text,
  which is wrong; nested blockquotes inside list items are not
  in scope for this session and would need recursion analogous
  to the nested-LIST branch. Leave the short-circuit; revisit
  if the next block of List-items failures includes one of
  these forms.

### Suggested next targets, ranked

1. **Formatter fix for nested-only outer LIST_ITEM** — now the
   #1 unblocker. Lifting it removes both the same-line
   nested-LIST gate (unlocking #298, #299) AND the same-line
   blockquote-in-list-item gate (would let Pandoc match
   pandoc-markdown's actual shape). The formatter today drops
   the LIST_MARKER when LIST_ITEM's first structural child is
   a BLOCK_QUOTE or LIST. Likely lives in
   `crates/panache-formatter/src/formatter/lists.rs`'s LIST_ITEM
   emission path; add a CommonMark formatter golden case
   alongside the fix.
2. **Proper delimiter-stack for emphasis (#402, #408, #412, #417,
   #426, #445, #457, #464, #465, #466, #468)** — rewrite emphasis
   to use CMark's process_emphasis algorithm. Largest single fix;
   substantial; gate on `Dialect::CommonMark`. (Carried.)
3. **#472 `*foo *bar baz*`** — CommonMark expects `*foo <em>bar
   baz</em>`. Likely needs delimiter-stack work. (Carried.)
4. **Reference-link nesting (#569, #571)** — `[foo][bar][baz]`
   with only `[baz]` defined should parse as `[foo]` + ref-link
   `[bar][baz]`. CMark left-bracket scanner stack with
   refdef-aware resolution. Probably the next big link cluster.
   (Carried; #533 stays in this group.)
5. **#533** — inline link with emphasis closing inside the
   bracket text plus a trailing reference link. (Carried;
   companion to #569/#571.)
6. **#278 `-\n  foo\n-\n  ```\n…`** — empty marker followed by
   indented content; multiple bugs. (Carried over.)
7. **#300 setext-in-list-item** — `- # Foo\n- Bar\n  ---\n  baz`
    should treat `Bar\n  ---` as setext h2. (Carried over.)
8. **#523 `*foo [bar* baz]`** — emphasis closes inside link
    bracket text mid-flight. Likely needs delimiter-stack work +
    bracket scanner integration. (Carried over.)

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
