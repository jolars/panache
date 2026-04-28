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

## Latest session — 2026-04-28 (d)

**Pass count: 412 → 415 / 652 (63.7%, +3)**

Small dialect-divergence fix: under `Dialect::CommonMark`, allow ANY ASCII
punctuation as a backslash escape (CommonMark §2.4). Previously the parser
only recognized `BASE_ESCAPABLE = "\\\`*_{}[]()>#+-.!|~"` unless
`all_symbols_escapable` was on. Pandoc default has `all_symbols_escapable`
on, so the bug was masked there; CommonMark intentionally leaves it off
(it widens to whitespace, a Pandoc-only construct).

### Targets and root causes

- Picked Backslash escapes (9/4) as a small section with a likely shared
  root cause. Probing #12 showed the parser emitting some chars as
  `ESCAPED_CHAR` and others as `TEXT` under CommonMark — the literal-escape
  set was too narrow.
- Single-line parser fix in `inlines/core.rs` widened the escapable set
  for CommonMark dialect to `ch.is_ascii_punctuation()`. Verified against
  `pandoc -f commonmark -t native` and `pandoc -f markdown -t native` ---
  both decode all of `\!\"\#...\~` to the literal punctuation string.

### Files changed

- Parser fix:
  `crates/panache-parser/src/parser/inlines/core.rs` --- in the
  `try_parse_escape` ladder, widen `EscapeType::Literal` gating with
  `|| (config.dialect == Dialect::CommonMark && ch.is_ascii_punctuation())`.
  Comment updated to explain why this is dialect-gated rather than tied to
  the `all_symbols_escapable` extension (which also widens to whitespace,
  outside the CommonMark spec).
- Paired parser fixtures (CST snapshots via insta):
  `crates/panache-parser/tests/fixtures/cases/all_punctuation_escapes_{commonmark,pandoc}/`
  --- both pin every ASCII-punct escape as `ESCAPED_CHAR`. The Pandoc
  fixture is structurally identical (default Pandoc has
  `all_symbols_escapable` on) but documents the parity and guards
  against accidentally narrowing the Pandoc set. Registered in
  `tests/golden_parser_cases.rs`.
- Allowlist additions: `tests/commonmark/allowlist.txt`
  - Backslash escapes: +#12, +#14
  - Raw HTML: +#632 (`<a href="\"">` --- the `\"` escape made the
    raw-HTML attribute string break, so we now correctly fall through to
    a paragraph with HTML-escaped angle brackets, matching the spec).

No formatter golden case needed: the formatter round-trips
`\<punct>` byte-for-byte under both flavors (verified manually with
`cargo run -- format` and `cargo run -- format --config flavor=commonmark`),
so the new ESCAPED_CHAR shape doesn't change formatted output.

### Don't redo

- The `all_symbols_escapable` extension is intentionally OFF for
  CommonMark --- it widens beyond ASCII punctuation (to whitespace) which
  is wrong for the spec. The fix path is `Dialect::CommonMark`-gated,
  not extension-gated.
- The Pandoc fixture's CST happens to match CommonMark's because
  default Pandoc has `all_symbols_escapable` on. Don't try to make them
  diverge --- the divergence only shows under `markdown_strict`-style
  Pandoc, which we don't need to fixture today.
- Backslash escapes #21 (`<a href="/bar\/)">` expected as raw HTML) was
  NOT unlocked --- raw inline HTML recognition is its own (larger) gap;
  see "Raw HTML" priority below.
- Code spans #342 (`[not a \`link](/foo\`)`) and #344, plus Hard line
  breaks #642 / #643, all need raw-HTML / link-precedence work --- not
  in scope this session.

### Suggested next targets, ranked

1. **Lists (5/21) + List items (17/31)** --- still the biggest
   low-pass-rate sections; loose/tight detection,
   thematic-break-terminates-list, lazy continuation. HR-interrupts-list
   work (carried forward) is likely the biggest single unlock.
2. **Raw HTML (7/13) + HTML blocks (24/20)** --- inline raw HTML
   recognition is what's blocking #21, #344, #642, #643 plus the bulk of
   "HTML blocks" failures. Probably one shared parser fix.
3. **Emphasis and strong emphasis (85/47)** --- largest remaining
   absolute failure count; flanking-rule edge cases +
   intraword-underscore.
4. **Setext headings (19/8)** --- what's left needs multi-line-content
   support (#95) or blockquote/list interaction (#92, 94, 99, 101).
   Defer until lists are unblocked.
5. **Indented code blocks (8/4)** --- failures #108, #109 are list
   interaction; #111 is blank-line preservation in indented code; #115
   is heading + indented-code interleaving. All need other work first.
6. Hard line breaks #642, #643 --- multi-line raw inline HTML
   preservation.
7. Known link follow-ups still in `blocked.txt`: #488, #490, #493,
   #508, #523, #546.
8. Entity reference follow-ups: #31 (HTML-block detection gap) and #41
   (entity refs as structural quotes around link title).

### Carried-forward notes

Remaining setext / thematic-break gaps not unlocked yet:

- **#57, #60, #61** --- thematic break interrupting a list. List
  continuation/termination logic is more involved than the paragraph
  case.
- **#92, #94, #99, #101** --- setext underline / thematic break
  following `> Foo` or `- Foo`. Dialect divergence; needs paired
  fixtures plus blockquote/list interruption work.
- **#95** --- multi-line setext content (`Foo\nBar\n---`). Bigger
  refactor of the paragraph parser to retroactively convert
  accumulated lines into a setext heading.
