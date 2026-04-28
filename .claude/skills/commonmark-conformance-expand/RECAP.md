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

## Latest session — 2026-04-28 (g)

**Pass count: 427 → 437 / 652 (67.0%, +10)**

Targeted Fenced code blocks (the recap's #6 follow-up) plus low-risk renderer
fixes for blank lines inside indented code blocks. The fenced-code dialect gate
was the biggest leverage win (8 examples in Fenced code + 1 cascading unlock in
Block quotes from the same root cause).

### Targets and root causes

- **#111, #112 (Indented code blocks)**: blank lines inside an indented code
  block should have *up to 4* leading spaces stripped — the rest preserved.
  The renderer's old branch only handled `strip_prefix("    ")` and fell
  through to `content.push_str(line)` for shorter lines, leaking partial
  whitespace. Renderer-only: replace with a byte-counting strip capped at 4
  spaces, plus a "blank line shorter than the indent collapses to just `\n`"
  rule. The fix is symmetric for content lines (CommonMark §4.4 doesn't
  distinguish "blank-ish" vs "content" lines for stripping — every line gets
  up to 4 spaces removed). #112 was already in the allowlist; the original
  attempt regressed it before being corrected.
- **#126, #127, #131, #132, #133, #137, #139, #140 (Fenced code blocks)** and
  **#237 (Block quotes)** — share two root causes:
  - **Dialect divergence**: CommonMark fenced code blocks always interrupt
    paragraphs and run to end-of-document if the closing fence is missing
    (spec §4.5). Pandoc-markdown requires a closer (else falls back to a
    paragraph). Verified with `pandoc -f commonmark` vs `pandoc -f markdown`.
    Fix: in `block_dispatcher.rs::FencedCodeBlockParser::detect_prepared`,
    short-circuit on `!has_matching_closer` only when the dialect is *not*
    CommonMark; under CommonMark, also force `YesCanInterrupt` so a bare
    fence interrupts paragraphs without needing the existing transcript-like
    special cases. Unlocked: #126 (`\`\`\`` alone), #127 (longer opener,
    inner closer too short), #128 partially, #137, #139, #140, plus #237
    (`> \`\`\`\nfoo\n\`\`\`` — inner blockquote sees an unclosed fence and
    must close it at the blockquote boundary).
  - **Renderer-only — fence-opener indent stripping**: per CommonMark §4.5,
    if the opening fence is indented, content lines have an *equivalent*
    amount of leading whitespace removed (capped at the opener's indent —
    extra is preserved). The CST already captures the opener indent as a
    leading WHITESPACE token on the CODE_BLOCK before CODE_FENCE_OPEN, so
    this is a renderer fix. Added `fenced_opener_indent()` helper and a
    per-line strip in `code_block_content()`. Unlocked: #131, #132, #133.

### Files changed

- **Renderer (test-only)**:
  `crates/panache-parser/tests/commonmark/html_renderer.rs`
  - `code_block_content` (indented branch): cap-strip up to 4 leading spaces
    per line; collapse short whitespace-only lines to bare `\n`.
  - `code_block_content` (fenced branch): strip opener-indent worth of
    leading spaces per content line.
  - Added `fenced_opener_indent(node) -> usize`.
- **Parser (dialect gate)**:
  `crates/panache-parser/src/parser/block_dispatcher.rs`
  - `FencedCodeBlockParser::detect_prepared` now reads
    `ctx.config.dialect == Dialect::CommonMark` and:
    - allows `!has_matching_closer` (no early return) under CommonMark
    - sets `YesCanInterrupt` for bare fences under CommonMark
- **Parser fixtures (paired CST snapshots via insta)**:
  - `crates/panache-parser/tests/fixtures/cases/fenced_code_unclosed_{commonmark,pandoc}/`
    pin the divergent shape for `text\n\`\`\`\ncode\n`. CommonMark snapshot
    shows PARAGRAPH + CODE_BLOCK; Pandoc snapshot shows a single PARAGRAPH.
  - Registered in `crates/panache-parser/tests/golden_parser_cases.rs`.
- **Formatter golden**:
  - `tests/fixtures/cases/fenced_code_unclosed_commonmark/` —
    `flavor = "commonmark"`. Input `text\n\`\`\`\ncode\n`; expected
    `text\n\n\`\`\`\ncode\n` (paragraph then unclosed fenced block, formatter
    preserves the missing closer). Different block sequence than Pandoc, where
    the same input formats as a single paragraph with backslash-escaped
    backticks.
  - Registered in `tests/golden_cases.rs`. No paired Pandoc formatter case
    (existing top-level fixtures already cover Pandoc-default).
- **Allowlist additions**: `tests/commonmark/allowlist.txt`
  - Indented code blocks: +#111
  - Fenced code blocks: +#126, +#127, +#131, +#132, +#133, +#137, +#139, +#140
  - Block quotes: +#237

### Don't redo

- The renderer is the right home for both the fenced-opener indent stripping
  *and* the indented-code blank-line stripping. CSTs already pin the shape;
  moving these into the parser would entangle whitespace policy with
  tokenization.
- The CommonMark dialect gate in `FencedCodeBlockParser::detect_prepared` is
  load-bearing for #126/#127/#140 — don't simplify it back to "interrupt only
  with a matching closer" without preserving the `common_mark_dialect` branch
  that *also* allows unclosed fences. The two branches are independent and
  both needed.
- For #128 (`> \`\`\`\n> aaa\n\nbbb`): we now correctly *detect* a fenced code
  block inside the blockquote, but the rendered content includes the inner
  `> ` prefix from the second line (`<pre><code>&gt; aaa`). The fenced-code
  parser strips the surrounding blockquote markers from line 1 (the opener)
  but not from subsequent content lines when the fence is unclosed and the
  blockquote ends at a blank line. Fix lives in
  `parse_fenced_code_block`'s content-collection loop (around
  `crates/panache-parser/src/parser/blocks/code_blocks.rs:1140`) — strip
  `bq_depth` markers from content lines too, not just for the closer check.
- The "passing-by-accident under prior defaults" exception in
  `.claude/rules/commonmark.md` did not come into play this session — every
  unlock is a real correctness improvement, no allowlist removals.

### Suggested next targets, ranked

1. **HTML blocks (24/20)** — bulk of failures fall into two patterns:
   blank-line-separated HTML blocks with markdown between (#151, #188, #191)
   and HTML blocks not detected (#161, #162, #174). Both need parser work.
   Inline raw HTML (the §6.6 grammar) is a sibling target: would unlock
   Raw HTML #623, #625–631, plus Hard line breaks #642/#643 and Backslash
   escapes #21 — all currently failing because the parser doesn't recognize
   `<a href="...">` etc. as inline raw HTML and the renderer escapes the
   characters. Implementing the §6.6 inline HTML recognizer is a meaningful
   feature but bounded; the failing cases share one grammar.
2. **Lists (5/21) + List items (17/31)** — biggest absolute pass-rate gap and
   shares root cause with thematic-break #57, #60, #61 and blockquote #234,
   #246 (HR-interrupts-list / -blockquote). #115 (Indented code blocks)
   chained on the same paragraph-vs-block-interruption refactor.
3. **#128 (Block quotes / fenced code in blockquote)** — single-example
   follow-up to this session's dialect gate; fix in `parse_fenced_code_block`
   to strip blockquote markers from content lines (see "Don't redo" above).
4. **Emphasis and strong emphasis (85/47)** — largest remaining absolute
   failure count; flanking-rule and autolink-precedence edge cases (#480,
   #481 are autolink-vs-emphasis precedence).
5. **Link reference definitions (15/12)** — #194 (label with `\]`), #195
   (multiline title), #196, #198 (multiline destination), #208, #213, #216,
   #217: the LRD parser currently captures the whole line(s) as raw TEXT
   instead of structured nodes. Bigger refactor.
6. **Tabs (6/5)** — #2, #4, #5, #6, #7 all need tab→space expansion with
   column alignment in indented-code, list, and blockquote contexts. Spec
   §2.2. One shared fix across most of these.
7. **Setext heading multi-line content (#81, #82, #95)** + **#115
   (`# Heading\n    foo`)** — paragraph parser refactor: needs to recognize
   indented-code-after-heading without a blank line, plus retroactive setext
   conversion of accumulated paragraph lines. These are interrelated.
8. **Hard line breaks #642, #643** + **Backslash escapes #21** — all blocked
   on inline raw HTML (see target 1).

### Carried-forward notes

Remaining setext / thematic-break gaps not unlocked yet (carried over from
prior session):

- **#57, #60, #61** — thematic break interrupting a list. List
  continuation/termination logic is more involved than the paragraph case.
- **#92, #94, #99, #101** — setext underline / thematic break following
  `> Foo` or `- Foo`. Needs blockquote/list interruption work; verify
  dialect parity before fixing.
- **#81, #82, #95** — multi-line setext content (`Foo\nBar\n---`). Bigger
  refactor of the paragraph parser to retroactively convert accumulated
  lines into a setext heading.

Renderer-only quick check before deeper parser work: `report.txt` plus a
throwaway `probe_examples` test in `commonmark.rs` (printing
markdown/expected/got/match for a small list of failing numbers) is the
fastest way to triage whether a failure is a 1-line renderer fix or a real
parser-shape gap. Don't forget to reset its example numbers before finishing.
