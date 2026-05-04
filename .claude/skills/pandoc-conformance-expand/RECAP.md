# Pandoc-conformance recap

Rolling handoff between sessions. Keep terse. Read `report.txt` for the full
state; this file is judgment calls only.

## Suggested next targets

Ranked by likely shared root cause and leverage. Numbers in parentheses are the
count of currently-failing imports remaining under that bucket in the latest
`report.txt`.

1. **Citations proper (DONE 2026-05-04 --- #38 unlocked)**. Projector
   now emits full `Cite [Citation { citationId, citationPrefix,
   citationSuffix, citationMode, citationNoteNum, citationHash }]
   [Inline]` shape with prefix/suffix inline parsing (smart-typography
   applied), `@key [locator]` AuthorInText absorption via
   `inlines_from`-level look-ahead, and a doc-order `noteNum` pre-pass
   that increments per Cite group outside footnotes and once per
   FOOTNOTE_REFERENCE entry (all cites inside a footnote share that
   number).
2. **Tables --- remaining (\~1)** --- Simple/Multiline/Headerless basics
   landed, plus multiline inline-formatting, short-header,
   indented-pipe-table-with-caption-attributes, **grid_table multi-line
   cells / TableFoot / block-reparse (DONE 2026-05-03 — unlocked #68 and
   #70)**. What remains:
   - **#71** (grid_table_planets) --- rowspan/colspan layout. Pandoc emits
     `RowSpan N` / `ColSpan N` for cells whose `+   +-----+`-style
     separators omit the column-divider `+` to span the cell into the next
     row, and similarly for column merges. Our `grid_column_ranges`
     helper currently skips empty `+`-to-`+` ranges (which represent
     spans), so spanned cells produce wrong column ordering. Heavy.
3. **Footnotes --- DefinitionList-inside-Note (DONE 2026-05-03)** --- both #66
   and #67 unlocked. Parser-shape fix in `handle_footnote_open_effect` to
   detect that the first content line is a term, plus a continuation-policy
   fix so blank lines inside a footnote-body don't close the
   DefinitionList/DefinitionItem when the next `:` marker still sits at the
   footnote's content column. Companion formatter arm added for the new
   `FOOTNOTE_DEFINITION > DEFINITION_LIST` first-child shape so the term
   stays on the same line as `[^id]:`.
4. **Definition list nesting (\~1 --- case #43)** ---
   `definition_list`. #44 unlocked 2026-05-03 (parent-LIST WHITESPACE
   added to `list_item_content_offset`). #43 still has parser-shape
   issues where nested bullets inside definitions aren't parsed as
   `BulletList` in *some* contexts (the bare-leading-list path now
   handles the simple case but the `Orange\n: > a / : - List with lazy
   continuation` paths inside #43 still fail).
5. **HTML blocks / fenced divs with raw HTML adjacency (DONE 2026-05-04 ---
   #181 unlocked)**. Projector now splits multi-line block-level HTML
   blocks per line via `emit_html_block`: tag-only lines emit as RawBlock,
   inline-text lines emit as Plain (with parsed inlines). Verbatim
   constructs (`<!-- -->` comments, `<script>`, `<style>`, `<pre>`,
   `<textarea>`, processing instructions, declarations) still emit as a
   single RawBlock with newlines preserved. `<div ...>...</div>` keeps
   its existing `try_div_html_block` path (Div block). Multi-tag-on-one-
   line splitting (e.g. `<tr><td>foo</td></tr>` on one line — pandoc
   splits into 4 RawBlocks + 1 Plain) is NOT yet supported; the line is
   treated as one inline-text line. Acceptable for #181 since each tag
   sits on its own line in the input.
6. **Block-level cases where parser splits paragraphs around inline HTML
   comments --- DONE 2026-05-03 (#79 unlocked)**. Parser dispatcher gates
   Comment Type-2 paragraph-interrupt on `Dialect::CommonMark`; under
   Pandoc, an `<!-- ... -->` line abutting a paragraph stays inline as
   `INLINE_HTML` rather than splitting into `HTML_BLOCK`. Companion
   formatter changes wire the directive system (panache-ignore-*) to also
   recognize INLINE_HTML directives so existing ignore-region tests still
   work — see Don't-redo entry on `collect_inline_directives`.
7. **Misc remaining**:
   - **Same-line BLOCK_QUOTE marker (#93, #108) --- DONE 2026-05-03**.
     Parser ungated for Pandoc; companion `format_list_item`
     leading-BLOCK_QUOTE arm added so the LIST_MARKER survives
     round-trip. The previously-feared `BlockQuote::depth()`
     over-count for outer-BQ contexts (#108) didn't actually block
     conformance: the projector renders from the CST, which is now
     correctly nested, so the projector match works regardless of
     whether the formatter's depth path is right. (If a formatter
     idempotency case for `> 1. > foo\n> bar` shows up later, the
     depth offset will need to land then.)
   - Other blockquote/list/definition-list nesting cases (#34,
     #91, #96) where blank lines or lazy continuation cross
     container boundaries differently from pandoc.
     Parser-shape gaps in continuation policy.

Suggested first session: **#1 (Citations proper)** is still the largest
single-fix unlock (14 cases) and the heaviest projector entry. After
Example-list numbering proved the document-level counter pre-pass shape
(`example_list_start_by_offset`), the Citation projector can lean on the same
`RefsCtx` pre-pass to assign `citationNoteNum` per inline-occurrence. After
that, the table buckets (#2) are the next largest leverage.

## Don't redo

- The CST → pandoc-native projector is **test-only** at
  `crates/panache-parser/tests/pandoc/native_projector.rs`. Do not move it under
  `src/` or wire it into the public API.
- Slugifier in the projector is intentionally a copy of
  `panache-formatter::utils::pandoc_slugify` --- the parser crate cannot depend
  on the formatter crate (would cycle). Keep it inline.
- `expected.native` files are pinned to pandoc 3.9.0.2 (see
  `tests/fixtures/pandoc-conformance/.panache-source`). Regenerate via
  `scripts/update-pandoc-conformance-corpus.sh` only when intentionally bumping
  pandoc.
- The bulk-import script (`import-pandoc-conformance-from-parser-fixtures.sh`)
  uses leading-zero-stripped IDs to avoid POSIX shell octal interpretation in
  `$((...))`. Do not refactor it back to direct `$((0025 + 1))`-style
  arithmetic.
- `imported-*` cases live alongside hand-curated cases under `corpus/`. The
  script wipes prior `*-imported-*` dirs before re-running, so the import is
  idempotent --- but **do not hand-edit** an imported case's `input.md` or
  `expected.native`. If a hand-curated variant is needed, copy it into a new
  `<NNNN>-<section>-<slug>/` dir with a non-`imported` section prefix.
- The reference-link resolver uses a `thread_local!<RefCell<RefsCtx>>` populated
  at `project()` entry. Cleared at `project()` exit. Do **not** refactor to a
  parameter-threading model --- every projector function takes only
  `&SyntaxNode`, and the rewrite would touch the entire module for no functional
  gain.
- Inline-link vs reference-link discrimination uses presence of
  `LINK_DEST_START` / `IMAGE_DEST_START` *tokens* --- not `LINK_DEST` node. An
  empty `[Empty]()` still has `LINK_DEST_START`, so the token check is the
  correct discriminator. (Reference-style `[foo][bar]` has no `LINK_DEST_START`
  at all.)
- Unresolved reference links emit `Str "[" + text + "]<suffix>"` rather than a
  `Link` with empty dest, matching pandoc's "preserve original bytes" behavior.
  Do not switch to `Link ("","")` --- it produces a spurious Link node in the
  output.
- Reference labels are normalized via `normalize_ref_label()`: unescape
  ASCII-punct backslash escapes, lowercase, collapse runs of whitespace to one
  space, trim. Both def labels (raw `LINK_TEXT.text()` with literal escapes) and
  body labels (mix of TEXT + ESCAPED_CHAR tokens, `text()` already 9-byte raw)
  feed this same normalizer so they match.
- Pandoc-native `Double` rendering is *not* the same as Rust's `Display`
  for `f64`. Use the existing `show_double` helper in the projector (it
  matches Haskell's `Show Double`: decimal in `[0.1, 1e7)`, scientific
  outside, `.0` suffix on whole-number mantissas). Don't reach for
  `format!("{:.16}", x)` or hand-rolled rendering — both diverge from
  pandoc's `ppShow` output for `ColWidth N`, `citationNoteNum`, etc.

## Latest session

- **Date**: 2026-05-04 (Citations proper — `Cite [Citation, ...] [Inline,
  ...]` projection with prefix/suffix inline parsing, `@key [locator]`
  AuthorInText absorption, and doc-order noteNum pre-pass)
- **Pass before → after**: 181 → 182 / 187 (+1 import: #38). Projector-only
  fix. Six new helpers landed in `native_projector.rs`:
  - `Inline::Cite(Vec<Citation>, Vec<Inline>)` variant + `Citation` struct
    + `CitationMode` enum, with matching `clone_inline` / `write_inline` /
    `inlines_to_plaintext` arms.
  - `render_citation_inline(node, out, extra_suffix_text)` — full
    projection. Walks CITATION tokens (LINK_START / CITATION_MARKER /
    CITATION_KEY / CITATION_BRACE_OPEN/CLOSE / CITATION_CONTENT /
    CITATION_SEPARATOR / LINK_DEST) into a `CitationBuilder` per-citation,
    splitting CITATION_CONTENT into prefix-of-next vs suffix-of-current at
    each `@`. Mode dispatch: bracketed + `-@` → SuppressAuthor; bracketed +
    `@` → NormalCitation; bare → AuthorInText. Falls back to the legacy
    Example-list carve-out (Str "N") if the first key resolves to a
    `(@label)` Example item.
  - `parse_cite_affix_inlines(raw, is_prefix)` — reparses prefix/suffix
    raw text as Pandoc-flavored inlines and runs through
    `coalesce_inlines` (which applies smart-quotes, smart-dashes, and
    abbreviation NBSP). Wraps input with a `Z ` sentinel and strips the
    sentinel from the result so block-level list-marker detection
    (e.g. `p. 33` → LIST with marker `p.`) cannot eat the leading word.
    Suffix-side preserves leading whitespace as `Inline::Space`; prefix-side
    trims both ends.
  - `literal_inlines(text)` — tokenizes raw input into `Str` + `Space` +
    `SoftBreak` (no markup, no smart). Used for the literal `[Inline]`
    payload that pandoc emits as the second arg of `Cite`.
  - `emit_citation_with_absorb(node, iter, out)` — wired into
    `inlines_from`'s match (alongside the existing LATEX_COMMAND arm).
    For bare AuthorInText CITATIONs, uses rowan sibling navigation
    (`next_sibling_or_token`) to verify both peeked elements (TEXT
    starting with space + LINK with no LINK_DEST_START) before consuming
    the iter. The verified locator's `LINK_TEXT` becomes the absorbed
    suffix; the literal text gets `" [<locator>]"` appended so the
    `[Inline]` payload reflects the absorbed bytes.
  - `collect_cite_note_nums(tree, ctx)` + `visit_for_cite_nums(...)` —
    new pre-pass populating `RefsCtx.cite_note_num_by_offset`. Walks
    document blocks (skipping top-level FOOTNOTE_DEFINITIONs since they
    are visited via FOOTNOTE_REFERENCE recursion). Each CITATION node
    gets one counter increment outside footnotes; each
    FOOTNOTE_REFERENCE bumps the counter once on entry, then all cites
    inside the footnote share that fixed number.
  Build-order matters: `collect_cite_note_nums` and
  `collect_example_numbering` now run BEFORE `collect_refs_and_headings`,
  and the partial ctx is mirrored into `REFS_CTX` thread-local before
  refs gathering — because `parse_footnote_def` (called from refs
  gathering) eagerly parses footnote bodies through `inlines_from`,
  which calls `render_citation_inline`, which reads the noteNum lookup.
  Without the early mirror, footnote-body cites all fall back to
  noteNum=1. Single allowlist add (#38 inserted between 37 and 39 under
  `# imported`). CommonMark allowlist green; pandoc allowlist green;
  full parser-crate suite green; clippy + fmt clean.
- **Cases unlocked** (+1, allowlisted under `# imported`):
  - 38 (imported-citations)
- **Files changed (classified)**:
  - **projector**:
    `crates/panache-parser/tests/pandoc/native_projector.rs`
    (Cite/Citation types; render_citation_inline rewritten;
    parse_cite_affix_inlines + literal_inlines + emit_citation_with_absorb
    + collect_cite_note_nums helpers; build_refs_ctx ordering rework;
    clone_inline / write_inline / inlines_to_plaintext arms updated;
    inlines_from CITATION arm wired to the absorb path)
  - **allowlist**:
    `crates/panache-parser/tests/pandoc/allowlist.txt` (+1: 38 inserted
    between 37 and 39, under `# imported`)
- **Don't redo**:
  - The `Z `-sentinel wrap in `parse_cite_affix_inlines` is load-bearing.
    Without it, `p. 33` (or any text starting with an alphabetical
    list-marker pattern like `a.`, `b.`, `i.`, `IV.`, etc.) reparses as a
    `LIST` — the first token is consumed as `LIST_MARKER`, the second as
    list-item content, and the prefix-word disappears entirely from the
    inline stream. Don't switch to feeding `raw` directly through
    `parse_cell_text_inlines`. If a future case lands a citation with an
    affix that legitimately starts with `Z ` (highly unlikely but
    theoretically possible), pick a different ASCII-letter sentinel and
    update the strip check; keep the wrap-strip pattern.
  - `literal_inlines` does NOT apply pandoc smart-typography. The
    `[Inline]` payload of `Cite` is a verbatim representation of the
    original bytes (including `*see*`, `**32**`, `[`, `;`, `]`). Don't
    route it through `coalesce_inlines` or `parse_cell_text_inlines` —
    pandoc's expected output preserves the raw markdown spelling, not
    the smart-converted form. Newline → SoftBreak, runs of space/tab → a
    single Space, everything else (including `*`, `_`, etc.) is a plain
    `Str`.
  - The build-order rework in `build_refs_ctx` matters because
    `parse_footnote_def` eagerly parses blocks through `inlines_from` →
    `render_citation_inline`, which reads `REFS_CTX.cite_note_num_by_offset`.
    The thread-local must be populated with the cite-note-num map BEFORE
    footnote bodies are parsed, otherwise cites inside footnotes get
    noteNum=1 fallback. The fix: run the cite-num pre-pass first, mirror
    the partial ctx into REFS_CTX, then run refs gathering. Don't move
    `collect_refs_and_headings` back to run first — the dependency is
    real and the ordering matters.
  - `emit_citation_with_absorb` uses rowan's `next_sibling_or_token` for
    the look-ahead rather than `iter.next()`, because rowan iterators
    don't support push-back. The pattern: navigate via the SyntaxNode
    tree to verify both peeked elements satisfy the absorption shape
    (whitespace TEXT + LINK without LINK_DEST_START), then advance the
    iter (`iter.next()` twice) only on commit. Don't try a "consume,
    then maybe re-emit" flow — the consumed TEXT can't be put back into
    the inlines stream cleanly.
  - The `noteNum` pre-pass increments the counter on every
    FOOTNOTE_REFERENCE entry, regardless of whether the footnote contains
    cites. This matches pandoc — verified by probe: a footnote with no
    cites still bumps the counter for the next outside cite. Don't
    optimize "skip increment if no inner cites" — it would break the
    counter alignment for any document with mixed cite/no-cite footnotes.
  - The `apply_abbreviations` post-pass already handles `pp.`, `chap.`,
    `p.` etc. inserting NBSP after the abbreviation. The
    `parse_cite_affix_inlines` helper just calls into `coalesce_inlines`
    (which calls `apply_abbreviations`) — don't re-implement abbreviation
    handling in the citation projector.

## Earlier session (2026-05-04, HTML block per-line splitting projector via `markdown_in_html_blocks`)

- **Pass before → after**: 180 → 181 / 187 (+1 import: #181). Projector-only
  fix: `emit_html_block` splits multi-line HTML_BLOCKs into per-line Blocks
  (each tag-only line → RawBlock; each text line → Plain with parsed
  inlines). Verbatim constructs (comments, `<script>`/`<style>`/`<pre>`/
  `<textarea>`, PIs, declarations) emit as single RawBlock with newlines
  preserved. `<div>` keeps its existing `try_div_html_block` Div path.
  All seven block-walker call sites that previously did
  `if let Some(b) = block_from(&child) { out.push(b); }` switched to a new
  `collect_block(&child, &mut out)` wrapper that dispatches to
  `emit_html_block` for HTML_BLOCK (one→many) and to `block_from`
  otherwise. The grid-table-cell call site (line 2028) keeps `block_from`
  directly because its Para→Plain transform applies per-Block, and
  HTML_BLOCK splitting inside a grid-table cell is unusual. CommonMark
  allowlist green; pandoc allowlist green; full parser-crate suite green
  (996 + 263 + 38 + 35 + 14 + 17 + ... passes); clippy + fmt clean.
- **What landed**:
  - **Projector: per-line HTML block splitting
    (`crates/panache-parser/tests/pandoc/native_projector.rs`)** — new
    helpers:
    - `emit_html_block(node, out)` — entry point that decides whether
      to split. Strips trailing newlines, delegates to `try_div_html_block`
      first, then early-returns single-RawBlock for verbatim constructs
      (comment / PI / declaration / `<script>` / `<style>` / `<pre>` /
      `<textarea>`) and single-line blocks. Multi-line otherwise: split
      lines, each line → RawBlock or Plain.
    - `is_raw_text_element_open(s)` — case-insensitive check that the
      first tag is `<script`, `<style`, `<pre`, or `<textarea`, followed
      by whitespace/`>`/`/`/end.
    - `is_complete_html_tag_line(s)` — line is a single complete `<...>`
      with no trailing content. Skips quoted-attribute content so `>`
      inside `class="..."` doesn't terminate early.
    - `collect_block(node, out)` — top-level dispatcher that calls
      `emit_html_block` for HTML_BLOCK and `block_from` otherwise.
  - **Call-site updates** (7 sites; all simple `if let Some(b) =
    block_from(&child) { out.push(b); }` patterns swapped to
    `collect_block(&child, &mut out)`):
    - `blocks_from_doc` (document body)
    - `blockquote_blocks`
    - `parse_pandoc_blocks` (the helper used by `try_div_html_block`)
    - footnote-definition body collection
    - `fenced_div` body collection
    - definition-list body collection (`extra > 0` indented-code branch)
    - list-item body collection (`item_indent` indented-code branch)
- **Cases unlocked** (+1, allowlisted under `# imported`):
  - 181 (imported-writer_html_blocks)
- **Files changed (classified)**:
  - **projector**:
    `crates/panache-parser/tests/pandoc/native_projector.rs`
    (new `emit_html_block`, `is_raw_text_element_open`,
    `is_complete_html_tag_line`, `collect_block` helpers; 7 call sites
    updated to `collect_block`)
  - **allowlist**:
    `crates/panache-parser/tests/pandoc/allowlist.txt` (+1: 181 inserted
    between 180 and 182, under `# imported`)
- **Don't redo**:
  - The line-by-line splitter does NOT handle multi-tag-on-one-line cases
    (e.g. `<tr><td>foo</td></tr>` on a single line — pandoc splits into
    4 RawBlocks + 1 Plain). For #181 each tag sits on its own line, so
    this is acceptable. If a future case lands one with multi-tag lines,
    the fix is to extend `is_complete_html_tag_line`-based scanner into
    a tag-tokenizer that walks each line emitting per-tag RawBlocks
    interleaved with text spans. Don't try to bring that in pre-emptively.
  - The grid-table-cell call site (`parse_cell_text_blocks` body, around
    line 2028) is intentionally left calling `block_from` directly so its
    `Para` → `Plain` transform stays per-Block. HTML_BLOCK splitting
    inside a grid-table cell is unusual; if it ever shows up, the fix is
    to apply the transform after splitting (push splits into a temp Vec,
    map Para→Plain, extend `out`).
  - `is_raw_text_element_open` lowercases the candidate tag name for
    matching (pandoc tags are case-insensitive). Don't switch to
    case-sensitive comparison or `<SCRIPT>` would be projected as a
    splittable block.
  - The verbatim-constructs early return checks `<!--` BEFORE the generic
    `<!` declaration check (since `<!--` is a prefix of `<!`). Order
    matters; don't reorder these checks.
  - Empty/whitespace-only lines in a multi-line HTML_BLOCK are skipped
    (continue) rather than emitting a Plain. This matches pandoc's
    behavior of not emitting blocks for blank inner lines (they break
    paragraph context). Don't try to emit an empty Plain or
    SoftBreak-bearing block.

## Earlier session (2026-05-04, HTML `<div>` block → `Div(attr, blocks)` projector conversion via `markdown_in_html_blocks`)
- **Pass before → after**: 179 → 180 / 187 (+1 import: #78).
  Projector-only fix: `html_block()` now detects an outer `<div ...>...</div>`
  shape on any `HTML_BLOCK` and projects it as `Div(attr, blocks)` with the
  inner content reparsed via the new `parse_pandoc_blocks` helper. `<div>`
  is the only block tag pandoc treats this way under `markdown_in_html_blocks`
  (default-on for `markdown` flavor); other block tags (`<table>`, `<hr>`,
  ...) fall through to the existing `RawBlock` path. The reparse promotes the
  resulting block to `Plain` only when the open tag is on the same source line
  as the close tag (single-line `<div>foo</div>`); multi-line content keeps
  `Para` shape. CommonMark allowlist green; pandoc allowlist green; full
  parser-crate suite green; clippy + fmt clean.
- **What landed**:
  - **Projector: `<div>` HTML block → `Div`
    (`crates/panache-parser/tests/pandoc/native_projector.rs`)** — `html_block()`
    now delegates to a new `try_div_html_block()` helper that:
    1. Skips leading whitespace and matches `<div` followed by a separator
       (` `, `\t`, `\n`, `>`, or `/`).
    2. Parses the open-tag attrs via the existing `parse_html_attrs()` after
       trimming a trailing `/`.
    3. Searches for a `</div>` close tag at the trailing edge (lowercase
       suffix match) — leaves the projector's existing RawBlock path for any
       HTML block that doesn't match this shape.
    4. Reparses the inner content (with leading/trailing `\n` trimmed) as
       Pandoc-flavored markdown via the new `parse_pandoc_blocks()`.
    5. Promotes a single inner `Para` to `Plain` only when the open tag is
       on the same source line as the close tag (`!multiline`), matching
       pandoc's behavior for `<div>foo</div>`.
  - **New helper `parse_pandoc_blocks` (same file)** — parses text as
    Pandoc-flavored markdown and returns top-level blocks unchanged. Distinct
    from `parse_cell_text_blocks` (which forces Para→Plain for grid-cell
    contexts); per the prior Don't-redo, kept as a separate helper.
- **Cases unlocked** (+1, allowlisted under `# imported`):
  - 78 (imported-html_block)
- **Files changed (classified)**:
  - **projector**:
    `crates/panache-parser/tests/pandoc/native_projector.rs`
    (new `try_div_html_block` helper, new `parse_pandoc_blocks` helper,
    `html_block` delegates through `try_div_html_block`)
  - **allowlist**:
    `crates/panache-parser/tests/pandoc/allowlist.txt` (+1: 78 inserted
    between 77 and 79, under `# imported`)
- **Don't redo**:
  - `try_div_html_block` matches `<div` only — not `<table>`, `<hr>`, or
    other block tags. Pandoc's `markdown_in_html_blocks` is `<div>`-only
    in practice (verified via `pandoc -f markdown -t native` probes); broader
    matching would diverge from pandoc-native. Don't widen this without
    pandoc-source verification.
  - The `multiline` flag is computed by inspecting whether the byte
    immediately after the open tag's `>` is `\n`. This is the right signal
    for distinguishing `<div>foo</div>` (Plain) from `<div>\nfoo\n</div>`
    (Para) — don't switch to checking inner content for newlines, since
    inner content is later trimmed of leading/trailing `\n` and would
    miss the boundary.
  - The close-tag detection trims trailing whitespace/newlines from the
    raw HTML_BLOCK content, then checks the suffix (case-insensitive)
    ends with `</div>`. Don't switch to a forward search — partial
    `<div>...</div>...<div>` blocks shouldn't be projected as Div even
    though the parser may unify them; the suffix check guards against
    that. (CommonMark spec & pandoc both close the HTML block at the
    final `</div>` followed by a blank line, but in defensive depth this
    keeps the projector matching pandoc's per-block decision rather than
    trying to re-segment the parser's output.)
  - `parse_pandoc_blocks` is the right helper for any future "reparse a
    fragment, keep block kinds intact" projector need (e.g. fenced div
    contents that need block-level reparse). `parse_cell_text_blocks`
    keeps the Para→Plain conversion specific to grid/pipe table cells.

## Earlier session (2026-05-03, Grid-table multi-line cells + TableFoot via
  block-reparse projector path)
- **Pass before → after**: 177 → 179 / 187 (+2 imports: #68, #70).
  Projector-only fix: `grid_table()` now slices each row's text by
  `+`-derived column ranges (one per column, via new
  `grid_column_ranges` helper), strips the `| ` 1-space padding, joins
  multi-line content per column with `\n`, and reparses the joined text
  as block-level Pandoc markdown via the new `parse_cell_text_blocks`.
  This unlocks: (a) `Population\<NL>(in 2018)` projecting as
  `Plain [Str "Population", LineBreak, Str "(in", Space, Str "2018)"]`
  for #70's multi-line header cells; (b) `      B` (5+ leading spaces
  after padding strip) projecting as `CodeBlock ("",[],[]) " B"` for
  #68's table 2; (c) the existing `Plain [Str "X"]` shape for ordinary
  cells (Para → Plain conversion in `parse_cell_text_blocks`).
  TableFoot rendering also added: `TableData` got a `foot_rows` field;
  `grid_table()` collects `TABLE_FOOTER` children alongside
  `TABLE_HEADER` and `TABLE_ROW`; `write_table()` emits foot rows
  inside the previously-empty `( TableFoot ( "" , [ ] , [ ] ) [ ] )`
  slot. Pipe / simple / multiline tables initialize `foot_rows` to
  `Vec::new()` since none currently expose footer rows.
  CommonMark allowlist green; pandoc allowlist green; full
  parser-crate suite green; full workspace tests green; clippy + fmt
  clean.
- **What landed**:
  - **Projector: TableData gets `foot_rows`
    (`crates/panache-parser/tests/pandoc/native_projector.rs`)** — new
    field; pipe/simple/multiline TableData constructors initialize to
    `Vec::new()`. `write_table()` now emits foot rows in the
    `TableFoot ( "" , [ ] , [ ] ) [ <rows> ]` slot.
  - **Projector: `grid_table` switched to multi-line slicing
    (same file)** — replaced the `grid_row_cells` (TABLE_CELL-only,
    inline-coalescing) helper with `grid_row_cells_blocks` that:
    splits the row's `.text()` by `\n`, slices each line by char
    ranges from `grid_column_ranges`, strips one leading space (cell
    padding), trims trailing whitespace, joins surviving lines with
    `\n`, and reparses via `parse_cell_text_blocks`. Top-level Para
    inside a cell becomes Plain (pandoc grid-cell rule); other block
    kinds (CodeBlock, BulletList, ...) round-trip as-is.
  - **Projector: TABLE_FOOTER children are now collected** as
    `foot_rows` using the same row helper, so #70's `Total` row lands
    in the table foot rather than being dropped.
- **Cases unlocked** (+2, allowlisted under `# imported`):
  - 68 (imported-grid_table)
  - 70 (imported-grid_table_nordics)
- **Files changed (classified)**:
  - **projector**:
    `crates/panache-parser/tests/pandoc/native_projector.rs`
    (TableData.foot_rows; new helpers `grid_column_ranges`,
    `grid_row_cells_blocks`, `parse_cell_text_blocks`; rewrote
    `grid_table()`; updated `write_table()`)
  - **allowlist**:
    `crates/panache-parser/tests/pandoc/allowlist.txt` (+2: 68
    inserted between 67 and 69, 70 inserted between 69 and 72, both
    under `# imported`)
- **Don't redo**:
  - `grid_column_ranges` skips empty `+`-to-`+` segments (cases where
    two `+` characters are adjacent or only whitespace separates them).
    These represent rowspan/colspan separators in #71 — see Suggested
    next targets entry under "Tables — remaining". Don't fold #71's
    span handling into this helper without a new layout model;
    `grid_row_cells_blocks` would also need to assign cells to the
    correct columns when rows have fewer cells than the column count.
  - `parse_cell_text_blocks` always converts top-level `Para` to
    `Plain`. This is correct for grid (and pipe) table cells where
    pandoc emits Plain rather than Para, but **don't** reuse this
    helper for blockquote / list-item / footnote-definition contexts
    — there pandoc DOES use Para. If you need a generic block-reparse,
    add a separate helper instead of generalizing this one.
  - The cell padding strip is "exactly one leading space" via
    `strip_prefix(' ')`. Don't switch to `trim_start()` or it will
    eat the 4+ leading spaces that signal an indented code block
    inside a cell (e.g. #68's `|      B |` → `CodeBlock " B"`).
  - The header is detected via the *first* `TABLE_SEPARATOR` child
    for column-range purposes. If pandoc ever changes which separator
    defines column widths for grid tables, only the canonical-aligns
    selection needs to update — the column-range derivation is
    independent (always uses first separator's `+` positions).
  - The previous `grid_row_cells(row)` helper was deleted entirely;
    the multi-line path is the only path now. If a regression appears
    where a complex inline construct (e.g. nested emphasis spanning a
    cell) re-projects differently, the fix is in
    `parse_cell_text_blocks`, NOT to bring back the old
    inlines-from-CST helper.

## Earlier session (2026-05-03, HTML comment paragraph-interrupt gated by dialect; directive system extended to INLINE_HTML)
- **Pass before → after**: 176 → 177 / 187 (+1 import: #79).
  Parser-shape fix: under `Dialect::Pandoc`, an HTML comment (Type 2) no
  longer interrupts a paragraph — `\n<!-- ... -->\n` between two
  paragraph lines stays as `INLINE_HTML` instead of splitting into a
  sibling `HTML_BLOCK`. CommonMark dialect retains the existing
  paragraph-interrupting behavior (CommonMark §4.6 allows Type 2 to
  interrupt). Companion formatter work: the panache-ignore directive
  system now also extracts directives from `INLINE_HTML` nodes, so
  ignore-format/lint regions still close correctly when the END marker
  inlines into the surrounding paragraph. CommonMark allowlist green;
  pandoc allowlist green; full workspace tests green; clippy + fmt clean.
- **What landed**:
  - **Parser: dialect-gate Comment paragraph-interrupt
    (`crates/panache-parser/src/parser/block_dispatcher.rs`)** — in
    `HtmlBlockParser::detect_prepared`, mark Comment as
    `cannot_interrupt` when `dialect == Dialect::Pandoc`, mirroring
    Type 7. With `has_blank_before || at_document_start`, comments
    still emit as `HTML_BLOCK`; otherwise return None and let the
    paragraph absorb the comment line, where `try_parse_inline_html`
    later picks it up as `INLINE_HTML`.
  - **Formatter: directive extraction now accepts `INLINE_HTML`
    (`crates/panache-formatter/src/directives.rs` and the top-level
    duplicate at `src/directives.rs`)** — `extract_directive_from_node`
    additionally matches `SyntaxKind::INLINE_HTML`. New helper
    `collect_inline_directives(node)` scans descendants for
    INLINE_HTML directives in document order; only added to the
    formatter crate (the linter walks via `preorder()` and picks up
    INLINE_HTML directives directly through `extract_directive_from_node`).
  - **Formatter: paragraph/plain inline-directive handling
    (`crates/panache-formatter/src/formatter/core.rs`)** — at
    `format_node_sync` entry, after the existing ignored-mode
    short-circuit, two new branches:
    1. If we're in ignored mode and the verbatim node contains
       INLINE_HTML directives, replay them in the tracker AFTER
       outputting verbatim (so subsequent blocks see the END's
       transition).
    2. If the node is a `PARAGRAPH | PLAIN` carrying inline
       directives that AFFECT FORMATTING (ignore-format / ignore-both),
       output the whole node verbatim and replay all directives.
       Lint-only directives don't need verbatim output: replay them
       upfront and fall through to the normal render path (lint state
       doesn't change rendering).
  - **Formatter: list item content with inline format-directive
    (`crates/panache-formatter/src/formatter/lists.rs`)** — list-item
    content goes through its own wrap pipeline (`inline_layout::
    wrapped_lines_for_node`), not `format_node_sync`. Added
    `content_has_format_directive` check that, when true, populates
    `preserve_lines` with the verbatim content lines (mirroring the
    `WrapMode::Preserve` path) so multi-space content between an
    inline START and END isn't reflowed.
  - **New parser fixtures (paired)**:
    `crates/panache-parser/tests/fixtures/cases/html_comment_after_paragraph_pandoc/`
    and `..._commonmark/` — pin the dialect divergence: same `input.md`
    (Para line + comment line + Para line + standalone comment + Para
    line + trailing comment), different `parser-options.toml`. Pandoc
    snapshot has comments inline within paragraphs except the
    blank-separated standalone; CommonMark snapshot has every comment
    as a sibling `HTML_BLOCK`.
  - **Snapshot regeneration**: 1 parser CST snapshot updated to reflect
    the new shape — `parser_cst_ignore_directives.snap` (the parser's
    own ignore_directives fixture) where four formerly-HTML_BLOCK
    comments now sit inline as INLINE_HTML inside their surrounding
    PARAGRAPH/PLAIN. The two new fixtures' snapshots are net-new.
- **Cases unlocked** (+1, allowlisted under `# imported`):
  - 79 (imported-ignore_directives)
- **Files changed (classified)**:
  - **parser-shape** (Dialect::Pandoc only):
    `crates/panache-parser/src/parser/block_dispatcher.rs`
    (`HtmlBlockParser::detect_prepared` cannot_interrupt branch)
  - **formatter / linter directive infra** (companion to parser-shape;
    required so existing ignore-directive tests keep passing under
    the new INLINE_HTML shape):
    - `crates/panache-formatter/src/directives.rs`
    - `src/directives.rs` (top-level duplicate kept in sync)
    - `crates/panache-formatter/src/formatter/core.rs`
      (paragraph/plain inline-directive replay path)
    - `crates/panache-formatter/src/formatter/lists.rs`
      (list-item content preserve-on-format-directive)
  - **fixtures (parser-only)**: two new dirs under
    `crates/panache-parser/tests/fixtures/cases/html_comment_after_paragraph_*`
    + registered in `crates/panache-parser/tests/golden_parser_cases.rs`
  - **snapshots**: 3 `.snap` files
    (`parser_cst_ignore_directives` updated; two new for the paired
    fixtures) under `crates/panache-parser/tests/snapshots/`
  - **allowlist**:
    `crates/panache-parser/tests/pandoc/allowlist.txt` (+1: 79
    inserted between 77 and 80, under `# imported`)
- **Don't redo**:
  - Top-level `src/directives.rs` and
    `crates/panache-formatter/src/directives.rs` are duplicate copies
    — the formatter crate has the same module copied because the
    formatter is dependency-lean (no top-level dep). The
    `extract_directive_from_node` change must be mirrored in BOTH
    files for both the linter (top-level) and the formatter to
    recognize INLINE_HTML directives. `collect_inline_directives` is
    only in the formatter copy because only the formatter needs it
    (the linter's `preorder()` walk visits INLINE_HTML descendants
    directly).
  - The parser fix is gated specifically to `HtmlBlockType::Comment`
    (Type 2). Other types (Type 1 `<script>`/`<pre>`, Type 6 block
    tags like `<div>`, Type 7 generic tags, declarations, CDATA, PIs)
    retain their existing dispatcher behavior. Pandoc-native shows
    that `<style>` *also* doesn't interrupt and `<script>` does — a
    finer-grained Type-1 split could land later but isn't required
    for the cases currently under test.
  - The PARAGRAPH/PLAIN inline-directive branch in
    `format_node_sync` only short-circuits to verbatim when at least
    one inline directive **affects formatting**. Lint-only directives
    are processed upfront (tracker state updated) and rendering
    continues normally — so reflow still happens around `panache-
    ignore-lint-*` markers. Don't unify these paths or you'll
    reintroduce the regression where `test_ignore_lint_does_not_
    affect_formatting` fails.
  - The list-item path
    (`format_list_item` in `lists.rs`) needs its OWN check because
    list-item content_node is wrapped via `inline_layout::
    wrapped_lines_for_node`, NOT through `format_node_sync`. Don't
    assume the format_node_sync branch alone covers list items.
  - The new paired parser fixtures pin the dialect divergence. If
    pandoc-native ever changes its Comment Type-2 behavior, regenerate
    `expected.native` for #79 before adjusting the fixtures.

## Earlier session (2026-05-03, Same-line BLOCK_QUOTE inside LIST_ITEM ungated for Pandoc)
- **Pass before → after**: 174 → 176 / 187 (+2 imports: #93, #108).
  Two-step fix: (1) parser ungates the same-line `>`-after-list-marker
  branch in `finish_list_item_with_optional_nested` so Pandoc also
  emits `LIST_ITEM > BLOCK_QUOTE > PARAGRAPH` for `- > foo` shapes;
  (2) formatter adds a leading-`BLOCK_QUOTE` arm to `format_list_item`
  so the LIST_MARKER survives round-trip (was being dropped because no
  arm emitted the marker before delegating to the BQ child). The
  projector already renders the new CST correctly without depth
  juggling, so #108's outer-BQ context (`> 1. > foo`) passed too —
  the previously-feared `BlockQuote::depth()` over-count never
  triggered because the conformance harness compares projector
  output, not formatter output. CommonMark allowlist green; pandoc
  allowlist green; full parser-crate suite green; full workspace
  tests green; clippy + fmt clean.
- **What landed**:
  - **Parser: ungate same-line BQ inside LIST_ITEM
    (`crates/panache-parser/src/parser/blocks/lists.rs`)** —
    deleted `let dialect_allows_nested = config.dialect ==
    Dialect::CommonMark;` and the `dialect_allows_nested &&` guard
    on the `text_to_buffer.starts_with('>')` branch. Both dialects
    now emit the nested-BQ shape pandoc-native expects.
  - **Formatter: leading-BLOCK_QUOTE arm in `format_list_item`
    (`crates/panache-formatter/src/formatter/lists.rs`)** — mirrors
    the leading-LIST and leading-HEADING arms. When
    `first_non_blank_child` is a BLOCK_QUOTE and there's no PLAIN/
    PARAGRAPH content, emits `total_indent + marker_padding +
    marker + spaces_after + checkbox?` and then calls
    `format_node_sync(leading_bq, 0)` so the BQ's `> ` abuts the
    list marker on the same output line.
  - **Test pin update: parser unit test
    (`crates/panache-parser/src/parser/blocks/tests/blockquotes.rs`)**
    — `definition_list_list_blockquote_continuation_stays_structural`
    pinned the OLD (broken) shape with 2 BQ markers; updated to the
    new correct count of 3 (each `> a/b/c` line now contributes a
    marker inside the single BQ rather than `> a` being TEXT and
    only `> b/c` being the BQ).
  - **Snapshots regenerated**: 4 parser CST snapshots updated to
    reflect the new same-line BQ shape:
    `definition_list`,
    `issue_174_blockquote_list_reorder_losslessness`,
    `issue_209_definition_list_blockquote_continuation`,
    `list_item_same_line_blockquote_marker_pandoc`.
- **Cases unlocked** (+2, allowlisted under `# imported`):
  - 93 (issue_209_definition_list_blockquote_continuation)
  - 108 (list_item_same_line_blockquote_marker_pandoc)
- **Files changed (classified)**:
  - **parser-shape**:
    `crates/panache-parser/src/parser/blocks/lists.rs` (drop
    CommonMark-only gate on same-line BQ inside LIST_ITEM)
  - **formatter** (companion to parser-shape change, required for
    idempotency of the new CST):
    `crates/panache-formatter/src/formatter/lists.rs`
    (leading-BLOCK_QUOTE arm in `format_list_item`)
  - **test pin**:
    `crates/panache-parser/src/parser/blocks/tests/blockquotes.rs`
    (marker count updated 2 → 3 in
    `definition_list_list_blockquote_continuation_stays_structural`)
  - **snapshots**: 4 `.snap` files updated under
    `crates/panache-parser/tests/snapshots/`
  - **allowlist**:
    `crates/panache-parser/tests/pandoc/allowlist.txt` (+2: 93
    inserted between 92 and 94, 108 inserted between 107 and 109,
    both under `# imported`)
- **Don't redo**:
  - The CommonMark same-line nested LIST gating comment block
    (`dialect_allows_nested ... is kept for the BLOCK_QUOTE
    same-line case below`) was removed entirely with the parser
    change. The same-line nested LIST emission was always
    dialect-agnostic; the variable existed solely for the BQ
    branch. Don't reintroduce it.
  - #108 passes via the projector even though the formatter still
    has a `BlockQuote::depth()` over-count for outer-BQ nested
    contexts. The formatter currently produces a wrong shape for
    `> 1. > foo` style inputs (extra `>` prefixes from
    depth-2-counted ancestor) but no formatter golden case exercises
    it. **If a formatter idempotency case lands later that surfaces
    this, the fix is to subtract the outer-BQ render-depth context
    when computing inner BQ's `depth` in
    `crates/panache-formatter/src/formatter/core.rs::SyntaxKind::BLOCK_QUOTE`
    arm.** Don't try to land that pre-emptively here — the conformance
    win is independent.
  - The leading-BLOCK_QUOTE arm intentionally calls
    `format_node_sync(leading_bq, 0)` without stripping a leading
    newline (unlike the leading-LIST arm which strips one). The BQ
    formatter doesn't emit a leading newline; only `format_list`
    does. Verified empirically — the output `1. > foo\n` was correct
    on first run.
  - Snapshot for `issue_174_blockquote_list_reorder_losslessness`
    (#91) DID change shape with this work but #91 still fails
    conformance — the new CST has the first BQ-bearing list item
    swallowing subsequent items into its BQ paragraph. That's a
    deeper continuation-policy bug (recursive same-line nested
    detection inside BQ content); the snapshot was accepted because
    it accurately captures current parser behavior, not because the
    shape is correct. Re-tackling #91 means recursing
    `try_parse_list_marker` inside the BQ branch (similar to the
    same-line nested LIST recursion already there).

## Earlier session (2026-05-03, List-item indent: include parent-LIST leading WHITESPACE in content offset)

## Prior sessions

Older session logs were pruned to keep the recap scannable. Use `git log` on
`crates/panache-parser/tests/pandoc/allowlist.txt` and the projector to
trace which case unlocked when. Cross-session lessons that still apply have
been folded into the global "Don't redo" section above.
