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

## Latest session — 2026-04-29 (h)

**Pass count: 561 → 563 / 652 (86.3%, +2)**

Targeted the parser losslessness bug carried forward from session
(g). The fix unlocked the two examples it was blocking — #564
(`*[foo*]` with `[foo*]: /url`) and #534 (`*[foo*][ref]` with
`[ref]: /uri`). Both produced extra `*]` (or `*][bar]`) bytes in the
CST and could not parse losslessly.

### Root cause: emphasis closer scanner only skipped inline links

`parse_until_closer_with_nested_two` and `_one` (in
`crates/panache-parser/src/parser/inlines/core.rs`) had a `[`-skip
that called `try_parse_inline_link` — but only the inline
`[text](url)` form. For shortcut/full reference brackets like
`[foo*]`, the inline link parser returns None, so the scanner
advanced byte-by-byte and picked the `*` *inside* the bracket label
as the emphasis closer.

That caused two failures simultaneously:

1. **Losslessness violation.** Once the closer was at the inner `*`
   (position 19 for `*[foo*]\n`), `parse_inline_range_nested` was
   called for the emphasis content [15, 19). Inside, the *unbounded*
   `try_parse_reference_link` greedily consumed the whole `[foo*]`
   bracket pair (6 bytes), going past the assumed closer. The
   emphasis still emitted its own closing `*`, so the CST gained 2
   extra bytes (`*]`). Same shape for #534, just longer
   (`*][bar]`).
2. **Wrong semantic.** Even after fixing losslessness alone, the
   link would be lost entirely — the bytes would parse as
   `<em>[foo</em>]` instead of `* + LINK[foo*]`, contradicting
   *both* dialects (verified with `pandoc -f commonmark` and
   `-f markdown` — both produce `* + Link("foo*", /url)` when the
   ref is defined).

The fix: in both closer scanners, after the existing inline-link
skip, also call `try_parse_reference_link` (gated on
`config.extensions.reference_links`) and skip past it if it
matches. With brackets treated as opaque during the closer scan,
emphasis correctly fails (no `*` closer found between the leading
`*` and the paragraph end), and the outer parser then resolves the
bracket pair as a real LINK.

### Files changed

- **Parser (parser-shape gap)**:
  - `crates/panache-parser/src/parser/inlines/core.rs`: added a
    reference-link skip block in *both*
    `parse_until_closer_with_nested_two` (right after the
    inline-link skip near line ~693) and
    `parse_until_closer_with_nested_one` (matching block near line
    ~904). Gated on `config.extensions.reference_links`; passes
    `shortcut_reference_links` and `inline_links` through to
    `try_parse_reference_link` so the existing
    `inline_links_disabled_keeps_inline_link_literal` invariant
    still holds.
- **New parser fixture + snapshot**:
  - `emphasis_skips_shortcut_reference_link` — pins the CST for
    `[foo*]: /url\n\n*[foo*]\n`. Single dialect-agnostic fixture
    (verified: pandoc commonmark and markdown produce identical
    native AST). The CST shows the `*` as a literal TEXT and
    `[foo*]` as a shortcut LINK at the paragraph top level.
- **Allowlist additions**: #534, #564 (Links section, appended to
  the trailing block).

### Don't redo

- Don't drop the `config.extensions.reference_links` gate on the
  new skip. If the extension is off, `try_parse_reference_link`
  isn't called from `parse_inline_range_impl` either, so skipping
  here would silently let bracket pairs swallow emphasis closers
  in flavors that don't enable reference links at all.
- Don't try to fix the no-ref-def case to match CommonMark spec
  (`*[foo*]` alone → `<em>[foo</em>]`). With our fix the parser
  treats `[foo*]` as a shortcut LINK whether or not it resolves,
  and the renderer's shortcut-fallback emits literal `[foo*]` —
  net result `*[foo*]` literal, which matches Pandoc but diverges
  from CommonMark spec for *unresolved* refs. Proper CommonMark
  behavior here requires the delimiter-stack algorithm (process
  brackets first, deactivate emphasis delimiters between matched
  brackets, fall back if no link resolves) — see suggested next
  targets about scanner-stack work. Don't try to bolt that onto
  the current Pandoc-style closer scanner; it'll regress Pandoc.
- Don't only add the skip to one of the two closer scanners. Both
  `_two` (`**...**`) and `_one` (`*...*`) need it — see e.g.
  `**[foo*]bar**` shaped inputs.

### Suggested next targets, ranked

1. **Multi-line setext heading + losslessness bug (#81, #82, #95,
   #115)** — carried forward. Big change.
2. **Empty list item closes the list when followed by blank line
   (#280)** — `-\n\n  foo\n` should produce
   `<ul><li></li></ul><p>foo</p>`.
3. **Multi-empty-marker with subsequent indented content (#278)** —
   partially comes along once #280 is solved.
4. **Reference link followed by another bracket pair (#569, #571)**
   — `[foo][bar][baz]` requires the CMark "left-bracket scanner"
   stack model. Larger fix.
5. **Nested LINKs in link text (#518, #519, #520, #532, #533)** —
   CommonMark §6.4 forbids real nesting; outer must un-link itself
   when inner resolves. Same scanner-stack work as #569.
6. **HTML-tag/autolink interaction with link brackets (#524, #526,
   #536, #538)** — bracket scanner must skip past raw HTML and
   autolinks too.
7. **Tabs (#2, #5, #6, #7)** — column-aware tab expansion for
   indented-code inside containers. Substantial.
8. **Block quotes lazy-continuation (#235, #236, #251)** — lazy
   continuation must not extend a list or code block inside a
   blockquote.
9. **Fence inside blockquote inside list item (#321)**.
10. **Loose-vs-tight nested loose lists (#312, #326)** — renderer's
    loose-detection gap for nested lists.
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
