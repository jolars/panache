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

## Latest session — 2026-04-29 (c)

**Pass count: 534 → 539 / 652 (82.7%, +5)**

Targeted prior recap's #5 (`10)` paren-marker recognition for #296/#297) and
the §5.2 paragraph-interrupt rules behind #303/#305. Two dialect-divergence
fixes — one in the list-marker parser, one in the parser's paragraph-
interrupt gate — unlocked five conformance examples (#296, #297, #302, #303,
#305).

### Single root cause: decimal-paren markers gated on `fancy_lists`

`try_parse_list_marker` rejected `RightParen` markers (`10)`) whenever
`fancy_lists` was off, regardless of dialect. CommonMark §5.2 lists `1-9
digits + . or )` as part of the *core* grammar, not an extension. Pandoc-
markdown defaults `fancy_lists` ON, so the bug only surfaced under
`Flavor::CommonMark`. Verified with `pandoc -f commonmark -t native` that
both dialects agree on `10) foo` becoming an ordered list when the marker is
recognized — the divergence is purely in *what gates recognition*, not in
the resulting CST shape. Bypass added: dialect == CommonMark skips the
fancy_lists check; Pandoc-default behavior is unchanged.

### Single root cause: parser unconditionally blocked list-interrupts

CommonMark §5.2 allows bullet lists *and* ordered lists starting with `1` to
interrupt a paragraph without a blank line. Pandoc-markdown forbids any list
interruption. The check at `core.rs::parse_line` for `BlockEffect::OpenList
&& YesCanInterrupt && paragraph_open && !in_list && content_indent == 0`
appended the line to the paragraph regardless of dialect. Verified with
pandoc that the divergence is real (`Foo\n- bar` is paragraph + list under
commonmark, single paragraph under markdown). Fix gates the block on
dialect: under CommonMark, allow the interrupt when the prepared marker is
`Bullet(_)` or `Ordered(Decimal { number == "1", .. })`. Pandoc behavior
unchanged.

### Files changed

- **Parser (dialect divergence × 2)**:
  - `crates/panache-parser/src/parser/blocks/lists.rs`: the
    `RightParen && !fancy_lists` rejection in `try_parse_list_marker` now
    skips when `config.dialect == Dialect::CommonMark`.
  - `crates/panache-parser/src/parser/core.rs`: the list-interrupt block in
    `parse_line` (around the `YesCanInterrupt` arm) now reads the
    `ListPrepared` payload to decide if the marker is allowed to interrupt
    under CommonMark (bullet OR ordered-decimal with `number == "1"`); all
    other markers fall through to the existing paragraph-append path.
- **Parser fixtures (paired where divergent)**:
  - `crates/panache-parser/tests/fixtures/cases/list_interrupts_paragraph_{commonmark,pandoc}/`
    — `Foo\n- bar\n\nBaz\n1. qux\n\nQuux\n2. corge\n` parses as
    paragraph + bullet list, paragraph + ordered list (start=1), single
    paragraph (start=2 doesn't interrupt) under CommonMark; under Pandoc all
    three are single paragraphs.
  - `crates/panache-parser/tests/fixtures/cases/ordered_paren_marker_decimal_commonmark/`
    — `10) foo\n    - bar\n` parses as an ordered list (LIST_MARKER `10)`)
    with a nested bullet. CommonMark-only fixture; Pandoc default (with
    `fancy_lists`) already accepts the same shape, so a paired fixture
    would be churn.
- **Formatter golden case (CommonMark-only structural shape)**:
  - `tests/fixtures/cases/list_interrupts_paragraph_commonmark/` — pins the
    formatted output for the same input under `flavor = "commonmark"`. The
    third block (`Quux\n2. corge`) reflows to `Quux 2. corge` because
    CommonMark treats it as a single paragraph; the formatter is idempotent.
    Skipped paired Pandoc case — Pandoc-default produces a different shape
    (single paragraph each), already exercised by the existing fixture
    suite.
- **Allowlist additions**:
  - List items: +#296, +#297.
  - Lists: +#302, +#303, +#305.

### Don't redo

- Don't drop the dialect gate from the `RightParen` marker check thinking
  enabling `fancy_lists` for `commonmark_defaults()` would suffice.
  `fancy_lists` covers letters and roman numerals too — those are *not*
  valid CommonMark markers (CommonMark §5.2 restricts ordered markers to
  digits). Enabling the extension would over-permit and likely regress
  Pandoc-flavored fixtures.
- Don't broaden the list-interrupt allowance to ordered-with-any-start
  under CommonMark. The spec is explicit: only `1.` or `1)` may interrupt
  (this is the rule that lets `windows house\n14. doors` stay a paragraph
  in #304). Removing the `number == "1"` check would regress #304.
- Don't move the list-interrupt gate into the dispatcher's `detect_prepared`.
  At detection time the dispatcher doesn't know if a paragraph is currently
  open vs whether the line is at document start with no prior context — the
  gate has to consult parser state (`is_paragraph_open && !in_list &&
  content_indent == 0`), which lives in core.rs. Keeping the marker-aware
  check colocated with that gate avoids a duplicated paragraph-state probe.
- Don't expect bullet markers (`-`, `+`, `*`) to be subject to the
  `fancy_lists` gate at all. Their match path is at the top of
  `try_parse_list_marker` and is unconditional — only the ordered-decimal
  paren branch and the letters/roman branches care about `fancy_lists`.

### Suggested next targets, ranked

1. **Multi-line setext heading + losslessness bug (#81, #82, #95, #115)** —
   under `Dialect::CommonMark`, a paragraph of >1 line followed by a setext
   underline yields a broken CST (paragraph text appears in reverse order).
   Verify losslessness fails first
   (`printf 'Foo\nBar\n---\n' | cargo run -- debug format --checks
   losslessness`), then fix in the setext detection path
   (`SetextHeadingParser`, possibly the paragraph-buffer drain logic in
   `parser/core.rs`). Pandoc keeps the paragraph-with-em-dash behavior.
   Will need paired fixtures + a top-level CommonMark formatter golden
   because the new shape is structurally different. Big change, plan a
   session for it.
2. **Bracketed ref-def label with escapes (#194)** —
   `[Foo*bar\]]:my_(url) 'title (with parens)'`. Two bugs:
   `emit_reference_definition_content` uses `rest.find(']')` which stops at
   the *escaped* `]`; the renderer's label normalization doesn't decode
   `\]`. Fix the emit by walking the label with the same `escape_next`
   logic the parser uses, and decode backslash escapes before
   label-matching in the renderer.
3. **Empty list item closes the list when followed by blank line (#280)** —
   markdown `-\n\n  foo\n` should produce `<ul><li></li></ul><p>foo</p>`.
   Likely needs a list-blank-handling branch in `core.rs` or
   `list_postprocessor.rs`.
4. **Multi-empty-marker with subsequent indented content (#278)** — chaotic
   parse; partially comes along once #280 is solved.
5. **Tabs (#2, #5, #6, #7)** — column-aware tab expansion needed for
   indented-code inside containers. Substantial.
6. **Block quotes lazy-continuation (#235, #236, #251)** — lazy
   continuation must not extend a list or code block inside a blockquote.
   CommonMark §5.1.
7. **Fence inside blockquote inside list item (#321)** — list-item
   continuation can be interrupted by a fence at content column;
   dispatcher's continuation-line fence detection only fires when
   `bq_depth > 0`.
8. **Loose-vs-tight nested loose lists (#312, #326)** — mixed
   parser/renderer shape. #326's outer list should be loose (blank line
   between items) so `a` and `d` need `<p>` wrappers, but renderer treats
   it as tight. Likely a renderer-side loose-detection gap.
9. **Lazy / nested marker continuation (#298, #299)** — `- - foo` and
   `1. - 2. foo` should produce nested list-on-same-line; parser flattens.
10. **Multi-block content in `1.     code` items (#273, #274)** —
    `1.` followed by 5+ spaces should open a list item whose first block is
    an indented code block.
11. **Setext-in-list-item (#300)** — `- # Foo\n- Bar\n  ---\n  baz` needs
    `<h2>Bar</h2>` inside the second item.
12. **Emphasis and strong emphasis (47 fails)** — flanking-rule and
    autolink-precedence edge cases.
13. **Ref-def dialect divergence #201** — `[foo]: <bar>(baz)`. Fix requires
    gating the strict-EOL check on `Dialect::CommonMark` and adding a
    paired fixture; Pandoc-markdown accepts it, current code rejects both.
    Low priority.

--------------------------------------------------------------------------------

## Previous session — 2026-04-29 (b)

**Pass count: 529 → 534 / 652 (81.9%, +5)**

Targeted prior recap's #1 (ref-def can't interrupt paragraph), #2 (ref-def vs
setext priority), #4 (multi-line label ref-def), and the #218 follow-up (ref-def
inside blockquote losslessness). Four parser-shape fixes plus one dialect
divergence unlocked five conformance examples (#208, #213, #216, #218, #546)
plus two side-effect wins (#108, #109).

### Single root cause: parser dispatcher allowed ref-def to interrupt a paragraph

Spec §4.7 is explicit: a reference link definition cannot interrupt a paragraph.
The dispatcher's `Yes` arm in `parser/core.rs` only protected `fenced_div_open`;
ref-defs fell through and the renderer concatenated the def's link inline with
the paragraph's text, producing scrambled output. Verified pandoc agrees on both
dialects, so no `Dialect` gate.

### Single root cause: ref-def parser couldn't span lines for the label

Label loop in `try_parse_reference_definition_with_mode` rejected on `\n`, so
`[\nfoo\n]: /url` always fell through. Spec §4.7 allows newlines inside a label
(blank line terminates). Fixed by allowing `\n` and `\r\n`, terminating on a
blank line, and rejecting unescaped `[` per spec.

### Single root cause: setext consumed a ref-def line in CommonMark

Registry order checks `SetextHeadingParser` before `ReferenceDefinitionParser`,
and §4.3/§4.7 say a setext heading text line may not itself be a reference
definition. Pandoc-markdown takes the setext, so this is a `Dialect::CommonMark`
gate — not a free fix.

### Single root cause: ref-def parse_prepared duplicated the blockquote marker

Inside a blockquote the dispatcher emits `BLOCK_QUOTE_MARKER` + `WHITESPACE`
itself, then calls `ReferenceDefinitionParser::parse_prepared`, which read
`lines[line_pos]` directly — so the `>` ended up emitted twice (CST losslessness
violation: `> > [foo]: /url`). Fixed by using `ctx.content` (already stripped)
when `ctx.blockquote_depth > 0`. detect_prepared already restricts blockquote
context to single-line.

### Files changed

- **Parser (parser-shape gap × 3)**:
  - `crates/panache-parser/src/parser/core.rs`: added `reference_definition`
    branch to the `BlockDetectionResult::Yes` paragraph-protection arm,
    mirroring the existing `fenced_div_open` pattern.
  - `crates/panache-parser/src/parser/blocks/reference_links.rs`:
    `try_parse_reference_definition_with_mode` label loop now allows newlines
    inside the label (terminates on blank line, rejects unescaped `[`).
    `if label.is_empty()` becomes `label.trim().is_empty()` so a label of
    only whitespace (e.g. `[\n  \n]:`) is rejected.
  - `crates/panache-parser/src/parser/block_dispatcher.rs`:
    `ReferenceDefinitionParser::parse_prepared` now uses `ctx.content` when
    `ctx.blockquote_depth > 0` to avoid duplicating the `>` marker that the
    dispatcher already emitted; non-blockquote path is unchanged.
- **Parser (dialect divergence)**:
  - `crates/panache-parser/src/parser/block_dispatcher.rs`:
    `SetextHeadingParser::detect_prepared` now declines under
    `Dialect::CommonMark` when the candidate text line is itself a valid
    reference definition (gated on `extensions.reference_links`). Pandoc
    behavior unchanged.
- **Test fixture parity**:
  - `tests/linting/unused_definitions.md` and the inline string in
    `tests/cli/lint.rs::test_lint_reports_unused_definitions` — both fixtures
    relied on a ref-def directly following a paragraph line being parsed as a
    ref-def. After the §4.7 fix that's a single paragraph, so a blank line was
    inserted between `See [UsedLabel][].` and the ref-defs to reflect what
    callers actually have to write. Behavior assertions unchanged.
- **Parser fixtures (pin new behavior)**:
  - `crates/panache-parser/tests/fixtures/cases/reference_definition_no_interrupt_paragraph/`
    — `Foo\n[bar]: /baz\n\n[bar]\n` → single PARAGRAPH(Foo + LINK[bar] + ": /baz")
    + BLANK_LINE + PARAGRAPH(LINK[bar]). No paired Pandoc fixture (both dialects
    agree).
  - `crates/panache-parser/tests/fixtures/cases/reference_definition_inside_blockquote/`
    — `[foo]\n\n> [foo]: /url\n` → BLOCK_QUOTE containing
    REFERENCE_DEFINITION whose LINK label is correctly emitted (no duplicated
    marker). Pins the losslessness fix.
  - `crates/panache-parser/tests/fixtures/cases/reference_definition_multiline_label/`
    — `[\nfoo\n]: /url\nbar\n` → REFERENCE_DEFINITION spanning the first three
    lines + PARAGRAPH(bar). Pandoc agrees on shape, so no paired fixture.
  - `crates/panache-parser/tests/fixtures/cases/setext_vs_reference_definition_{commonmark,pandoc}/`
    — paired fixtures pinning the dialect divergence:
    `[foo]: /url\n===\n[foo]\n` parses as REFERENCE_DEFINITION + PARAGRAPH under
    CommonMark, HEADING(setext H1) + PARAGRAPH under Pandoc.
- **Allowlist additions**:
  - Indented code blocks: +#108, +#109 (side-effect wins from the §4.7 fix —
    the trailing ref-def line in those examples is now correctly part of the
    list-item paragraph rather than a stray ref-def).
  - Link reference definitions: +#208, +#213, +#216, +#218.
  - Links: +#546 (side-effect from rejecting unescaped `[` in the label —
    `[ref[]: /uri` is now correctly *not* a ref-def, so the line stays a
    paragraph as the spec requires).

### Don't redo

- Don't drop the dialect gate from the new setext-vs-ref-def check. Pandoc
  *does* parse `[foo]: /url\n===\n` as an H1 with text `[foo]: /url`, verified
  with `pandoc -f markdown -t native`. Removing the gate would regress
  Pandoc-flavored parsing for a lot of legitimate-looking headings.
- Don't try to widen the new ref-def label loop to handle backslash-escaped
  `]`-then-newline cleverly without testing #194 (`[Foo*bar\]]:my_(url)
  'title (with parens)'`). #194 still fails because
  `emit_reference_definition_content` uses `rest.find(']')` and stops at the
  *escaped* `]`. The label loop in the parser itself respects escapes; the gap
  is in the emit path that builds the LINK_TEXT node.
- Don't move the ref-def parse_prepared blockquote special-case to assume
  multi-line input is possible inside blockquotes. detect_prepared
  intentionally restricts blockquote-context defs to a single line, because
  joining `lines[]` would feed the `>` markers back to the parser. Multi-line
  ref-defs inside blockquotes remain a separate piece of work.
- Don't rely on the renderer's `parse_reference_definition` to extract a label
  from a multi-line ref-def CST today. The current emit emits the multi-line
  label as TEXT tokens (no LINK), so the renderer falls back to "no label,
  skip." That's why #208 happens to work — the def isn't actually used in the
  output. If a future test needs the multi-line label resolved, the emit path
  needs to construct the LINK + LINK_TEXT properly across newlines.
- Don't remove the `label.trim().is_empty()` check thinking the original
  `is_empty()` was equivalent. With multi-line labels enabled, a label of
  just whitespace (e.g. `[\n  \n` followed by `]: /url`) would otherwise pass
  the empty-check and create a useless ref-def.

### Suggested next targets, ranked

1. **Multi-line setext heading + losslessness bug (#81, #82, #95, #115)** —
   under `Dialect::CommonMark`, a paragraph of >1 line followed by a setext
   underline yields a broken CST (paragraph text appears in reverse order in
   the output). Verify losslessness fails first
   (`printf 'Foo\nBar\n---\n' | cargo run -- debug format --checks
   losslessness`), then fix in the setext detection path
   (`SetextHeadingParser`, possibly the paragraph-buffer drain logic in
   `parser/core.rs`). Pandoc keeps the paragraph-with-em-dash behavior.
   Will need paired fixtures + a top-level CommonMark formatter golden
   because the new shape is structurally different. Big change, plan a
   session for it.
2. **Bracketed ref-def label with escapes (#194)** —
   `[Foo*bar\]]:my_(url) 'title (with parens)'`. Two bugs:
   `emit_reference_definition_content` uses `rest.find(']')` which stops at
   the *escaped* `]`; the renderer's label normalization doesn't decode
   `\]`. Fix the emit by walking the label with the same `escape_next` logic
   the parser uses, and decode backslash escapes before label-matching in
   the renderer.
3. **Empty list item closes the list when followed by blank line (#280)** —
   markdown `-\n\n  foo\n` should produce `<ul><li></li></ul><p>foo</p>`.
   Likely needs a list-blank-handling branch in `core.rs` or
   `list_postprocessor.rs`.
4. **Multi-empty-marker with subsequent indented content (#278)** — chaotic
   parse; partially comes along once #280 is solved.
5. **Tabs (#2, #5, #6, #7)** — column-aware tab expansion needed for
   indented-code inside containers. Substantial.
6. **Block quotes lazy-continuation (#235, #236, #251)** — lazy
   continuation must not extend a list or code block inside a blockquote.
   CommonMark §5.1.
7. **Fence inside blockquote inside list item (#321)** — list-item
   continuation can be interrupted by a fence at content column;
   dispatcher's continuation-line fence detection only fires when
   `bq_depth > 0`.
8. **Loose-vs-tight nested loose lists (#312, #326)** — mixed
   parser/renderer shape.
9. **Lazy / nested marker continuation (#296, #297, #305)** —
   `10) foo\n  - bar` should produce nested list; currently parses as a
   paragraph.
10. **Multi-block content in `1.     code` items (#273, #274)** —
    `1.` followed by 5+ spaces should open a list item whose first block is
    an indented code block.
11. **Setext-in-list-item (#300)** — `- # Foo\n- Bar\n  ---\n  baz` needs
    `<h2>Bar</h2>` inside the second item.
12. **Marker-on-same-line nesting (#298, #299)** — `- - foo` should be
    nested lists; parser flattens.
13. **Emphasis and strong emphasis (47 fails)** — flanking-rule and
    autolink-precedence edge cases.
14. **Ref-def dialect divergence #201** — `[foo]: <bar>(baz)`. Fix requires
    gating the strict-EOL check on `Dialect::CommonMark` and adding a
    paired fixture; Pandoc-markdown accepts it, current code rejects both.
    Low priority.

--------------------------------------------------------------------------------

## Previous session — 2026-04-29

**Pass count: 522 → 529 / 652 (81.1%, +7)**

Targeted prior recap's #2 (multi-line link reference definitions). One
parser-shape rewrite + one renderer fix unlocked 7 examples spanning two
sections. Verified with pandoc that both dialects agree on every change
made — no `Dialect` gating was needed.

### Single root cause: ref-def parser was single-line only

`try_parse_reference_definition` consumed exactly one line; the
dispatcher fed it `ctx.content`. Spec §4.7 allows up to one line ending
between `:` and the destination, and another between destination and
title. The parser also lacked an end-of-line check after the title, so
junk like `[foo]: /url "title" ok` was being accepted (and then
suppressed by the renderer).

### Files changed

- **Parser (parser-shape gap)**:
  - `crates/panache-parser/src/parser/blocks/reference_links.rs`:
    - Rewrote `try_parse_reference_definition` to take multi-line input.
    - Added `skip_ws_one_newline` helper to enforce "at most one line
      ending" between the colon → URL and URL → title gaps.
    - Added strict end-of-line validation (`consume_to_eol`) after the
      title, with a fallback to "no title, end of URL line" when the
      title attempt fails *after* crossing a newline (matches spec
      example #210: `[foo]: /url\n"title" ok\n` → ref def `[foo]: /url`,
      paragraph `"title" ok`).
    - Split into `try_parse_reference_definition` (strict) and
      `try_parse_reference_definition_lax` (MMD-flavored — tolerates
      trailing same-line content like `"title" width=20px ...`).
  - `crates/panache-parser/src/parser/block_dispatcher.rs`:
    - `ReferenceDefinitionParser::detect_prepared` now joins consecutive
      non-blank lines into a multi-line candidate (top-level only;
      blockquote-context falls back to single-line on `ctx.content`
      because the raw `lines[]` carry `>` markers).
    - Translates `bytes_consumed` from the parser into a `consumed_lines`
      count for emission.
    - Selects strict vs lax parser based on
      `extensions.mmd_link_attributes`.
  - `emit_reference_definition_content` now strips up to 3 leading
     spaces and emits them as a `WHITESPACE` token before the `LINK`
     node (otherwise indented ref defs like `   [foo]: /url` regressed
     to a single TEXT token and the renderer couldn't extract the
     label).
- **Renderer (renderer-only fix in test code)**:
  - `crates/panache-parser/tests/commonmark/html_renderer.rs`:
    - `parse_reference_definition` now collects `NEWLINE` and
      `WHITESPACE` tokens into the tail in addition to `TEXT`, so
      multi-line URLs/titles survive concatenation.
    - Tail trim now uses `.trim_start().trim_start_matches(':').trim()`
      so a leading `WHITESPACE` token from the indent doesn't block the
      colon strip.
    - `split_dest_and_title` now special-cases `<…>` URLs (e.g. `<my
      url>`), preserving spaces inside the angle brackets. Also
      benefited inline link #489 (`[link](</my uri>)`).
- **Parser fixture (pins multi-line ref-def CST)**:
  - `crates/panache-parser/tests/fixtures/cases/reference_definition_multiline_destination/`
    — `[foo]:\n/url\n\n[foo]\n` parses as a single `REFERENCE_DEFINITION`
    spanning both lines, followed by a paragraph with the link
    reference. No paired Pandoc fixture: pandoc-markdown produces the
    same shape (verified via `pandoc -f markdown -t native`), so a
    duplicate would be churn.
- **Existing fixture corrected**:
  - `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_mmd_link_attributes_disabled.snap`
    — was pinning the lax-parser behavior (accepting `[ref]: /url
    "title" key=val` as a ref def). Pandoc-markdown rejects this as a
    paragraph. Snapshot updated to match the new strict behavior; this
    is *more correct*, not a regression.
  - `tests/fixtures/cases/mmd_link_attributes_disabled/expected.md`
    — formatter expectation updated correspondingly: the whole `[ref]:
    ...key=val` block is now one paragraph (idempotent).
- **Allowlist additions**:
  - Link reference definitions: +#193, +#195, +#196, +#198, +#209, +#217
  - Links: +#489 (side effect of `<…>` URL handling in renderer)

### Single root cause: ordered list marker accepted >9 digits

CommonMark §5.2 restricts ordered list markers to 1–9 arabic digits.
Pandoc-markdown accepts arbitrary digit counts. The decimal branch
of `try_parse_list_marker` did not gate on dialect, so
`1234567890. not ok` parsed as an ordered list under both flavors.

### Files changed

- **Parser (dialect divergence)**:
  - `crates/panache-parser/src/parser/blocks/lists.rs`: added a
    9-digit cap on the ordered-decimal marker branch, gated on
    `Dialect::CommonMark`. Pandoc behavior unchanged.
- **Parser fixtures (paired, pin dialect-specific CST)**:
  - `crates/panache-parser/tests/fixtures/cases/ordered_marker_max_digits_commonmark/`
    — `flavor = "commonmark"`, `1234567890. not ok\n` parses as a
    PARAGRAPH (no LIST).
  - `crates/panache-parser/tests/fixtures/cases/ordered_marker_max_digits_pandoc/`
    — `flavor = "pandoc"`, same input parses as a LIST with a
    10-digit LIST_MARKER. Wired into `golden_parser_cases.rs`.
- **Allowlist additions** (List items):
  - +#266

No CommonMark-specific formatter golden case added: both dialects
produce the same multi-line ref-def CST and the same formatted
output, so adding a CommonMark-only fixture would be churn (per the
formatter-fixture rule).

### Don't redo

- Don't try to widen `try_parse_reference_definition` to handle
  multi-line **labels** (#208: `[\nfoo\n]: /url\nbar`). That requires
  the label loop to cross newlines and a different "blank line ends
  the label attempt" check, plus careful handling of the
  passing-by-accident risk that ordinary `[\n` openers no longer fall
  through to inline link parsing. It's a separate change with its own
  fixture story.
- Don't drop the `consume_to_eol` strict check thinking
  `consume_to_eol_lax` could replace it for everyone. The strict
  check is what makes #209 / #210 / `mmd_link_attributes_disabled`
  produce the spec-correct paragraph fall-through; only the MMD
  attribute-continuation path needs lax behavior.
- Don't remove the blockquote-context single-line fallback in
  `ReferenceDefinitionParser::detect_prepared`. Inside a blockquote,
  the raw `lines[]` still carry `>` markers; the multi-line join
  would feed them to the parser and the very first byte (`>`) makes
  the parser reject. Multi-line ref defs inside blockquotes are
  tracked separately (see #218 below).
- Don't expect the renderer's `parse_reference_definition` to keep
  working if you stop emitting `WHITESPACE` for the leading-indent of
  an indented ref def. The renderer concatenates TEXT + NEWLINE +
  WHITESPACE tokens to reconstruct the def's tail; dropping any of
  the three breaks indented-def reconstruction (#193).
- The strict-EOL check intentionally rejects `[foo]: <bar>(baz)`
  (#201) under both dialects. Pandoc-markdown actually *accepts* this
  as a ref def — the fix matches CommonMark, not Pandoc. If/when
  Pandoc-flavor #201 behavior is needed for parity with the Haskell
  pandoc, gate the strict check on `Dialect::CommonMark` and add a
  paired fixture; do not just relax the check globally.
- The current strict parser correctly rejects `[ref]: /url "title"
  width=20px` (Pandoc) — verified against `pandoc -f markdown -t
  native`. The previous golden snapshot expected REFERENCE_DEFINITION
  for this input; the snapshot was wrong. New snapshot is more
  correct.

### Suggested next targets, ranked

1. **Ref def can't interrupt a paragraph (#213)** — `Foo\n[bar]:
   /baz\n\n[bar]\n` currently emits `REFERENCE_DEFINITION` *inside*
   an open paragraph (CST byte ranges look correct but the textual
   order is jumbled — looks like the same buffered-paragraph pattern
   the multi-line setext bug exhibits). Easiest fix: in
   `parser/core.rs` around line 2316–2335, mirror the
   `fenced_div_open` paragraph-protection branch for
   `parser_name == "reference_definition"` so a `Yes` ref-def
   detection while a paragraph is open just appends to the paragraph
   instead. Add a paired fixture under
   `tests/fixtures/cases/reference_definition_no_interrupt_paragraph/`.
2. **Ref-def vs setext priority (#216)** — `[foo]: /url\n===\n[foo]`
   currently matches the SetextHeadingParser before
   ReferenceDefinitionParser (registry order). Spec §4.2: setext
   underline content must not itself parse as a ref def. Fix in
   `SetextHeadingParser::detect_prepared`: before claiming the line,
   try `try_parse_reference_definition` on the buffered paragraph
   line; if it succeeds, decline. Verify with pandoc both ways.
3. **Multi-line setext heading + losslessness bug (#81, #82, #95,
   #115)** — under `Dialect::CommonMark`, the parser produces a
   broken CST when a paragraph of >1 line is followed by a setext
   underline. Verify losslessness fails (`tree.text() != input`)
   then fix in the setext detection path
   (`block_dispatcher.rs::SetextHeadingParser`, possibly
   `parser/core.rs` paragraph-buffer drain logic). Pandoc keeps the
   paragraph-with-em-dash behavior. Will need paired fixtures + a
   top-level CommonMark formatter golden because the new shape is
   structurally different. Big change, plan a session for it.
4. **Multi-line label ref def (#208)** — `[\nfoo\n]: /url\nbar` →
   `<p>bar</p>`. The label loop in
   `try_parse_reference_definition` rejects on `\n`. Allowing
   newlines inside the label needs a blank-line check (a blank line
   inside the label terminates the def attempt, not just stops the
   loop) and label normalization (whitespace runs collapse). Add a
   paired fixture; verify with pandoc.
5. **Bracketed ref-def label with escapes (#194)** —
   `[Foo*bar\]]:my_(url) 'title (with parens)'`. Two bugs:
   `emit_reference_definition_content` uses `rest.find(']')` which
   stops at the *escaped* `]`; the renderer's label normalization
   doesn't decode `\]`. Fix the emit by walking the label with the
   same `escape_next` logic the parser uses, and decode backslash
   escapes before label-matching in the renderer.
6. **Ref-def inside blockquote affects outer scope (#218)** —
   `[foo]\n\n> [foo]: /url\n` should resolve `[foo]` outside the
   blockquote. The dispatcher *does* register a REFERENCE_DEFINITION
   inside the blockquote, but the renderer's `collect_references`
   currently descends into all `REFERENCE_DEFINITION` nodes — verify.
   Likely a renderer issue with how blockquoted defs are emitted: the
   multi-line single-line fallback in detect_prepared may be wrong.
7. **Empty list item closes the list when followed by blank line
   (#280)** — markdown `-\n\n  foo\n` should produce
   `<ul><li></li></ul><p>foo</p>`. Likely needs a list-blank-handling
   branch in `core.rs` or `list_postprocessor.rs`.
8. **Multi-empty-marker with subsequent indented content (#278)** —
   chaotic parse; partially comes along once #280 is solved.
9. **Tabs (#2, #5, #6, #7)** — column-aware tab expansion needed
   for indented-code inside containers. Substantial.
10. **Block quotes lazy-continuation (#235, #236, #251)** — lazy
    continuation must not extend a list or code block inside a
    blockquote. CommonMark §5.1.
11. **Fence inside blockquote inside list item (#321)** — list-item
    continuation can be interrupted by a fence at content column;
    dispatcher's continuation-line fence detection only fires when
    `bq_depth > 0`.
12. **Loose-vs-tight nested loose lists (#312, #326)** — mixed
    parser/renderer shape.
13. **Lazy / nested marker continuation (#296, #297, #305)** —
    `10) foo\n  - bar` should produce nested list; currently parses
    as a paragraph.
14. **Multi-block content in `1.     code` items (#273, #274)** —
    `1.` followed by 5+ spaces should open a list item whose first
    block is an indented code block.
15. **Setext-in-list-item (#300)** — `- # Foo\n- Bar\n  ---\n  baz`
    needs `<h2>Bar</h2>` inside the second item.
16. **Marker-on-same-line nesting (#298, #299)** — `- - foo` should
    be nested lists; parser flattens.
17. **Emphasis and strong emphasis (47 fails)** — flanking-rule and
    autolink-precedence edge cases.
18. **Ref-def dialect divergence #201** — `[foo]: <bar>(baz)`. Fix
    requires gating the strict-EOL check on `Dialect::CommonMark`
    and adding a paired fixture; Pandoc-markdown accepts it, current
    code rejects both. Low priority.

--------------------------------------------------------------------------------

## Previous session — 2026-04-28 (q)

**Pass count: 518 → 521 / 652 (79.9%, +3)**

Targeted prior recap's #5 (empty list items: #266, #278, #280, #281,
#283, #284). One small parser-shape gap unlocked #281, #283, #284.
#280 still fails — different problem: empty list-item with one
intervening blank line should close the list before subsequent
content. #278 unchanged. #266 is unrelated (10-digit ordered marker
acceptance — should be 9-digit max in CommonMark).

### Single root cause: bare list markers were not recognized

The marker parser at
`crates/panache-parser/src/parser/blocks/lists.rs:158`
accepted `is_empty()` after-marker-text but not when the line ended
with a newline (`*\n` → `after_marker = "\n"`, none of the gates
matched). That alone made bare markers fall through to paragraph
parsing.

Even after fixing the marker parser, the dispatcher gate at
`crates/panache-parser/src/parser/block_dispatcher.rs:596` rejected
*every* `spaces_after_cols == 0` match. That gate exists to reject
the task-checkbox-without-space case (`-[ ] foo`), so it must remain
— but needs to allow bare markers and refuse interrupting a
document-level paragraph (CommonMark §5.2: "An empty list item cannot
interrupt a paragraph"). #285 (`foo\n*\n` → `<p>foo *</p>`) keeps
passing thanks to the `!at_document_start && !has_blank_before
&& !in_list` clause.

### Files changed

- **Parser (parser-shape gap × 2)**:
  - `crates/panache-parser/src/parser/blocks/lists.rs`:
    `try_parse_list_marker` now strips trailing `\r`/`\n` from the
    line at entry, so trailing-newline bare markers are recognized.
  - `crates/panache-parser/src/parser/block_dispatcher.rs`:
    `ListParser::detect_prepared` accepts `spaces_after_cols == 0`
    when (a) `after_marker_text.trim_end_matches(['\r','\n'])` is
    empty (bare marker, not task-without-space) and (b) the bare
    marker is at document start, after a blank line, or already
    inside a list. Preserves the prior gate's intent (reject
    `-[ ] foo`) while allowing CommonMark/Pandoc empty list items.
- **Parser fixture (pins the CST shape)**:
  - `crates/panache-parser/tests/fixtures/cases/list_item_bare_marker_empty_commonmark/`
    — pins the CST for `- foo\n-\n- bar\n` (single LIST with three
    LIST_ITEM children; middle item is bare LIST_MARKER + PLAIN
    containing only NEWLINE). `flavor = "commonmark"`. Wired into
    `golden_parser_cases.rs`. No paired Pandoc fixture — pandoc
    `-f markdown` produces the same shape (verified), so a duplicate
    would be churn.
- **Allowlist additions** (List items):
  - +#281, +#283, +#284

No formatter golden case added: pandoc-default and CommonMark produce
the same formatted output (`- foo\n- \n- bar\n`, idempotent). Per the
rule, only add a CommonMark formatter case when the dialects diverge
structurally on the new shape.

### Don't redo

- Don't drop the `in_list` clause from the dispatcher gate. Without
  it, a bare marker on a continuation line of a top-level paragraph
  (#285's `foo\n*\n`) would interrupt the paragraph and open a list,
  regressing the spec. Keep all three: at_document_start OR
  has_blank_before OR in_list.
- Don't move the `\r\n` strip out of `try_parse_list_marker`.
  Several callers pass synthetic bare-marker lookahead lines without
  newlines (e.g. unit tests at lines 632–642). Stripping at entry
  keeps both call shapes working without churn at every callsite.
- Don't reuse `after_marker_text.is_empty()` directly in the
  dispatcher. `ctx.content` retains its trailing newline, so
  `after_marker_text` is `"\n"` for `*\n`. Use
  `trim_end_matches(['\r', '\n']).is_empty()` to accept that case
  while still rejecting `-[ ] foo` (`after_marker_text = "[ ] foo"`
  ≠ empty after trimming).
- The new fixture pins the empty item as `LIST_MARKER + PLAIN(NEWLINE)`,
  not as `LIST_MARKER` alone. That's deliberate — list-item children
  always include a tail node (PLAIN/PARAGRAPH) for whitespace
  bookkeeping in the formatter. Don't try to "tighten" by removing
  the PLAIN.

### Suggested next targets, ranked

1. **Empty list item closes the list when followed by blank line
   (#280)** — markdown `-\n\n  foo\n` should produce
   `<ul><li></li></ul><p>foo</p>` (the empty item closes after the
   blank, foo is *outside* the list). Currently produces
   `<ul><li><p></p><p>foo</p></li></ul>` because the blank-line +
   indented-content rule keeps the foo inside the empty item. Likely
   needs a list-blank-handling branch: when the list item has *no*
   non-newline content and the next line after a blank doesn't
   strictly belong to a sibling marker, close the list. Possibly in
   `core.rs` blank-line handling or in
   `list_postprocessor.rs`. Verify the spec expects this for both
   bullet and ordered (#283/#284 cousins are siblings, this is the
   "blank line empties out" sibling).
2. **Multi-empty-marker with subsequent indented content (#278)** —
   `-\n  foo\n-\n  ```\n  bar\n  ```\n-\n      baz\n`. Three bare
   markers each followed by indented content (or blank) on next
   line. Currently parses chaotically. Once #280 is solved this may
   partially come along. Verify behavior shape before touching.
3. **10-digit ordered marker rejection (#266)** — markdown
   `1234567890. not ok\n`. CommonMark spec restricts ordered-list
   markers to 1–9 digits. Current parser accepts arbitrary digit
   counts. Fix in marker parser's ordered branch; tighten under
   `Dialect::CommonMark` only (Pandoc may agree, verify with pandoc
   first). Single-example fix.
4. **Fence inside blockquote inside list item (#321)** — markdown
   `- a\n  > b\n  \`\`\`\n  c\n  \`\`\`\n- d\n`. The fence is on a
   continuation line of the list item *outside* the blockquote
   (column 2 = list-item content column). Current parse buries
   everything in a blockquote and never recognizes the fence.
   Dispatcher's continuation-line fence detection in `parse_line`
   (~lines 1614-1620) only fires when `bq_depth > 0`; this case has
   the list-item ending the blockquote on the prior line. Likely
   needs a "list-item continuation can be interrupted by a fence at
   content column" branch in the non-blockquote continuation path.
5. **Loose-vs-tight nested loose lists (#312, #326)** — top-level
   loose list with tight inner lists; renderer over-wraps inner
   items in `<p>` or splits at the wrong level. Mixed parser/renderer
   shape.
6. **Lazy / nested marker continuation (#296, #297, #305)** —
   `10) foo\n  - bar` should produce nested list; currently parses
   as a paragraph. Parser issue: ordered-list-with-paren-marker
   doesn't accept a nested bullet without a blank line.
7. **Multi-block content in `1.     code` items (#273, #274)** —
   `1.` followed by 5+ spaces should open a list item whose first
   block is an indented code block. Currently parser falls through
   to plain text.
8. **Setext-in-list-item (#300)** — `- # Foo\n- Bar\n  ---\n  baz\n`
   needs `<h2>Bar</h2>` inside the second item; currently produces
   `<hr />` because the setext underline isn't recognized at the
   list-item content column.
9. **Marker-on-same-line nesting (#298, #299)** — `- - foo\n`
   should be nested lists; parser flattens.
10. **Emphasis and strong emphasis (47 fails)** — flanking-rule and
    autolink-precedence edge cases (#480, #481 are
    autolink-vs-emphasis).
11. **Tabs (4 fails)** — #2, #5, #6, #7 all need tab→space expansion
    with column alignment.
12. **Link reference definitions (12 fails)**, **Setext heading
    multi-line content** (#81, #82, #95) + **#115**, **Pandoc
    setext-in-blockquote losslessness**, **#342 / #148**.
