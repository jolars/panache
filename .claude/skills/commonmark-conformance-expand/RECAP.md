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

## Latest session — 2026-04-28 (b)

**Pass count: 407 → 412 / 652 (62.4% → 63.2%)**

### Targets and root causes

- **Thematic breaks 14 → 15 / 19** and **Setext headings 16 → 18 / 27** ---
  three coupled CommonMark dialect rules around thematic breaks:
  1. CommonMark §4.1 --- a thematic break can interrupt a paragraph (no leading
     blank line required). Pandoc-markdown disagrees (`pandoc -f      markdown`
     keeps `Foo\n***\nbar` as one paragraph), so this is a `Dialect::CommonMark`
     branch on `HorizontalRuleParser`, not an extension toggle.
  2. CommonMark §4.3 --- a setext heading text line cannot itself be a valid
     thematic break (`***\n---` is two HRs, not `<h2>***</h2>`). Same dialect
     divergence: pandoc-markdown happily makes this a setext h2.
  3. List markers must yield to thematic break recognition mid-paragraph in
     CommonMark, so `Foo\n* * *\nbar` triggers HR rather than the `*`-marker
     list parser.
- **Indent guard backfill** --- `try_parse_horizontal_rule` was previously
  whitespace-trimming without a leading-indent check. Once HR became willing to
  interrupt paragraphs, examples 49 (`Foo\n    ***`) and 87 (`Foo\n    ---`)
  regressed because 4-space indented lines were being promoted to HR.
  CommonMark/pandoc agree these are paragraph continuation; added the standard
  `leading < 4` guard.

### Files changed

- Parser (dialect branch):
  `crates/panache-parser/src/parser/block_dispatcher.rs` ---
  `HorizontalRuleParser::detect_prepared` returns `YesCanInterrupt` and ignores
  `has_blank_before` when `Dialect::CommonMark`;
  `SetextHeadingParser::detect_prepared` rejects when the text line is a
  thematic break under `Dialect::CommonMark`; `ListParser::detect_prepared`
  yields to thematic breaks unconditionally under `Dialect::CommonMark`.
- Parser (general fix):
  `crates/panache-parser/src/parser/blocks/horizontal_rules.rs` ---
  `try_parse_horizontal_rule` rejects 4+ leading spaces. Applies to both
  dialects (verified against pandoc).
- Parser fixtures: paired commonmark/pandoc cases under
  `crates/panache-parser/tests/fixtures/cases/` ---
  `thematic_break_interrupts_paragraph_{commonmark,pandoc}` and
  `setext_text_thematic_break_{commonmark,pandoc}`. Registered in
  `golden_parser_cases.rs`; CST snapshots committed.
- Formatter fixtures: CommonMark-only top-level cases ---
  `tests/fixtures/cases/thematic_break_interrupts_paragraph_commonmark/` and
  `tests/fixtures/cases/setext_text_thematic_break_commonmark/` (no paired
  Pandoc case; existing fixtures cover that). Registered in
  `tests/golden_cases.rs`. The thematic-break case caught a real formatter
  idempotency bug (HR with no blank line before it round-tripped as a setext
  h2), now fixed.
- Formatter fix: `crates/panache-formatter/src/formatter/core.rs` --- emit a
  blank line before HR when the previous output ended with a non-blank line.
  Without this, `Foo\n--------\n` round-trips as `## Foo` (setext h2). General
  fix, not dialect-gated.
- Fixture loader fix: `tests/golden_cases.rs` now re-derives `extensions` and
  `formatter_extensions` from `flavor` when the fixture's `panache.toml` doesn't
  declare them explicitly. Without this, a fixture with `flavor = "commonmark"`
  would silently fall back to Pandoc-default extensions.
- Serde alias: `crates/panache-parser/src/options.rs` --- `Flavor::CommonMark`
  now accepts `"commonmark"` as a serde alias to the kebab-case `"common-mark"`.
  Both work in fixture / config tomls.
- Allowlist: +5 entries --- Thematic breaks 43, 58; Setext headings 88, 98, 105.

### Don't redo

- The HR/setext/list dialect branches and the leading-indent guard on
  `try_parse_horizontal_rule`. All in place.
- `***\n---` precedence (HR over setext) under CommonMark.
- `Foo\n* * *\nbar` precedence (HR over list) under CommonMark.

### Remaining setext / thematic-break gaps (not unlocked this session)

- **#57, #60, #61** --- thematic break interrupting a list. List
  continuation/termination logic is more involved than the paragraph case; needs
  follow-up.
- **#92, #94, #99, #101** --- setext underline / thematic break following
  `> Foo` or `- Foo`. Pandoc-markdown sees `> Foo\n---` as a setext h2 (`> Foo`
  text), CommonMark sees blockquote + HR. Dialect divergence, needs paired
  fixtures plus blockquote/list interruption work.
- **#95** --- multi-line setext content (`Foo\nBar\n---`). Requires the
  paragraph parser to retroactively convert accumulated lines into a setext
  heading when it hits a valid underline. Bigger refactor.

### Suggested next targets, ranked

0. **Audit existing CommonMark/GFM fixtures.** The fixture-loader bug fixed this
   session means any prior fixture that set only `flavor = "..."` (without an
   explicit `[extensions]` table) was silently running with Pandoc-default
   extensions --- so its committed `expected.md` may be frozen to the
   Pandoc-flavored output, not the declared flavor's. Procedure: grep
   `tests/fixtures/cases/*/panache.toml` and
   `crates/panache-parser/tests/fixtures/cases/*/parser-options.toml` for
   `flavor = "commonmark"` / `"common-mark"` / `"gfm"`; for each, run the case
   and diff the rendered output against the committed `expected.md` / CST
   snapshot. Update only intentional regenerations; investigate others
   case-by-case. The parser-crate loader (`load_test_parser_options` in
   `golden_parser_cases.rs`) already resolves extensions from flavor correctly,
   but the audit should still confirm.
1. **Lists (5/21) + List items (17/31)** --- biggest low-pass-rate sections;
   likely shared root causes (loose/tight detection,
   thematic-break-terminates-list, lazy continuation). Now that HR interrupts
   paragraphs cleanly, the list-termination branch should be tractable next.
2. **Emphasis and strong emphasis (85/47)** --- largest remaining absolute
   failure count; flanking-rule edge cases + intraword-underscore.
3. **HTML blocks (24/20) + Raw HTML (6/14)** --- probably one shared fix in
   HTML-tag/comment recognition.
4. **Setext headings (18/9)** --- what's left needs multi-line-content support
   (#95) or blockquote/list interaction (#92, 94, 99, 101). Defer until lists
   are unblocked.
5. Hard line breaks #642, #643 --- multi-line raw inline HTML preservation.
6. Known link follow-ups still in `blocked.txt`: #488, #490, #493, #508, #523,
   #546.
7. Entity reference follow-ups: #31 (HTML-block detection gap) and #41 (entity
   refs as structural quotes around link title).
