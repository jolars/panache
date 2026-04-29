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

## Latest session — 2026-04-29 (d)

**Pass count: 539 → 545 / 652 (83.6%, +6)**

Targeted prior recap's #2 (#194 — bracketed ref-def label with escapes). One
parser-shape fix in the ref-def emit walker and three small renderer fixes
unlocked six conformance examples (#194, #545, #550, #553, #555, #566).
Pandoc-verified that no `Dialect` gating is needed — both dialects agree on
the construct shapes affected.

### Single root cause: `emit_reference_definition_content` truncated label at escaped `]`

`emit_reference_definition_content` used `rest.find(']')` to locate the
end-of-label, which stopped at the *escaped* `]` in `[Foo*bar\]]:`. The whole
ref-def fell through to a single TEXT token; the renderer's
`collect_references` then couldn't extract a label, so the def was silently
dropped. Replaced with a small `find_label_close` walker that mirrors the
parser's `escape_next`-style logic. Both Pandoc and CommonMark agree on the
output, so no `Dialect` gate; this is a pure parser-shape gap.

### Single root cause: renderer's `collect_text` stripped `ESCAPED_CHAR` tokens

The inline link `[Foo*bar\]]` produces `LINK_TEXT` with separate `TEXT` and
`ESCAPED_CHAR` tokens; the corresponding ref-def emit (after fix above)
produces a single `TEXT` token containing `Foo*bar\]`. `collect_text`
filtered to `TEXT` only, so the inline side dropped the `\]` and the labels
never matched. Added `collect_label_text` that includes `ESCAPED_CHAR` (and
WHITESPACE/NEWLINE) tokens with their backslashes intact. Per CommonMark
§6.4 example #545, label matching is performed on the *raw* string, not
decoded inline content — so `normalize_label` deliberately does NOT decode
escapes (verified against pandoc and the spec).

### Single root cause: collapsed reference `[foo][]` lookup

`render_link` looked up the empty `LINK_REF` text, which never matches
anything. The fix mirrors what `render_image` already did: when
`LINK_REF.text().trim()` is empty, fall back to `LINK_TEXT` as the label.
Came along with #553, #555, #566 as the side-effect.

### Single root cause: unresolved-link rendering didn't decode escapes

`[bar][foo\!]\n\n[foo!]: /url\n` — per spec example #545 — must NOT match
(matching is on raw text, `foo\!` ≠ `foo!`), but the rendered output should
still decode the `\!` to `!` for display. The unresolved-link branch was
emitting `node.text()` raw; piped it through `decode_backslash_escapes`.

### Files changed

- **Parser (parser-shape gap)**:
  - `crates/panache-parser/src/parser/block_dispatcher.rs`: added a private
    `find_label_close` walker; `emit_reference_definition_content` uses it
    instead of `rest.find(']')` so escaped `]` inside a ref-def label is
    skipped during label extraction.
- **Renderer (renderer gap × 3)**:
  - `crates/panache-parser/tests/commonmark/html_renderer.rs`:
    - Replaced `collect_text` with `collect_label_text` at three sites
      (ref-def label, full-reference link, shortcut link). Removed the
      now-unused `collect_text` function.
    - `normalize_label` documents the §6.4 raw-matching rule. No decoding
      is applied — that is intentional, see #545 commentary.
    - `render_link` now falls back to `LINK_TEXT` when the `LINK_REF` is
      empty (`[foo][]` collapsed reference).
    - The unresolved-reference verbatim-render branches now apply
      `decode_backslash_escapes` to the raw text so `[bar][foo\!]` renders
      as `[bar][foo!]` for display.
- **Parser fixture (pins emit shape)**:
  - `crates/panache-parser/tests/fixtures/cases/reference_definition_label_with_escaped_bracket/`
    — `[Foo*bar\]]:my_(url) 'title (with parens)'\n\n[Foo*bar\]]\n` parses
    with REFERENCE_DEFINITION whose LINK_TEXT contains the full `Foo*bar\]`
    label. No paired Pandoc fixture: pandoc-markdown produces the same
    shape (verified), so a duplicate would be churn.
- **Allowlist additions**:
  - Link reference definitions: +#194.
  - Links: +#545, +#550, +#553, +#555, +#566.

### Don't redo

- Don't decode backslash escapes inside `normalize_label`. CommonMark §6.4
  example #545 is explicit: matching uses raw strings. Decoding would make
  `[bar][foo\!]` resolve to `[foo!]:` (it must not). Verified against
  pandoc which agrees: the link is unresolved and rendered as
  `[bar][foo!]`.
- Don't drop `decode_backslash_escapes` from the unresolved-link verbatim
  branches. Without it, #545 regresses to `[bar][foo\!]` literal output —
  the spec wants the escape decoded for *display* even when matching
  rejects the link. The two paths are separate concerns.
- Don't try to merge `collect_label_text` back into `collect_text`. Other
  callsites (autolinks, raw text dumps) still want the TEXT-only behavior;
  only the label-matching paths need ESCAPED_CHAR included raw. Keeping
  them separate keeps the intent explicit.
- Don't extend `find_label_close` to handle multi-line labels here. The
  emit path receives `content_without_newline`; multi-line ref-def labels
  are still pinned at the dispatcher level (see #208 fixture, prior
  session). If/when multi-line label *emit* is needed, that is its own
  fixture story.
- Don't expect #559 (`[[*foo* bar]]`) to come along from this fix. That is
  a parser-shape bug — panache treats the outer brackets as a LINK
  wrapping the inner LINK; pandoc treats them as literal `[` and `]`
  flanking the inner LINK. Separate, larger change.

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
2. **Empty list item closes the list when followed by blank line (#280)** —
   markdown `-\n\n  foo\n` should produce `<ul><li></li></ul><p>foo</p>`.
   Likely needs a list-blank-handling branch in `core.rs` or
   `list_postprocessor.rs`.
3. **Multi-empty-marker with subsequent indented content (#278)** — chaotic
   parse; partially comes along once #280 is solved.
4. **Tabs (#2, #5, #6, #7)** — column-aware tab expansion needed for
   indented-code inside containers. Substantial.
5. **Block quotes lazy-continuation (#235, #236, #251)** — lazy
   continuation must not extend a list or code block inside a blockquote.
   CommonMark §5.1.
6. **Unicode case folding for label matching (#540)** — `[ẞ]\n\n[SS]: /url`
   should match (German capital sharp S `ẞ` case-folds to `SS`). Current
   `normalize_label` uses `to_lowercase` which lowercases `ẞ` to `ß`
   (single char), missing the `SS` equivalence. Fix: use a Unicode
   case-folding crate or implement the spec's casefold algorithm. Renderer
   change only.
7. **Multi-line label collapsing in inline match (#541)** —
   `[Foo\n  bar]: /url\n\n[Baz][Foo bar]` should match. The ref-def label
   is `Foo\n  bar`; `normalize_label` collapses whitespace correctly.
   But the inline `[Baz][Foo bar]` LINK_REF normalize is `foo bar`. So
   they should match. Suspect the parser drops the multi-line ref-def
   into REFERENCE_DEFINITION but the collected label has unexpected
   bytes. Probe before assuming the cause.
8. **Fence inside blockquote inside list item (#321)** — list-item
   continuation can be interrupted by a fence at content column;
   dispatcher's continuation-line fence detection only fires when
   `bq_depth > 0`.
9. **Loose-vs-tight nested loose lists (#312, #326)** — mixed
   parser/renderer shape. #326's outer list should be loose (blank line
   between items) so `a` and `d` need `<p>` wrappers, but renderer treats
   it as tight. Likely a renderer-side loose-detection gap.
10. **Lazy / nested marker continuation (#298, #299)** — `- - foo` and
    `1. - 2. foo` should produce nested list-on-same-line; parser flattens.
11. **Multi-block content in `1.     code` items (#273, #274)** —
    `1.` followed by 5+ spaces should open a list item whose first block is
    an indented code block.
12. **Setext-in-list-item (#300)** — `- # Foo\n- Bar\n  ---\n  baz` needs
    `<h2>Bar</h2>` inside the second item.
13. **Nested-bracket label resolution (#559)** — `[[*foo* bar]]` should
    render as `[<a>*foo* bar</a>]` with the outer brackets literal.
    Parser currently treats the outer `[ ]` as a LINK wrapping the inner
    LINK. Fix in the inline parser.
14. **Emphasis and strong emphasis (47 fails)** — flanking-rule and
    autolink-precedence edge cases.
15. **Ref-def dialect divergence #201** — `[foo]: <bar>(baz)`. Fix requires
    gating the strict-EOL check on `Dialect::CommonMark` and adding a
    paired fixture; Pandoc-markdown accepts it, current code rejects both.
    Low priority.

