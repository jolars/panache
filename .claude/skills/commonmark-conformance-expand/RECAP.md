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

## Latest session — 2026-04-29 (iii)

**Pass count: 569 → 572 / 652 (87.7%, +3)**

Took recap target #2 (loose-vs-tight semantic gaps, renderer-only).
Three wins in the Lists section: #315, #317, #326. The other two
candidates the recap floated for the same fix — #312, #320 — are
*not* renderer-only and remained failing (confirmed parser-shape
gaps; see "Don't redo" below).

### Root cause: `is_loose_list` over-detected and under-detected

The previous `is_loose_list` made three structurally different
mistakes:

1. **Over-eager descendant scan.** It returned true on any
   PARAGRAPH *descendant*, so a paragraph inside a nested
   blockquote made the *outer* list loose. This was the recap's
   diagnosis. Fix: drop the descendant scan; check PARAGRAPH only
   at the *direct* child level (which is the parser's signal).
2. **Missing trailing-blank case.** For `- a\n  - b\n  - c\n\n- d
   ...` (#326), the parser puts the blank line as the last child
   of the *inner* sublist. The outer list's children are just
   LIST_ITEM, LIST_ITEM with no sibling BLANK_LINE — the existing
   between-items check missed it. Fix: also walk the previous
   item's descendants and look for a BLANK_LINE whose end byte
   coincides with the item's end byte.
3. **REFERENCE_DEFINITION not counted as block-level.** For
   `- a\n- b\n\n  [ref]: /url\n- d\n` (#317), item 2 directly
   contains PLAIN, BLANK_LINE, REFERENCE_DEFINITION — by §5.3 (b)
   that is two block-level children separated by a blank, so the
   list is loose. `is_block_child` excluded REFERENCE_DEFINITION.
   Fix: add it.

Plus a small list-rendering gap surfaced by #315: `* a\n*\n\n* c\n`
makes a loose list whose middle item has empty PLAIN content
(`PLAIN { NEWLINE }`). The existing renderer wrapped it as
`<p></p>` inside `<li>`; the spec wants bare `<li></li>`. Fix: in
`render_list_item`, skip the paragraph wrapper when the PLAIN's
text is whitespace-only.

### Why renderer-only

All three winning examples produce CSTs whose shapes are already
correct — just being rendered to the wrong HTML. The remaining
list-section failures (#312, #320) are genuine parser-shape gaps
the renderer cannot paper over: #312 splits a 5th list item that
should continue the 4th (marker-indent tracking), #320 emits a
stray PARAGRAPH for `  > b` instead of extending the blockquote
(blockquote-continuation under list-item indent).

### Files changed

- **Renderer gap**:
  - `crates/panache-parser/tests/commonmark/html_renderer.rs`:
    rewrote `is_loose_list` per CommonMark §5.3 cleanly (drops
    descendant PARAGRAPH scan; adds `list_item_ends_with_blank`
    helper); added REFERENCE_DEFINITION to `is_block_child`;
    skipped empty PLAIN paragraph wrapper in `render_list_item`
    via a new `plain_is_empty` helper.
- **New parser fixtures + snapshots** (CommonMark-flavored — these
  pin the CST shapes the renderer fix now leans on, so the
  invariants don't rot silently in `html_renderer.rs`):
  - `list_item_blank_then_refdef_commonmark`: pins LIST_ITEM
    containing PLAIN + BLANK_LINE + REFERENCE_DEFINITION (the
    "internal blank between two block children" shape used for
    §5.3 (b) detection in #317).
  - `nested_list_blank_between_outer_items_commonmark`: pins
    inner LIST whose last child is a BLANK_LINE that ends at the
    outer LIST_ITEM's end byte (the shape `list_item_ends_with_blank`
    walks for #326).
  - The empty-PLAIN shape for #315 is already pinned by
    `list_item_bare_marker_empty_commonmark`.
- **Allowlist additions** (Lists section): #315, #317, #326.

### Don't redo

- Don't try to fix #320 via the renderer. The CST has a stray
  direct-child PARAGRAPH for `  > b` that is *outside* the
  intended BLOCK_QUOTE — even with perfect loose-vs-tight
  detection the rendered text body would still emit
  `<p>  &gt; b</p>` instead of a blockquote. Real fix is in the
  parser's blockquote-continuation under list-item indent.
- Don't try to fix #312 via the renderer. The 5th item splits
  off into its own outer list because the parser interprets
  4-space indent as a nested-list start; this is marker-indent
  tracking, not a render-time decision.
- Don't reintroduce the descendant PARAGRAPH scan in
  `is_loose_list`. It was over-detecting (paragraphs inside
  nested blockquotes/sublists triggered loose), which is exactly
  what #320's expected output (TIGHT outer with a blockquote
  child) showed was wrong; the new direct-child check is what
  spec §5.3 actually requires. The two new fixtures pin the
  shapes the new logic relies on, and the existing
  `lists_*` golden snapshots verify it doesn't false-positive.
- Don't promote `plain_is_empty` to the renderer's general
  paragraph path. It is specifically a list-item rendering
  rule (§5.3 example #315) — for top-level paragraphs an empty
  paragraph is a parser bug worth surfacing, not silencing.

### Suggested next targets, ranked

1. **Empty list item closes the list when followed by blank line
   (#280)** — `-\n\n  foo\n` should produce
   `<ul><li></li></ul><p>foo</p>`. Parser-shape gap: parser keeps
   `  foo` inside the same LIST_ITEM as a second PLAIN child
   instead of closing the list. Touches list-item continuation
   when the item starts with bare-marker + blank.
2. **List with non-uniform marker indentation (#312)** —
   `- a\n - b\n  - c\n   - d\n    - e\n` should keep all five at
   the same list level (last "- e" is lazy continuation of "d"
   per CommonMark indent rules). Currently splits at "- e"
   because the parser interprets 4-space indent as starting a
   nested list. Parser-shape gap; touches list-marker indent
   tracking.
3. **Blockquote inside list item misaligned (#320)** — `* a\n  >
   b\n  >\n* c\n` should produce a single BLOCK_QUOTE inside
   item 1 (containing "b"); parser instead emits PARAGRAPH("  >
   b") + BLOCK_QUOTE("  >" + blank). Parser-shape gap in
   blockquote-continuation under list-item indent.
4. **Tabs (#2, #5, #6, #7)** — column-aware tab expansion for
   indented-code inside containers. Substantial; touches
   `leading_indent` and tab-stop logic.
5. **HTML block #148** — raw HTML `<pre>`-block contains a blank
   line that should be emitted verbatim, but our parser/renderer
   reformats `_world_` as inline emphasis inside the `<pre>`. May
   be a renderer bug (HTML block content should be byte-perfect).
6. **Reference link followed by another bracket pair (#569, #571)**
   — requires CMark "left-bracket scanner" stack model. Large.
7. **Nested LINKs in link text (#518, #519, #520, #532, #533)** —
   CommonMark §6.4 forbids real nesting; outer must un-link
   itself when inner resolves. Same scanner-stack work as #569.
8. **HTML-tag/autolink interaction with link brackets (#524, #526,
   #536, #538)** — bracket scanner must skip past raw HTML and
   autolinks too.
9. **Block quotes lazy-continuation #235, #251** — last two
   blockquote failures.
10. **Fence inside blockquote inside list item (#321)**.
11. **Lazy / nested marker continuation (#298, #299)**.
12. **Multi-block content in `1.     code` items (#273, #274)**.
13. **Setext-in-list-item (#300)**.
14. **Emphasis and strong emphasis (47 fails)** — flanking-rule
    edge cases. #352 (`a*"foo"*`), #354 (`*$*alpha`),
    #366/#367/#368/#369, #372–376 (underscore intra-word). Need
    proper CommonMark flanking-rule gating; current emphasis
    parser leans on Pandoc's looser semantics.
15. **Ref-def dialect divergence #201** — `[foo]: <bar>(baz)`. Low
    priority.
