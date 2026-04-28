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
