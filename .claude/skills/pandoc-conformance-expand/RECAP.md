# Pandoc-conformance recap

Rolling handoff between sessions. Keep terse. Read `report.txt` for the full
state; this file is judgment calls only.

## Suggested next targets

Corpus is at **187 / 187 passing (100%)**. There are no failing imports left
in the seed. Future work should focus on **growing the corpus**:

1. **Expand corpus with new pandoc-markdown constructs.** Pick areas not
   yet covered — e.g. `definition_list_with_continuation_paragraphs`,
   `figure_with_attribute_id_and_caption_attrs`, `nested_block_quotes`,
   `inline_math_with_attrs`, larger walking-skeleton-style mixed-flavor
   corner cases. Add `<NNNN>-<section>-<slug>/` dirs with `input.md` and
   `expected.native` regenerated via `pandoc -f markdown -t native`.
   Allowlist when green; otherwise leave un-allowlisted and triage.
2. **Re-run with a newer pandoc build** (intentional pin bump only).
   `scripts/update-pandoc-conformance-corpus.sh` regenerates every
   `expected.native` from local pandoc; review the full diff before
   committing — drift in pandoc's output is information, not a bug.

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
- Grid-table layout (`grid_table` + `find_grid_cell` in
  `native_projector.rs`) is the `gridtables`-style 2D pass: union of `+`
  positions across all sep-style lines (full + partial) gives canonical
  column boundaries; indices of those sep-style lines give canonical row
  boundaries. Cells are detected by walking unoccupied (block_row,
  col) in scan order and finding the smallest valid bounding rectangle
  (top/bottom in `{-,=,:,+}`, left/right in `{|,+}`, no fully-spanning
  interior separator). Don't go back to range-based row slicing — it
  can't represent ColSpan ≥ 2 cells whose canonical column positions
  are missing on the row's top separator. The `interior_split` check
  is load-bearing: it's what stops a tall cell from being chosen when
  a partial separator inside it would split the cell at a fully
  spanning row.
- Per-cell `RowSpan`/`ColSpan` lives on `GridCell`, not on
  `TableData`. Pipe/simple/multiline tables wrap their cells via
  `GridCell::no_span(...)`. Don't add a parallel `Vec<Vec<(u32,u32)>>`
  next to `head_rows`/`body_rows`; the struct stays in sync.

## Latest session

- **Date**: 2026-05-04 (Grid-table layout pass: canonical col/row
  boundaries from union of `+` positions in sep-style lines, scan-order
  cell detection with smallest-valid-rectangle search and interior-split
  guard. Closes the seed corpus.)
- **Pass before → after**: 186 → 187 / 187 (+1 import: #71,
  `imported-grid_table_planets`). 100% pass rate.
- **What landed**:
  - **Projector: layout-aware grid table**
    (`crates/panache-parser/tests/pandoc/native_projector.rs` —
    `grid_table` rewritten, new `find_grid_cell`, new
    `parse_grid_cell_text`). Old per-row `grid_row_cells_blocks` /
    `grid_column_ranges` / `parse_cell_text_blocks` removed — the new
    function consumes the whole `GRID_TABLE` node and tags each line
    with the parent `SyntaxKind` (TABLE_HEADER/ROW/FOOTER) so head/foot
    classification still works after the row-block re-discretization.
    Lines split on `\n` (not `split_inclusive`); padded to a 2D char
    grid; sep-style lines = "has `+` AND only `+/-/=/:/|/space`";
    canonical cols/rows derived from those. Cells found by scan-order
    `for sr in 0..nrows / for sc in 0..ncols` with an `occupied[sr][sc]`
    grid; `find_grid_cell` does a `for ec / for er` search picking the
    smallest valid (er, ec) and rejects rectangles whose top edge runs
    into a `|`/space (early `break` on top-edge fail), whose left/right
    edges aren't `|` or `+`, whose bottom edge isn't `-/=/:/+`, or
    that contain a fully-spanning interior partial separator.
  - **Projector: `GridCell` struct + per-cell spans on `TableData`**.
    `head_rows`/`body_rows`/`foot_rows` flipped from
    `Vec<Vec<Vec<Block>>>` to `Vec<Vec<GridCell>>`.
    `cells_to_plain_blocks` returns `Vec<GridCell>`. Pipe / simple /
    multiline table builders wrap their cells via `GridCell::no_span`;
    `write_table_row` emits `RowSpan {n}` / `ColSpan {n}` from the
    cell instead of the literal `RowSpan 1 ColSpan 1`.
  - **Allowlist**: `crates/panache-parser/tests/pandoc/allowlist.txt`
    (+1: 71 inserted between 70 and 72, under `# imported`).
- **Cases unlocked** (+1, allowlisted under `# imported`):
  - 71 (imported-grid_table_planets)
- **Files changed (classified)**:
  - **projector**:
    `crates/panache-parser/tests/pandoc/native_projector.rs` —
    `TableData` field types, new `GridCell` struct, full grid_table
    rewrite, `find_grid_cell` + `parse_grid_cell_text` helpers,
    `write_table_row` reads cell spans, `cells_to_plain_blocks`
    returns `Vec<GridCell>`, pipe / simple / multiline builders wrap
    cells via `GridCell::no_span`.
  - **allowlist**:
    `crates/panache-parser/tests/pandoc/allowlist.txt` (+1: 71).
- **Don't redo**:
  - The `interior_split` rejection inside `find_grid_cell` walks rows
    `(i+1)..l` looking for a row that has `+` at BOTH col j and col k
    *and* sep-only chars between. If only one of the two corners has
    `+` (or if the inner span has any `|`), that's a partial sep that
    splits SOME cells (the one bounded at that col) but does NOT split
    THIS cell. Don't tighten to "any `+` between" — multi-row cells
    legitimately step over partial seps that don't fully cross them.
  - Header / body / foot classification reads the `SyntaxKind` of each
    interior line in a row block, not the row block's bounding
    separators. The bounding separator's parent in the CST is whichever
    node it falls into syntactically; using the *interior* lines is
    robust because the parser tags TABLE_HEADER on the actual header
    content, TABLE_ROW on body content, TABLE_FOOTER on foot content.
    Don't reach for the bounding sep's parent — it's brittle and
    sometimes ambiguous (a sep can be a sibling of both).
  - Column widths and alignments come from the alignment-bearing
    separator (the one with `:`s) when present, falling back to the
    first separator. The first separator may be a partial-cols header
    border (e.g. planets case has 11 `+`s on line 1 but 13 on line 5,
    and line 5 is the canonical) so don't lock width/align extraction
    to the first separator just because both are `TABLE_SEPARATOR`s.
  - The cell-content extraction strips ONE leading space (the cell
    pad inside `| `) per interior line and trims trailing whitespace,
    matching the old `grid_row_cells_blocks` rule. Don't switch to
    `trim_start()` — a 4-space-indented cell needs to remain
    indented-by-3 after stripping the single pad space, which is what
    pandoc treats as a code block.
  - The `for r in (i+1)..l` / `for c in sc..ec` style indexed loops
    in `grid_table` and `find_grid_cell` are gated by
    `#[allow(clippy::needless_range_loop)]`. The lint complains
    because `r`/`c` index multiple parallel arrays (`grid`,
    `occupied`); the iterator rewrite would lose readability for no
    win. Don't strip the allow.

## Earlier session (2026-05-04, Same-line BQ inside LIST_ITEM: recurse
  list-marker detection inside the BQ's content, sibling-list-marker
  continuation across the BQ prefix when the deepest container is an
  inner LIST inside the BQ, and matching formatter fix to emit outer
  continuation indent on BQ continuation lines)
- **Pass before → after**: 185 → 186 / 187 (+1 import: #91). Two
  parser-shape fixes plus one formatter fix that compose to unlock #91:
  1. **Recursive list-marker open inside same-line BQ-in-list-item**:
     `finish_list_item_with_optional_nested` (`lists.rs`)'s existing
     same-line BLOCK_QUOTE branch (`text_to_buffer.starts_with('>')`)
     unconditionally opened a PARAGRAPH for the post-`>` content. Now,
     when the post-`>` content begins with another list marker followed
     by real content, recursively open a nested LIST + LIST_ITEM inside
     the BLOCK_QUOTE so `- > - foo` produces
     `BulletList [BlockQuote [BulletList [[Plain "foo"]]]]` instead of
     `BulletList [BlockQuote [Para [Str "- foo"]]]`. Both pandoc-markdown
     and CommonMark agree (verified via `pandoc -f markdown` /
     `pandoc -f commonmark`). Mirrors the pattern of the same-line
     nested-list block immediately above it (`- - foo`). The bare-marker
     case (`after_inner.is_empty()`) and thematic-break case fall through
     to the existing PARAGRAPH path, matching pandoc.
  2. **Sibling-list-marker continuation across BQ prefix**: in
     `parse_line` (`core.rs`)'s `bq_depth == current_bq_depth > 0`
     branch, before the existing close-LIST_ITEM logic, check whether
     the BQ-stripped content is a list marker matching an open inner
     LIST in the container stack. If so — and the marker's leading
     whitespace inside the BQ is below the marker's threshold (i.e.
     wouldn't push it into the previous inner item's content area) —
     close down to the inner LIST level, emit BQ markers as direct
     children of the inner LIST, and add a sibling LIST_ITEM. The
     match deliberately ignores the marker's source column (just
     marker-type + bq-depth alignment) so both column-aligned
     continuation (`-  > - 0:` then `   > - 2:`, case #91 input) and
     lazy continuation (`- > - foo` then `> - bar`, our own formatter
     output) attach as siblings. Without this, the dispatcher saw the
     post-strip `- 2:` at column 0 and opened a new outer LIST_ITEM,
     and re-parsing the formatter output broke idempotency.
- **What landed**:
  - **Parser: recursive list-marker inside BQ-in-list content**
    (`crates/panache-parser/src/parser/blocks/lists.rs` —
    `finish_list_item_with_optional_nested`, inserted inside the
    existing same-line `text_to_buffer.starts_with('>')` branch, after
    the BLOCK_QUOTE node opens and the BlockQuote container is pushed,
    before the fallback `start_paragraph_if_needed` call). Mirrors the
    nested-list recursion pattern at the top of the function; sets
    `indent_cols: bq_content_col, indent_bytes: 0` so the new inner
    LIST's `base_indent_cols` reflects the inner content's source
    column.
  - **Parser: sibling-list continuation across BQ prefix**
    (`crates/panache-parser/src/parser/core.rs` — `parse_line`'s
    `bq_depth > 0` branch, inserted at the very start of the branch
    before the existing close-LIST_ITEM check). Walks the stack
    deepest-first to find a matching LIST whose BQ-count == bq_depth,
    gated by `inner_indent_cols_raw < marker_len + spaces_after_cols`
    so leading whitespace inside the BQ that would push the marker
    into the previous inner LIST_ITEM's content area falls through
    (those are nested-list cases, not siblings). On match: close down
    to the LIST level, emit BQ markers as direct children, then
    `add_list_item` with `indent_cols = matched LIST.base_indent_cols`
    so subsequent lines at the matched column still see the LIST in
    `find_matching_list_level`.
  - **New parser fixture**:
    `crates/panache-parser/tests/fixtures/cases/list_item_blockquote_inner_list/`
    (`- > - foo`) — pins the recursive shape: LIST > LIST_ITEM >
    BLOCK_QUOTE > LIST > LIST_ITEM > PLAIN. Wired into
    `golden_parser_cases.rs` between
    `list_item_same_line_blockquote_marker_commonmark` and
    `list_item_same_line_blockquote_marker_pandoc`.
  - **Snapshot regeneration**: 1 parser CST snapshot
    (`parser_cst_issue_174_blockquote_list_reorder_losslessness`)
    updated to reflect the new shape (3 outer LIST_ITEMs each with
    BLOCK_QUOTE > inner LIST, instead of one big PARAGRAPH).
  - **Formatter: outer-indent prefix on BQ continuation lines in
    same-line BQ-in-list-item case**
    (`crates/panache-formatter/src/formatter/lists.rs` —
    `format_list_item`'s leading-BQ branch). Captures the BQ output
    starting from `bq_start = self.output.len()`, then post-processes
    `split_off(bq_start)` to splice `&" ".repeat(hanging)` before each
    non-first non-blank line. Pandoc emits `  > foo` (with outer
    indent) for continuation, not `> foo`. Without this, the `>`
    BLOCK_QUOTE prefix sits at column 0 on subsequent lines, and
    re-parsing drops the outer LIST_ITEM context (idempotency fails).
    Localized to format_list_item rather than threading a new arg
    through the BQ formatter.
  - **New formatter golden case**:
    `tests/fixtures/cases/list_item_blockquote_inner_list_siblings/`
    (input `- > - foo / [2 spaces]> - bar`, expected `- > - foo /
    [2 spaces]> - bar`) — pins the pandoc-equivalent formatter output
    and exercises idempotency for the new structural shape. Wired
    into `tests/golden_cases.rs` between
    `list_interrupts_paragraph_commonmark` and
    `list_mixed_bullets_commonmark`.
- **Cases unlocked** (+1, allowlisted under `# imported`):
  - 91 (imported-issue_174_blockquote_list_reorder_losslessness)
- **Files changed (classified)**:
  - **parser-shape**:
    `crates/panache-parser/src/parser/blocks/lists.rs`
    (`finish_list_item_with_optional_nested` — recursive list open in
    same-line BQ-in-list branch),
    `crates/panache-parser/src/parser/core.rs` (`parse_line`'s
    `bq_depth > 0` branch — sibling-list-marker continuation block).
  - **formatter**:
    `crates/panache-formatter/src/formatter/lists.rs`
    (`format_list_item`'s leading-BQ branch — outer-indent prefix on
    BQ continuation lines via post-process splice).
  - **parser fixture**: new `list_item_blockquote_inner_list/`
    directory under `crates/panache-parser/tests/fixtures/cases/` and
    matching `parser_cst_list_item_blockquote_inner_list.snap`. Wired
    into `golden_parser_cases.rs`.
  - **snapshot**:
    `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_issue_174_blockquote_list_reorder_losslessness.snap`
    (existing parser fixture pinned to the pandoc-correct shape).
  - **formatter golden**: new
    `tests/fixtures/cases/list_item_blockquote_inner_list_siblings/`
    with `input.md` + `expected.md`. Wired into
    `tests/golden_cases.rs`.
  - **allowlist**:
    `crates/panache-parser/tests/pandoc/allowlist.txt` (+1: 91
    inserted between 90 and 92, under `# imported`).
- **Don't redo**:
  - The recursive-list branch fires *only* in the same-line BQ-in-list
    path (i.e. `text_to_buffer.starts_with('>')`). The bare-marker
    case (e.g. `- > -` with no content after the inner `-`) and the
    thematic-break-shaped case (`- > * * *`) fall through to the
    fallback `start_paragraph_if_needed` path, matching pandoc only
    partially (the thematic-break case still produces a Para in our
    parser, not a HorizontalRule). If a future case needs the
    thematic-break-inside-BQ-inside-LIST shape, that's a separate
    sub-task — don't try to fold it into this branch.
  - The sibling-continuation matching is intentionally lenient: it
    matches by marker type + bq-depth, NOT by exact source column.
    The threshold gate
    (`inner_indent_cols_raw < marker_len + spaces_after_cols`) is the
    safety check: it prevents stealing the nested-list case
    (`>   - bar` with 2+ spaces inside BQ, which pandoc treats as a
    nested LIST inside the previous item, not a sibling). Don't
    tighten the match to require exact column equality — even though
    the formatter now emits properly-indented continuation lines
    (`  > - bar`), pandoc itself permits the dropped-indent form
    (`> - bar`) as lazy continuation. Both should attach as siblings.
  - The formatter post-process splice in the leading-BQ branch uses
    `bq_block.split_inclusive('\n')` so the trailing `\n` stays with
    each line, and skips prefix on blank lines (otherwise blank lines
    in BQs would gain trailing whitespace). The `first` flag skips
    the very first line because the outer `- ` marker is already
    emitted before the BQ formatter runs. Don't switch to
    `lines()` — it strips the trailing newline and breaks
    reconstruction.
  - The matched-LIST's `base_indent_cols` is read BEFORE
    `close_containers_to(list_level + 1)` because the close mutates
    the stack. Don't move the read after the close — the stack slice
    indexing would point at a different container.
  - The sibling LIST_ITEM is added with
    `indent_cols = matched LIST.base_indent_cols` (not the new line's
    source column). This is load-bearing for subsequent
    `find_matching_list_level` calls — if we passed the lazy line's
    actual source column (e.g. 2 for `> - bar`), a later
    column-aligned line wouldn't find this LIST as the right level.
  - Pandoc's BQ-relative column semantics are NOT fully modeled by
    this fix. Cases like `>   - bar` (2 spaces inside BQ) where
    pandoc opens a nested LIST inside the previous inner item still
    produce the wrong shape in our parser (it falls through and
    likely starts a new outer item). If a future conformance case
    surfaces that shape, the fix is a separate session — not a
    relaxation of this branch's threshold gate.
  - Case #91's parser fixture
    (`crates/panache-parser/tests/fixtures/cases/issue_174_blockquote_list_reorder_losslessness/`)
    has no `parser-options.toml` — it defaults to Pandoc dialect.
    Both fixes apply to CommonMark too (verified by both `pandoc -f
    markdown` and `pandoc -f commonmark`), so no per-dialect
    branching was added.

## Prior sessions

Older session logs were pruned to keep the recap scannable. Use `git log` on
`crates/panache-parser/tests/pandoc/allowlist.txt` and the projector to
trace which case unlocked when. Cross-session lessons that still apply have
been folded into the global "Don't redo" section above.

- 2026-05-04: Definition-list continuation — `>` continuation markers
  and bullet-list openings recognized at the definition's content
  column (#43 unlocked) — see git log on
  `crates/panache-parser/src/parser/utils/continuation.rs` and
  `crates/panache-parser/src/parser/core.rs`
  (`shifted_blockquote_from_list`).
- 2026-05-04: Lazy ListItem continuation in BQ-list — no-`>`
  plain-text line folds into the deepest open list item's buffer
  rather than closing the outer blockquote (#96 unlocked) — see git
  log on `crates/panache-parser/src/parser/core.rs` (`parse_line`
  lazy ListItem buffer continuation block).
- 2026-05-04: Blank-line peek-loop inside blockquote — skip
  blank-in-BQ lines so multi-blank-then-continuation list items don't
  prematurely close (#34 unlocked) — see git log on
  `crates/panache-parser/src/parser/core.rs` (blank-line peek loop).
- 2026-05-04: Citations proper — `Cite [Citation, ...] [Inline, ...]`
  projection with prefix/suffix inline parsing, `@key [locator]`
  AuthorInText absorption, and doc-order noteNum pre-pass (#38 unlocked) —
  see git log on `tests/pandoc/native_projector.rs`.

- 2026-05-04: HTML block per-line splitting projector via
  `markdown_in_html_blocks` (#181 unlocked) — see git log on
  `tests/pandoc/native_projector.rs`.
- 2026-05-04: HTML `<div>` block → `Div(attr, blocks)` projector via
  `markdown_in_html_blocks` (#78 unlocked) — see git log on
  `tests/pandoc/native_projector.rs`.
- 2026-05-03: Grid-table multi-line cells + TableFoot via block-reparse
  projector path (#68, #70 unlocked).
- 2026-05-03: HTML comment paragraph-interrupt gated by dialect; directive
  system extended to INLINE_HTML (#79 unlocked).
- 2026-05-03: Same-line BLOCK_QUOTE inside LIST_ITEM ungated for Pandoc
  (#93, #108 unlocked).
- 2026-05-03: List-item indent — include parent-LIST leading WHITESPACE in
  content offset (#44 unlocked).
