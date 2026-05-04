# Pandoc-conformance recap

Rolling handoff between sessions. Keep terse. Read `report.txt` for the full
state; this file is judgment calls only.

## Suggested next targets

Ranked by likely shared root cause and leverage. Numbers in parentheses are the
count of currently-failing imports remaining under that bucket in the latest
`report.txt`. 2 imports remain failing total (185 / 187 passing).

1. **Tables --- #71 grid_table_planets (1)** --- rowspan/colspan layout.
   Pandoc emits `RowSpan N` / `ColSpan N` for cells whose `+   +-----+`-style
   separators omit the column-divider `+` to span the cell into the next
   row, and similarly for column merges. Our `grid_column_ranges` helper
   currently skips empty `+`-to-`+` ranges (which represent spans), so
   spanned cells produce wrong column ordering. Heavy — needs a layout
   model that assigns cells to specific columns and tracks RowSpan/ColSpan
   counts per cell.

2. **#91 sibling-list-marker recursion inside BQ-content (1)** ---
   `-  > -` siblings + indented `   > -` continuation: parser merges item
   content into one BlockQuote/Para instead of splitting into 3 outer items
   each containing a BlockQuote with an inner BulletList. Wants recursion
   of list-marker detection inside `>`-prefixed list-item content. Bounded
   continuation-policy fix.

Suggested first session: **#1 (Tables — #71)** is the single remaining
"feature" gap (rowspan/colspan in grid tables); **#2 (#91)** is the
remaining bounded continuation-policy fix.

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

- **Date**: 2026-05-04 (Lazy ListItem continuation in BQ-list: no-`>`
  plain-text line folds into the deepest open list item's buffer
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

## Prior sessions

Older session logs were pruned to keep the recap scannable. Use `git log` on
`crates/panache-parser/tests/pandoc/allowlist.txt` and the projector to
trace which case unlocked when. Cross-session lessons that still apply have
been folded into the global "Don't redo" section above.

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
