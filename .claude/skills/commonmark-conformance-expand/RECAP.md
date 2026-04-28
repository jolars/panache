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

## Latest session — 2026-04-28 (e)

**Pass count: 415 → 417 / 652 (64.0%, +2)**

Two small parser-shape gaps in the heading parser. Neither is a dialect
divergence — pandoc agrees with the CommonMark spec for both flavors.

### Targets and root causes

- ATX #79 (`### ###`): closing fence detection in `emit_atx_heading`
  required at least one non-hash byte before the trailing hashes. With
  empty content the post-marker space was already consumed, so
  `before_hashes` was empty and the closing `###` was emitted as TEXT
  inside `HEADING_CONTENT`. Fix: also accept the closing fence when
  `before_hashes` is empty AND we already consumed `spaces_after_marker`.
- Setext #83 (trailing `Foo\n=`): `try_parse_setext_heading` required the
  underline trimmed length to be ≥3. Per CommonMark §4.3 (and verified
  with `pandoc -f commonmark` and `pandoc -f markdown`), a setext
  underline can be any non-zero length. Fix: replace `< 3` with
  `is_empty()`. The `chars().all(c == first_char)` check still rejects
  things like `--- -` (which is a thematic break, not an underline).

### Files changed

- Parser fix:
  `crates/panache-parser/src/parser/blocks/headings.rs`
  - `emit_atx_heading`: widen the closing-fence guard to also accept
    `before_hashes.is_empty() && spaces_after_marker_count > 0`.
  - `try_parse_setext_heading`: change `len() < 3` to `is_empty()` and
    update the doc comment. Replaced `test_setext_minimum_three_chars`
    with `test_setext_any_underline_length` covering 1/2/3-char
    underlines.
- Parser fixtures (CST snapshots via insta):
  - `tests/fixtures/cases/atx_empty_with_closing_fence/` (no flavor;
    behavior is identical in both dialects).
  - `tests/fixtures/cases/setext_short_underline_{commonmark,pandoc}/`
    --- paired fixtures pinning the same CST shape under both flavors.
    The two snapshots are byte-identical, which documents that this
    is a parser-shape gap, not a dialect divergence.
  - All three registered in `tests/golden_parser_cases.rs`.
- Allowlist additions: `tests/commonmark/allowlist.txt`
  - ATX headings: +#79
  - Setext headings: +#83

No formatter golden case needed: the formatter normalizes `Foo\n=` and
`Foo\n-` to ATX (`# Foo` / `## Foo`) under both dialects, so behavior is
structurally identical to the existing Pandoc path. Manual round-trip
check confirmed idempotency.

### Don't redo

- `try_parse_setext_heading` is the only place the underline-length
  check lived; there is no parallel paragraph-side check to update.
- The `chars().all()` underline-shape check still correctly rejects
  `--- -`, `= =`, etc. — `Foo\n--- -` (#88) still parses as paragraph +
  HR, which is what the spec wants.
- Pandoc-default also accepts single-char underlines; do not gate this
  on `Dialect::CommonMark`. The earlier paired-Pandoc fixture
  documents the parity.
- ATX #79's heading-content is now empty *after* the
  `spaces_after_marker` is consumed; the renderer's empty-content path
  already emits `<h3></h3>`, no renderer change required.

### Suggested next targets, ranked

1. **Lists (5/21) + List items (17/31)** --- biggest absolute pass-rate
   gap. HR-interrupts-list and thematic-break-terminates-list likely
   share root cause with #57, #60, #61 and #234, #246. Probably the
   biggest single unlock.
2. **Raw HTML (7/13) + HTML blocks (24/20)** --- inline raw HTML
   recognition is what's blocking #21, #344, #597, #606, #609, #642,
   #643 plus the bulk of HTML blocks failures. One shared parser fix.
3. **Emphasis and strong emphasis (85/47)** --- largest remaining
   absolute failure count; flanking-rule edge cases.
4. **Tabs (6/5)** --- #2, #4, #5, #6, #7 all need tab→space expansion
   (with column alignment) interacting with indented code, list, and
   blockquote contexts. Spec §2.2. Probably one shared fix.
5. **Block quotes (18/7)** --- #234 (`> foo\n---`) and #246 (`> aaa\n***`)
   want HR-interrupts-blockquote (sibling of HR-interrupts-list above);
   #237 wants fenced-code-inside-blockquote terminate-on-no-marker;
   #251 (`>>> foo\n> bar\n>>baz`) wants varying blockquote depth lazy.
6. **Setext headings remaining** --- #81, #82, #95 still need multi-line
   setext content (paragraph-line accumulation that retroactively
   converts to setext heading). #92, #94, #99, #101 need
   blockquote/list interaction.
7. Hard line breaks #642, #643 --- multi-line raw inline HTML
   preservation (covered by "Raw HTML" above).
8. Known link follow-ups still in `blocked.txt`: #488, #490, #493,
   #508, #523, #546.
9. Entity reference follow-ups: #31 (HTML-block detection gap) and #41
   (entity refs as structural quotes around link title).

### Carried-forward notes

Remaining setext / thematic-break gaps not unlocked yet:

- **#57, #60, #61** --- thematic break interrupting a list. List
  continuation/termination logic is more involved than the paragraph
  case.
- **#92, #94, #99, #101** --- setext underline / thematic break
  following `> Foo` or `- Foo`. Needs blockquote/list interruption
  work; verify dialect parity before fixing (pandoc behavior may
  differ).
- **#81, #82, #95** --- multi-line setext content (`Foo\nBar\n---`).
  Bigger refactor of the paragraph parser to retroactively convert
  accumulated lines into a setext heading.
