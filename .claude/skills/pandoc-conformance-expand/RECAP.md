# Pandoc-conformance recap

Rolling handoff between sessions. Keep terse. Read `report.txt` for the full
state; this file is judgment calls only.

## Suggested next targets

Ranked by likely shared root cause and leverage. Numbers in parentheses are
the count of currently-failing imports remaining under that bucket in the
latest `report.txt`.

1. **Citations (~14 Unsupported "CITATION")** — `citations`, plus
   embedded inline cites in many natural-text cases. Projector has zero
   coverage. Pandoc shape: `Cite [Citation { citationId, citationPrefix,
   citationSuffix, citationMode = AuthorInText | NormalCitation |
   SuppressAuthor, citationNoteNum, citationHash }] [Inline]`. The
   `citationMode` and the 5-field `Citation` record make this the most
   structurally heavy projector entry — but all 14 cases share one fix.
2. **Tables — Simple/Headerless/Multiline/Grid (~13)** — only
   `PIPE_TABLE` is projected so far. Simple/Multiline/Grid tables
   need: (a) explicit `ColWidth N` math derived from dash counts vs
   total width (Multiline/Grid); (b) alignment derivation from
   header column position (Simple/Headerless); (c) parser fix:
   trailing `-------` separator in headerless simple table is
   currently parsed as a `TABLE_ROW` of dash cells (parser bug).
3. **Example list document-level numbering (#114 alone)** — the
   `Example` style classifier landed; what remains is that pandoc
   numbers Example items across the *entire document*, not within
   a single OrderedList. `1, 2` for the first list, then `3, 4, 5`
   for the second list, etc. Plus `(@label)` reference resolution to
   the matching example number (currently projects as
   `Unsupported "CITATION"`). Document-level Example counter — same
   shape as the heading-id pre-pass that landed earlier
   (`heading_id_by_offset`).
4. **Lists — `lists_fancy` (#115) needs a parser fix** — `I.`
   (single space, not double) is parsed as a list by panache but
   pandoc rejects (single capital letter requires double-space).
   Parser is too permissive on uppercase markers. Parser-shape gap.
5. **Footnotes — DefinitionList-inside-Note (~2 — cases #66, #67)** —
   the basic Note resolver landed; what remains in this bucket is the
   parser-shape gap where a definition list inside a footnote body
   isn't parsed as `DefinitionList`. Parser fix territory, not
   projector.
6. **Definition list nesting (~2 — cases #43, #44, #45)** —
   `definition_list_nesting`, `*_pandoc_loose_compact`,
   `definition_list`. Per-item loose/tight detection landed (#179);
   #44 still has a nested-list-inside-definition offset
   propagation gap (the `LIST` carries a leading WHITESPACE sibling
   that `list_item_content_offset` doesn't see); #43 / #45 have
   parser-shape issues where nested bullets inside definitions
   aren't parsed as `BulletList`.
7. **HTML blocks / fenced divs with raw HTML adjacency (~3)** —
   `writer_html_blocks`, `html_block` cases with adjacent HTML.
   Pandoc splits each `<tag>` line into its own `RawBlock`; we
   coalesce them into one block. Parser-shape gap: HTML_BLOCK
   currently spans contiguous HTML lines; would need to split on
   tag boundaries.
8. **Misc remaining**:
   - `pandoc_title_block` (Unsupported "PANDOC_TITLE_BLOCK" — pandoc
     converts to Meta).
   - `links` (case #101) where pandoc `[text](url){.cls key=value}`
     attaches a Link attribute that's currently dropped (parser-shape
     gap: ATTRIBUTE not attached to LINK in CST).
   - `double_backslash_math` (#51) — `\(`/`\[` shouldn't trigger
     inline math parsing. Parser-shape gap.
   - `ignore_directives` (#79) — `<!-- … -->` inside list items
     should project as RawBlock not as part of a Plain.
   - `indented_code_after_atx_heading_pandoc` (#82) — parser doesn't
     start a code block after an ATX heading.
   - `images` (#81) — Figure block (`Unsupported "FIGURE"`).
     Pandoc emits `Figure` for an image-only paragraph followed by
     an explicit caption.
   - `emphasis_nested_inlines` (#56) — single edge case where
     unclosed `~~` is split as `Subscript [] + Str`. Niche.
   - `nested_headings_in_containers` (#128) — parser doesn't parse
     `# Heading` inside list items / definition items as Header.

Suggested first session: **#1 (Citations)** is still the largest
single-fix unlock (14 cases), but is also the most structurally heavy
projector entry. The pre-pass infrastructure for document-level
counters (`heading_id_by_offset`) is the right template for both
Citations and Example-list numbering — once the pre-pass shape is
in place, both can land in adjacent sessions.

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
- **Pass before → after**: 134 → 147 / 187 (+13 imports). All wins are
  **projector-only** again — no parser code was touched this session.
  CommonMark allowlist stayed green; full parser-crate suite green;
  clippy + fmt clean.
- **What landed (all in `tests/pandoc/native_projector.rs`)**:
  - **Pandoc abbreviations (`+abbreviations` extension).** Added a
    fixed list of pandoc's default abbrevs (verbatim from
    `pandoc/data/abbreviations`, ~80 entries) and an
    `apply_abbreviations` post-pass run after `smart_quote_pairs`
    inside `coalesce_inlines_inner`. Rule: a `Str` ending in an
    abbrev followed by `Space` has the `Space` replaced by `\u{a0}`
    (NBSP) appended to the `Str`, and the next `Str` (if any)
    merged in. The match is suffix-anchored: the abbrev must end the
    Str and be preceded by either start-of-Str or a non-letter,
    non-dot char (matches pandoc's parser behavior where the abbrev
    is parsed as an isolated token before coalescing). Recurses into
    `Quoted` content because `Quoted` is built inside
    `smart_quote_pairs` and bypasses the per-marker
    `coalesce_inlines_keep_edges` recursion. Unlocked **#152, #157**.
  - **OrderedList style/delim classifier.** Replaced the
    always-`Decimal/Period` stub with `classify_ordered_marker` that
    mirrors pandoc's `anyOrderedListMarker` parser: try `decimal` →
    `exampleNum` (`@label`) → `defaultNum` (`#`) → `romanOne`
    (single `i`/`I`) → single-letter alpha → multi-char roman, in
    that order. Added `roman_to_int` for roman parsing. Delimiters
    derived from the marker punctuation: `(X)` →
    `TwoParens`, `X)` → `OneParen`, `X.` → `Period`. `#` style
    forces `DefaultDelim` regardless of punctuation (per pandoc's
    `inPeriod`). Unlocked **#117** and contributed to **#116**.
  - **Task-list checkbox glyph.** `list_item_blocks` now reads the
    `TASK_CHECKBOX` token from the `LIST_ITEM` and prepends
    `Str "\u{2610}"` (☐) or `Str "\u{2612}"` (☒) plus a `Space` to
    the first non-empty `PLAIN`/`PARAGRAPH` content. The checkbox
    only applies to the first inline-content block per item; later
    blocks are unchanged. Unlocked **#118, #120, #121**.
  - **Code-block language normalization.** Added `normalize_lang_id`
    mirroring pandoc's `toLanguageId`: lowercase, `c++` → `cpp`,
    `objective-c` → `objectivec`. Applied at both attribute-block
    and shortcut paths in `code_block_attr`. Unlocked **#113** (in
    combination with the offset fix).
  - **Nested-list-item content offset includes leading
    WHITESPACE.** `list_item_content_offset` previously only counted
    `LIST_MARKER + WHITESPACE-after-marker`. Nested list items also
    carry leading WHITESPACE *before* the marker (the outer item's
    content offset). Including those spaces makes the cumulative
    offset correct for stripping nested fenced/indented code-block
    bodies. The CODE_BLOCK arm in `list_item_blocks` now routes
    *both* fenced and indented code through
    `indented_code_block_with_extra_strip` so the offset gets
    stripped uniformly.
  - **Definition-item loose vs. tight.** `definition_blocks` now
    takes a `loose` flag set by `is_loose_definition_item`. The
    rule: a `DEFINITION_ITEM` is loose iff there is a `BLANK_LINE`
    between its `TERM` and the first `DEFINITION` (per-item, not
    per-definition). When loose, all `PLAIN` children become `Para`;
    when tight, they stay `Plain`. Unlocked **#139** and **#179**.
  - **Raw block via `{=format}` info string.** Added
    `code_block_raw_format` that detects the pandoc raw-attribute
    form (info string of the shape `{=fmt}`, no other attrs). When
    matched, `code_block` and `indented_code_block_with_extra_strip`
    return `RawBlock(fmt, content)` instead of `CodeBlock`. Unlocked
    **#40, #140**.
  - **Tab expansion in code blocks.** Pandoc tab-expands code-block
    bodies to 4-col tab stops *before* any indent stripping. Added
    `expand_tabs_to_4` and applied it: in
    `strip_indented_code_indent` (before the 4-col strip), in
    `indented_code_block_with_extra_strip` (before the leading-space
    strip), and in `code_block` for fenced bodies. Unlocked **#83**.
    Also added `advance_col` so `definition_content_offset` measures
    in *columns* (with tab-rounding) rather than chars — without
    this, `:\t` was reading as offset 2 instead of the correct
    column 4.
- **Cases unlocked** (+13, all allowlisted under `# imported`):
  - 40 (code_blocks_raw) — `{=format}` → RawBlock
  - 83 (indented_code_mixed_tab_space) — tab expansion
  - 113 (lists_code) — c++→cpp, nested code offset
  - 116 (lists_nested) — fell out from list classifier + offset
  - 117 (lists_ordered) — `#.` DefaultStyle
  - 118 (lists_task) — task checkbox glyphs
  - 120 (lists_wrapping_nested) — task checkbox in nested
  - 121 (lists_wrapping_simple) — task checkbox in simple
  - 139 (plain_continuation_edge_cases) — definition loose/tight
  - 140 (raw_blocks) — `{=format}` → RawBlock
  - 152 (sentence_wrap_abbreviations) — abbreviation NBSP
  - 157 (sentence_wrap_inline_code_sentence_end) — abbreviation NBSP
  - 179 (writer_definition_lists_multiblock) — definition loose
- **Files changed (classified)**:
  - **projector** (single file): `tests/pandoc/native_projector.rs`
  - **allowlist**: `tests/pandoc/allowlist.txt` (+13 imported IDs)
- **Don't redo**:
  - The `PANDOC_ABBREVIATIONS` list is a verbatim copy of
    `pandoc/data/abbreviations`. When pandoc updates that file,
    refresh — but don't try to derive abbreviations from heuristics
    (e.g. "ends with `.`"). Pandoc rejects `etc.` and `X.Y.Z.`
    despite both ending with a dot — the explicit allowlist is
    load-bearing.
  - The abbreviation match requires the char preceding the abbrev
    inside the Str to be neither alphanumeric nor `.`. The `.`
    exclusion is critical: `a.M.D.` must NOT match (pandoc rejects
    because its tokenizer parses the whole thing as one Str token,
    then the result `a.M.D.` isn't in the abbrev set). Don't relax
    to `!is_alphanumeric()` alone.
  - `apply_abbreviations` recurses into `Quoted` because Quoted
    content is built inside `smart_quote_pairs` *after* its source
    has been coalesced — the parent's abbrev pass won't see Quoted
    contents. Other inline wrappers (Emph/Strong/Link/Image/Note)
    have their content built via their own `coalesce_inlines_*`
    call, so they get the abbrev pass for free. Don't add explicit
    recursion for those — it'd run twice.
  - The ordered-list classifier follows pandoc's *parser order*: try
    decimal first, then example, then default, then romanOne, then
    single-letter alpha, then multi-char roman. Critical: `i.`
    becomes `LowerRoman` (not `LowerAlpha`) because `romanOne` runs
    before `lowerAlpha` in pandoc. Don't reorder. Multi-char
    lowercase non-roman (e.g. `ab.`) won't reach the classifier
    because the parser wouldn't accept it as a list marker — the
    fallback `Decimal` arm exists only to keep the projector
    rendering rather than panicking on parser-permissive markers.
  - The task-checkbox glyph is `\u{2610}` (BALLOT BOX) for `[ ]` and
    `\u{2612}` (BALLOT BOX WITH X) for `[x]` / `[X]`. Pandoc emits
    them as a single-char `Str` followed by `Space`; do *not* fold
    the glyph + space into one `Str` (`\u{2612} foo`). Pandoc keeps
    them separate so it can reflow.
  - `expand_tabs_to_4` uses 4-column tab stops measured from column
    0 of each line. The CST already strips outer container offsets
    *visually*, but the body line text is raw. Don't adjust the
    starting column — tabs in source columns N still expand based
    on the real column N, which equals the byte column once we're
    in CODE_CONTENT (the parser doesn't shift content columns).
  - `definition_content_offset` returns *columns* (tab-aware), not
    chars. The strip in `indented_code_block_with_extra_strip`
    operates on tab-*expanded* body, so the offset must be in
    columns to match. Don't switch to char-counting; it'll silently
    over/under-strip on tab-indented definitions.
  - `code_block_raw_format` requires the info string to be exactly
    `{=fmt}` with no spaces, classes, ids, or kvs. If pandoc
    accepts `{=html .extra}` etc. in some future version, this is
    where to relax — but probe first; current pandoc rejects.
- **Next**: same as before — **Citations (~14 remaining)** is the
  largest single-fix unlock but heavy. Smaller leverage targets
  now:
  - **#114 lists_example** — needs document-level Example numbering
    (counter across all OrderedList(_, Example, _) in the doc) plus
    `(@label)` reference resolution. The `heading_id_by_offset`
    pre-pass is the right template. Single-case unlock once both
    pieces land.
  - **#43/#44/#45 definition list** (3 cases) — multiple parser +
    projector gaps; #44 in particular has the nested-list-inside-
    definition offset propagation issue (LIST has leading
    WHITESPACE sibling that `list_item_content_offset` doesn't
    see).
  - **Tables (~13 across simple/headerless/multiline/grid)** — all
    still gated on parser-shape and projector buildout.
  - **#115 lists_fancy** — parser too permissive on uppercase
    markers (`I.` with single space accepted as list).
  - **Footnotes #66/#67** — definition-list-inside-Note parser
    shape.
  - **HTML block coalescence (#78/#181)** — parser splits each
    `<tag>` line into separate raw blocks under pandoc; we
    coalesce.
  - **Misc remaining**: #51 double-backslash math (parser-shape:
    `\(`/`\[` shouldn't trigger inline parsing), #79
    ignore_directives, #82 indented-code-after-heading, #128
    nested-headings-in-containers (parser).

## Previous session (2026-05-01 earlier)

- **Pass before → after**: 123 → 134 / 187 (+11 imports). All wins are
  **projector-only** again — no parser code was touched this session.
  CommonMark allowlist stayed green; full parser-crate suite green;
  clippy + fmt clean.
- **What landed (all in `tests/pandoc/native_projector.rs`)**:
  - **Misc small fixes from recap-#9.**
    - **#92 (HTML span attrs).** `<span class="rtl">…</span>` was
      emitted as `Unsupported "BRACKETED_SPAN"`. The parser CST shape
      was already correct; the projector just needed to (a) read
      `SPAN_ATTRIBUTES` via `children_with_tokens()` (it's a *token*
      for HTML form, but a *node* for `[text]{.cls}` form), and (b)
      parse HTML-style `class="x" id="y" key="z"` attributes via a
      new `parse_html_attrs` helper. `class` splits on whitespace.
    - **#29 (autolink scheme allowlist).** `<m:abc>` was projected as
      a uri Link, but pandoc rejects the autolink (scheme `m` is too
      short / not in pandoc's known-schemes set) and falls back to
      `RawInline (Format "html") "<m:abc>"`. Added the full pandoc
      schemes list (sorted, ~280 entries from
      `pandoc/src/Text/Pandoc/URI.hs`) and an `is_known_uri_scheme`
      check. Anything that isn't email *and* isn't a known scheme is
      now projected as RawInline html.
    - **#41 (all-space inline code).** `strip_inline_code_padding`
      wasn't stripping all-whitespace inline code to empty. Pandoc
      does (`( )` → `Code "" ""`). Added a fast path before the
      surround-pair-strip arm.
    - **#87/#88 (link dest URL escaping).** `parse_link_dest` was
      truncating at the first space (so `[link](/my uri)` lost
      `uri`) and not stripping angle-bracket wrappers (so
      `[link](<foo(and(bar)>)` kept the `<…>`). Rewrote to (a) strip
      `<…>` wrapping, (b) split URL/title only when the trailing
      whitespace is followed by `"`/`'`/`(`, (c) percent-escape per
      pandoc's `escapeURI` set: ASCII whitespace + `<>|"{}[]^\``.
      Backslash and Unicode are preserved (pandoc-tested).
  - **Heading-id pre-pass (#167).** `***\n---\n` projects as a setext
    H2 with content `***`, but our slugifier returned `""` (no
    alphanum), so the id was empty. Pandoc's auto-id falls back to
    `section` and disambiguates duplicates against ALL prior
    auto-generated ids (but explicit `{#x}` ids are kept verbatim
    even on conflict — pandoc only warns). Replaced
    `fixup_empty_heading_ids` (which only handled bare-marker
    headings with empty *inlines*) with a `RefsCtx` pre-pass that
    walks every HEADING in document order, classifies as
    explicit/auto via `heading_id_with_explicitness`, applies
    `section`/disambiguation only to auto, and stores the final id
    in `heading_id_by_offset`. `heading_block` now consults that map
    instead of slugifying inline.
  - **Loose-list "blank between blocks of one item" (#105/#107/#158).**
    `is_loose_list` only checked for blanks *between items* and
    items containing a `PARAGRAPH`. It missed CommonMark's other
    half: a list is also loose if any single item directly contains
    a blank line between two of its block-level children. Added
    `has_internal_blank_between_blocks` — but with a critical caveat
    surfaced by #61 (regressed mid-session): bare-marker lines emit
    an *empty* PLAIN node (NEWLINE only), and pandoc does *not*
    count that as the "first block". Added `child_is_empty_plain`
    to skip those. Verified vs `-\n\n  foo` (tight, Plain) and `-
    bar\n\n  foo` (loose, Para) by probing pandoc directly.
  - **List-item content offset for indented code (#107/#106).**
    Indented code blocks inside list items are doubly indented in
    the CST (item-content offset + the 4-space code-block indent).
    `list_item_blocks` now computes `list_item_content_offset` and
    routes non-fenced code through
    `indented_code_block_with_extra_strip`. The offset rule
    (verified against pandoc): bare-marker line (no WHITESPACE
    after LIST_MARKER) → offset = marker width; marker followed by
    space(s) → offset = marker_width + ws_width.
  - **Definition body content offset for fenced code (#176).**
    Same shape as list items: a fenced code block inside a
    `: …` definition has the body's indent on each content line.
    Added `definition_content_offset` and threaded it through
    `definition_blocks`; also generalized
    `indented_code_block_with_extra_strip` to skip
    `strip_indented_code_indent` when fenced (the offset strip is
    sufficient — no extra 4-space removal).
- **Cases unlocked** (+11, all allowlisted under `# imported`):
  - 29 (autolink_strict_validation_pandoc) — known-schemes allowlist
  - 41 (code_spans) — all-space inline code → empty
  - 87 (inline_link_dest_angle_brackets_with_parens) — `<…>` strip
  - 88 (inline_link_dest_strict_pandoc) — space → %20
  - 92 (issue_175_native_span_unicode_panic) — HTML span attrs
  - 105 (list_item_blank_line_inside) — internal-blank → loose
  - 106 (list_item_empty_marker_indented_code_next_line) — bare
    marker offset
  - 107 (list_item_indented_code) — list-item code-block strip
  - 158 (sentence_wrap_lazy_continuation) — fell out from #105
    (loose-list rule)
  - 167 (setext_text_thematic_break_pandoc) — `section` fallback
  - 176 (unicode) — definition fenced-code offset strip
- **Files changed (classified)**:
  - **projector** (single file): `tests/pandoc/native_projector.rs`
  - **allowlist**: `tests/pandoc/allowlist.txt` (+11 imported IDs)
- **Don't redo**:
  - The pandoc URI-scheme allowlist
    (`PANDOC_KNOWN_SCHEMES`) is a verbatim copy of pandoc's
    `Text.Pandoc.URI.schemes` (sorted alphabetically for
    `binary_search`). When pandoc adds/removes a scheme, refresh
    this list — but don't try to derive it from `Network.URI`
    parsing rules. The test for "is this a URI autolink?" is *not*
    RFC3986 conformance; it's "is this scheme in pandoc's
    allowlist?".
  - `parse_html_attrs` is intentionally minimal and does *not*
    handle attribute-value-less keys (e.g. `<input disabled>`).
    Pandoc's HTML-span reader doesn't need them — adding support
    would require a different code path. Leave it narrow.
  - The percent-escape set in `escape_link_dest` is exactly
    `isSpace || c \in "<>|\"{}[]^\``" — copied from pandoc's
    `escapeURI`. Backslash is *not* in the set, even though it
    would be a syntax-significant char in raw URLs. Don't add
    backslash without re-probing pandoc — `[a](foo\\bar)` →
    `"foo\\bar"`, preserved.
  - The auto-id pre-pass uses `text_range().start()` as the map key
    (a `u32` since rowan's `TextSize` is u32-based). Don't change
    the key type — explicit `usize` would conflict with rowan's
    type. Heading offsets are unique per document.
  - `child_is_empty_plain` only counts `NEWLINE`/`WHITESPACE` tokens
    as "empty". Don't broaden to count, e.g., comment-only PLAIN
    nodes — that's not what pandoc considers empty.
  - The list-item content offset is *measured*, not assumed: the
    bare-marker rule (offset = marker_width, no `+1`) is verified
    against pandoc and matches its behavior, contradicting the
    naive CommonMark §5.2 reading. Don't refactor to a "marker
    width + 1" universal rule.
  - `indented_code_block_with_extra_strip` now branches on
    `is_fenced` to skip the legacy `strip_indented_code_indent`
    pass when the block is fenced. The offset strip alone is
    sufficient for fenced; layering both produces over-strip in
    nested `: ` + ` ``` ` cases.
- **Next**: same as before — **Citations (~14 remaining)** is the
  largest single-fix unlock but heavy. Smaller leverage targets
  now:
  - **#43/#44/#45 definition list** (3 cases) — multiple parser
    + projector gaps; #44 in particular has a fenced code with
    tabs that lose tab-stops in the projector.
  - **Tables (~18 across simple/headerless/multiline/grid)** — all
    still gated on parser-shape and projector buildout.
  - **Lists (#113/#114/#115/#116/#117/#118 etc.)** — fancy/example/
    ordered styles still need `LowerRoman`/`UpperAlpha`/`OneParen`/
    etc. projector entries.
  - **Footnotes #66/#67** — definition-list-inside-Note parser
    shape.
  - **HTML block coalescence (#78/#181)** — parser splits each
    `<tag>` line into separate raw blocks under pandoc; we
    coalesce.
  - **Misc remaining**: #51 double-backslash math (parser-shape:
    `\(`/`\[` shouldn't trigger inline parsing), #79
    ignore_directives (block-level `<!-- -->` comment is RawBlock
    in pandoc but our `<!--…-->` inside lists projects as
    RawBlock-with-leading-spaces).

