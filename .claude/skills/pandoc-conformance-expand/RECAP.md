# Pandoc-conformance recap

Rolling handoff between sessions. Keep terse. Read `report.txt` for the full
state; this file is judgment calls only.

## Suggested next targets

Ranked by likely shared root cause and leverage. Numbers in parentheses are the
count of currently-failing imports remaining under that bucket in the latest
`report.txt`. 4 imports remain failing total (183 / 187 passing).

1. **Tables --- #71 grid_table_planets (1)** --- rowspan/colspan layout.
   Pandoc emits `RowSpan N` / `ColSpan N` for cells whose `+   +-----+`-style
   separators omit the column-divider `+` to span the cell into the next
   row, and similarly for column merges. Our `grid_column_ranges` helper
   currently skips empty `+`-to-`+` ranges (which represent spans), so
   spanned cells produce wrong column ordering. Heavy — needs a layout
   model that assigns cells to specific columns and tracks RowSpan/ColSpan
   counts per cell.

2. **Definition list nesting --- #43 (1)** --- `definition_list`. Nested
   bullets inside definitions aren't parsed as `BulletList` in *some*
   contexts (the bare-leading-list path handles the simple case but the
   `Orange\n: > a / : - List with lazy continuation` paths inside #43
   still fail). Parser-shape gap in definition-list continuation across
   blockquote/list-marker lines.

3. **Continuation policy across container boundaries --- #91, #96 (2)**
   --- where lazy continuation or sibling-list-item handling crosses
   blockquote/list boundaries differently from pandoc.
   - **#91** (`-  > -` siblings + indented `   > -` continuation):
     parser merges item content into one BlockQuote/Para instead of
     splitting into 3 outer items each containing a BlockQuote with an
     inner BulletList.
   - **#96** (lazy continuation across deep nesting): the
     `> - This is a list item with\nlazy continuation...` lines get
     split into a sibling Para outside the BQ instead of staying as
     SoftBreak-joined PLAIN inside the list item.

   Probably *not* a single shared fix. #91 wants recursion of
   list-marker detection inside BQ-content-followed-by-`-` siblings;
   #96 wants no-`>`-prefix lazy continuation to stay inside the
   inner-BQ list item. Pick whichever has the simpler entrance.

Suggested first session: **#1 (Tables — #71)** is the single
remaining "feature" gap (rowspan/colspan in grid tables); the rest are
parser continuation-policy gaps. If the rowspan model is too heavy for
one session, **#2 (#43)** is bounded — it's the same definition-list
continuation family that produced #44 in 2026-05-03, just one more
context to handle.

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

- **Date**: 2026-05-04 (Blank-line peek-loop inside blockquote: skip
  blank-in-BQ lines so multi-blank-then-continuation list items don't
  prematurely close)
- **Pass before → after**: 182 → 183 / 187 (+1 import: #34). Parser-shape
  fix: in `parse_line`'s blank-line branch in `core.rs`, the peek-ahead
  loop that skips trailing blank lines now also skips lines that are
  functionally blank in the current blockquote context (e.g. `>` or
  `>   ` when inside a `> ` blockquote). Previously the loop only used
  `is_blank_line(self.lines[peek])`, which treats `>\n` as non-blank;
  the next-line context fed to `compute_levels_to_keep` was therefore a
  blank-in-BQ line with `raw_indent_cols=0`, prompting the policy to
  close the parent List+ListItem prematurely. The fix peels off
  `bq_depth` markers via `strip_n_blockquote_markers` and skips when
  the inner content `is_blank_line`. CommonMark allowlist green;
  pandoc allowlist green; full parser-crate suite green; full
  workspace tests green; clippy + fmt clean.
- **What landed**:
  - **Parser: blank-line peek skips blank-in-BQ lines
    (`crates/panache-parser/src/parser/core.rs` blank-line branch in
    `parse_line`)** — replaced the simple `while is_blank_line(...)`
    peek loop with one that also skips lines whose
    `strip_n_blockquote_markers(line, bq_depth)` is blank, when
    `bq_depth > 0` and `peek_bq >= bq_depth`. The depth condition
    avoids skipping a `<` -depth line (which is a real BQ-close
    event).
  - **Snapshot regeneration**: 1 parser CST snapshot updated to
    reflect the new shape — `parser_cst_blockquote_list_blockquote`
    now shows the LIST containing two `LIST_ITEM`s (instead of one
    item + a sibling PARAGRAPH + a second LIST). The previous
    snapshot was an "accept current state" capture, not a deliberate
    pin.
  - **Formatter golden update**: `tests/fixtures/cases/blockquote_list_blockquote/expected.md`
    regenerated. The new formatted output indents `Back to list item
    content` to match the (now-correct) list-item continuation, and
    is idempotent.
- **Cases unlocked** (+1, allowlisted under `# imported`):
  - 34 (imported-blockquote_list_blockquote)
- **Files changed (classified)**:
  - **parser-shape**:
    `crates/panache-parser/src/parser/core.rs` (blank-line peek
    loop)
  - **snapshot**:
    `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_blockquote_list_blockquote.snap`
  - **formatter golden**:
    `tests/fixtures/cases/blockquote_list_blockquote/expected.md`
  - **allowlist**:
    `crates/panache-parser/tests/pandoc/allowlist.txt` (+1: 34
    inserted between 33 and 35, under `# imported`)
- **Don't redo**:
  - The peek-loop guard requires both `peek_bq >= bq_depth` AND
    `is_blank_line(peek_inner)` to skip. Don't drop the depth check —
    a line with `peek_bq < bq_depth` (e.g. plain text after BQ close,
    or `>` instead of `> >`) is a real container-close event that
    `compute_levels_to_keep` needs to see. Skipping it would silently
    keep deeper containers open across BQ boundaries.
  - The check uses `bq_depth` (current line's BQ depth) rather than
    `current_bq_depth` (container-stack depth). For the blank-in-BQ
    case both happen to match because the current line carries `>`
    markers and `current_bq_depth` reflects the open BQ container.
    If a divergence shows up (e.g. shifted blockquote prefixes inside
    list-item content), revisit this — but for now `bq_depth` is the
    natural "what depth is this `>` line at" signal.
  - The previous fixture snapshot for `blockquote_list_blockquote`
    pinned the broken shape. It WAS a known-broken pin (recap noted
    it). The new snapshot pins pandoc-correct shape: LIST with two
    LIST_ITEMs (first contains both segments, second contains only
    "Second list item"). Don't revert.
  - Cases #91 and #96 still fail. Both involve different
    continuation-policy issues (lazy continuation across blockquote
    depth boundaries; list-item closure mid-blockquote). The
    blank-in-BQ peek fix doesn't help them — they involve real
    non-blank lines whose ContinuationPolicy outcome differs from
    pandoc.

## Earlier session (2026-05-04, Citations proper — `Cite [Citation, ...] [Inline,
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

## Prior sessions

Older session logs were pruned to keep the recap scannable. Use `git log` on
`crates/panache-parser/tests/pandoc/allowlist.txt` and the projector to
trace which case unlocked when. Cross-session lessons that still apply have
been folded into the global "Don't redo" section above.

- 2026-05-03: Grid-table multi-line cells + TableFoot via block-reparse
  projector path (#68, #70 unlocked).
- 2026-05-03: HTML comment paragraph-interrupt gated by dialect; directive
  system extended to INLINE_HTML (#79 unlocked).
- 2026-05-03: Same-line BLOCK_QUOTE inside LIST_ITEM ungated for Pandoc
  (#93, #108 unlocked).
- 2026-05-03: List-item indent — include parent-LIST leading WHITESPACE in
  content offset (#44 unlocked).
