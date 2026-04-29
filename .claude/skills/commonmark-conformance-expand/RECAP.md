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

## Latest session — 2026-04-29 (i)

**Pass count: 563 → 566 / 652 (86.8%, +3)**

Took the carried-forward "Multi-line setext heading + losslessness
bug" target. The fix unlocked #81, #82, #95 (multi-line setext
forms). #115 was on the same target list but turned out to be a
different (orthogonal) bug — indented-code-after-ATX-heading is
mis-buffered as paragraph text, and that bug existed both before
and after this fix.

### Root cause: setext detected at the text line ignored prior buffered paragraph

For `Foo\nBar\n---\n` under CommonMark:

- At line "Foo", dispatcher checks `[Foo, Bar]` → not setext;
  buffer "Foo\n" in paragraph.
- At line "Bar", dispatcher checks `[Bar, ---]` → setext! With
  `blank_before_header=false` (CommonMark default), detection
  proceeded. The dispatcher fall-through called `parse_prepared`,
  which emitted a fresh `HEADING { text="Bar", underline="---" }`
  while the paragraph container was still open with "Foo\n"
  buffered.
- Result: `HEADING` emitted *inside* the open `PARAGRAPH`, then
  buffered "Foo" got flushed *after* the heading, scrambling byte
  order: the green tree text came out `Bar\n---\nFoo\n` for input
  `Foo\nBar\n---\n` (losslessness violation).

Pandoc-markdown does *not* form a multi-line setext here (verified
with `pandoc -f commonmark` vs `-f markdown` — they disagree), so
the fix is **dialect-gated** on `Dialect::CommonMark`.

### Fix: deferred PARAGRAPH wrapper via rowan checkpoint + fold path

Two moving parts:

1. **Refactor paragraph emission to use `rowan::Checkpoint`.**
   `start_paragraph_if_needed` no longer calls `start_node(PARAGRAPH)`
   eagerly; it stores a checkpoint in `Container::Paragraph` and
   the close paths (`close_containers_to`) call
   `start_node_at(checkpoint, PARAGRAPH)` at close. With nothing
   committed to the green tree until close, the kind can change.
2. **CommonMark setext fold.** In core.rs's no-blank-before
   dispatch (`BlockDetectionResult::Yes` arm), when
   `parser_name == "setext_heading"` AND a paragraph is open AND
   dialect is CommonMark: take the buffered text + current line
   as the heading content, pop the paragraph container without
   emitting `PARAGRAPH`, and `start_node_at(checkpoint, HEADING)`
   to wrap retroactively from the paragraph start. Body is emitted
   via the new `emit_setext_heading_body` helper (extracted from
   `emit_setext_heading` so callers can supply their own outer
   `HEADING` wrapper).

Formatter follow-on: under CommonMark, `Foo\nBar\n---` now parses
as a heading whose `HEADING_CONTENT` contains an internal NEWLINE
(between TEXT "Foo" and TEXT "Bar"). The formatter writes
ATX-only, so the inner NEWLINE was being passed through verbatim
as `## Foo\nBar`, which round-trips as `## Foo` + a `Bar`
paragraph, breaking idempotency. Fix: in `format_heading`'s second
pass (core.rs), collapse NEWLINE tokens inside HEADING_CONTENT to
a single space.

### Files changed

- **Parser-shape gap (CommonMark dialect divergence)**:
  - `crates/panache-parser/src/parser/utils/container_stack.rs`:
    added `start_checkpoint: rowan::Checkpoint` to
    `Container::Paragraph`.
  - `crates/panache-parser/src/parser/blocks/paragraphs.rs`:
    `start_paragraph_if_needed` now takes a checkpoint instead of
    calling `start_node(PARAGRAPH)`. `append_paragraph_line` and
    other patterns updated to ignore the new field.
  - `crates/panache-parser/src/parser/core.rs`:
    `close_containers_to` paragraph branches call
    `start_node_at(checkpoint, PARAGRAPH)` before emitting buffer
    or finishing empty. New `emit_setext_heading_folding_paragraph`
    method. Dispatcher fall-through (no-blank-before
    `BlockDetectionResult::Yes`) routes setext+open-paragraph to
    the fold under CommonMark dialect.
  - `crates/panache-parser/src/parser/blocks/headings.rs`:
    extracted `emit_setext_heading_body` from `emit_setext_heading`.
- **Formatter (consequent fix to keep idempotency)**:
  - `crates/panache-formatter/src/formatter/core.rs`: HEADING_CONTENT
    inner NEWLINE tokens now collapse to a single space.
- **New parser fixtures + snapshots**:
  - `setext_multiline_commonmark` (parser): paired CommonMark
    fixture for `Foo\nBar\n---\n`. CST shows HEADING wrapping
    HEADING_CONTENT `[TEXT Foo, NEWLINE, TEXT Bar]` then NEWLINE,
    SETEXT_HEADING_UNDERLINE `---`, NEWLINE.
  - `setext_multiline_pandoc` (parser): the dialect partner;
    PARAGRAPH containing TEXT/NEWLINE for "Foo Bar ---" (Pandoc
    refuses multi-line setext).
- **New formatter fixture**:
  - `setext_multiline_commonmark` (formatter, top-level
    `tests/fixtures/cases/`): `panache.toml` with
    `flavor = "commonmark"`. Pins
    `Foo\nBar\n---\n` → `## Foo Bar\n` and verifies idempotency.
    No paired Pandoc formatter case (existing top-level fixtures
    already cover the Pandoc paragraph path).
- **Allowlist additions**: #81, #82, #95 (Setext headings section,
  inserted in numerical order between #80 and #83/etc.).

### Don't redo

- Don't try to make this fix work without the checkpoint refactor.
  The PARAGRAPH start_node was being emitted *before* the buffer
  filled, so there's no way to convert it to HEADING after the
  fact without `start_node_at`. Calling `finish_node` on an empty
  PARAGRAPH and then emitting HEADING would leave a stray empty
  PARAGRAPH in the tree.
- Don't drop the `Dialect::CommonMark` gate on the fold path. The
  Pandoc dispatcher already declines setext detection here because
  `blank_before_header` is on by default, so the gate is partly
  defensive — but if anyone toggles `blank_before_header=false`
  while staying on the Pandoc dialect, *Pandoc itself* still
  doesn't recognize multi-line setext (verified). The dialect gate
  is what keeps that behavior aligned.
- Don't try to also fix #115 here. Its expected output requires
  `# Heading\n    foo\n` to start an indented code block on
  line 2, which currently buffers as paragraph continuation. The
  setext fold makes the *symptom* slightly different (now folds
  "    foo\nHeading" into the H2) but the underlying gap is
  unrelated — see "Suggested next targets" #1.
- Don't move the formatter NEWLINE-flatten logic into the parser.
  The parser's CST is the lossless source of truth; the formatter
  is what owns "ATX-only output" policy and therefore where the
  multi-line collapse belongs.

### Suggested next targets, ranked

1. **Indented code block after ATX heading (#115)** — input
   `# Heading\n    foo\nHeading\n----` should produce ATX H1 +
   indented code "foo" + setext H2 "Heading" + indented code +
   HR. Currently "    foo" gets buffered as paragraph
   continuation. Likely a missed `at_document_start`-style reset
   in the dispatcher's indented-code precondition after an ATX
   heading. Single example, but may unlock more if the gap is
   systemic.
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
