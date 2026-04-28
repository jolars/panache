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

## Latest session — 2026-04-28 (i)

**Pass count: 458 → 475 / 652 (72.9%, +17)**

Targeted HTML blocks (the prior recap's #1 target), which had 17 failing
examples driven by three shared root causes in §4.6 detection. Implementing
all three under a single `Dialect::CommonMark` gate cleared 15 of those (the
remaining 2 — #148, #174 — are renderer-shape gaps, not detection gaps),
plus two incidental unlocks via type-7 inline raw HTML on a line by itself
(#21 in Backslash escapes, #31 in Entity references).

### Targets and root causes

- **HTML blocks #151, #152, #155, #161, #166, #167, #186, #188, #190, #191
  (10)** — type 6 end condition: under CommonMark, type-6 blocks end at a
  blank line, not at a matching `</tag>`. The legacy implementation treated
  any block-tag start as "open until matching close tag", so `<div>...</div>`
  on consecutive lines was always one block and `<div>\n\n*md*\n\n</div>`
  was wrapped as one HTML block instead of HTML / paragraph / HTML. Fix:
  `closed_by_blank_line` flag on the `BlockTag` variant, set true when the
  dialect is CommonMark and the tag is non-verbatim.
- **HTML blocks #151, #165 (2)** — type 6 also accepts a *closing* tag
  (`</div>`, `</ins>`) at the start of the block line. The legacy tag-name
  extractor rejected `</...` outright. Fix: `extract_block_tag_name` now
  accepts the closing form when `accept_closing` (CommonMark) is true.
- **HTML blocks #162, #163, #164, #165, #166, #167, #184 (7)** — type 7
  detection was missing entirely: a complete open or close tag whose name
  isn't in the type-1 verbatim list, on a line by itself, opens a block
  that ends at a blank line. Reused the existing inline-HTML
  `parse_open_tag` / `parse_close_tag` recognizers (made `pub(crate)`)
  and added a tail-only-whitespace check, plus a "tag-name is not pre /
  script / style / textarea" guard so the type-1 starts above keep
  priority. Type 7 is the only HTML-block type that **cannot** interrupt
  a paragraph — wired in the dispatcher by returning `None` (instead of
  `YesCanInterrupt`) when no blank line precedes a Type7 candidate.
- **Backslash escapes #21, Entity references #31 (2 incidental)** —
  `<a href="\*">` and `<a href="&ouml;...">` are now type-7 HTML blocks
  on a line by itself. Renderer emits the verbatim bytes (no escape /
  entity decoding, same as inline raw HTML) and that matches spec
  expected output. No special handling needed.

  Verified with pandoc: `pandoc -f commonmark -t native` and
  `pandoc -f markdown -t native` disagree on every probed case (Pandoc-
  markdown turns `<DIV>` into a fenced div, sees `</div>` as a paragraph,
  treats `<a href="x">` as inline raw HTML in a paragraph), so this is a
  **dialect divergence**. The new behavior is gated on
  `config.dialect == Dialect::CommonMark`; Pandoc keeps the legacy
  matching-close-tag semantics for type 6 and rejects type-7 starts.

### Files changed

- **Parser (dialect divergence + missing feature)**:
  - `crates/panache-parser/src/parser/blocks/html_blocks.rs` —
    `HtmlBlockType` gained a `closed_by_blank_line` field on `BlockTag`
    and a new `Type7` variant. `try_parse_html_block_start` now takes
    `is_commonmark: bool`; under CommonMark it accepts closing-tag
    starts for type 6 and matches type 7 via the inline-HTML
    open/close recognizers. `parse_html_block` short-circuits the
    matching-close-tag scan for blank-line-terminated types and
    instead breaks at the first blank line (after stripping any
    blockquote markers, so types 6/7 inside `> ` end correctly when
    the blockquote ends).
  - `crates/panache-parser/src/parser/inlines/inline_html.rs` —
    `parse_open_tag` and `parse_close_tag` are now `pub(crate)` so
    the HTML-block recognizer can reuse the §6.6 grammar without
    duplicating attribute/quote logic.
  - `crates/panache-parser/src/parser/block_dispatcher.rs` — passes
    the new `is_commonmark` flag, and gates Type7 to "cannot
    interrupt a paragraph" by returning `None` when there's no blank
    line before.
  - `crates/panache-parser/src/parser/utils/continuation.rs` — pass
    the dialect flag to `try_parse_html_block_start` from the
    paragraph-continuation check (this is the lookahead that decides
    whether a paragraph line is interruptible).
- **Parser fixtures (paired CST snapshots via insta)**:
  - `crates/panache-parser/tests/fixtures/cases/html_block_commonmark_type6_type7_{commonmark,pandoc}/`
    pin the divergent CST shape for an input that exercises type-6
    blank-line termination, `</tag>` start, type-7 open/close, and
    type-6 with internal markdown-paragraph between blank lines.
    Registered in `crates/panache-parser/tests/golden_parser_cases.rs`.
- **Formatter golden**:
  - `tests/fixtures/cases/html_block_commonmark_type6_type7/` —
    `flavor = "commonmark"`. Same input as the parser fixture; expected
    output is byte-identical (HTML blocks emit verbatim and round-trip
    cleanly under the new shape). Different block sequence than
    Pandoc-default (where `</div>` and `<a>` collapse into paragraphs
    with inline raw HTML and reflowed lines), so a dedicated CommonMark
    case is needed. Registered in `tests/golden_cases.rs`.
- **Allowlist additions**: `tests/commonmark/allowlist.txt`
  - Backslash escapes: +#21
  - Entity and numeric character references: +#31
  - HTML blocks: +#151, +#152, +#155, +#161, +#162, +#163, +#164, +#165,
    +#166, +#167, +#184, +#186, +#188, +#190, +#191

### Don't redo

- The CommonMark dialect gate is load-bearing for both call sites
  (`block_dispatcher` and `continuation`). The continuation lookahead has
  to use the same dialect flag, otherwise paragraphs swallow lines that
  should start a fresh HTML block. Don't drop one and keep the other.
- `closed_by_blank_line` is set on the `BlockTag` payload at *detection*
  time and read inside `parse_html_block`. Don't compute it from
  `is_verbatim` alone — the same `is_verbatim: false` BlockTag means
  different things under CommonMark vs Pandoc, which is exactly what the
  flag captures.
- The Type 7 recognizer rejects when the tag name is in the type-1
  verbatim set (`pre`/`script`/`style`/`textarea`). Don't drop that guard
  — without it, an opening `<pre/>` (self-closing) on a line by itself
  would be detected as type 7 and end at a blank line, but the *type-1*
  start condition for `<pre` (substring match, no full-tag requirement)
  would normally have grabbed it first. Keeping the guard means a
  surprising self-closing pre falls through to type 7 cleanly.
- The blank-line check inside `parse_html_block` strips blockquote
  markers via `count_blockquote_markers` before testing the inner
  content. This is intentional: a `>` line with only whitespace after
  the marker is a blank line *inside the blockquote* and ends a type-6
  block there, not at the outer document level.
- #148 (`<table><tr><td>\n<pre>\n**Hello**,\n\n_world_.\n</pre>\n...`)
  is **not** unlocked by this work. Expected output requires the
  paragraph that starts at `_world_.` to be wrapped `<p>...</p>` and
  the `</pre></p></td></tr></table>` tail to be emitted as raw text
  from the same HTML block. Our renderer wraps the paragraph correctly
  but the HTML block boundary detection consumes the trailing `</pre>`
  + table close lines as part of the block, and the renderer emits
  them on separate lines. This is a renderer-shape interaction with
  type 1 (`<pre>`) embedded inside type 6 (`<table>`); the spec's
  expected HTML is genuinely odd and worth tackling as its own micro-
  task, not bundled into the bulk fix.
- #174 (`> <div>\n> foo\n\nbar`) is a **renderer gap**, not a detection
  gap. The parser correctly closes the type-6 HTML block at the blank
  line after stripping blockquote markers. But the rendered HTML block
  text still contains the literal `> ` prefix on each content line
  because the renderer prints `node.text()` verbatim. Same shape as the
  blockquote-prefix bug from the prior session's #128 follow-up. Fix
  lives in `render_html_block` in
  `crates/panache-parser/tests/commonmark/html_renderer.rs`: strip the
  blockquote marker prefix when rendering an HTML block whose
  containing context includes one. See #128 don't-redo from session (g).

### Suggested next targets, ranked

1. **Lists (5/21) + List items (17/31)** — biggest absolute pass-rate
   gap (52 fails). Shares root causes with thematic-break #57, #60, #61
   (HR-interrupts-list) and blockquote #234, #246 (HR-interrupts-
   blockquote). #115 (Indented code blocks) chains on the same
   paragraph-vs-block-interruption refactor. High leverage but bigger
   surgery than HTML blocks.
2. **Emphasis and strong emphasis (85/47)** — largest remaining absolute
   failure count; flanking-rule and autolink-precedence edge cases
   (#480, #481 are autolink-vs-emphasis precedence).
3. **#128 (Block quotes / fenced code in blockquote)** + **#174 (HTML
   block in blockquote)** — both are the same renderer-prefix-stripping
   gap. One small fix in `html_renderer.rs` should unlock both, plus
   any similar containment cases.
4. **Link reference definitions (15/12)** — #194 (label with `\]`),
   #195 (multiline title), #196, #198 (multiline destination), #208,
   #213, #216, #217: the LRD parser currently captures the whole line(s)
   as raw TEXT instead of structured nodes. Bigger refactor.
5. **Tabs (6/5)** — #2, #4, #5, #6, #7 all need tab→space expansion
   with column alignment in indented-code, list, and blockquote
   contexts. Spec §2.2. One shared fix across most of these.
6. **Setext heading multi-line content (#81, #82, #95)** + **#115
   (`# Heading\n    foo`)** — paragraph parser refactor: needs to
   recognize indented-code-after-heading without a blank line, plus
   retroactive setext conversion of accumulated paragraph lines. These
   are interrelated.
7. **#148 (HTML blocks)** — the `<table>` containing `<pre>` containing
   markdown paragraph; renderer interaction. One-off, low priority.
