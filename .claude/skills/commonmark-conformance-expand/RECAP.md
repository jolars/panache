# CommonMark conformance — running session recap

This file is the rolling, terse handoff between sessions of the
`commonmark-conformance-expand` skill. Read it at the start of a session for
suggested next targets and known follow-ups; rewrite the **Latest session**
entry at the end with what changed and what to look at next.

Keep entries short. The full triage data lives in
`crates/panache-parser/tests/commonmark/report.txt` and
`docs/development/commonmark-report.json`; this file is for the *judgment calls*
a fresh session can't reconstruct from those artifacts (why a target was picked,
what was deliberately skipped, which fix unlocked which group).

--------------------------------------------------------------------------------

## Latest session — 2026-04-28 (l)

**Pass count: 483 → 487 / 652 (74.7%, +4)**

Picked up the prior recap's #1 group: list/HR interaction (#57, #60, #61). Two
of the three (#57, #60) shared a single root cause and fall to a small
dialect-gated change in `parse_inner_content`. The same fix incidentally
unlocks #94 and #99 — both setext-shape inputs (`- Foo\n---\n`,
`- foo\n-----\n`) where the `---`/`-----` line resolves to an HR after the
prior session's setext container-depth gate. #61 needs separate work in
list-item content emission and is deferred — see "Don't redo" below.

### Targets and root causes

- **Thematic breaks #57, #60 + Setext headings #94, #99 (4)** — HR at
  column 0 inside an open list. Dispatcher correctly identifies `***` /
  `---` as a `YesCanInterrupt` HR (or as setext-driving HR), but the
  no-blank-before path in `parse_inner_content` only closed an open
  `Paragraph`, leaving the `ListItem` (and its parent `List`) on the
  stack. The HR then emitted *inside* the list item rather than at the
  outer level. Per CommonMark §5.2, a line whose indent is less than the
  surrounding list-item content column cannot be a continuation — the
  item (and the list, since nothing else continues it) close before the
  new block emits. Fix: a new `close_lists_above_indent(indent_cols)`
  helper closes any open `ListItem`/`List` pair whose `content_col`
  exceeds the line's indent, called once after the existing
  `close-paragraph` step in the `!has_blank_before` `YesCanInterrupt`
  branch, gated on `Dialect::CommonMark` and skipped when the new block
  is itself a list (`BlockEffect::OpenList`) so a same-level marker
  still continues the list. Pandoc is unaffected: under Pandoc the HR
  detector returns None on `***` without a blank before, so the new
  branch never fires.

### Files changed

- **Parser (dialect divergence × 1)**:
  - `crates/panache-parser/src/parser/core.rs` — adds
    `close_lists_above_indent(usize)` near `close_containers_to`, and
    invokes it in the no-blank-before `YesCanInterrupt` branch in
    `parse_inner_content` after the existing list-item-buffer flush
    and paragraph close. Gated on `config.dialect ==
    Dialect::CommonMark` and skipped when `block_match.effect ==
    BlockEffect::OpenList`.
- **Parser fixtures (paired CST snapshots via insta)**:
  - `crates/panache-parser/tests/fixtures/cases/hr_closes_list_{commonmark,pandoc}/`
    pin the divergent CST for `- foo\n***\n- bar\n`. CommonMark: three
    top-level blocks (`LIST > LIST_ITEM(foo)` / `HORIZONTAL_RULE` /
    `LIST > LIST_ITEM(bar)`). Pandoc: a single `LIST` with two items;
    the first item's `PLAIN` interleaves `foo\n` and `***\n` as text
    (existing Pandoc continuation behavior, unchanged).
  - Both registered in
    `crates/panache-parser/tests/golden_parser_cases.rs`.
- **Formatter golden (CommonMark only)**:
  - `tests/fixtures/cases/hr_closes_list_commonmark/` with
    `panache.toml` setting `flavor = "commonmark"`. Pins formatter
    output `- foo\n\n` + 80-dash HR + `\n\n- bar\n` and exercises
    idempotency. Wired into `tests/golden_cases.rs`. **No paired Pandoc
    formatter case** — Pandoc's parse keeps the same single-list shape
    that existing pandoc-default fixtures already exercise.
- **Allowlist additions**: `crates/panache-parser/tests/commonmark/allowlist.txt`
  - Thematic breaks: +#57, +#60
  - Setext headings: +#94, +#99

### Don't redo

- The `close_lists_above_indent` helper closes a `ListItem` followed by its
  containing `List` in one iteration. The pairing matters: if you only close
  the `ListItem`, the HR emits as a sibling node *inside* the parent `LIST`
  rather than as a top-level block, and the next list line just extends the
  same list. Don't unify this with the `close_containers_to` blank-line
  logic — that path uses `levels_to_keep` from `ContinuationPolicy` and
  has its own loose/tight item-buffering rules that this helper deliberately
  bypasses (we know we're emitting an interrupting block right now, not
  weighing whether the list continues across blanks).
- The new branch is gated on `BlockEffect::OpenList` being absent. Without
  that exclusion, a same-level list marker (`- bar` after `- foo` with no
  blank line) would close the list before re-opening it — observable as a
  spurious extra LIST sibling pair in the CST and a regression of the
  bullet-list goldens. Don't drop the gate.
- #61 (`- Foo\n- * * *\n` → `<li><hr/></li>`) is **not** unlocked by this
  fix and is a different code path: the `* * *` content of the second list
  item is buffered as `PLAIN` text via the `ListItemBuffer`/`emit_list_item`
  path. Recognizing it as `HORIZONTAL_RULE` requires teaching list-item
  emission (or buffer flush) to dispatch single-line content through the
  block dispatcher first — and to do so under both dialects, since pandoc
  agrees with CommonMark here (panache's *current* Pandoc output for #61
  is also wrong vs. real Pandoc; see `pandoc -f markdown -t native`).
  Treat #61 as a paired-dialect parser bug, not a CommonMark-only fix.
- The fix only covers the `!has_blank_before` branch. The
  `has_blank_before` branch was checked: the existing blank-line handler's
  `levels_to_keep` already closes the list item in the cases the conformance
  suite exercises (e.g. `- foo\n\n***\n- bar\n` already passed before this
  session). Don't preemptively duplicate the helper into that branch
  without a failing example to anchor it — the levels-to-keep logic is
  load-bearing and the duplicate close would interact with item-loose
  buffering in ways that are easy to get wrong.

### Suggested next targets, ranked

1. **#61 + list-item single-line block dispatch** — the second list item's
   content `* * *` should resolve to `HORIZONTAL_RULE`. Pandoc agrees with
   CommonMark on this, so the fix should be unconditional (or at minimum
   not dialect-gated) and lands as a paired fixture under both dialects.
   Likely involves running the dispatcher over single-line item content in
   `add_list_item` / `emit_list_item_buffer_if_needed`. May unlock more in
   §Lists / §List items.
2. **Lists (5/21) + List items (17/31)** — biggest absolute pass-rate gap
   (52 fails combined). After #61, triage the rest by shape: lazy
   continuation, loose-vs-tight, indented continuation, blank-line-between-
   items. Check the two §Lists/§List items failure patterns in
   `report.txt` for shared root causes before picking sub-targets.
3. **Emphasis and strong emphasis (85/47)** — largest remaining absolute
   failure count; flanking-rule and autolink-precedence edge cases
   (#480, #481 are autolink-vs-emphasis precedence).
4. **Generalize the lazy-continuation interrupter check from session (k)**
   — currently only HR breaks lazy paragraph continuation in blockquotes.
   Per CommonMark §5.1 the full set (ATX heading, fenced code,
   blockquote-start, list-marker-with-content, HR) should also break it.
   Anchor each addition on a failing-spec example to avoid over-interrupting
   Pandoc-flavored docs.
5. **Tabs (5 fails)** — #2, #4, #5, #6, #7 all need tab→space expansion
   with column alignment. Mechanical but its own session.
6. **Link reference definitions (12 fails)** — bigger refactor; currently
   captures whole line(s) as raw TEXT.
7. **Setext heading multi-line content (#81, #82, #95)** + **#115**.
8. **Pandoc setext-in-blockquote losslessness** — pre-existing parser bug
   surfaced in session (k) while writing fixtures. Not a spec unlock, but
   worth fixing for correctness; mirrors the `parse_html_block` fix from
   session (j).
9. **#342 (code-span/link precedence)**, **#148 (table-pre nesting)** —
   one-offs.

--------------------------------------------------------------------------------

## Earlier session — 2026-04-28 (k)

**Pass count: 479 → 483 / 652 (74.1%, +4)** — Block quotes #234, #246 +
Setext headings #92, #101.

Targeted HR-interrupts-paragraph in containers. Two dialect-gated parser
changes (both in CommonMark only):

- `SetextHeadingParser::detect_prepared` rejects a setext underline whose
  raw blockquote depth (`count_blockquote_markers(next_line).0`) differs
  from the active `ctx.blockquote_depth`. CommonMark §4.3 requires
  underline + text in the same container.
- `core.rs` lazy-paragraph continuation in the close-blockquote branch
  (around the `bq_depth < current_bq_depth && bq_depth == 0` check) checks
  `try_parse_horizontal_rule(line)` first; on match it skips
  `append_paragraph_line` so the HR dispatches at the outer level after
  the blockquote closes.

Paired parser fixtures:
- `setext_underline_crosses_blockquote_commonmark/` (CommonMark only —
  Pandoc has a pre-existing setext-in-blockquote losslessness bug; see
  follow-up #8 above).
- `hr_interrupts_lazy_blockquote_paragraph_{commonmark,pandoc}/`.

Don't-redo from (k) still relevant:
- The setext gate uses *raw* depth from the next line bytes, not
  `ctx.content`'s already-stripped depth. The dispatcher has consumed the
  current line's markers by the time `detect_prepared` runs, so comparing
  to anything other than the next-line raw depth is a footgun.
- The HR-interrupts-lazy gate currently fires *only* on HR. Per
  CommonMark §5.1 the full set of paragraph-interrupting blocks should
  also break lazy continuation, but generalize only with a failing-spec
  example anchoring each addition (see follow-up #4 above).
