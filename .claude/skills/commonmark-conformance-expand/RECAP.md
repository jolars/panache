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

## Latest session — 2026-04-29 (ii)

**Pass count: 566 → 569 / 652 (87.3%, +3)**

Took the carried-forward "Indented code block after ATX heading
(#115)" target. Single-site fix at the indented-code dispatcher
gate. Unlocked #115 plus two related blockquote-with-indented-code
cases (#236, #252) that were blocked by the same overly-strict
gate.

### Root cause: indented-code gate required literal blank line before

`IndentedCodeBlockParser::detect_prepared` consulted
`ctx.has_blank_before_strict`, which is true only when the previous
line was *literally* blank (or pos==0). That misses the case where
the previous block was a heading / fenced code / HR — those don't
continue, so the next indented line is at block-level start, not
mid-paragraph. CommonMark §4.4 says only paragraphs block
interruption; non-paragraph predecessors are fine.

For `# Heading\n    foo\n`:
- Line 1 prev is `# Heading` (not blank) → strict=false → indented
  code rejected → falls through to paragraph buffer.

The relaxed `has_blank_before` field already encodes the right
signal (paragraph-not-open / prev-block-is-self-closing), since
`previous_block_requires_blank_before_heading()` returns false
after a heading.

### Why dialect-gated

For `>     foo\n    bar\n`, the dialects diverge (verified with
pandoc):
- `pandoc -f commonmark`: closes the BQ, `    bar` is a top-level
  indented code block. The relaxed gate is correct.
- `pandoc -f markdown`: lazily extends the BQ; `    bar` is a
  paragraph inside the BQ. The strict gate (or some other
  Pandoc-specific lazy-continuation logic) is needed.

So the relaxed gate applies under `Dialect::CommonMark` only; Pandoc
keeps `has_blank_before_strict`. Pandoc-side `# Heading\n    foo\n`
is *also* buggy under our parser (buffers as paragraph instead of
heading + code block per pandoc's own native output), but the OLD
behavior wasn't tested by any fixture and fixing it under Pandoc
would touch lazy-continuation rules; left as a follow-up.

### Files changed

- **Parser-shape gap (CommonMark dialect divergence)**:
  - `crates/panache-parser/src/parser/block_dispatcher.rs`: in
    `IndentedCodeBlockParser::detect_prepared`, replace the
    `!ctx.has_blank_before_strict` gate with a dialect-aware check:
    under `Dialect::CommonMark`, allow when
    `has_blank_before || at_document_start`; otherwise keep the
    literal-blank-line strict gate.
- **New parser fixtures + snapshots (paired)**:
  - `indented_code_after_atx_heading_commonmark`: pins HEADING +
    CODE_BLOCK with WHITESPACE("    ") + TEXT("foo") under
    `flavor = "commonmark"`.
  - `indented_code_after_atx_heading_pandoc`: pins the current
    Pandoc behavior (HEADING + PARAGRAPH containing
    TEXT("    foo")). This locks in that the dialect gate stays;
    a future Pandoc-side fix will need to update this fixture
    intentionally.
- **New formatter fixture**:
  - `indented_code_after_atx_heading_commonmark` (top-level
    `tests/fixtures/cases/`): pins
    `# Heading\n    foo\n` → `# Heading\n\n` + fenced code
    `\`\`\`\nfoo\n\`\`\`\n`. Idempotency verified by the harness.
- **Allowlist additions**: #115 (Indented code blocks); #236, #252
  (Block quotes — both were blocked by the same gate).

### Don't redo

- Don't drop the `Dialect::CommonMark` gate. Removing it
  globally regresses Pandoc lazy-continuation: `>     foo\n    bar`
  would close the blockquote and emit a top-level code block,
  contradicting `pandoc -f markdown` (which keeps it inside the
  BQ as a lazy paragraph). The Pandoc fixture
  `indented_code_after_atx_heading_pandoc` will fail loudly if
  the gate is dropped.
- Don't try to also "fix" Pandoc's `# Heading\n    foo\n`
  behavior in the same change. It's a separate Pandoc-side bug
  (the strict gate is over-strict there too) but resolving it
  needs care around BQ lazy continuation. The fixture pins the
  current behavior so a future fix is a clean diff.
- Don't widen the renderer's loose-list logic. The four "Lists"
  failures (#312, #315, #317, #320, #326) are NOT in this fix's
  blast radius — verified by re-running the report.

### Suggested next targets, ranked

1. **Empty list item closes the list when followed by blank line
   (#280)** — `-\n\n  foo\n` should produce
   `<ul><li></li></ul><p>foo</p>`. Parser-shape gap.
2. **Loose-vs-tight semantic gaps (#315, #320, #326)** —
   `is_loose_list` in the test renderer overshoots: it returns
   true on any PARAGRAPH descendant (including paragraphs inside
   nested blockquotes). Per CommonMark §5.3, looseness is about
   blank-line separation, and the parser already encodes the
   PLAIN-vs-PARAGRAPH distinction at the *direct* child level.
   Likely renderer-only fix: change `descendants()` to
   `children()` in `is_loose_list`'s PARAGRAPH check, then verify
   the BLANK_LINE-between-items rule still fires for #326.
   Unblocks at least 3 examples (the loose-vs-tight detection
   also affects #312/#317 indirectly).
3. **List with non-uniform marker indentation (#312)** —
   `- a\n - b\n  - c\n   - d\n    - e\n` should keep all five at
   the same list level (last "- e" is lazy continuation of "d"
   per CommonMark indent rules). Currently splits at "- e"
   because the parser interprets 4-space indent as starting a
   nested list. Parser-shape gap; touches list-marker indent
   tracking.
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
   blockquote failures (after this session's #236, #252 wins).
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
