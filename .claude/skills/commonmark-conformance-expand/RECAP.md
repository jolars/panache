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

## Latest session — 2026-04-28 (k)

**Pass count: 479 → 483 / 652 (74.1%, +4)**

Targeted the prior recap's #3 group: HR-interrupts-paragraph in containers
(#234, #246, plus the list variants #57, #60, #61). The two blockquote cases
(#234 setext-underline-crosses-blockquote, #246 lazy-paragraph-not-interrupted-
by-HR) shared a single root cause: the parser was letting paragraph
continuation rules cross blockquote boundaries. Two small dialect-gated checks
fix both, and they incidentally unlock #92 and #101 (same setext-underline
shape with different inputs). The list cases (#57, #60, #61) need separate
parser surgery in the LIST/HR interaction and were intentionally deferred —
see "Don't redo" for the trap.

### Targets and root causes

- **Block quotes #234, Setext headings #92, #101 (3)** — setext-underline-
  crosses-blockquote-boundary. `> foo\n---\n` was being parsed as a blockquote
  containing a setext H2 (with `> foo` as the heading text). CommonMark §4.3
  requires the underline to be in the same container as the text — the
  underline at depth 0 cannot underline a paragraph at depth 1. Fix:
  `SetextHeadingParser::detect_prepared` now rejects when
  `count_blockquote_markers(next_line).0 != ctx.blockquote_depth`, gated on
  `Dialect::CommonMark`. Pandoc keeps the historical behavior (Pandoc actually
  treats `> foo\n---\n` as a top-level setext H2 with text `> foo` — neither
  matches CommonMark, but Pandoc's pre-existing behavior is its own thing).
- **Block quotes #246 (1)** — HR doesn't interrupt lazy paragraph
  continuation. `> aaa\n***\n> bbb\n` was being collapsed into a single
  blockquote paragraph because lazy continuation appended `***` as plain
  paragraph text instead of recognizing it as a thematic break. Per
  CommonMark §5.1, lazy continuation only applies if the line wouldn't
  otherwise be parsed as a different block. Fix: in `core.rs` lazy paragraph
  continuation path (the `bq_depth < current_bq_depth && bq_depth == 0`
  branch, around line 1416), skip the `append_paragraph_line` and fall
  through to normal close+dispatch when `try_parse_horizontal_rule(line)`
  matches, gated on `Dialect::CommonMark`. The fall-through then closes the
  paragraph + blockquote and the regular dispatch emits the HR.

### Files changed

- **Parser (dialect divergence × 2)**:
  - `crates/panache-parser/src/parser/block_dispatcher.rs` —
    `SetextHeadingParser::detect_prepared` adds the container-depth gate
    described above.
  - `crates/panache-parser/src/parser/core.rs` — lazy paragraph
    continuation in the close-blockquote branch checks
    `try_parse_horizontal_rule` first under CommonMark; on match it skips
    the lazy append so the line dispatches as an HR after the blockquote
    closes. New import:
    `use super::blocks::horizontal_rules::try_parse_horizontal_rule;`.
- **Parser fixtures (paired CST snapshots via insta)**:
  - `crates/panache-parser/tests/fixtures/cases/setext_underline_crosses_blockquote_commonmark/`
    pins the new shape: `BLOCK_QUOTE > PARAGRAPH("foo")` followed by
    `HORIZONTAL_RULE`. **No paired Pandoc fixture** — Pandoc's parse of the
    same input hits a pre-existing losslessness bug (HEADING_CONTENT TEXT
    contains the literal `> foo`, and the BLOCK_QUOTE wrapper has already
    emitted its marker, so the CST text becomes `> > foo\n---\n`). That bug
    is its own session; do not fix it as a side effect here.
  - `crates/panache-parser/tests/fixtures/cases/hr_interrupts_lazy_blockquote_paragraph_{commonmark,pandoc}/`
    pin the divergent CST: under CommonMark the `***` line is a top-level
    `HORIZONTAL_RULE` between two `BLOCK_QUOTE` nodes; under Pandoc all
    three lines collapse into one `BLOCK_QUOTE > PARAGRAPH` (the `> bbb`
    line's marker becomes a `BLOCK_QUOTE_MARKER` token *inside* the
    paragraph, courtesy of Pandoc's lazy-continuation semantics).
  - All three registered in
    `crates/panache-parser/tests/golden_parser_cases.rs`.
- **Formatter goldens**: none. Verified that the formatter output for
  `> foo\n---\n` under both dialects is byte-identical (`> foo\n\n` + 80-dash
  HR), even though the CSTs diverge — Pandoc's setext-in-blockquote
  formatter path coincidentally renders the same two-block sequence the
  CommonMark parser produces structurally. Per the formatter rule, no
  paired CommonMark golden is needed when output matches.
- **Allowlist additions**: `tests/commonmark/allowlist.txt`
  - Block quotes: +#234, +#246
  - Setext headings: +#92, +#101

### Don't redo

- The setext container-depth gate uses `count_blockquote_markers(next_line).0`
  (the *raw* depth from the next line bytes), compared against
  `ctx.blockquote_depth` (the *active container* depth from the parser
  stack). These are equal only when the underline line has the same
  blockquote prefix shape as the text line. Don't accidentally compare to
  `ctx.content`'s depth (it's already stripped) or to the raw text-line
  depth (the dispatcher has already consumed those markers from the
  current line by the time `detect_prepared` runs).
- The HR-interrupts-lazy gate currently checks *only* horizontal rules.
  Per CommonMark §5.1, the full set of paragraph-interrupting blocks
  (ATX heading, fenced code, blockquote start, list-item with non-empty
  marker content, HR) all *should* break lazy continuation. We only added
  HR because that's what the targets exercise; expanding the check is the
  natural follow-up. Don't generalize prematurely without a failing-spec
  example to anchor each addition — it's easy to over-interrupt and
  regress Pandoc-flavored docs.
- #57, #60, #61 (list HR interactions) are **not** unlocked by this work
  and are *deeper* than the lazy-continuation fix:
  - #57/#60: parser correctly recognizes `***` as HR, but emits it
    *inside* the open list item rather than closing the list. The fix
    is in the LIST item-content dispatch, not the lazy-paragraph path.
  - #61: parser sees `- * * *` as a new list item with text `* * *`
    rather than as a list item containing an HR. The HR detection
    needs to fire on item content, which currently goes straight to
    paragraph/PLAIN.
  Both require touching LIST_ITEM emission — different code path.
- Pandoc has a pre-existing losslessness bug for setext heading inside
  blockquote (`> foo\n---\n` parses to a BLOCK_QUOTE containing a HEADING
  whose HEADING_CONTENT TEXT contains `> foo`, double-counting the marker
  bytes). Same shape as the HTML-block-in-blockquote bug fixed in session
  (j); the fix would mirror `parse_html_block`'s
  `strip_n_blockquote_markers` + `emit_html_block_line` pattern, but this
  time in the setext heading emission. Out of scope for HR work.

### Suggested next targets, ranked

1. **Lists HR interaction (#57, #60, #61)** — three failing examples in
   §Thematic breaks, all involving HR inside a list. Different code
   paths from the lazy-paragraph fix above. #57 / #60 need the HR
   detection to close the list (not just the item); #61 needs HR
   detection inside list-item content. Likely a single session, fixes
   the three plus possibly a few more in §Lists / §List items.
2. **Lists (5/21) + List items (17/31)** — biggest absolute pass-rate
   gap (52 fails). Some will fall to the #1 fix above; the rest need
   separate looks at list-item paragraph parsing, loose vs tight, and
   indented continuation.
3. **Emphasis and strong emphasis (85/47)** — largest remaining
   absolute failure count; flanking-rule and autolink-precedence edge
   cases (#480, #481 are autolink-vs-emphasis precedence).
4. **Generalize the lazy-continuation interrupter check** — currently
   only HR breaks lazy paragraph continuation. ATX heading, fenced
   code, blockquote-start, and list-marker-with-content should also.
   Each needs a failing-spec example to anchor it; check report.txt
   for §Block quotes failures that look like
   `> para\n# heading\n` shapes.
5. **Tabs (5 fails)** — #2, #4, #5, #6, #7 all need tab→space
   expansion with column alignment. Mechanical but its own session.
6. **Link reference definitions (12 fails)** — bigger refactor;
   currently captures whole line(s) as raw TEXT.
7. **Setext heading multi-line content (#81, #82, #95)** + **#115**.
8. **Pandoc setext-in-blockquote losslessness** — pre-existing parser
   bug surfaced while writing fixtures this session. Not a spec
   unlock, but worth fixing for correctness; mirrors the
   `parse_html_block` fix from session (j).
9. **#342 (code-span/link precedence)**, **#148 (table-pre nesting)** —
   one-offs.
