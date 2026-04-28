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

## Latest session — 2026-04-28 (f)

**Pass count: 417 → 427 / 652 (65.5%, +10)**

Mix of small renderer-only fixes plus one CommonMark-strict link-destination
parser gate. The link-dest fix was the highest-leverage win (4 examples in
Links + 1 in Entity refs from a single root cause).

### Targets and root causes

- **#24 (Backslash escapes)**: info string `foo\+bar` should produce class
  `foo+bar` per spec ("Backslash escapes ... work in fenced code block info
  strings"). Renderer-only: `code_block_language()` was returning the raw
  text. Fix: pipe through `decode_backslash_escapes` after `decode_entities`.
- **#597 (Autolinks, MAILTO uppercase)**: `<MAILTO:FOO@BAR.BAZ>` was rendered
  as `mailto:MAILTO:FOO@BAR.BAZ` because `target.starts_with("mailto:")` was
  case-sensitive and we fell into the email branch. Renderer-only: replace
  with a proper `has_uri_scheme()` check (alpha + alphanum/+./- 2-32 chars,
  then `:`) — per spec §6.5 a URI autolink starts with a scheme.
- **#603 (Autolinks, backslash in href)**: `<https://example.com/\[\>` should
  encode the `[` as `%5B` in the href. Renderer-only: removed `[` and `]`
  from `is_url_safe()` set. Spec examples consistently encode brackets in
  rendered URLs (cf. #7954, #8112, #8857). No regressions in passing tests.
- **#130, #144 (Fenced code blocks, empty content)**: empty fenced code
  blocks rendered with a stray `\n` between the tags. Renderer-only:
  `code_block_content()` unconditionally appended `\n` if content didn't
  end in one — including when content was empty. Guard the push with
  `!content.is_empty()`. #130 (`\`\`\`\n\`\`\``) was unlocked as a freebie.
- **#41, #488, #490, #493, #508 (Entity refs / Links — link destination)**:
  CommonMark §6.4 link destinations may not contain spaces or ASCII control
  chars in the bare form, must be `<...>`-bracketed otherwise; what follows
  must be either empty or a properly-delimited title. Pandoc-markdown is
  more permissive (URL-encodes spaces, accepts everything between parens).
  Verified the divergence with `pandoc -f commonmark` vs `pandoc -f markdown`.
  Fix: dialect-gated. `try_parse_inline_link` now takes a `strict_dest: bool`
  flag; callers in `inlines/core.rs` pass
  `config.dialect == Dialect::CommonMark`. New `dest_and_title_ok_commonmark`
  helper validates the destination + optional title structure; if it fails,
  parsing falls through and the source stays as plain text (which is what
  CommonMark renders for these examples).

### Files changed

- **Renderer (test-only)**:
  `crates/panache-parser/tests/commonmark/html_renderer.rs`
  - `code_block_language`: chain `decode_backslash_escapes(decode_entities(...))`.
  - `code_block_content`: only push trailing `\n` if content nonempty.
  - `render_autolink`: switch from `starts_with("mailto:")` to
    `has_uri_scheme()`.
  - `is_url_safe`: drop `[` and `]` from the safe set.
  - Added `has_uri_scheme(s: &str) -> bool`.
- **Parser (dialect gate)**:
  `crates/panache-parser/src/parser/inlines/links.rs`
  - `try_parse_inline_link` gained a `strict_dest: bool` parameter.
  - Added `dest_and_title_ok_commonmark()` validator (bracketed + bare
    destination forms, optional title in `"..."` / `'...'` / `(...)`,
    only whitespace allowed before/after).
  - Updated all in-file unit tests to pass `false` (Pandoc-permissive).
  `crates/panache-parser/src/parser/inlines/core.rs`
  - Three callers (`parse_until_closer_with_nested_two`,
    `parse_until_closer_with_nested_one`, `parse_inline_range_impl`) now
    pass `config.dialect == Dialect::CommonMark`.
- **Parser fixtures (CST snapshots via insta)**:
  - `tests/fixtures/cases/inline_link_dest_strict_{commonmark,pandoc}/`
    pin the divergent CST shapes for `[link](/my uri)`. CommonMark snapshot
    shows plain TEXT; Pandoc snapshot shows the LINK structure.
  - Registered in `tests/golden_parser_cases.rs`.
- **Formatter golden**:
  - `tests/fixtures/cases/inline_link_dest_strict_commonmark/` —
    `flavor = "commonmark"`, `[link](/my uri)` → `\[link\](/my uri)`. The
    formatter escapes the brackets so re-parsing (under CommonMark) keeps
    them literal — different block sequence than Pandoc, where the same
    input parses as a LINK and round-trips with literal brackets.
  - Registered in `tests/golden_cases.rs`. No paired Pandoc formatter
    case (existing top-level fixtures already cover Pandoc-default).
- **Allowlist additions**: `tests/commonmark/allowlist.txt`
  - Backslash escapes: +#24
  - Entity and numeric character references: +#41
  - Fenced code blocks: +#130, +#144
  - Autolinks: +#597, +#603
  - Links: +#488, +#490, +#493, +#508

### Don't redo

- The renderer is the right home for the `decode_backslash_escapes` and
  `[`/`]` URL-encoding fixes — these are spec-mandated rendering steps,
  not parser CST shapes. Don't move them into the parser.
- `has_uri_scheme()` already enforces the 2–32-char rule; that means
  `<m:abc>` (#609) still fails because the parser at
  `inlines/links.rs::try_parse_autolink` accepts ANY content with `:`.
  Tightening the autolink parser to require a scheme is a separate
  parser-shape change (and is dialect-divergent — pandoc-markdown
  accepts `<m:abc>` as raw HTML).
- The CommonMark link-dest validator does **not** cover the bracketed
  form's edge cases like `<b)c>` (#494) — `try_parse_inline_link`'s
  paren-balance loop terminates on the first `)` regardless of angle
  brackets, so `dest_content` for that case is `<b` and the validator
  accepts it but the rendered link text is wrong. A real fix needs
  bracket-aware destination scanning in the inline-link parser, not
  more validator logic.
- Don't drop the `strict_dest` parameter back to a bare `bool` — pass
  it as `config.dialect == Dialect::CommonMark` from callers so the
  Pandoc path stays untouched.

### Suggested next targets, ranked

1. **Lists (5/21) + List items (17/31)** — biggest absolute pass-rate
   gap and likely shares root cause with thematic-break #57, #60, #61
   and blockquote #234, #246 (HR-interrupts-list / -blockquote).
2. **HTML blocks (24/20)** — bulk of failures fall into two patterns:
   blank-line-separated HTML blocks with markdown between (#151, #188,
   #191) and HTML blocks not detected (#161, #162, #174). Both need
   parser work.
3. **Emphasis and strong emphasis (85/47)** — largest remaining absolute
   failure count; flanking-rule and autolink-precedence edge cases
   (#480, #481 are autolink-vs-emphasis precedence).
4. **Link reference definitions (15/12)** — #194 (label with `\]`),
   #195 (multiline title), #196, #198 (multiline destination); the
   reference def parser captures the whole line as raw TEXT instead of
   structured nodes. Bigger refactor.
5. **Tabs (6/5)** — #2, #4, #5, #6, #7 all need tab→space expansion
   with column alignment in indented-code, list, and blockquote
   contexts. Spec §2.2. Probably one shared fix.
6. **Fenced code blocks remaining (20/9)** — #126, #127, #128 want
   "unclosed fence = code block to EOF" (CommonMark) vs Pandoc's
   "unclosed = paragraph". Dialect-gated parser work in
   `block_dispatcher.rs::FencedCodeBlockParser` (`has_matching_closer`
   guard).
7. **Block quotes (18/7)** — #234 (`> foo\n---`) and #246 (`> aaa\n***`)
   need HR-to-interrupt-blockquote logic. Verify dialect parity.
8. Setext heading multi-line content (#81, #82, #95) — paragraph
   parser refactor.
9. Hard line breaks #642, #643 — multi-line raw inline HTML
   (covered by "HTML blocks" / "Raw HTML").
10. Known link/autolink follow-ups: #523, #546, #606 (`<foo\+@…>` —
    autolink parser too lax), #609 (`<m:abc>` — dialect divergence).

### Carried-forward notes

Remaining setext / thematic-break gaps not unlocked yet:

- **#57, #60, #61** — thematic break interrupting a list. List
  continuation/termination logic is more involved than the paragraph
  case.
- **#92, #94, #99, #101** — setext underline / thematic break
  following `> Foo` or `- Foo`. Needs blockquote/list interruption
  work; verify dialect parity before fixing (pandoc behavior may
  differ).
- **#81, #82, #95** — multi-line setext content (`Foo\nBar\n---`).
  Bigger refactor of the paragraph parser to retroactively convert
  accumulated lines into a setext heading.

Renderer-only quick check before deeper parser work: `report.txt` plus a
throwaway `probe_examples` test in `commonmark.rs` (printing
markdown/expected/got/match for a small list of failing numbers) is the
fastest way to triage whether a failure is a 1-line renderer fix or a
real parser-shape gap.
