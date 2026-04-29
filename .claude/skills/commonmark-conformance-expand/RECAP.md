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

## Latest session — 2026-04-29 (g)

**Pass count: 556 → 561 / 652 (86.0%, +5)**

Targeted prior recap's #4 (nested-bracket label, #559), #5 (emphasis
+ shortcut ref, #564), and several Links-section reference-resolution
edge cases (#554/#558/#568). #564 turned out to be blocked by an
unrelated parser losslessness bug (input `*[foo*]` produces extra `*]`
bytes), so it was left out. #537 came along for free with the
code-span precedence fix being extended to the reference-link path.

### Root cause: collect_label_text stripped emphasis markers

The CommonMark renderer's `collect_label_text` filtered the LINK_TEXT
node down to TEXT/ESCAPED_CHAR/WHITESPACE/NEWLINE tokens. For
`[*foo* bar][]` (#554) and `[*foo* bar]` (#558), the LINK_TEXT
contains an EMPHASIS subtree whose `*` markers were dropped, yielding
the lookup key `foo bar` — but the ref-def has the raw key
`*foo* bar`. CommonMark §6.4 specifies matching against the raw label
bytes, so switched `collect_label_text` to `node.text().to_string()`.
That preserves the byte-perfect label and still produces the same
result for the existing `[bar][foo\!]` case (#545) since
ESCAPED_CHAR's raw text already contains the backslash.

### Root cause: reference-link bracket scanner didn't skip code spans

The prior session unlocked #342/#525 for *inline* links by routing
the bracket scanner through `find_link_close_bracket` (which calls
`try_parse_code_span`). `try_parse_reference_link` still had its own
naive scanner that closed on the first `]`, so `[foo`][ref]`` (#537)
was misparsed: the `]` inside the matched code span terminated the
link text early. Switched the reference-link path to use
`find_link_close_bracket` too — same dialect-agnostic precedence
rule (verified with pandoc), so a single shared helper.

### Root cause: reference-link parser bailed when followed by `(...)`

`try_parse_reference_link` used to early-return `None` whenever `]`
was followed by `(` or `{`, on the assumption that those payloads
belong to inline links / bracketed spans. That's correct for the
common case, but it *also* shut the door on CommonMark §6 spec
example #568: `[foo](not a link)` with `[foo]: /url1`. The strict
inline-link form rejects the destination (spaces in a bare URL), and
the spec wants `[foo]` to fall back to a shortcut reference, leaving
`(not a link)` as literal text. Added an `inline_link_attempted`
flag — only fall through to shortcut after `(...)` when the caller
has already tried the inline-link parser (i.e., `inline_links`
extension is on). Kept the early-return for `{...}` since bracketed
spans are an extension, not a parser-policy fallback.

### Root cause: shortcut-fallback rendering didn't recurse into nested links

`render_link`'s shortcut-unresolved branch dumped the LINK's full
raw text via `decode_backslash_escapes`. For `[[*foo* bar]]` (#559)
the parser produces an *outer* LINK whose LINK_TEXT contains an
*inner* LINK (CommonMark allows the inner to resolve as a shortcut
even though links can't truly nest). The outer label `[*foo* bar]`
doesn't match any ref-def, so the renderer was emitting the raw
nested form as text. Changed the shortcut fallback to emit `[` +
`render_inlines(text_node)` + `]` — so any inner resolved LINK,
emphasis, or escaped char inside the failed bracket pair still
renders correctly. This also brings the simpler unresolved
shortcut cases like `[*foo*]` (no def) closer to spec; the prior
fallback emitted raw `[*foo*]` text without expanding the `*`s.

### Files changed

- **Renderer (renderer gap)**:
  - `crates/panache-parser/tests/commonmark/html_renderer.rs`:
    - `collect_label_text` now returns `node.text().to_string()`
      (raw bytes including emphasis markers).
    - Shortcut-unresolved fallback recurses into LINK_TEXT children
      with `[` … `]` wrappers.
- **Parser (parser-shape gap + dialect-divergence reuse)**:
  - `crates/panache-parser/src/parser/inlines/links.rs`:
    - `try_parse_reference_link` now uses `find_link_close_bracket`
      so a `]` inside a code span no longer terminates the link
      text (#537 — same root cause as #342/#525).
    - Added `inline_link_attempted` parameter; when true, allow
      shortcut fall-through past `(...)` payloads (#568). When
      false, preserve the prior bail-out so disabling
      `inline_links` doesn't silently let `[text](url)` become a
      shortcut + literal text (existing
      `inline_links_disabled_keeps_inline_link_literal` regression).
    - `core.rs` call site passes `config.extensions.inline_links`.
    - Updated all unit-test call sites; replaced the old
      "rejects inline-link shape" test with a pair: one for the
      shortcut-disabled path (still rejects) and one for the
      shortcut + inline-link-attempted fallthrough (now accepts).
- **Snapshots**:
  - `parser_cst_inline_link_dest_strict_commonmark.snap` — the
    new shape contains a shortcut LINK node + literal `(/my uri)`
    text instead of one TEXT span. Lossless and round-trips.
- **Formatter golden case**:
  - `tests/fixtures/cases/inline_link_dest_strict_commonmark/expected.md`
    — updated from `\[link\](/my uri)` to `[link](/my uri)`. The
    formatter no longer needs defensive bracket escapes because the
    shortcut LINK preserves the structure; the new output is also
    shorter and idempotent.
- **New parser fixture + snapshot**:
  - `reference_link_code_span_precedence` — pins the CST for
    `[foo`][ref]`` so a `]` inside a code span doesn't terminate
    the link text. Single dialect-agnostic fixture.
- **Allowlist additions** (Links section, all under the same
  trailing block since #537/#554/#558/#559/#568 are all Links).

### Don't redo

- Don't go back to filtering token kinds in `collect_label_text`.
  CommonMark §6.4 matches against the raw label bytes; emphasis
  markers and other inline punctuation must survive. ESCAPED_CHAR's
  raw text already includes the backslash, so the simple
  `node.text()` does the right thing for #545-style cases.
- Don't strip the `inline_link_attempted` parameter back out of
  `try_parse_reference_link`. The naïve "always allow shortcut
  past `(...)`" version regresses
  `inline_links_disabled_keeps_inline_link_literal` — silently
  turning `[text](url)` into a shortcut LINK when the inline-link
  extension is off. The flag carries the upstream context that the
  function alone can't recover.
- Don't reintroduce the simple bracket scanner inside
  `try_parse_reference_link`. Code-span precedence (CommonMark §6)
  is dialect-agnostic and applies to every link form, so the
  shared `find_link_close_bracket` helper must own the scan.
- Don't widen the renderer's shortcut-fallback brackets to
  `escape_html(node.text())`. The recursive `[` + `render_inlines`
  + `]` form is what makes #559 (and analogous unresolved shortcut
  cases with inline content) emit correct HTML.
- #564 (`*[foo*]` with `[foo*]: /url`) is **blocked by a parser
  losslessness bug**: under CommonMark dialect, `*[foo*]\n` parses
  with extra `*]` bytes appended to the EMPHASIS span. Reproduce:
  `printf '[foo*]: /url\n\n*[foo*]\n' | cargo run -- debug format
  --checks losslessness` — diff shows `+*[foo*]*]`. Fix the
  losslessness bug first; #564 will likely fall out, possibly
  alongside #534 (`*[foo*][ref]`).

### Suggested next targets, ranked

1. **Parser losslessness bug for `*[foo*]`** — inserts extra `*]`
   bytes under CommonMark dialect. Found while triaging #564 (still
   failing). Blocks #564 and likely #534. High value: fixing the
   losslessness violation also unblocks the conformance work.
2. **Multi-line setext heading + losslessness bug (#81, #82, #95,
   #115)** — carried forward. Big change.
3. **Empty list item closes the list when followed by blank line
   (#280)** — `-\n\n  foo\n` should produce
   `<ul><li></li></ul><p>foo</p>`.
4. **Multi-empty-marker with subsequent indented content (#278)** —
   partially comes along once #280 is solved.
5. **Reference link followed by another bracket pair (#569, #571)**
   — `[foo][bar][baz]` requires the CMark "left-bracket scanner"
   stack model. Larger fix.
6. **Nested LINKs in link text (#518, #519, #520, #532, #533)** —
   CommonMark §6.4 forbids real nesting; outer must un-link itself
   when inner resolves. Same scanner-stack work as #569.
7. **HTML-tag/autolink interaction with link brackets (#524, #526,
   #536, #538)** — bracket scanner must skip past raw HTML and
   autolinks too.
8. **Tabs (#2, #5, #6, #7)** — column-aware tab expansion for
   indented-code inside containers. Substantial.
9. **Block quotes lazy-continuation (#235, #236, #251)** — lazy
   continuation must not extend a list or code block inside a
   blockquote.
10. **Fence inside blockquote inside list item (#321)**.
11. **Loose-vs-tight nested loose lists (#312, #326)** — renderer's
    loose-detection gap for nested lists.
12. **Lazy / nested marker continuation (#298, #299)**.
13. **Multi-block content in `1.     code` items (#273, #274)**.
14. **Setext-in-list-item (#300)**.
15. **Emphasis and strong emphasis (47 fails)** — flanking-rule
    edge cases. #352 (`a*"foo"*`), #354 (`*$*alpha`),
    #366/#367/#368/#369, #372–376 (underscore intra-word). Need
    proper CommonMark flanking-rule gating; current emphasis
    parser leans on Pandoc's looser semantics.
16. **Ref-def dialect divergence #201** — `[foo]: <bar>(baz)`. Low
    priority.

