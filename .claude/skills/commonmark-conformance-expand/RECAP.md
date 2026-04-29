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

## Latest session — 2026-04-29 (f)

**Pass count: 551 → 556 / 652 (85.3%, +5)**

Targeted prior recap's #4 (code-span vs link precedence, #342/#525),
#5 (angle-bracket URL with parens, #499), and #6 (NBSP in URL, #507).
All three fixes were independent and crisp. #492 came along with the
angle-bracket fix (same root cause: paren-tracking didn't know about
angle-bracket destinations).

### Single root cause: paren tracker ignored angle-bracket state

`try_parse_inline_link` and `try_parse_inline_image` both scanned the
destination for the closing `)` with a paren-depth counter that
balanced `(` and `)` regardless of whether they sat inside `<...>`.
For `[link](<foo(and(bar)>)` (#499) and `[a](<b)c>)` (#492), the
parser counted parens inside the angle-bracket payload and either
matched the wrong `)` or never found one. Verified with pandoc that
both dialects agree on this shape — no `Dialect` gate. Added an
`in_angle` flag to both scanners that toggles on `<`/`>` and
suppresses `(`/`)` accounting while inside.

### Single root cause: bracket scanner didn't yield to higher-precedence code spans

CommonMark §6 mandates code spans bind tighter than links. For
`[not a `link](/foo`)` (#342) and `[foo`](/uri)`` (#525), the link
parser found a `]` *inside* what is actually a code span and committed
to the link. Replaced the inline-bracket-scan loops with a helper
`find_link_close_bracket` that calls `try_parse_code_span` whenever it
hits a backtick: if the backticks open a valid code span, the entire
span (with any attribute block) is skipped, so a `]` inside it can no
longer terminate the link. Same helper is reused by both
`try_parse_inline_link` and `try_parse_inline_image`. Also extracted
`find_dest_close_paren` to share the destination scanner between the
two. Pandoc agrees on both examples — dialect-agnostic.

### Single root cause: renderer split URL on Unicode whitespace

`split_dest_and_title` used `c.is_whitespace()` to find the split
between destination and title. CommonMark §6.6 only treats *ASCII*
whitespace (space/tab/LF/VT/FF/CR) as the separator; NBSP (U+00A0) is
part of the destination and must be percent-encoded. For
`[link](/url\u{a0}"title")` (#507) the renderer was producing
`<a href="/url" title="title">` instead of `<a href="/url%C2%A0%22title%22">`.
Tightened the match to the explicit ASCII-whitespace set. The parser
side was already correct — LINK_DEST contained the full
`/url\u{a0}"title"` blob.

### Files changed

- **Parser (parser-shape gap)**:
  - `crates/panache-parser/src/parser/inlines/links.rs`: extracted
    two helpers (`find_link_close_bracket`, `find_dest_close_paren`,
    plus a `step` UTF-8 byte-stepping helper). The bracket helper
    calls `try_parse_code_span` to skip past code-span ranges so a
    `]` inside a code span no longer terminates the link's text. The
    paren helper tracks `in_angle` so unbalanced parens inside
    `<...>` destinations are accepted. Both `try_parse_inline_link`
    and `try_parse_inline_image` now delegate their bracket and
    destination scans to these helpers — duplicated logic gone.
- **Renderer (renderer gap)**:
  - `crates/panache-parser/tests/commonmark/html_renderer.rs`:
    `split_dest_and_title` now splits only on the CommonMark
    ASCII-whitespace set (space/tab/LF/VT/FF/CR). Comment cites
    #507.
- **New parser fixtures + snapshots**:
  - `inline_link_dest_angle_brackets_with_parens` — pins LINK CST
    shape for `[link](<foo(and(bar)>)`.
  - `inline_link_code_span_precedence` — pins paragraph CST for both
    `[not a `link](/foo`)` and `[foo`](/uri)`` (TEXT + INLINE_CODE +
    TEXT, no LINK node). Both dialect-agnostic; single fixture each
    (no Pandoc paired case).
- **Allowlist additions** (Code spans section: +342; Links section:
  +492, +499, +507, +525).

### Don't redo

- Don't reintroduce paren-balanced scanning for `(`/`)` without
  honoring `in_angle`. Inside `<...>` destinations, parens are part
  of the URL payload — counting them as nesting yields off-by-one
  matches and #492/#499-style misparses.
- Don't fold the new code-span check into the link parser by hand —
  call `try_parse_code_span`. It already handles attribute trailers
  and unmatched runs, and reusing it keeps the precedence logic in
  one place. If you need a leaner check later, extract a "code-span
  bounds only" helper from `try_parse_code_span`; don't reimplement.
- Don't use `c.is_whitespace()` for CommonMark URL/title splitting.
  Spec-mandated set is ASCII-only (space/tab/LF/VT/FF/CR). Pulling
  in Unicode whitespace turns NBSP / etc. into separators and breaks
  URL percent-encoding.
- Don't add formatter golden cases for these fixes. The CommonMark
  block sequence is identical to the Pandoc path (parser change is
  dialect-agnostic, formatter output unchanged); per the rules,
  CommonMark formatter cases are only needed when the new behavior
  produces a *different* block sequence than Pandoc.

### Suggested next targets, ranked

1. **Multi-line setext heading + losslessness bug (#81, #82, #95, #115)** —
   carried forward. Under `Dialect::CommonMark`, a paragraph of >1 line
   followed by a setext underline yields a broken CST (paragraph text
   in reverse order). Big change; plan a session for it.
2. **Empty list item closes the list when followed by blank line (#280)** —
   `-\n\n  foo\n` should produce `<ul><li></li></ul><p>foo</p>`. Currently
   the parser keeps the trailing paragraph inside the list item.
3. **Multi-empty-marker with subsequent indented content (#278)** —
   chaotic parse; partially comes along once #280 is solved.
4. **Nested-bracket label resolution (#559)** — `[[*foo* bar]]` outer
   brackets must be literal; inner `[*foo* bar]` resolves as a shortcut
   reference. Currently the parser produces a malformed nested LINK
   structure (LINK_TEXT_END missing) so the renderer falls through to
   raw text. Probably means the inline parser needs a "longest-bracket
   wins / outermost-fail-rolls-to-inner" rule for shortcut refs.
5. **Emphasis interleaving with shortcut refs (#564)** — `*[foo*]`
   where `[foo*]: /url` is defined. Expected `*<a href="/url">foo*</a>`,
   we get `<em>...foo*</em>]` — the `*` should be literal because the
   emphasis can't be closed inside the link text. Related to bracket
   parsing precedence; might land alongside #559.
6. **Tabs (#2, #5, #6, #7)** — column-aware tab expansion needed for
   indented-code inside containers. Substantial.
7. **Block quotes lazy-continuation (#235, #236, #251)** — lazy
   continuation must not extend a list or code block inside a blockquote.
8. **Fence inside blockquote inside list item (#321)** — list-item
   continuation can be interrupted by a fence at content column.
9. **Loose-vs-tight nested loose lists (#312, #326)** — renderer's
   loose-detection gap for nested lists.
10. **Lazy / nested marker continuation (#298, #299)** — `- - foo` and
    `1. - 2. foo` should produce nested list-on-same-line.
11. **Multi-block content in `1.     code` items (#273, #274)**.
12. **Setext-in-list-item (#300)**.
13. **Emphasis and strong emphasis (47 fails)** — flanking-rule and
    autolink-precedence edge cases.
14. **Ref-def dialect divergence #201** — `[foo]: <bar>(baz)`. Low priority.

