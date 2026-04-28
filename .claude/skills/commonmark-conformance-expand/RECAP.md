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

## Latest session — 2026-04-28 (o)

**Pass count: 508 → 516 / 652 (79.1%, +8)**

Targeted prior recap's #1 first sub-bucket (Code blocks inside list
items). Single renderer-only root cause unlocked all 8.

**Indented code blocks inside list items kept the list-item content
column on every line.** Per CommonMark §4.4 / §5.2, an indented code
block inside a list item requires `content_col + 4` spaces of indent on
each line; the rendered code content should have those `content_col + 4`
spaces stripped. The parser preserves the full indent inside CODE_CONTENT
(losslessness); the renderer's existing strip only removed the 4-space
indented-code marker, leaving the list-item content column behind. Fix:
walk up to the immediate enclosing LIST_ITEM, compute its content column
from the leading WHITESPACE + LIST_MARKER + trailing WHITESPACE tokens,
and strip that many leading spaces from each CODE_CONTENT line *before*
the existing 4-space marker strip.

This is purely an indented-code-block fix. Fenced code blocks already
handle the list-item indent because the parser includes the leading WS
inside the CODE_BLOCK before CODE_FENCE_OPEN, which `fenced_opener_indent`
absorbs into its `opener_indent` strip — that path was unchanged.

### Targets and root causes

- **Single root-cause group (renderer × 1)**: 8 unlocks across §List
  items (#254, #264, #270, #271, #286, #287, #288, #290). All are
  examples of indented code blocks inside list items where the renderer
  was leaking the list content column into the `<pre><code>` payload.

### Files changed

- **Renderer (renderer gap × 1)**:
  - `crates/panache-parser/tests/commonmark/html_renderer.rs`:
    - New helpers `enclosing_list_item_content_column`,
      `list_item_content_column`, and `strip_leading_spaces_per_line`.
    - `code_block_content` now applies the list-item content-column
      strip *before* the indented-code 4-space marker strip on each
      CODE_CONTENT line. Only fires when the CODE_BLOCK has a LIST_ITEM
      ancestor; preserves prior behavior for top-level indented code
      blocks (no LIST_ITEM ancestor → 0-column strip → no-op).
- **Parser fixture (pins the CST shape the renderer relies on)**:
  - `crates/panache-parser/tests/fixtures/cases/list_item_indented_code/`
    — pins the CST for `- foo\n\n      bar\n` (LIST_ITEM contains
    LIST_MARKER + WHITESPACE + PLAIN + BLANK_LINE + CODE_BLOCK with
    CODE_CONTENT preserving all 6 leading spaces). Single dialect
    (Pandoc and CommonMark agree on this shape).
    Wired into `golden_parser_cases.rs`.
- **Allowlist additions** (List items):
  - +#254, +#264, +#270, +#271, +#286, +#287, +#288, +#290

### Don't redo

- The strip targets *only* the immediate enclosing LIST_ITEM. The walk
  short-circuits at the first LIST_ITEM ancestor because deeper nesting
  is already accounted for: the inner LIST_ITEM's leading WHITESPACE
  token includes the outer-list content column. Don't sum content
  columns across all ancestor LIST_ITEMs — you'd double-strip and chew
  into the actual code.
- Don't apply the strip to fenced code blocks inside list items. The
  fenced path uses `fenced_opener_indent` which already counts the WS
  before `CODE_FENCE_OPEN` (and the parser places that WS *inside* the
  CODE_BLOCK so it lines up with the list-item content column).
  Stacking another list-item strip on top would be a double-strip.
- `list_item_content_column` walks tokens until the first
  `WHITESPACE`/`LIST_MARKER` is followed by something else. Don't extend
  it to include `NEWLINE` or content tokens — the content column is
  defined by the marker line only.
- Tab handling is char-count based (`.chars().count()`), not column
  based. None of the current §List items failures use tabs in marker
  positions, but if a future case does, this helper undercounts. Use a
  spec-aligned column expansion (next multiple of 4) at that point.

### Suggested next targets, ranked

1. **Fenced code blocks inside list items (#318, #321, #324)** — the
   parser doesn't currently recognize a fenced block that opens at the
   list-item content column when there's no leading WHITESPACE in front
   of the marker bytes. E.g. `1. \`\`\`\n   foo\n   \`\`\`\n` parses the
   triple-backticks as INLINE_CODE inside a PLAIN, not as a CODE_BLOCK.
   Needs a parser fix in the list-item content-parsing path (likely
   `crates/panache-parser/src/parser/blocks/list.rs` or wherever
   list-item child blocks are dispatched).
2. **Loose-vs-tight nested loose lists (#312, #326)** — top-level list
   contains items separated by blank lines (loose), but the inner lists
   should still be tight. The renderer is currently making the inner
   children too tight (#312 splits at the wrong level) or
   over-wrapping #326 with `<p>` for inner items. Mixed shape.
3. **Lazy / nested marker continuation (#296, #297, #305)** — `10) foo\n
   - bar` should produce nested list; currently parses as a paragraph.
   Parser issue: ordered-list-with-paren-marker doesn't accept a nested
   bullet. Same root cause likely covers #305 (number-other-than-1
   interruption rules).
4. **Multi-block content in `1.     code` items (#273, #274)** — `1.`
   followed by 5+ spaces should start a list item whose first block is
   an indented code block (content column = `1.` + 1 = 3, indented code
   needs +4 = 7+). Currently parser falls through to plain text.
   Parser fix in list-item first-line handling.
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

### Previous session — 2026-04-28 (n)

**Pass count: 488 → 508 / 652 (77.9%, +20)**

Targeted Lists / List items. Two coupled renderer-only root causes:

1. **Loose-list detection missed BLANK_LINE *inside* a LIST_ITEM**.
   New `list_item_has_internal_blank` helper in `is_loose_list`.
2. **PLAIN/PARAGRAPH inside LIST_ITEM weren't stripping line indent**.
   PARAGRAPH path routed through `render_paragraph`, PLAIN path
   strips per-line indent while preserving the trailing newline.

Files: `html_renderer.rs` (renderer), parser fixture
`list_item_blank_line_inside/`. Allowlisted §Tabs +#4, §List items
+#256/258/259/262/263/277/279/291, §Lists +#306/307/309/311/313/314/
316/319/325.

**Don't redo**: don't trim the trailing `\n` from PLAIN-in-tight before
the `<li>` normalization step (breaks `<li>foo\n<ul>` shape for nested
lists). Don't double-wrap the PARAGRAPH path in `<p>...</p>`.
`list_item_has_internal_blank` requires *both* neighbors to be
block-level — don't expand to WHITESPACE/NEWLINE.
