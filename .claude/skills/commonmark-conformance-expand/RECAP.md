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

## Latest session — 2026-04-28 (c)

**Pass count: 412 → 412 / 652 (63.2%, unchanged --- formatter-only fix)**

This session was a follow-up to (b): backfilling formatter fixtures for parser
fixtures landed since 5f38d4d, plus a formatter-side blank-line-before-heading
fix that surfaced when verifying the new `atx_interrupts_paragraph_commonmark`
fixture against `pandoc -f commonmark -t commonmark`.

### Targets and root causes

- **Backfill missing formatter fixtures.** Parser fixtures added since 5f38d4d
  need a paired CommonMark-flavor formatter golden whenever CommonMark produces
  a *different* block/inline sequence than the Pandoc path; per the rule we skip
  duplicates that format identically. Audit covered seven new parser fixtures
  and identified two that warrant formatter goldens:
  - `atx_interrupts_paragraph` --- CM gives 3 blocks (para + heading + para)
    where Pandoc gives 1 paragraph. Different shape.
  - `code_spans_unmatched_backtick_run` --- CM escapes every backtick
    (`\`\`\`foo\`\``) where Pandoc treats first` as escape, rest as code span
    (\`\`\`foo\`\`). Different inline rendering.
  - The remaining five (`commonmark_entity_references_preserved`,
    `commonmark_image_paragraph_no_figure`, `paragraph_leading_whitespace`,
    `setext_text_thematic_break_{commonmark,pandoc}`,
    `thematic_break_interrupts_paragraph_{commonmark,pandoc}`) either format
    identically across flavors or already have CM formatter fixtures.
- **Formatter blank-line-before-heading.** While verifying the
  `atx_interrupts_paragraph_commonmark` formatter output, pandoc's commonmark
  roundtrip showed a blank line before `# baz` that panache wasn't emitting.
  Pandoc adds the blank for both `markdown` and `commonmark` flavors when a
  heading is preceded by a content block, so this is general formatter hygiene,
  not a CM-only concern. Mirrors the existing HR before-blank rule.

### Files changed

- Formatter fix: `crates/panache-formatter/src/formatter/core.rs` --- HEADING
  branch now emits a leading blank line when `prev_sibling` is itself a block
  element (per `is_block_element`) and the output doesn't already end with
  `\n\n`. The `is_block_element` filter is load-bearing: it skips fenced-div
  openers (`DIV_FENCE_OPEN`) and `HTML_BLOCK` so we don't break fenced-div
  fixtures or the `ignore_directives` fixture (which intentionally preserves
  no-blank-line content inside ignore regions). Mirrors the HORIZONTAL_RULE
  branch's leading-blank logic.
- Formatter fixtures:
  `tests/fixtures/cases/atx_interrupts_paragraph_commonmark/` and
  `tests/fixtures/cases/code_spans_unmatched_backtick_run_commonmark/` ---
  `input.md`, `expected.md`, `panache.toml` (`flavor = "commonmark"`).
  Registered in `tests/golden_cases.rs`. Both pin idempotent CommonMark-flavor
  output and verified byte-equal against `pandoc -f commonmark -t commonmark`.

No parser changes, no allowlist changes, no spec.txt-conformance shift.

### Don't redo

- The blank-line-before-HEADING formatter rule and its
  `is_block_element(prev_sibling.kind())` gating. The first cut was
  unconditional and broke `ignore_directives`, `fenced_div_close_grid_table`,
  `fenced_div_list_idempotency_setup`; the gating is required.
- The audit of post-5f38d4d parser fixtures for missing formatter goldens ---
  the only two that warranted goldens are now landed; the rest format
  identically across flavors.

### Suggested next targets, ranked

(Largely unchanged from session (b); the audit task is now complete.)

1. **Lists (5/21) + List items (17/31)** --- biggest low-pass-rate sections;
   likely shared root causes (loose/tight detection,
   thematic-break-terminates-list, lazy continuation). Now that HR interrupts
   paragraphs cleanly, the list-termination branch should be tractable next.
2. **Emphasis and strong emphasis (85/47)** --- largest remaining absolute
   failure count; flanking-rule edge cases + intraword-underscore.
3. **HTML blocks (24/20) + Raw HTML (6/14)** --- probably one shared fix in
   HTML-tag/comment recognition.
4. **Setext headings (19/8)** --- what's left needs multi-line-content support
   (#95) or blockquote/list interaction (#92, 94, 99, 101). Defer until lists
   are unblocked.
5. Hard line breaks #642, #643 --- multi-line raw inline HTML preservation.
6. Known link follow-ups still in `blocked.txt`: #488, #490, #493, #508, #523,
   #546.
7. Entity reference follow-ups: #31 (HTML-block detection gap) and #41 (entity
   refs as structural quotes around link title).

### Carried-forward notes from session (b)

Remaining setext / thematic-break gaps not unlocked yet:

- **#57, #60, #61** --- thematic break interrupting a list. List
  continuation/termination logic is more involved than the paragraph case.
- **#92, #94, #99, #101** --- setext underline / thematic break following
  `> Foo` or `- Foo`. Dialect divergence; needs paired fixtures plus
  blockquote/list interruption work.
- **#95** --- multi-line setext content (`Foo\nBar\n---`). Bigger refactor of
  the paragraph parser to retroactively convert accumulated lines into a setext
  heading.
