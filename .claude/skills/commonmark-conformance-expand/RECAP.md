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

## Latest session — 2026-04-28 (n)

**Pass count: 488 → 508 / 652 (77.9%, +20)**

Targeted prior recap's #1 (Lists / List items). Two coupled renderer-only
root causes unlocked the bulk:

1. **Loose-list detection missed BLANK_LINE *inside* a LIST_ITEM**.
   Per CommonMark §5.3, an item with a blank line between two block-level
   children makes the *whole list* loose. The previous `is_loose_list`
   only checked PARAGRAPH descendants and BLANK_LINE *between* items.
   Added a `list_item_has_internal_blank` helper that returns true if any
   LIST_ITEM has a BLANK_LINE between two block-level children
   (PLAIN/PARAGRAPH/HEADING/CODE_BLOCK/BLOCK_QUOTE/LIST/HORIZONTAL_RULE/
   HTML_BLOCK).
2. **PLAIN/PARAGRAPH inside LIST_ITEM weren't stripping line indent**.
   The parser preserves source bytes (continuation lines keep their
   list-content-column indent as TEXT), but `render_list_item` called
   `render_inlines` directly. Routed PARAGRAPH children through
   `render_paragraph` (which already does `strip_paragraph_line_indent`),
   and applied the same strip to PLAIN children — preserving the trailing
   newline so tight items with nested lists still render
   `<li>foo\n<ul>` (not `<li>foo<ul>`).

These two fixes are coupled: without (1) the loose flag never flips, so
PARAGRAPHs inside loose items don't get rendered as `<p>...</p>`. Without
(2) the indent leaks into the `<p>` content. Together they fix the §List
items / §Lists / one §Tabs example simultaneously.

### Targets and root causes

- **Single root-cause group (renderer × 2)**: 19 unlocks across §List items
  (#256, #258, #259, #262, #263, #277, #279, #291), §Lists (#306, #307,
  #309, #311, #313, #314, #316, #319, #325), §Tabs (#4 — pre-existing
  blank-line-in-item shape that flipped from "wrong loose detection" to
  "right loose detection" + indent strip).

### Files changed

- **Renderer (renderer gap × 1)**:
  - `crates/panache-parser/tests/commonmark/html_renderer.rs`:
    - `is_loose_list` now also returns true when any LIST_ITEM has a
      BLANK_LINE between two block-level children. New helpers
      `list_item_has_internal_blank` and `is_block_child`.
    - `render_list_item` PLAIN-in-loose path now calls `render_paragraph`
      (which strips line indent and emits `<p>…</p>`).
    - `render_list_item` PLAIN-in-tight path now applies
      `strip_paragraph_line_indent` to the rendered inlines (preserving
      the trailing newline so nested-list cases still match the spec's
      `<li>foo\n<ul>` shape).
    - `render_list_item` PARAGRAPH path now calls `render_paragraph`
      directly (replacing the inline-only emission).
- **Parser fixture (renderer leans on this CST shape)**:
  - `crates/panache-parser/tests/fixtures/cases/list_item_blank_line_inside/`
    — pins the CST for `- one\n\n  two\n` (LIST_ITEM contains
    PLAIN("one") + BLANK_LINE + PLAIN("  two")). Single dialect (default,
    no `parser-options.toml`) because Pandoc and CommonMark agree on this
    shape and existing Pandoc fixtures already exercise loose lists.
    Wired into `golden_parser_cases.rs`.
- **Allowlist additions**:
  - Tabs: +#4
  - List items: +#256, +#258, +#259, +#262, +#263, +#277, +#279, +#291
  - Lists: +#306, +#307, +#309, +#311, +#313, +#314, +#316, +#319, +#325

### Don't redo

- The PLAIN-in-tight strip preserves the trailing newline. The first
  attempt used `inner.trim_end_matches('\n')` which dropped the newline
  *before* the `<li>` normalization step, breaking #9 / #294 / #323
  (e.g. `- a\n  - b\n` rendered as `<li>a<ul>` instead of `<li>a\n<ul>`).
  Lesson: `<li>foo\n…` shape is load-bearing for the
  `<li>\n` / `\n</li>` collapse normalization — don't trim the trailing
  newline before output.
- The PARAGRAPH path now calls `render_paragraph`, which emits its own
  `<p>…</p>\n`. Don't wrap the call in `<p>…</p>` again or you'll
  double-tag.
- `list_item_has_internal_blank` only fires when both neighbors are
  block-level (per `is_block_child`). Don't expand to include
  WHITESPACE/NEWLINE tokens — a blank-line *inside* a paragraph (which
  the parser models as a single PLAIN with an interior NEWLINE) would
  spuriously flip the list to loose.

### Suggested next targets, ranked

1. **Remaining Lists / List items (35 fails combined: 12 + 23)** — the
   loose-detection + indent-strip fix didn't unlock everything. Big
   remaining buckets visible in `report.txt`:
   - Code blocks inside list items (#254, #263, #264, #270, #271, etc.)
     — the renderer needs to strip the list-item *content column* (not
     just per-line whitespace) from CODE_BLOCK content. Different from
     the paragraph indent strip — the parser preserves N spaces of
     content-column indent on each line and the renderer must subtract
     N. Look at how `code_block_content` already handles fenced opener
     indent; this is similar but for indented-code-blocks inside list
     items.
   - Lazy / nested marker continuation (#296, #297, #305) — `10) foo\n
     - bar` should produce `<ol><li>foo<ul><li>bar</li></ul></li></ol>`
     but currently parses as a paragraph. Likely a parser issue where
     ordered-list-with-paren-marker doesn't accept a nested bullet.
2. **Emphasis and strong emphasis (85/47)** — flanking-rule and
   autolink-precedence edge cases (#480, #481 are autolink-vs-emphasis
   precedence).
3. **Generalize the lazy-continuation interrupter check from session
   (k)** per CommonMark §5.1.
4. **Tabs (4 fails)** — #2, #5, #6, #7 all need tab→space expansion
   with column alignment.
5. **Link reference definitions (12 fails)** — bigger refactor.
6. **Setext heading multi-line content** (#81, #82, #95) + **#115**.
7. **Pandoc setext-in-blockquote losslessness** (parser bug from k).
8. **#342 (code-span/link precedence)**, **#148 (table-pre nesting)**.

--------------------------------------------------------------------------------

## Earlier session — 2026-04-28 (m)

**Pass count: 487 → 488 / 652 (74.8%, +1)**

Picked up the prior recap's #1 target: #61 (Thematic breaks — HR as the sole
content of a list item, e.g. `- Foo\n- * * *\n`). Single root cause across
two layers: parser was buffering `* * *` as PLAIN text in `ListItemBuffer`,
and the formatter had no path to render an HR child of LIST_ITEM. Pandoc
agrees with CommonMark on this construct, so the fix is dialect-agnostic.

### Targets and root causes

- **Thematic breaks #61 (1)** — `- * * *` should produce `<li><hr/></li>`.
  Two unrelated layers needed touching:
  1. **Parser-shape gap**: `ListItemBuffer::emit_as_block` already detects a
     single-line buffer that's an ATX heading and emits HEADING. Extended it
     to detect `try_parse_horizontal_rule` and emit HORIZONTAL_RULE the same
     way. Unconditional (no dialect gate) — pandoc and commonmark both want
     this shape.
  2. **Formatter gap**: with the parser fix, the LIST_ITEM now has a
     HORIZONTAL_RULE child but no PLAIN/PARAGRAPH content_node. The list
     wrapping pass emits nothing (no `lines`), so the marker was never
     written; the children loop's `_` arm formatted the HR as a top-level
     80-dash rule via `format_node_sync`, which broke out of the list and
     ruined idempotency. Added a HORIZONTAL_RULE arm in
     `format_list_item`'s children loop that, when `lines.is_empty()` &
     `content_node.is_none()` & HR is the first real child, emits
     `<marker><space><source HR bytes><\n>` (e.g. `- * * *`). Other paths
     (HR after a paragraph, etc.) still flow through `format_node_sync`.

### Files changed

- **Parser (parser-shape gap × 1)**:
  - `crates/panache-parser/src/parser/utils/list_item_buffer.rs` — adds an
    HR detection branch alongside the existing ATX-heading branch in
    `emit_as_block`; emits HORIZONTAL_RULE for single-line buffers whose
    content matches `try_parse_horizontal_rule`.
- **Formatter**:
  - `crates/panache-formatter/src/formatter/lists.rs` — adds
    `SyntaxKind::HORIZONTAL_RULE` arm in the `format_list_item` children
    loop. When no content was emitted (no PLAIN/PARAGRAPH content_node, no
    preserve/sentence/empty-nested cases) and HR is the first non-trivial
    child, inlines marker + source HR text. Falls back to
    `format_node_sync` otherwise.
- **Parser fixtures (paired CST snapshots via insta)**:
  - `crates/panache-parser/tests/fixtures/cases/hr_as_list_item_content_{commonmark,pandoc}/`
    pin the same CST shape for `- Foo\n- * * *\n`: second LIST_ITEM contains
    HORIZONTAL_RULE (not PLAIN). Both registered in `golden_parser_cases.rs`.
- **Formatter golden (Pandoc default — no flavor specified)**:
  - `tests/fixtures/cases/hr_as_list_item_content/` — input `- Foo\n- * * *\n`
    formats unchanged and is idempotent. No `panache.toml` because the
    behavior is dialect-agnostic. Wired into `tests/golden_cases.rs`. **No
    paired CommonMark formatter case** — both dialects produce the same
    output.
- **Allowlist additions**: Thematic breaks: +#61.

### Don't redo

- The formatter inlines the HR with **source bytes** (`* * *`, `***`, `___`,
  …) rather than the canonical 80-dash form. This is intentional: with the
  canonical 80-dash HR, `- ----` re-parses as a top-level
  `HORIZONTAL_RULE@0..6 "- ----"` (because runs of `-` with spaces are
  themselves HR text), which would break round-trip when the marker is `-`.
  Source bytes preserve user intent and avoid the conflict for any marker.
  Don't switch to canonical form without solving the marker/HR-char
  collision.
- The HR-inline path is gated on `lines.is_empty()` AND
  `content_node.is_none()` AND HR being the first non-paragraph block child.
  Without this gate, items like `- Foo\n\n  ***` (paragraph + trailing HR)
  would double-emit the marker. The current spec failure list does not
  exercise that multi-block shape; defer until a failing example anchors it.
- The parser fix lives in `ListItemBuffer::emit_as_block` next to the
  existing ATX-heading detection — single-line, no inner newlines. Don't
  extend to multi-line buffers; the buffer is concatenated text and parsing
  inner-line transitions (paragraph → HR) requires a different pass outside
  this helper's scope.
- `heading_with_remainder` is a `(String, String)` tuple; the body of
  `format_list_item` partially moves it before the children loop. The new
  no-content gate uses `content_node.is_none()` instead, which is implied by
  `heading_with_remainder.is_none()` (heading_with_remainder requires a
  content_node) and avoids the post-move borrow error.

--------------------------------------------------------------------------------

## Earlier session — 2026-04-28 (l)

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
