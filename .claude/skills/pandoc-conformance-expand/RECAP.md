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

- **Date**: 2026-05-03 (Same-line BLOCK_QUOTE inside LIST_ITEM ungated for Pandoc)
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
