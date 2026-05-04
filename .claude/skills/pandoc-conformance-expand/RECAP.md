# Pandoc-conformance recap

Rolling handoff between sessions. Keep terse. Read `report.txt` for the full
state; this file is judgment calls only.

## Suggested next targets

Corpus is at **192 / 192 passing (100%)**. There are no failing imports left
in the seed. Future work should focus on **growing the corpus**:

1. **Smart typography case** — pandoc's `markdown` flavor enables `smart`
   by default, so `"foo"`/`---`/`...`/`don't` produce
   `Quoted DoubleQuote`/`\8212`/`\8230`/`\8217` in pandoc-native. Our
   projector currently emits raw bytes. Adding a focused
   `smart_typography` case would force a projector pass that handles
   smart-quote pairing (DoubleQuote/SingleQuote) and codepoint
   substitution for em-dash, en-dash, ellipsis, and apostrophes.
   Substantive but bounded — projector-only, no parser change.
2. **Line block with indented continuation lines** — pandoc maps leading
   whitespace inside line-block lines to non-breaking-space codepoints
   (`\160`), so `|     Indented` becomes `Str "\160\160\160\160Indented"`.
   Our `line_block` projector splits on whitespace (Space inlines). A
   `line_block_indented` case would force a projector tweak: when a
   LINE_BLOCK_LINE TEXT token starts with leading spaces, convert those
   spaces to NBSP and prepend to the first Str. Bounded projector-only fix.
3. **Other un-covered pandoc-markdown constructs.** Rough candidates
   probed but not added this session: `header_attrs_classes_only` (e.g.
   `# Title {.cls1 .cls2}`), `image_with_full_attrs`
   (`![alt](pic.png){#id .cls width="100"}`),
   `definition_list_with_continuation_paragraphs` (focused), `link_attrs_combined_with_title_in_dest`. Add `<NNNN>-<section>-<slug>/` dirs
   with `input.md` and `expected.native` regenerated via `pandoc -f
   markdown -t native`. Allowlist when green; otherwise leave
   un-allowlisted and triage.
4. **Re-run with a newer pandoc build** (intentional pin bump only).
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
- `figure_block` migrates *only* the image's id to the Figure attr
  and **clears the id from the image** (the image keeps its classes
  and key-value pairs). Pandoc's `implicit_figures` extension behaves
  this way: `Figure ("id", [], []) (Caption ...) [Plain [Image ("",
  [], []) ...]]`. Don't leave the id on the image — pandoc-native
  diverges on it. Implementation in
  `crates/panache-parser/tests/pandoc/native_projector.rs:figure_block`
  uses `std::mem::take(&mut attr.id)` to move the id without cloning.

## Latest session

- **Date**: 2026-05-04 (Parser-shape fix: bullet marker at indent ≥ 4
  cannot continue a shallow-base list across a blank line. Closes #192,
  the orphan list-marker case where pandoc emits a CodeBlock between two
  BulletLists rather than a sibling BulletList.)
- **Pass before → after**: 191 → 192 / 192 (+1 hand-curated case,
  parser-shape fix). 100% pass rate.
- **What landed**:
  - **Parser-shape fix**
    (`crates/panache-parser/src/parser/utils/continuation.rs` —
    `compute_levels_to_keep`'s LIST/Bullet branch). Previous logic
    kept a bullet LIST open across a blank line whenever
    `effective_indent` fell within `[base, base+3]`, even when
    `effective_indent ≥ 4` and `base < 4`. That kept the list open in
    a state where no LIST_ITEM could absorb the deeper indent (item
    content_col > 4), and `handle_list_open_effect`'s "close all,
    start new top-level LIST" fallback then produced a sibling
    BulletList where pandoc emits a CodeBlock. Added a
    `jumps_out_of_shallow_list` guard: a bullet at indent ≥ 4 cannot
    continue a list whose `base_indent_cols < 4`. The LIST_ITEM
    branch below still rescues the LIST when the previous item's
    content column accommodates the deeper indent (keep_level is
    monotonic), so this only closes the list when no item can absorb
    it. Verified against pandoc on `* a / blank / "    + sub"`
    (sublist of a, item rescues), `  * a / "    * b"` (sublist, item
    rescues), `  * a 1 / "   * misindented" / blank /
    "    + sub"` (CodeBlock, no rescue — the failing case).
  - **Corpus +1**:
    - `0192-block-list-orphan-marker-codeblock/` — pandoc emits
      `BulletList[item1, Misindented], CodeBlock "+ Sub a.\n+ Sub b.\n+ Sub c.", BulletList[Outer 2]`
      from the misindented-list-then-deeper-indent input. The `+`
      lines are not a sibling/sublist; they're an indented code
      block once the surrounding list closes.
  - **Parser fixture**:
    `crates/panache-parser/tests/fixtures/cases/list_orphan_indent4_marker_after_blank_becomes_codeblock/`
    pins the new CST shape (LIST → CODE_BLOCK → LIST) before the
    allowlist guard.
  - **Formatter fixture**:
    `tests/fixtures/cases/list_orphan_indent4_marker_after_blank_becomes_codeblock/`
    pins the formatter output (indented code converts to fenced
    code) and exercises idempotency on the new structural shape.
  - **Allowlist**: `crates/panache-parser/tests/pandoc/allowlist.txt`
    (+1: 192 inserted under `# block` after 189). Block section now
    15 entries.
- **Cases unlocked** (+1):
  - 192 (block-list-orphan-marker-codeblock) — needed parser-shape fix
- **Files changed (classified)**:
  - **parser-shape**:
    `crates/panache-parser/src/parser/utils/continuation.rs` —
    `compute_levels_to_keep`'s Bullet branch in the LIST container,
    new `jumps_out_of_shallow_list` guard.
  - **corpus**: 1 new directory under
    `crates/panache-parser/tests/fixtures/pandoc-conformance/corpus/`
    (`0192-block-list-orphan-marker-codeblock/`) with `input.md` +
    `expected.native` (regenerated via `pandoc -f markdown -t
    native`).
  - **parser fixture**: 1 new directory under
    `crates/panache-parser/tests/fixtures/cases/`
    (`list_orphan_indent4_marker_after_blank_becomes_codeblock/`)
    with `input.md`; CST snapshot under `tests/snapshots/`.
  - **formatter fixture**: 1 new directory under
    `tests/fixtures/cases/`
    (`list_orphan_indent4_marker_after_blank_becomes_codeblock/`)
    with `input.md` + `expected.md`.
  - **wiring**: `crates/panache-parser/tests/golden_parser_cases.rs`
    (+1 entry), `tests/golden_cases.rs` (+1 entry).
  - **allowlist**:
    `crates/panache-parser/tests/pandoc/allowlist.txt` (+1: 192).
- **Don't redo**:
  - `compute_levels_to_keep`'s `keep_level` is **monotonic** — a LIST
    that says "don't keep me" can still be kept open if an enclosed
    LIST_ITEM at a deeper index says "keep me". That's the load-bearing
    interaction behind the new `jumps_out_of_shallow_list` guard: it's
    safe to set `continues_list = false` for the LIST whenever the
    indent jumps to ≥ 4 from a shallow base, because the LIST_ITEM
    branch's `effective_indent >= content_col` check still rescues the
    LIST (and starts a sublist of the open item) whenever the open
    item's content column can accommodate the deeper indent. The LIST
    only genuinely closes when *no* item can absorb the line.
  - The fix lives in `compute_levels_to_keep`, not in
    `block_dispatcher`'s `if indent_cols >= 4 && !ctx.in_list`
    gate. Tightening the dispatcher gate to also reject
    `(in_list && list_indent_info.is_none())` would work but would
    leave the LIST open in containers — the resulting CodeBlock
    would end up inside `LIST` instead of as a top-level sibling of
    `BulletList`, which doesn't match pandoc-native. Closing the LIST
    in `compute_levels_to_keep` keeps the structural placement correct.
  - When verifying this kind of indent-vs-marker rule, the
    discriminator is **not** "marker at indent ≥ 4" alone. It's
    "marker at indent ≥ 4 with a shallow-base parent list AND no open
    item whose content column accommodates the indent." Probe pandoc
    with both a content-col-4 parent (`*   item / blank /
    "    + sub"` — sublist) and a content-col-5 parent (`   * item /
    blank / "    + sub"` — CodeBlock) before rejecting the change as
    "too aggressive."

## Earlier session (2026-05-04, Corpus growth +4: nested blockquotes,
  figure with `{#fig:label}`, link with attrs, code-span with attrs.
  Two small projector fixes: figure id-migration clears image id;
  inline code now reads ATTRIBUTE child.)

- **Pass before → after**: 187 → 191 / 191 (+4 hand-curated cases).
  100% pass rate.
- **What landed**:
  - **Projector: figure id is moved off the image**
    (`crates/panache-parser/tests/pandoc/native_projector.rs` —
    `figure_block`). Previous code matched the image's `attr.id` and
    cloned it onto the Figure attr but left the original on the
    image, which diverged from pandoc-native (pandoc emits Figure
    `("id", [], [])` and Image `("", [], [])`). Rewrote the match to
    move the id via `std::mem::take(&mut attr.id)` and re-construct
    the Image with the cleared attr.
  - **Projector: inline code reads its ATTRIBUTE child**
    (`crates/panache-parser/tests/pandoc/native_projector.rs` —
    `inline_from_node`'s `INLINE_CODE` arm). Was hard-coded to
    `Attr::default()`; switched to `extract_attr_from_node(node)` so
    `` `bit`{#cs1 .raw} `` projects to `Code ("cs1", ["raw"], []) "bit"`
    instead of `Code ("", [], []) "bit"`. ATTRIBUTE was already a child
    of INLINE_CODE in the CST — the projector just wasn't reading it.
  - **Corpus +4**:
    - `0188-block-nested-blockquotes/` — 3-level nested BlockQuote
      (`> > > Third level.`) with multi-paragraph inner quote and
      lazy continuation across the outer prefix.
    - `0189-block-figure-with-attr-id/` — single
      `![A captioned figure](image.png){#fig:label}` paragraph
      exercising the figure attr migration.
    - `0190-inline-link-with-attrs/` — links with title plus
      `{.external title="ext"}` attribute, and id+class+kv form
      `{#link1 .ref key="value"}`.
    - `0191-inline-code-with-attrs/` — code spans with single
      `{.c}` attr, id+class form, and multi-class form
      `{.python .numberLines}`.
  - **Allowlist**: `crates/panache-parser/tests/pandoc/allowlist.txt`
    (+4: 188, 189 inserted under `# block`; 190, 191 inserted under
    `# inline` after 25). Block section now 14 entries; inline section
    now 15 entries.
- **Cases unlocked** (+4, all hand-curated additions):
  - 188 (block-nested-blockquotes)
  - 189 (block-figure-with-attr-id) — needed projector fix
  - 190 (inline-link-with-attrs) — already worked via
    `extract_attr_from_node` on LINK
  - 191 (inline-code-with-attrs) — needed projector fix
- **Files changed (classified)**:
  - **projector**:
    `crates/panache-parser/tests/pandoc/native_projector.rs` —
    `figure_block` (id migration clears image id);
    `inline_from_node`'s INLINE_CODE arm (read ATTRIBUTE child).
  - **corpus**: 4 new directories under
    `crates/panache-parser/tests/fixtures/pandoc-conformance/corpus/`
    (`0188-`, `0189-`, `0190-`, `0191-`), each with `input.md` +
    `expected.native` (the latter generated by
    `pandoc -f markdown -t native`).
  - **allowlist**:
    `crates/panache-parser/tests/pandoc/allowlist.txt` (+4: 188, 189,
    190, 191).
- **Don't redo**:
  - Image id migration — see new global Don't-redo entry above.
  - `INLINE_CODE` projector now uses `extract_attr_from_node`. It
    looks for either an ATTRIBUTE token *or* an ATTRIBUTE node child
    (the parser emits both shapes depending on context). Don't
    hand-roll a duplicate attr lookup — `extract_attr_from_node`
    already handles both.
  - Adding hand-curated cases bumps the next ID past the imported
    block (which ends at 187). New cases use a non-`imported` section
    prefix (`block`, `inline`) so the bulk import script's
    `wipe-then-rebuild` of `*-imported-*` dirs never touches them.
    Don't reuse the `imported` prefix for hand-curated additions.
  - The link-attrs case worked without any projector change because
    `render_link_inline` already calls `extract_attr_from_node(node)`.
    Don't add a duplicate attr extraction at the LINK arm of
    `inline_from_node` — that arm is unreachable for LINK (links go
    through `push_inline_node` → `render_link_inline`).

## Earlier session (2026-05-04, Grid-table layout pass: canonical col/row
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

## Prior sessions

Older session logs were pruned to keep the recap scannable. Use `git log` on
`crates/panache-parser/tests/pandoc/allowlist.txt` and the projector to
trace which case unlocked when. Cross-session lessons that still apply have
been folded into the global "Don't redo" section above.

- 2026-05-04: Same-line BQ inside LIST_ITEM — recursive list-marker open
  inside the BQ's content; sibling-list-marker continuation across the
  BQ prefix; matching formatter fix to emit outer-indent on BQ
  continuation lines (#91 unlocked) — see git log on
  `crates/panache-parser/src/parser/blocks/lists.rs`,
  `crates/panache-parser/src/parser/core.rs` (parse_line bq_depth
  branch), and `crates/panache-formatter/src/formatter/lists.rs`
  (`format_list_item` leading-BQ branch). The
  "sibling-continuation matching is intentionally lenient" rule and
  the "BQ-relative column semantics not fully modeled" caveat remain
  load-bearing for future BQ/list work.
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
