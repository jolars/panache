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

## Latest session — 2026-04-28 (h)

**Pass count: 437 → 458 / 652 (70.2%, +21)**

Targeted the inline raw HTML §6.6 grammar (the prior recap's #1 target), which
was a missing feature in the parser entirely — `<a href="x">` and friends fell
through to plain TEXT, escaped as `&lt;a&gt;` by the renderer. Implemented the
recognizer once; it cleared all of Raw HTML (7 → 20), the two Hard line breaks
that were really "no break inside HTML attribute" (642, 643), and incidental
unlocks in HTML blocks (168, 175, 187), Code spans (344), and Links (491, 494)
where raw HTML appeared inside other constructs.

### Targets and root causes

- **Raw HTML #613–#617, #623, #625–#631 (13)**, **Hard line breaks #642, #643
  (2)**, **HTML blocks #168, #175, #187 (3)**, **Code spans #344 (1)**, **Links
  #491, #494 (2)** — single shared root cause: the parser had no recognizer for
  CommonMark §6.6 *inline* raw HTML (only block-level `<div>` etc. via the
  HTML-block detector, and only `<span>...</span>` via the Pandoc native-spans
  extension). Added the recognizer; both CommonMark and Pandoc-markdown ship
  with `raw_html = true`, so the same code path serves both flavors and pandoc
  parity was confirmed (`pandoc -f commonmark -t native` and `pandoc -f markdown
  -t native` agree on every probed case). Not a dialect divergence.

  The §6.6 grammar covers six discriminated forms — open tag, close tag, comment
  (with the two degenerate `<!-->` and `<!--->` shapes), processing
  instruction, declaration, CDATA — each terminated either by `>` or by a
  delimiter pair. Quoted attribute values may contain anything except the
  closing quote and may span lines; whitespace between tag-name and attributes
  may include up to one line ending. The recognizer is a byte-level dispatch
  in priority order and bails to `None` on any unterminated form, so plausible
  but unclosed `<a foo="...` correctly falls through to plain text.

  CST shape: one `INLINE_HTML` node per matched span containing a single
  `INLINE_HTML_CONTENT` token with the verbatim bytes. Backslashes and entity
  references inside the span are *not* decoded — the dispatcher consumes the
  whole span before the standard escape/entity passes can fire on those bytes,
  which is what makes #21-style cases (`<a href="\*">`) and #630 (`&ouml;` in
  attr value) come out right.

### Files changed

- **Parser (missing feature)**:
  - `crates/panache-parser/src/parser/inlines/inline_html.rs` (new) —
    `try_parse_inline_html(text)` returns the byte length of a matched span,
    or `None`. Handles the six §6.6 forms in priority order. Has a focused
    set of unit tests for the recognizer (open/close/empty-element tags,
    multi-line quoted attribute values, comments incl. `<!-->` /`<!--->`,
    PI/CDATA/declaration, illegal-name and unterminated rejection).
  - `crates/panache-parser/src/parser/inlines.rs` — module wired in.
  - `crates/panache-parser/src/parser/inlines/core.rs` — dispatcher inserted
    after autolink + native-span checks (both more specific) and before the
    fallthrough to TEXT, gated on `config.extensions.raw_html` so it stays
    off when a flavor explicitly disables raw HTML.
  - `crates/panache-parser/src/syntax/kind.rs` — added `INLINE_HTML` and
    `INLINE_HTML_CONTENT` kinds. CST shape: one node per span, single child
    token with the verbatim bytes.
- **Renderer (test-only)**:
  `crates/panache-parser/tests/commonmark/html_renderer.rs`
  - `render_inline_node` matches `INLINE_HTML` and emits its
    `INLINE_HTML_CONTENT` text verbatim (no entity decoding, no
    backslash-escape processing, no HTML escaping) — but routes the
    characters through `protect_entity_whitespace`, the same private-use
    placeholder mechanism `decode_entities` already uses, so the spaces and
    tabs *inside* an HTML span survive `strip_paragraph_line_indent`.
    Without that, #642 (`<a href="foo  \nbar">`) loses the two-space
    trailing run inside the attribute value.
- **Parser fixture (CST snapshot via insta)**:
  `crates/panache-parser/tests/fixtures/cases/inline_html_basic_commonmark/`
  pins one paragraph per HTML form (open/close, comment incl. degenerate
  forms, PI, CDATA, declaration, attribute with backslash + entity).
  `parser-options.toml` sets `flavor = "commonmark"`. Registered in
  `crates/panache-parser/tests/golden_parser_cases.rs`. Single fixture is
  enough — pandoc-markdown produces the same CST under default extensions
  (verified by hand), and the rules say not to duplicate when both dialects
  agree.
- **Allowlist additions**: `tests/commonmark/allowlist.txt`
  - HTML blocks: +#168, +#175, +#187
  - Code spans: +#344
  - Links: +#491, +#494
  - Raw HTML: +#613, +#614, +#615, +#616, +#617, +#623, +#625, +#626, +#627,
    +#628, +#629, +#630, +#631
  - Hard line breaks: +#642, +#643

### Don't redo

- Don't reorder the inline `<` dispatchers. The current order
  (autolink → Pandoc native-span → inline raw HTML) is load-bearing:
  autolink is the most specific (`<scheme:...>`), Pandoc native-span only
  fires when `extensions.native_spans` is on (Pandoc-only) and consumes the
  full `<span>...</span>` pair, and inline raw HTML matches a single tag.
  Moving inline raw HTML earlier would steal `<span>` from native-span
  under Pandoc and regress the SPAN AST.
- Don't widen `try_parse_inline_html` to consume past a missing terminator
  ("be liberal"). The whole recognizer hinges on returning `None` when the
  span doesn't actually close, so the dispatcher falls through to TEXT and
  the bytes survive as plain text — that's what makes `<33>`, unterminated
  `<a foo="...`, illegal attribute names, etc. all render as escaped text
  instead of bogus HTML.
- Don't drop the `protect_entity_whitespace` pass on the rendered
  `INLINE_HTML_CONTENT`. `strip_paragraph_line_indent` runs after inline
  rendering and trims trailing whitespace before each `\n` in the inner
  paragraph buffer; without the placeholder substitution, attribute values
  that contain trailing-space-before-LF lose those spaces.
- The §6.6 recognizer is a *missing feature* that improved Pandoc-markdown
  too — both flavors had the same TEXT-only fallback. There's no flavor or
  dialect branch on this code path, only the existing `raw_html` extension
  flag, which is the right gate.
- #21 (Backslash escapes section) is *still failing*. Input is a single
  bare `<a href="/bar\/)">` line — under CommonMark this is a *block-level*
  HTML block (type 7), expected output omits the `<p>` wrapper entirely.
  Our parser still wraps it in `<p>...</p>`. That's an HTML-block detector
  gap, not an inline raw HTML gap. Belongs with the HTML blocks bucket.

### Suggested next targets, ranked

1. **HTML blocks (27/17)** — biggest remaining bucket where raw HTML matters.
   The §7 *block*-level HTML grammar still has gaps: missing block-type-7
   detection (#21), and the "blank-line-separated HTML block with markdown
   between" pattern (#151, #188, #191 from the prior session). Worth
   probing each cluster — they may share a fix.
2. **Lists (5/21) + List items (17/31)** — biggest absolute pass-rate gap and
   shares root cause with thematic-break #57, #60, #61 and blockquote #234,
   #246 (HR-interrupts-list / -blockquote). #115 (Indented code blocks)
   chained on the same paragraph-vs-block-interruption refactor.
3. **#128 (Block quotes / fenced code in blockquote)** — single-example
   follow-up to the prior session's dialect gate; fix in
   `parse_fenced_code_block` to strip blockquote markers from content lines
   (see prior recap's "Don't redo").
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

--------------------------------------------------------------------------------

## Earlier session — 2026-04-28 (g)

**Pass count: 427 → 437 / 652 (67.0%, +10)**

Targeted Fenced code blocks (the recap's #6 follow-up) plus low-risk renderer
fixes for blank lines inside indented code blocks. The fenced-code dialect gate
was the biggest leverage win (8 examples in Fenced code + 1 cascading unlock in
Block quotes from the same root cause).

### Targets and root causes (g)

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
