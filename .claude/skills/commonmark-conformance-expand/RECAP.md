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

## Latest session — 2026-04-28 (p)

**Pass count: 516 → 518 / 652 (79.4%, +2)**

Targeted prior recap's #1 (Fenced code blocks inside list items: #318,
#321, #324). One coupled root cause unlocked #318 and #324; #321 still
fails for a different reason (fence on continuation inside a blockquote
inside a list item — separate bucket).

**Two coupled gaps for fence-on-first-line of a list item:**

1. **Parser-shape gap.** When a list item opens with `- \`\`\`...` the
   post-marker text was buffered into `ListItemBuffer` and later emitted
   as PLAIN, where the inline parser matched the backticks as
   INLINE_CODE. The block dispatcher's fence detection never ran on
   that first-line content because it sits *inside* the list-item
   container handler (`handle_list_open_effect`), which always buffers.
   Fix: after `add_list_item` (and friends), peek at the buffer's first
   text segment. If it parses as a fence opener and the dispatcher's
   gating clears (has info string, OR has a matching closer below at
   the list-item content column, OR CommonMark dialect; plus the
   `backtick_code_blocks`/`fenced_code_blocks` extension flags), clear
   the buffer and call `parse_fenced_code_block` with
   `first_line_override = Some(buffered_text)` and
   `base_indent = content_col`. Compensate `self.pos` by `new_pos - 1`
   so the dispatcher's outer `pos += lines_consumed` (=1) lands on
   `new_pos`.
2. **Renderer gap.** `parse_fenced_code_block` was called with
   `base_indent = content_col`, which preserves the list-item
   indentation inside CODE_CONTENT for losslessness. The renderer's
   fenced branch only stripped `fenced_opener_indent` (the WS *before*
   `CODE_FENCE_OPEN` inside the CODE_BLOCK), which is 0 here. Fix: in
   the fenced branch, run `strip_leading_spaces_per_line(raw, li_indent)`
   *before* applying the opener-indent rule. Mirrors what the indented
   path already does. Reuses the existing
   `enclosing_list_item_content_column` helper from the previous
   session.

### Targets and root causes

- **Coupled parser+renderer (parser-shape × 1, renderer × 1)**: 2
  unlocks (#318, #324) — both opened a fenced code block on the same
  line as the list marker. After the parser fix, the block produced
  was structurally correct but the rendered HTML still leaked the
  list-item indent; the renderer fix closed that.
- **Not yet fixed**: #321 (fence on continuation line inside `> b`
  blockquote inside list item) — different shape, different root cause.
  See "Suggested next targets" below.

### Files changed

- **Parser (parser-shape gap × 1)**:
  - `crates/panache-parser/src/parser/utils/list_item_buffer.rs`:
    new `first_text(&self) -> Option<&str>` accessor.
  - `crates/panache-parser/src/parser/core.rs`:
    new `Parser::maybe_open_fenced_code_in_new_list_item` helper plus
    `has_matching_fence_closer` (mirrors the dispatcher's gating).
    Helper is called after every `add_list_item` /
    `add_list_item_with_nested_empty_list` / `start_nested_list` site
    inside `handle_list_open_effect` (4 sites). Other list-item
    creation paths in the parser (lazy continuation in/out of
    blockquote, document-start branches in `handle_definition_list_effect`)
    were left alone — none of the failing examples reach them, and
    the helper is intentionally narrow: it short-circuits when the
    buffer has more than one segment, when the first text isn't a
    fence opener, or when the dispatcher's gating wouldn't fire.
- **Renderer (renderer gap × 1)**:
  - `crates/panache-parser/tests/commonmark/html_renderer.rs`:
    fenced branch of `code_block_content` now strips `li_indent`
    leading spaces per line *before* applying the opener-indent rule.
- **Parser fixture (pins the CST shape)**:
  - `crates/panache-parser/tests/fixtures/cases/list_item_fenced_code_first_line_commonmark/`
    — pins the CST for `- \`\`\`\n  foo\n  \`\`\`\n` (LIST_ITEM contains
    LIST_MARKER + WHITESPACE + CODE_BLOCK with CODE_FENCE_OPEN +
    CODE_CONTENT preserving the 2-space list-item indent +
    CODE_FENCE_CLOSE). `flavor = "commonmark"`. Wired into
    `golden_parser_cases.rs`. No paired Pandoc fixture — pandoc agrees
    on the parse here, and adding a duplicate fixture would be churn.
- **Allowlist additions** (Lists):
  - +#318, +#324

### Don't redo

- Don't move the fence detection into `add_list_item` itself. The
  helper deliberately runs *after* the list-item is opened so that
  task-checkbox handling (which already strips `[ ] `/`[x] ` from the
  buffered text) and the empty-buffer fast-path stay simple. Reading
  `buffer.first_text()` after the fact is the cheapest place.
- Don't drop the `has_matching_fence_closer` gate. CommonMark dialect
  always opens a fence (even without a closer), but Pandoc-markdown
  needs `bare_fence_in_list_with_closer`. Removing the gate would
  divert Pandoc-flavored cases like `- \`\`\`` followed by paragraph
  text into a code block that runs to EOF, regressing Pandoc-only
  goldens.
- The `pos` math is `self.pos = new_pos.saturating_sub(1)` because
  `parse_inner_content` does `self.pos += lines_consumed` after
  `handle_list_open_effect` returns, and `lines_consumed = 1` from the
  list parser. If the dispatcher path ever changes that constant, this
  helper's compensation needs to be revisited.
- Don't extend the renderer fix to also tweak `fenced_opener_indent`.
  The opener_indent helper still measures the indent of `\`\`\`` *within
  the CODE_BLOCK*; it has no list-item awareness on purpose. The
  list-item strip happens *before* opener_indent stripping so the two
  rules compose without double-counting.
- Don't widen `first_text` to peek into `BlockquoteMarker` segments.
  By construction, when a list item is freshly opened the buffer is
  either empty or has a single Text segment (the post-marker remainder).
  The `segment_count() != 1` short-circuit is what keeps later
  continuation-line state from triggering the helper accidentally.

### Suggested next targets, ranked

1. **Fence inside blockquote inside list item (#321)** — markdown is
   `- a\n  > b\n  \`\`\`\n  c\n  \`\`\`\n- d\n`. The fence is on a
   continuation line of the list item *outside* the blockquote (column
   2 = list-item content column). Current parse buries everything in a
   blockquote and never recognizes the fence. The dispatcher's
   continuation-line fence detection in `parse_line` (lines ~1614-1620)
   only fires when `bq_depth > 0`; this case has the list-item ending
   the blockquote on the prior line. Likely needs a similar
   "list-item continuation can be interrupted by a fence at content
   column" branch in the non-blockquote list-item continuation path.
2. **Loose-vs-tight nested loose lists (#312, #326)** — top-level loose
   list with tight inner lists; renderer over-wraps inner items in
   `<p>` or splits at the wrong level. Mixed parser/renderer shape.
3. **Lazy / nested marker continuation (#296, #297, #305)** — `10) foo\n
   - bar` should produce nested list; currently parses as a paragraph.
   Parser issue: ordered-list-with-paren-marker doesn't accept a nested
   bullet without a blank line. Probably also covers #305.
4. **Multi-block content in `1.     code` items (#273, #274)** — `1.`
   followed by 5+ spaces should open a list item whose first block is
   an indented code block (content column = `1.` + 1 = 3, indented code
   needs +4 = 7+). Currently parser falls through to plain text.
5. **Empty list items (#266, #278, #280, #281, #283, #284)** — `*\n`,
   `- foo\n-\n- bar\n`. Parser currently treats a bare marker as a
   paragraph or bleeds into the next item. Needs explicit empty
   list-item recognition.
6. **Setext-in-list-item (#300)** — `- # Foo\n- Bar\n  ---\n  baz\n`
   needs `<h2>Bar</h2>` inside the second item; currently produces
   `<hr />` because the setext underline isn't recognized at the
   list-item content column.
7. **Marker-on-same-line nesting (#298, #299)** — `- - foo\n` should be
   nested lists; parser flattens.
8. **Emphasis and strong emphasis (47 fails)** — flanking-rule and
   autolink-precedence edge cases (#480, #481 are autolink-vs-emphasis).
9. **Tabs (4 fails)** — #2, #5, #6, #7 all need tab→space expansion
   with column alignment.
10. **Link reference definitions (12 fails)**, **Setext heading
    multi-line content** (#81, #82, #95) + **#115**, **Pandoc
    setext-in-blockquote losslessness**, **#342 / #148**.
