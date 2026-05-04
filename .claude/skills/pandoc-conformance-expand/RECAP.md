# Pandoc-conformance recap

Rolling handoff between sessions. Keep terse. Read `report.txt` for the full
state; this file is judgment calls only.

## Suggested next targets

Ranked by likely shared root cause and leverage. Numbers in parentheses are the
count of currently-failing imports remaining under that bucket in the latest
`report.txt`. 1 import remains failing total (186 / 187 passing).

1. **Tables --- #71 grid_table_planets (1)** --- rowspan/colspan layout.
   Pandoc emits `RowSpan N` / `ColSpan N` for cells whose `+   +-----+`-style
   separators omit the column-divider `+` to span the cell into the next
   row, and similarly for column merges. Our `grid_column_ranges` helper
   currently skips empty `+`-to-`+` ranges (which represent spans), so
   spanned cells produce wrong column ordering. Heavy — needs a layout
   model that assigns cells to specific columns and tracks RowSpan/ColSpan
   counts per cell. The single remaining failure.

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

- **Date**: 2026-05-04 (Same-line BQ inside LIST_ITEM: recurse list-marker
  detection inside the BQ's content, sibling-list-marker continuation
  across the BQ prefix when the deepest container is an inner LIST
  inside the BQ, and matching formatter fix to emit outer continuation
  indent on BQ continuation lines)
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

## Earlier session (2026-05-04, Lazy ListItem continuation in BQ-list:
  no-`>` plain-text line folds into the deepest open list item's buffer
  rather than closing the outer blockquote)
- **Pass before → after**: 184 → 185 / 187 (+1 import: #96).
  Parser-shape fix in `parse_line`'s `bq_depth < current_bq_depth`
  branch (`core.rs`): added a new lazy-continuation check that fires
  when the deepest container is a `ListItem` inside a blockquote and
  the line is plain text (not a list marker, not an HR/fence in
  CommonMark). The line is appended to the open `ListItemBuffer` (the
  buffered analogue of an open Paragraph) so the BQ stays open and the
  list item's PLAIN absorbs the lazy line as a SoftBreak-joined
  continuation, matching pandoc-native. This single fix cascades:
  in #96, keeping the outer BQ open also prevented the spurious
  sibling BQ that line 6 (`> > And...`) was creating, and the
  subsequent `> Back...` Para and `> - Second item` inner BulletList
  fall into place inside the same outer BQ. Three blocks of changes:
  parser core, parser CST snapshot for the existing
  `lazy_continuation_deep` fixture, and the formatter golden expected
  for the same case (the lazy line now reflows inside the list item
  with `>   ` continuation indent at line-width).
- **What landed**:
  - **Parser: lazy ListItem buffer continuation in BQ-list**
    (`crates/panache-parser/src/parser/core.rs` — `parse_line`,
    inserted between the existing lazy-paragraph and lazy-list-marker
    continuation blocks). Mirrors the lazy-paragraph branch's
    structure: bq_depth>0 path buffers any explicit `>` markers via
    `buffer.push_blockquote_marker` then appends `inner_content`;
    bq_depth==0 path appends the original `line`. Both clear
    `marker_only` when content is non-blank. HR/fence interrupt
    checks gated on Dialect::CommonMark mirror the paragraph branch.
  - **New parser fixture**:
    `crates/panache-parser/tests/fixtures/cases/blockquote_list_lazy_continuation_no_marker/`
    (`> - foo / bar`) — pins the minimal lazy ListItem continuation
    shape: BLOCK_QUOTE > LIST > LIST_ITEM > PLAIN with both `foo` and
    `bar` as TEXT siblings inside one PLAIN. Wired into
    `golden_parser_cases.rs` between `blockquote_list_blockquote` and
    `blockquote_list_no_marker_closes_commonmark`.
  - **Snapshot regeneration**: 1 parser CST snapshot
    (`parser_cst_lazy_continuation_deep`) updated to reflect the new
    shape — the lazy line `lazy continuation across the first level`
    now sits inside the BQ's BulletList item PLAIN, and the entire
    document is one outer BLOCK_QUOTE (prior snapshot had three
    sibling top-level nodes: BQ + PARAGRAPH + BQ).
  - **Formatter golden update**:
    `tests/fixtures/cases/lazy_continuation_deep/expected.md`
    regenerated. The first item now reflows as
    `> - This is a blockquoted list item with lazy continuation across the first / >   level`
    (continuation at hanging indent) instead of the prior
    `> - … with / lazy continuation …` shape that preserved the
    original lossy line break.
- **Cases unlocked** (+1, allowlisted under `# imported`):
  - 96 (imported-lazy_continuation_deep)
- **Files changed (classified)**:
  - **parser-shape**:
    `crates/panache-parser/src/parser/core.rs` (`parse_line` —
    new lazy ListItem buffer continuation block, ~58 lines).
  - **parser fixture**: new
    `blockquote_list_lazy_continuation_no_marker/` directory under
    `crates/panache-parser/tests/fixtures/cases/` and a matching
    `parser_cst_blockquote_list_lazy_continuation_no_marker.snap`.
    Wired into `golden_parser_cases.rs`.
  - **snapshot**:
    `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_lazy_continuation_deep.snap`
    (existing parser fixture pinned to the pandoc-correct shape).
  - **formatter golden**:
    `tests/fixtures/cases/lazy_continuation_deep/expected.md`.
  - **allowlist**:
    `crates/panache-parser/tests/pandoc/allowlist.txt` (+1: 96
    inserted between 95 and 97, under `# imported`).
- **Don't redo**:
  - The new branch fires *only* when the deepest container is
    `ListItem` AND we're `in_blockquote_list`. The `in_blockquote_list`
    guard matters: a top-level `- foo / bar` (no BQ) reaches a
    different code path entirely (the `bq_depth == current_bq_depth ==
    0` else-if at the bottom of `parse_line`) which already handles
    lazy ListItem continuation via `parse_inner_content`. Don't
    generalize this branch to fire outside BQ — it would double-fire.
  - The `try_parse_list_marker(line, self.config).is_none()` guard
    prevents this branch from stealing list-marker lines (e.g.
    `- bar`) from the existing lazy list-marker continuation block
    immediately below it. Don't remove the guard — the order matters
    too (this branch is positioned *before* the lazy list-marker
    branch, but the list-marker branch is `bq_depth == 0` only and
    Pandoc-only; the guard keeps the two branches non-overlapping).
  - The HR/fence interrupt checks intentionally mirror the lazy
    *paragraph* continuation branch's `is_commonmark` gate. Pandoc's
    actual behavior is more permissive than CommonMark (HR doesn't
    interrupt; fence interrupts even outside lazy contexts because it
    opens at column 0). Don't tighten the gate to fire fence interrupt
    in Pandoc here — fence opening at column 0 is handled elsewhere
    (paragraph closing on fence detection in dispatcher), and the
    lazy-continuation branch is just consistent with the paragraph
    sibling in this file.
  - Verified pandoc-native behavior: `# Heading`, table separators,
    and other paragraph-interrupting lines do NOT interrupt lazy
    continuation in pandoc — they are absorbed as text into the
    PLAIN. So the conservative HR/fence-only interrupt check matches
    pandoc, not just CommonMark. Don't add ATX-heading or
    table-shape interrupts.
  - Setext underline (`===`, `---`) lines below a `> - foo`
    pattern cause pandoc to *retroactively* reparse the entire prior
    block as a setext heading (treating `>` and `-` as text). This is
    a pandoc quirk we don't model. The current fix doesn't try to
    handle it — if a corpus case ever lands that depends on it,
    that's a separate setext-reparse session.
  - Case #91 still fails. It's a structurally different bug:
    list-marker inside BQ-prefixed content (`-  > - 0:`) needs to be
    detected as opening a nested BulletList inside the outer item's
    BlockQuote. Different code path (the `> -` content gets parsed
    as Paragraph text instead of recursing into list parsing). The
    new lazy-continuation branch doesn't help.

## Earlier session (2026-05-04, Definition-list continuation: `>`
  continuation markers and bullet-list openings recognized at the
  definition's content column)
- **Pass before → after**: 183 → 184 / 187 (+1 import: #43).
  Parser-shape: two narrow continuation-policy gaps inside `Definition`
  containers. (1) `shifted_blockquote_from_list` early-out
  `if !lists::in_list(...)` blocked the column-shift detection when the
  enclosing content container was a `Definition` (or `FootnoteDefinition`)
  rather than a `ListItem`. With the early-out gone, the existing
  `marker_col == 0` guard still handles the top-level case, and a `>`
  at the Definition's content column is recognized as a BQ continuation
  marker (e.g. `:   > a / > b` inside a definition). (2) In
  `definition_plain_can_continue`, a list marker (already followed by
  the existing prev-blank / in_list checks) now also returns false when
  `raw_indent_cols >= content_indent` — meaning a list marker indented
  to the definition's content column opens a nested BulletList inside
  the definition even without a separating blank line, matching
  pandoc-native. CommonMark allowlist green; pandoc allowlist green;
  full parser-crate suite green; full workspace tests green; clippy +
  fmt clean.
- **What landed**:
  - **Parser: drop list-only gate on column-shifted BQ detection**
    (`crates/panache-parser/src/parser/core.rs` —
    `shifted_blockquote_from_list`). Removed
    `if !lists::in_list(&self.containers) { return None; }`. The
    `marker_col == 0` guard already handles top-level / no-content-
    container cases. Function name kept (the old "from_list"
    framing is now historical; the math via
    `current_content_col + content_container_indent_to_strip`
    naturally generalizes).
  - **Parser: list marker at content_col opens inner list**
    (`crates/panache-parser/src/parser/utils/continuation.rs` —
    `definition_plain_can_continue`). Added a
    `content_indent > 0 && raw_indent_cols >= content_indent` short-
    circuit for the list-marker branch, returning false so the
    normal block dispatcher emits the LIST instead of buffering the
    line into the open PLAIN.
  - **Unit test flip**:
    `parser/blocks/tests/definition_lists.rs` —
    `definition_list_plain_does_not_start_list_without_blank_line`
    pinned the *broken* legacy behavior. Renamed to
    `definition_list_plain_starts_list_at_content_column_without_blank_line`
    and flipped the assertions to require PLAIN + LIST inside the
    Definition. Verified against pandoc-native.
  - **Snapshot regeneration**: 1 parser CST snapshot
    (`parser_cst_definition_list`) updated to reflect both fixes —
    `> b` / `> c` now tokenize as continuation markers, and the
    "A definition list with nested items" definition now contains
    PLAIN + LIST instead of one fat PLAIN.
  - **Formatter golden update**:
    `tests/fixtures/cases/definition_list/expected.md` regenerated.
    The "nested items" definition now formats as
    `:   Here comes a list (or wait, is it?)\n    - A\n    - B`
    instead of the collapsed `- A - B` plain text.
  - **New parser fixtures**: two minimal pin-down cases:
    - `crates/panache-parser/tests/fixtures/cases/definition_list_blockquote_continuation/`
      (`Term // : > a / > b / > c`) — pins BQ-marker recognition.
    - `crates/panache-parser/tests/fixtures/cases/definition_list_inner_list_no_blank/`
      (`Term / : plain content / - A / - B` indented at content col)
      — pins inner BulletList without separating blank.
    Both wired into `golden_parser_cases.rs` between
    `definition_list` and `definition_list_nesting`.
- **Cases unlocked** (+1, allowlisted under `# imported`):
  - 43 (imported-definition_list)
- **Files changed (classified)**:
  - **parser-shape**:
    `crates/panache-parser/src/parser/core.rs`
    (`shifted_blockquote_from_list`),
    `crates/panache-parser/src/parser/utils/continuation.rs`
    (`definition_plain_can_continue`).
  - **unit test**:
    `crates/panache-parser/src/parser/blocks/tests/definition_lists.rs`
    (rename + flip the assertions).
  - **snapshot**:
    `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_definition_list.snap`
    (+ two new snapshots from the new fixtures).
  - **parser fixtures**: new `definition_list_blockquote_continuation/`
    and `definition_list_inner_list_no_blank/` directories under
    `crates/panache-parser/tests/fixtures/cases/`, wired into
    `golden_parser_cases.rs`.
  - **formatter golden**:
    `tests/fixtures/cases/definition_list/expected.md`.
  - **allowlist**:
    `crates/panache-parser/tests/pandoc/allowlist.txt` (+1: 43
    inserted between 42 and 44, under `# imported`).
- **Don't redo**:
  - The `lists::in_list` early-out in `shifted_blockquote_from_list`
    was redundant with the `marker_col == 0` check. The function's
    math (`current_content_col` (innermost ListItem/FootnoteDefinition)
    + `content_container_indent_to_strip` (sum of FootnoteDefinition +
    Definition `content_col`s)) generalizes to definitions/footnotes
    naturally — `marker_col` ends up at the absolute column where a
    shifted `>` should sit. Don't reinstate the gate; the function
    name is historical, the behavior is "any indented content
    container".
  - Pandoc-native treats a list marker at the Definition's content
    column as opening a nested BulletList *regardless of whether a
    blank line precedes it*. The legacy unit test pinning the
    "no-list-without-blank" shape was preserving the parser's old
    bug. Don't revert.
  - The `content_indent > 0` part of the new continuation check is
    load-bearing: at top level (`content_indent == 0`,
    e.g. paragraph not inside a Definition) `definition_plain_can_continue`
    is only called when the last container is a Definition, so this
    is a defensive guard rather than a hot path — but dropping it
    would let a 0-indent list marker short-circuit the rest of the
    function in unexpected ways.
  - The `raw_indent_cols >= content_indent` check is intentionally
    "≥", not "==". Pandoc accepts list markers at any indent ≥
    content_col as opening the inner list; if the marker is more
    deeply indented (e.g. column 5 inside a `:   ` definition
    starting at column 4), pandoc still opens a list and uses the
    column for the list's own indent. Don't tighten to "==".
  - Cases #91 and #96 still fail. Both are different continuation-
    policy issues (lazy continuation across BQ depth boundaries;
    list-item closure mid-blockquote). The fixes here don't help
    them — they involve real non-blank lines with policy outcomes
    diverging from pandoc.

## Earlier session (2026-05-04, Blank-line peek-loop inside blockquote: skip blank-in-BQ lines so multi-blank-then-continuation list items don't prematurely close)

- **Pass before → after**: 182 → 183 / 187 (+1 import: #34). Parser-shape
  fix in `parse_line`'s blank-line branch (`core.rs`): the peek-ahead
  loop that skips trailing blank lines now also skips lines that are
  functionally blank in the current blockquote context (e.g. `>` or
  `>   ` when inside a `> ` blockquote). The fix peels off `bq_depth`
  markers via `strip_n_blockquote_markers` and skips when the inner
  content `is_blank_line`. Required updating one parser CST snapshot
  and one formatter golden expected.
- **Cases unlocked**: 34 (imported-blockquote_list_blockquote)
- **Files changed**:
  - parser-shape: `crates/panache-parser/src/parser/core.rs` (blank-line peek loop)
  - snapshot: `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_blockquote_list_blockquote.snap`
  - formatter golden: `tests/fixtures/cases/blockquote_list_blockquote/expected.md`
  - allowlist: `crates/panache-parser/tests/pandoc/allowlist.txt`
- **Don't redo**:
  - The peek-loop guard requires both `peek_bq >= bq_depth` AND
    `is_blank_line(peek_inner)` to skip. Don't drop the depth check —
    a line with `peek_bq < bq_depth` (e.g. plain text after BQ close)
    is a real container-close event that
    `compute_levels_to_keep` needs to see.
  - The check uses `bq_depth` (current line's BQ depth) rather than
    `current_bq_depth` (container-stack depth). For the blank-in-BQ
    case both happen to match because the current line carries `>`
    markers and `current_bq_depth` reflects the open BQ container.

## Prior sessions

Older session logs were pruned to keep the recap scannable. Use `git log` on
`crates/panache-parser/tests/pandoc/allowlist.txt` and the projector to
trace which case unlocked when. Cross-session lessons that still apply have
been folded into the global "Don't redo" section above.

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
