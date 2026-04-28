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

## Latest session — 2026-04-28 (r)

**Pass count: 521 → 522 / 652 (80.1%, +1)**

Targeted prior recap's #3 (10-digit ordered marker rejection, #266).
Single-example clean dialect-divergence fix. Probed several adjacent
candidates (Tabs #2/#5/#6/#7, multi-line setext #81/#82/#95/#115,
Block quotes #235/#236/#251/#252, Link reference defs 12 fails) and
documented why they were left for follow-up.

### Single root cause: ordered list marker accepted >9 digits

CommonMark §5.2 restricts ordered list markers to 1–9 arabic digits.
Pandoc-markdown accepts arbitrary digit counts. The decimal branch
of `try_parse_list_marker` did not gate on dialect, so
`1234567890. not ok` parsed as an ordered list under both flavors.

### Files changed

- **Parser (dialect divergence)**:
  - `crates/panache-parser/src/parser/blocks/lists.rs`: added a
    9-digit cap on the ordered-decimal marker branch, gated on
    `Dialect::CommonMark`. Pandoc behavior unchanged.
- **Parser fixtures (paired, pin dialect-specific CST)**:
  - `crates/panache-parser/tests/fixtures/cases/ordered_marker_max_digits_commonmark/`
    — `flavor = "commonmark"`, `1234567890. not ok\n` parses as a
    PARAGRAPH (no LIST).
  - `crates/panache-parser/tests/fixtures/cases/ordered_marker_max_digits_pandoc/`
    — `flavor = "pandoc"`, same input parses as a LIST with a
    10-digit LIST_MARKER. Wired into `golden_parser_cases.rs`.
- **Allowlist additions** (List items):
  - +#266

No formatter golden case added: the new CommonMark shape is just a
plain paragraph (the default fallback) — no new block sequence vs.
the Pandoc path-shape on disk, just a different parse. Per the rule,
formatter cases are only added when CommonMark produces a
*structurally different* block sequence.

### Don't redo

- Don't broaden the cap to `Dialect::Pandoc`. Pandoc accepts
  arbitrary-digit ordered markers (verified via
  `pandoc -f markdown -t native`). The fixture
  `ordered_marker_max_digits_pandoc` pins this — drop it and Pandoc
  behavior silently regresses.
- Don't try to reuse this gate for the `(2)` parenthesized form on
  line 252-275 of `lists.rs`. That form is already disabled under
  CommonMark (gated on `fancy_lists`, which CM does not enable),
  so no extra check needed there.
- The **multi-line setext heading** failures (#81, #82, #95, #115)
  produce a *broken CST* under `Dialect::CommonMark` — bytes appear
  in the wrong order (the HEADING node's content is from the second
  paragraph line, while the first paragraph line ends up as a sibling
  after it). Verified via `parse` on `Foo\nBar\n---\n`. This looks
  like a losslessness bug, not a feature gap. Pandoc treats that
  input as a paragraph (em-dash via intraword punctuation); CommonMark
  treats it as `<h2>Foo Bar</h2>`. Fixing requires reworking how the
  paragraph buffer is converted into a setext heading when the
  underline is encountered. **Big change, do not attempt as a
  conformance one-off** — needs its own session and probably a
  losslessness regression test first.

### Suggested next targets, ranked

1. **Multi-line setext heading + losslessness bug (#81, #82, #95,
   #115)** — under `Dialect::CommonMark`, the parser produces a
   broken CST when a paragraph of >1 line is followed by a setext
   underline. Verify losslessness fails (`tree.text() != input`)
   then fix in the setext detection path
   (`block_dispatcher.rs::SetextHeadingParser`, possibly
   `parser/core.rs` paragraph-buffer drain logic). Pandoc keeps
   the paragraph-with-em-dash behavior. Will need paired fixtures
   + a top-level CommonMark formatter golden because the new shape
   is structurally different.
2. **Multi-line link reference definitions (#193, #195, #196, #198,
   #208)** — ref defs whose URL or title are split across lines are
   not currently recognized. Single root cause; likely a
   parser-shape change in the inline / block scanning of
   `[label]:`. Verify each form against pandoc first.
3. **Empty list item closes the list when followed by blank line
   (#280)** — markdown `-\n\n  foo\n` should produce
   `<ul><li></li></ul><p>foo</p>` (the empty item closes after the
   blank, foo is *outside* the list). Currently produces
   `<ul><li><p></p><p>foo</p></li></ul>` because the blank-line +
   indented-content rule keeps the foo inside the empty item.
   Likely needs a list-blank-handling branch in `core.rs` or
   `list_postprocessor.rs`.
4. **Multi-empty-marker with subsequent indented content (#278)** —
   `-\n  foo\n-\n  ```\n  bar\n  ```\n-\n      baz\n`. Three bare
   markers each followed by indented content (or blank). Currently
   parses chaotically. Once #280 is solved this may partially come
   along.
5. **Tabs (#2, #5, #6, #7)** — column-aware tab expansion needed
   for indented-code inside containers. Tab at column 2 should
   expand to 2 spaces (to reach next tab stop = 4); tab at column 0
   should expand to 4. Mostly a `leading_indent` / indent-counting
   change but touches the dispatcher's column bookkeeping.
   Substantial.
6. **Block quotes lazy-continuation (#235, #236, #251)** — lazy
   continuation must not extend a list or code block inside a
   blockquote. CommonMark §5.1: laziness only applies to paragraph
   continuations.
7. **Fence inside blockquote inside list item (#321)** — list-item
   continuation can be interrupted by a fence at content column;
   dispatcher's continuation-line fence detection
   (`parse_line` ~lines 1614-1620) only fires when `bq_depth > 0`.
8. **Loose-vs-tight nested loose lists (#312, #326)** — top-level
   loose list with tight inner lists; renderer over-wraps inner
   items in `<p>` or splits at the wrong level. Mixed
   parser/renderer shape.
9. **Lazy / nested marker continuation (#296, #297, #305)** —
   `10) foo\n  - bar` should produce nested list; currently parses
   as a paragraph. Parser issue: ordered-list-with-paren-marker
   doesn't accept a nested bullet without a blank line.
10. **Multi-block content in `1.     code` items (#273, #274)** —
    `1.` followed by 5+ spaces should open a list item whose first
    block is an indented code block. Currently parser falls through
    to plain text.
11. **Setext-in-list-item (#300)** — `- # Foo\n- Bar\n  ---\n  baz\n`
    needs `<h2>Bar</h2>` inside the second item; currently produces
    `<hr />` because the setext underline isn't recognized at the
    list-item content column.
12. **Marker-on-same-line nesting (#298, #299)** — `- - foo\n`
    should be nested lists; parser flattens.
13. **Emphasis and strong emphasis (47 fails)** — flanking-rule and
    autolink-precedence edge cases (#480, #481 are
    autolink-vs-emphasis).
14. **Link reference definitions remainder (#194, #201, #213, #216,
    #217, #218)** — URL with parens, strict validation, setext
    eating defs, cross-block cascade, blockquoted def lookup.

--------------------------------------------------------------------------------

## Previous session — 2026-04-28 (q)

**Pass count: 518 → 521 / 652 (79.9%, +3)**

Targeted prior recap's #5 (empty list items: #266, #278, #280, #281,
#283, #284). One small parser-shape gap unlocked #281, #283, #284.
#280 still fails — different problem: empty list-item with one
intervening blank line should close the list before subsequent
content. #278 unchanged. #266 is unrelated (10-digit ordered marker
acceptance — should be 9-digit max in CommonMark).

### Single root cause: bare list markers were not recognized

The marker parser at
`crates/panache-parser/src/parser/blocks/lists.rs:158`
accepted `is_empty()` after-marker-text but not when the line ended
with a newline (`*\n` → `after_marker = "\n"`, none of the gates
matched). That alone made bare markers fall through to paragraph
parsing.

Even after fixing the marker parser, the dispatcher gate at
`crates/panache-parser/src/parser/block_dispatcher.rs:596` rejected
*every* `spaces_after_cols == 0` match. That gate exists to reject
the task-checkbox-without-space case (`-[ ] foo`), so it must remain
— but needs to allow bare markers and refuse interrupting a
document-level paragraph (CommonMark §5.2: "An empty list item cannot
interrupt a paragraph"). #285 (`foo\n*\n` → `<p>foo *</p>`) keeps
passing thanks to the `!at_document_start && !has_blank_before
&& !in_list` clause.

### Files changed

- **Parser (parser-shape gap × 2)**:
  - `crates/panache-parser/src/parser/blocks/lists.rs`:
    `try_parse_list_marker` now strips trailing `\r`/`\n` from the
    line at entry, so trailing-newline bare markers are recognized.
  - `crates/panache-parser/src/parser/block_dispatcher.rs`:
    `ListParser::detect_prepared` accepts `spaces_after_cols == 0`
    when (a) `after_marker_text.trim_end_matches(['\r','\n'])` is
    empty (bare marker, not task-without-space) and (b) the bare
    marker is at document start, after a blank line, or already
    inside a list. Preserves the prior gate's intent (reject
    `-[ ] foo`) while allowing CommonMark/Pandoc empty list items.
- **Parser fixture (pins the CST shape)**:
  - `crates/panache-parser/tests/fixtures/cases/list_item_bare_marker_empty_commonmark/`
    — pins the CST for `- foo\n-\n- bar\n` (single LIST with three
    LIST_ITEM children; middle item is bare LIST_MARKER + PLAIN
    containing only NEWLINE). `flavor = "commonmark"`. Wired into
    `golden_parser_cases.rs`. No paired Pandoc fixture — pandoc
    `-f markdown` produces the same shape (verified), so a duplicate
    would be churn.
- **Allowlist additions** (List items):
  - +#281, +#283, +#284

No formatter golden case added: pandoc-default and CommonMark produce
the same formatted output (`- foo\n- \n- bar\n`, idempotent). Per the
rule, only add a CommonMark formatter case when the dialects diverge
structurally on the new shape.

### Don't redo

- Don't drop the `in_list` clause from the dispatcher gate. Without
  it, a bare marker on a continuation line of a top-level paragraph
  (#285's `foo\n*\n`) would interrupt the paragraph and open a list,
  regressing the spec. Keep all three: at_document_start OR
  has_blank_before OR in_list.
- Don't move the `\r\n` strip out of `try_parse_list_marker`.
  Several callers pass synthetic bare-marker lookahead lines without
  newlines (e.g. unit tests at lines 632–642). Stripping at entry
  keeps both call shapes working without churn at every callsite.
- Don't reuse `after_marker_text.is_empty()` directly in the
  dispatcher. `ctx.content` retains its trailing newline, so
  `after_marker_text` is `"\n"` for `*\n`. Use
  `trim_end_matches(['\r', '\n']).is_empty()` to accept that case
  while still rejecting `-[ ] foo` (`after_marker_text = "[ ] foo"`
  ≠ empty after trimming).
- The new fixture pins the empty item as `LIST_MARKER + PLAIN(NEWLINE)`,
  not as `LIST_MARKER` alone. That's deliberate — list-item children
  always include a tail node (PLAIN/PARAGRAPH) for whitespace
  bookkeeping in the formatter. Don't try to "tighten" by removing
  the PLAIN.

### Suggested next targets, ranked

1. **Empty list item closes the list when followed by blank line
   (#280)** — markdown `-\n\n  foo\n` should produce
   `<ul><li></li></ul><p>foo</p>` (the empty item closes after the
   blank, foo is *outside* the list). Currently produces
   `<ul><li><p></p><p>foo</p></li></ul>` because the blank-line +
   indented-content rule keeps the foo inside the empty item. Likely
   needs a list-blank-handling branch: when the list item has *no*
   non-newline content and the next line after a blank doesn't
   strictly belong to a sibling marker, close the list. Possibly in
   `core.rs` blank-line handling or in
   `list_postprocessor.rs`. Verify the spec expects this for both
   bullet and ordered (#283/#284 cousins are siblings, this is the
   "blank line empties out" sibling).
2. **Multi-empty-marker with subsequent indented content (#278)** —
   `-\n  foo\n-\n  ```\n  bar\n  ```\n-\n      baz\n`. Three bare
   markers each followed by indented content (or blank) on next
   line. Currently parses chaotically. Once #280 is solved this may
   partially come along. Verify behavior shape before touching.
3. **10-digit ordered marker rejection (#266)** — markdown
   `1234567890. not ok\n`. CommonMark spec restricts ordered-list
   markers to 1–9 digits. Current parser accepts arbitrary digit
   counts. Fix in marker parser's ordered branch; tighten under
   `Dialect::CommonMark` only (Pandoc may agree, verify with pandoc
   first). Single-example fix.
4. **Fence inside blockquote inside list item (#321)** — markdown
   `- a\n  > b\n  \`\`\`\n  c\n  \`\`\`\n- d\n`. The fence is on a
   continuation line of the list item *outside* the blockquote
   (column 2 = list-item content column). Current parse buries
   everything in a blockquote and never recognizes the fence.
   Dispatcher's continuation-line fence detection in `parse_line`
   (~lines 1614-1620) only fires when `bq_depth > 0`; this case has
   the list-item ending the blockquote on the prior line. Likely
   needs a "list-item continuation can be interrupted by a fence at
   content column" branch in the non-blockquote continuation path.
5. **Loose-vs-tight nested loose lists (#312, #326)** — top-level
   loose list with tight inner lists; renderer over-wraps inner
   items in `<p>` or splits at the wrong level. Mixed parser/renderer
   shape.
6. **Lazy / nested marker continuation (#296, #297, #305)** —
   `10) foo\n  - bar` should produce nested list; currently parses
   as a paragraph. Parser issue: ordered-list-with-paren-marker
   doesn't accept a nested bullet without a blank line.
7. **Multi-block content in `1.     code` items (#273, #274)** —
   `1.` followed by 5+ spaces should open a list item whose first
   block is an indented code block. Currently parser falls through
   to plain text.
8. **Setext-in-list-item (#300)** — `- # Foo\n- Bar\n  ---\n  baz\n`
   needs `<h2>Bar</h2>` inside the second item; currently produces
   `<hr />` because the setext underline isn't recognized at the
   list-item content column.
9. **Marker-on-same-line nesting (#298, #299)** — `- - foo\n`
   should be nested lists; parser flattens.
10. **Emphasis and strong emphasis (47 fails)** — flanking-rule and
    autolink-precedence edge cases (#480, #481 are
    autolink-vs-emphasis).
11. **Tabs (4 fails)** — #2, #5, #6, #7 all need tab→space expansion
    with column alignment.
12. **Link reference definitions (12 fails)**, **Setext heading
    multi-line content** (#81, #82, #95) + **#115**, **Pandoc
    setext-in-blockquote losslessness**, **#342 / #148**.
