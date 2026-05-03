# Pandoc-conformance recap

Rolling handoff between sessions. Keep terse. Read `report.txt` for the full
state; this file is judgment calls only.

## Suggested next targets

Ranked by likely shared root cause and leverage. Numbers in parentheses are the
count of currently-failing imports remaining under that bucket in the latest
`report.txt`.

1. **Citations proper (\~14 Unsupported "CITATION", but only #38 is currently
   failing)** --- embedded inline cites need full pandoc shape:
   `Cite [Citation { citationId, citationPrefix,    citationSuffix, citationMode = AuthorInText | NormalCitation |    SuppressAuthor, citationNoteNum, citationHash }] [Inline]`.
   Most citation-bearing cases pass via Example-list carve-out; #38 is the
   single remaining real-citation showcase. Smaller leverage than the \~14
   occurrence count would suggest (one case, not many).
2. **Tables --- remaining (\~3)** --- Simple/Multiline/Headerless basics landed
   plus multiline inline-formatting, short-header, and
   indented-pipe-table-with-caption-attributes (+12 cases). What remains:
   - **#68/#70/#71** (grid_table) --- grid cells need block-level reparse (e.g.
     `B` → CodeBlock, multi-line cells → SoftBreak/LineBreak, complex span
     tables); requires running panache's block parser on each cell's content.
     #71 also has rowspan/colspan layout. Heavy. The new
     `parse_cell_text_inlines` helper proves the inline-reparse pattern; an
     analogous block-reparse helper using `panache_parser::parse` and walking
     children for blocks would unlock these.
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
5. **HTML blocks / fenced divs with raw HTML adjacency (\~3)** ---
   `writer_html_blocks`, `html_block` cases with adjacent HTML. Pandoc splits
   each `<tag>` line into its own `RawBlock`; we coalesce them into one block.
   Parser-shape gap: HTML_BLOCK currently spans contiguous HTML lines; would
   need to split on tag boundaries. `<div class="container">...</div>` is a
   related parser gap: pandoc parses as `Div ( "" , [ "container" ] , [ ] )`
   with markdown-parsed content; we wrap as a single RawBlock.
6. **Block-level cases where parser splits paragraphs around inline HTML
   comments (#79 ignore_directives)** --- pandoc keeps the comment as
   `RawInline (Format "html") "<!-- ... -->"` inside the surrounding paragraph
   (or as the trailing inline of a Para); we split into separate RawBlock and
   shorter Paras. Parser-shape gap in HTML_BLOCK detection: a comment that abuts
   a paragraph boundary should not always start a new block.
7. **Misc remaining**:
   - **Same-line BLOCK_QUOTE marker (#93, #108)** --- the
     `text_to_buffer.starts_with('>')` branch in
     `finish_list_item_with_optional_nested` is still
     CommonMark-only. Blocked on `BlockQuote::depth()`
     double-counting when a nested BQ is rendered to a temp
     buffer (see latest session's "Don't redo"). Needs a
     formatter-side fix that subtracts container-context depth
     before flipping the gate.
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

- **Date**: 2026-05-03 (List-item indent: include parent-LIST leading WHITESPACE in content offset)
- **Pass before → after**: 173 → 174 / 187 (+1 import: #44). One narrow
  projector fix to `list_item_content_offset` in
  `crates/panache-parser/tests/pandoc/native_projector.rs`. When a LIST is
  the direct child of an outer container (e.g. a DEFINITION body where
  the `- item` line is indented to the def-content column), the per-item
  leading indent lives on the parent LIST as a WHITESPACE token preceding
  each LIST_ITEM rather than inside the item's own children. The function
  was only walking the item's tokens, so for #44's
  `:   Definition 2 ... -  Bullet ... \`\`\`code\`\`\`` shape it returned
  `1 + 2 = 3` (marker + own ws), missing the 4-space def-body indent on
  the parent LIST. The downstream
  `indented_code_block_with_extra_strip(&child, item_indent)` call then
  stripped only 3 of the 7 leading spaces from each fenced-code body
  line, leaving `"    code"` instead of `"code"`. New helper
  `parent_list_leading_ws(item)` reads the WHITESPACE token immediately
  preceding `item` on its parent (`prev_sibling_or_token`); the offset
  is added to all four return paths in `list_item_content_offset`.
  Existing nested-list cases are unaffected because their leading
  WHITESPACE lives inside the LIST_ITEM (already handled), and the
  parent-LIST in those cases has no preceding WHITESPACE token to add.
  CommonMark allowlist green; pandoc allowlist green; full parser-crate
  suite green; full workspace tests green; clippy + fmt clean.
- **What landed**:
  - **Projector: include parent-LIST leading WHITESPACE in
    `list_item_content_offset`
    (`crates/panache-parser/tests/pandoc/native_projector.rs`)** ---
    new `parent_list_leading_ws(item)` helper returns the char-count of
    the WHITESPACE token immediately preceding `item` on its parent
    (or 0 if the prev sibling is a node, a non-WHITESPACE token, or
    `None`). `list_item_content_offset` now hoists `parent_ws =
    parent_list_leading_ws(item)` once at the top and adds it to every
    return path (early returns on WHITESPACE-after-marker, on
    non-marker token, on inline-node-after-marker, and the
    fall-through). Doc-comment expanded to describe the
    parent-container-indent case (LIST inside DEFINITION body, etc).
- **Cases unlocked** (+1, allowlisted under `# imported`):
  - 44 (definition_list_nesting)
- **Files changed (classified)**:
  - **projector**:
    `crates/panache-parser/tests/pandoc/native_projector.rs`
    (`list_item_content_offset` hoists parent-WS into all four return
    paths; new `parent_list_leading_ws` helper; doc-comment update)
  - **allowlist**:
    `crates/panache-parser/tests/pandoc/allowlist.txt` (+1: 44 inserted
    between 42 and 45 under `# imported`)
- **Don't redo**:
  - `parent_list_leading_ws` only inspects a single immediately-prior
    sibling. The CST shape for `LIST_ITEM, BLANK_LINE, WHITESPACE,
    LIST_ITEM` puts WHITESPACE directly before the second item (so it
    is found). The shape for `LIST_ITEM, WHITESPACE, BLANK_LINE,
    LIST_ITEM` would not hit this path, but that ordering is not what
    the parser emits for indented continuation lists. Don't iterate
    backward across multiple siblings — the WS is always immediately
    adjacent in the shapes we see, and a multi-sibling walk would
    risk double-counting blank-line whitespace.
  - The fix is added to *every* return path in
    `list_item_content_offset`, not just the fall-through. The
    function exits early on the first content-delimiter token after
    the marker (the most common path), so missing the early returns
    means the fix doesn't fire — verified by an initial commit that
    only added it to the fall-through. Don't refactor to add it once
    at the bottom; the early returns are load-bearing.
  - The helper is intentionally narrow: it returns 0 on `None`, on a
    node sibling, and on a non-WHITESPACE token. The CST sometimes has
    BLANK_LINE *node* between items rather than a WHITESPACE token, so
    matching only WHITESPACE tokens keeps the offset semantically
    correct (BLANK_LINE doesn't represent content-line indent).
  - The pinned parser fixture
    `crates/panache-parser/tests/fixtures/cases/definition_list_nesting/`
    already exercises this exact CST shape (LIST inside DEFINITION
    body, with leading WHITESPACE on the LIST), so this projector-only
    fix did not need a new parser golden case.

## Earlier session (2026-05-03, Pipe-table caption attributes + indented-separator alignment)

## Prior sessions

Older session logs were pruned to keep the recap scannable. Use `git log` on
`crates/panache-parser/tests/pandoc/allowlist.txt` and the projector to
trace which case unlocked when. Cross-session lessons that still apply have
been folded into the global "Don't redo" section above.
