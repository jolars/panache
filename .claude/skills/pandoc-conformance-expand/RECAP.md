# Pandoc-conformance recap

Rolling handoff between sessions. Keep terse. Read `report.txt` for the full
state; this file is judgment calls only.

## Suggested next targets

Ranked by likely shared root cause and leverage. Numbers in parentheses are
the count of currently-failing imports remaining under that bucket in the
latest `report.txt`.

1. **Citations (~16 Unsupported "CITATION")** — `citations`, plus
   embedded inline cites in many natural-text cases. Projector has zero
   coverage. Pandoc shape: `Cite [Citation { citationId, citationPrefix,
   citationSuffix, citationMode = AuthorInText | NormalCitation |
   SuppressAuthor, citationNoteNum, citationHash }] [Inline]`. The
   `citationMode` and the 5-field `Citation` record make this the most
   structurally heavy projector entry — but all 16 cases share one fix.
2. **Footnotes (~22 combined: 11 FOOTNOTE_REFERENCE + 11 FOOTNOTE_DEFINITION,
   plus 2 INLINE_FOOTNOTE)** — `footnote_*`, `inline_footnotes`,
   `reference_footnotes`. Projector has no cross-reference resolver: each
   `FOOTNOTE_REFERENCE` needs to look up its `FOOTNOTE_DEFINITION` body
   and emit `Note [Block]` inline. The definitions then disappear from
   the body block list. The reference-link resolver landed this session
   provides a similar pattern (collect at document level, lookup, emit) —
   reuse the structure but with FOOTNOTE_DEFINITION bodies as the values.
3. **Definition list nesting (~3 — cases #43, #44, #45)** —
   `definition_list_nesting`, `*_pandoc_loose_compact`,
   `definition_list` (the big multi-shape one). Cases with nested bullet
   lists inside definitions, code blocks with non-standard indent
   stripping, and Plain-vs-Para distinctions still fail. Mostly
   **parser-shape gaps** (parser doesn't recognize `- List` inside a
   definition body).
4. **Smart-quote `Quoted` restructuring edge cases (~5)** — Quoted
   spanning markup atoms (`"foo *bar*"`), Quoted across multiple inlines
   we currently skip over, and apostrophe-as-close-quote disambiguation.
   Improving `smart_quote_pairs()` to handle markup-atoms in the search
   range would catch most of these.
5. **Tables — Simple/Headerless/Multiline/Grid (~18 combined: 5 SIMPLE,
   1 headerless, 6 MULTILINE, 7 GRID)** — only `PIPE_TABLE` is projected
   so far. Simple/Multiline/Grid tables need:
   (a) explicit `ColWidth N` math derived from dash counts vs total
   width (Multiline/Grid);
   (b) alignment derivation from header column position
   (Simple/Headerless);
   (c) parser fix: trailing `-------` separator in headerless simple
   table is currently parsed as a `TABLE_ROW` of dash cells (parser
   bug).
6. **Lists — fancy/nested/ordered styles + parser shape (~10)** —
   `lists_fancy`, `lists_example`, `lists_nested`, `lists_wrapping_*`,
   `list_item_*`, `list_nested_same_line_marker_pandoc`,
   `lazy_continuation_deep`. Mix of:
   (a) ordered-list `OrderedList ( N , Style , Delim ) [...]` styles
   beyond `Decimal/Period` (need `LowerRoman`, `UpperAlpha`,
   `OneParen`, `TwoParens`, `Example`, `DefaultStyle`);
   (b) parser drops nested list / lazy continuations in some shapes.
7. **Emphasis edge cases (~7)** — `emphasis_*_pandoc`, `emphasis_complex`,
   `emphasis_nested_inlines`. Some are TEXT-token granularity diffs
   (TEXT-coalescence diffs are benign — pandoc-native doesn't pin TEXT
   split points), others are parser-shape gaps in flanking-rule edge
   cases.
8. **HTML blocks / fenced divs that contain raw HTML adjacency (~3)** —
   `writer_html_blocks`, `html_block` cases with adjacent HTML. Pandoc
   splits each `<tag>` line into its own `RawBlock`; we coalesce them
   into one block. Parser-shape gap: HTML_BLOCK currently spans
   contiguous HTML lines; would need to split on tag boundaries.
9. **Misc small fixes** —
   `autolink_strict_validation_pandoc` (`<m:abc>` should become
   `RawInline (Format "html") "<m:abc>"` not a uri Link),
   `pandoc_title_block` (Unsupported "PANDOC_TITLE_BLOCK" — pandoc
   converts to Meta), `code_spans` quirk where pandoc strips all-space
   inline code to empty, and `links` (case #101) where pandoc
   `[text](url){.cls key=value}` attaches a Link attribute that's
   currently dropped.

Suggested first session: **#1 (Citations)** is the largest single-fix
unlock (16 cases), but is also the most structurally heavy projector
entry. **#2 (Footnotes)** is comparable in size and now has a clear
template to follow (the document-level resolver landed this session).
Either is a high-leverage target.

## Don't redo

- The CST → pandoc-native projector is **test-only** at
  `crates/panache-parser/tests/pandoc/native_projector.rs`. Do not move
  it under `src/` or wire it into the public API.
- Slugifier in the projector is intentionally a copy of
  `panache-formatter::utils::pandoc_slugify` — the parser crate cannot
  depend on the formatter crate (would cycle). Keep it inline.
- `expected.native` files are pinned to pandoc 3.9.0.2 (see
  `tests/fixtures/pandoc-conformance/.panache-source`). Regenerate via
  `scripts/update-pandoc-conformance-corpus.sh` only when intentionally
  bumping pandoc.
- The bulk-import script (`import-pandoc-conformance-from-parser-fixtures.sh`)
  uses leading-zero-stripped IDs to avoid POSIX shell octal interpretation
  in `$((...))`. Do not refactor it back to direct `$((0025 + 1))`-style
  arithmetic.
- `imported-*` cases live alongside hand-curated cases under
  `corpus/`. The script wipes prior `*-imported-*` dirs before re-running,
  so the import is idempotent — but **do not hand-edit** an imported case's
  `input.md` or `expected.native`. If a hand-curated variant is needed,
  copy it into a new `<NNNN>-<section>-<slug>/` dir with a non-`imported`
  section prefix.
- The reference-link resolver uses a `thread_local!<RefCell<RefsCtx>>`
  populated at `project()` entry. Cleared at `project()` exit. Do **not**
  refactor to a parameter-threading model — every projector function
  takes only `&SyntaxNode`, and the rewrite would touch the entire
  module for no functional gain.
- Inline-link vs reference-link discrimination uses presence of
  `LINK_DEST_START` / `IMAGE_DEST_START` *tokens* — not `LINK_DEST` node.
  An empty `[Empty]()` still has `LINK_DEST_START`, so the token check
  is the correct discriminator. (Reference-style `[foo][bar]` has no
  `LINK_DEST_START` at all.)
- Unresolved reference links emit `Str "[" + text + "]<suffix>"` rather
  than a `Link` with empty dest, matching pandoc's "preserve original
  bytes" behavior. Do not switch to `Link ("","")` — it produces a
  spurious Link node in the output.
- Reference labels are normalized via `normalize_ref_label()`:
  unescape ASCII-punct backslash escapes, lowercase, collapse runs of
  whitespace to one space, trim. Both def labels (raw `LINK_TEXT.text()`
  with literal escapes) and body labels (mix of TEXT + ESCAPED_CHAR
  tokens, `text()` already 9-byte raw) feed this same normalizer so
  they match.

## Latest session

- **Date**: 2026-05-01
- **Pass before → after**: 103 → 115 / 187 (+12 imports). All wins are
  **projector-only** — no parser code was touched this session. The
  CommonMark allowlist stayed green; full parser-crate suite green.
- **What landed (all in `tests/pandoc/native_projector.rs`)**:
  - Document-level reference resolver. `build_refs_ctx()` walks the
    full tree (including nested REFERENCE_DEFINITION inside BlockQuote
    / Div / List), parses each into `(label, url, title)`, stores in a
    thread_local `RefsCtx`. Also collects HEADING ids for
    `implicit_header_references` resolution.
  - `parse_reference_def()` extracts URL+title from the
    REFERENCE_DEFINITION's tail text, supporting `<bar>(baz)` (angle
    URL + paren title) and multiline `[label]:\n/url` shapes.
  - `normalize_ref_label()` does the CommonMark/pandoc label
    normalization (unescape backslash-escapes, lowercase, collapse
    whitespace).
  - Refactored `link_inline`/`image_inline` into
    `render_link_inline`/`render_image_inline` (push-onto-Vec) so
    unresolved refs can emit multiple Str inlines (matching pandoc's
    "preserve original bytes" behavior). `inlines_from` and
    `inlines_from_marked` route LINK / IMAGE_LINK nodes through these.
  - Inline-vs-reference discrimination uses `LINK_DEST_START` /
    `IMAGE_DEST_START` *token* presence (not the `LINK_DEST` node, which
    is also present empty for `[Empty]()`).
  - Unresolved shortcut `[label]` falls back to heading-id lookup
    (`pandoc_slugify(label) ∈ heading_ids`) before emitting plain text.
- **Cases unlocked**: 12 new imported cases (37, 59, 100, 102, 142,
  143, 144, 145, 147, 149, 151, 168). All allowlisted.
- **Files changed (classified)**:
  - **projector** (single file): `tests/pandoc/native_projector.rs`
  - **allowlist**: `tests/pandoc/allowlist.txt` (+12 imported IDs)
- **Don't redo**:
  - The thread_local state is intentional. Don't refactor to
    parameter-threading — see "Don't redo" entry above.
  - Unresolved-ref bytes-preserving behavior is intentional. Don't
    emit `Link("","")`.
  - LinkDestStart token (not the LinkDest node) is the
    inline-vs-reference discriminator.
- **Next**: pick **#1 (Citations)** for ~16 unlocks, or **#2
  (Footnotes)** for ~22 unlocks now that the document-level resolver
  pattern is in place. Footnotes follow the same lookup-table pattern
  as references, just with Block lists as values instead of (url,
  title).
