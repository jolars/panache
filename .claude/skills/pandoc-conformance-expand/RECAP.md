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
4. **Definition list nesting (\~2 --- cases #43, #44)** ---
   `definition_list_nesting`, `definition_list`.
   Per-item loose/tight detection landed (#179); the bare-leading-list-marker
   gate was relaxed (#45 unlocked, 2026-05-03); #44 still has a
   nested-list-inside-definition offset propagation gap (the `LIST` carries a
   leading WHITESPACE sibling that `list_item_content_offset` doesn't see);
   #43 has parser-shape issues where nested bullets inside definitions aren't
   parsed as `BulletList` in *some* contexts (the bare-leading-list path now
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

- **Date**: 2026-05-03 (Pipe-table caption attributes + indented-separator alignment)
- **Pass before → after**: 172 → 173 / 187 (+1 import: #171). Two narrow
  projector fixes flipped `tables_in_divs`. First, `pipe_separator_aligns`
  was reading the `TABLE_SEPARATOR`'s leading whitespace as a phantom
  column — when a pipe table sits inside a fenced div the separator line
  is indented (e.g. `"  | --- | --- | --- |\n"`), and `trim_start_matches('|')`
  doesn't peel spaces, so the eventual `split('|')` produced an extra
  segment for the indent and `cols` was inflated by one. Adding a
  `raw.trim()` before the pipe-strip closes that gap. Second, the
  `+caption_attributes` extension was unimplemented in the projector: a
  trailing `{#tbl:foo-1}` on a `: My Caption ...` line was emitted as
  literal Str text in the caption with the Table id staying empty. New
  `extract_caption_attrs` walks back through the caption inlines for a
  balanced trailing `{...}` (Str/Space sequence only), parses it via
  the existing `parse_attr_block`, drops the brace span (and any
  preceding Space), and returns the resulting `Attr` for the Table's
  outer attribute slot. `TableData` gained an `attr` field, `write_table`
  emits it instead of the hardcoded empty triple, and all four table
  builders (`pipe_table` / `simple_table` / `grid_table` /
  `multiline_table`) feed their captions through the helper. CommonMark
  allowlist green; pandoc allowlist green; full parser-crate suite
  green; clippy + fmt clean.
- **What landed**:
  - **Projector: trim before pipe-strip in
    `pipe_separator_aligns`
    (`crates/panache-parser/tests/pandoc/native_projector.rs`)** ---
    swapped `trim_matches('\n' | '\r')` for plain `trim()` so that an
    indented separator (table inside a fenced div, etc.) doesn't
    contribute a leading-whitespace phantom column. The trailing
    `trim_end_matches('|')` already coped with trailing whitespace
    after the strip.
  - **Projector: caption-attribute extraction
    (same file)** --- new `extract_caption_attrs(inlines) ->
    (Attr, Vec<Inline>)` helper. Right-walks the caption inline list
    for the closing brace-bearing Str, then walks back across only
    Str/Space inlines to find an opening `{`-bearing Str, concatenates
    the span (Space → ' ') into a flat `{...}` text, peels the outer
    braces, and parses the inner via `parse_attr_block`. Returns
    `Attr::default()` and the original inlines unchanged when the
    pattern doesn't match (incl. when a non-text inline like Emph sits
    inside the candidate span). On match it truncates the inlines at
    `start_idx` and pops a trailing Space, so the caption text
    matches pandoc's "strip the attr span and one space" behavior.
  - **Projector: `TableData` carries an `Attr`
    (same file)** --- new `attr: Attr` field. All four builders
    (`pipe_table`, `simple_table`, `grid_table`, `multiline_table`)
    now feed `caption_inlines` through `extract_caption_attrs` and
    populate `attr` from the result. `write_table` writes the parsed
    attr via `write_attr` instead of the previous `( "" , [ ] , [ ] )`.
- **Cases unlocked** (+1, allowlisted under `# imported`):
  - 171 (tables_in_divs)
- **Files changed (classified)**:
  - **projector**:
    `crates/panache-parser/tests/pandoc/native_projector.rs`
    (`pipe_separator_aligns` trim; `extract_caption_attrs` new helper;
    `TableData` gains `attr`; all four table builders updated;
    `write_table` writes attr)
  - **allowlist**:
    `crates/panache-parser/tests/pandoc/allowlist.txt` (+1: 171
    inserted between 170 and 172 under `# imported`)
- **Don't redo**:
  - `extract_caption_attrs` short-circuits to
    `(Attr::default(), inlines)` when a non-Str/Space inline sits
    between the candidate `{` and `}`. Pandoc's caption_attributes
    extension only applies when the brace span is plain text — an
    Emph/Strong/Link inside the candidate region means the trailing
    "attribute" is literal caption content, not a real attribute. Keep
    that guard; relaxing it would over-eagerly swallow markup-bearing
    captions.
  - The Space drop after truncation is intentional — pandoc rule is
    "strip one separating space before the brace span". Don't loop and
    drop multiple — `[Str "x", Space, Space, Str "{#id}"]` is unusual
    in practice and pandoc keeps any extra spaces in the caption text.
  - `pipe_separator_aligns` now uses `trim()` (whitespace + newlines)
    rather than the prior `trim_matches('\n' | '\r')`. This is safe
    because every other path that needed trailing whitespace gone was
    already calling `trim_end_matches('|')` after, but a pipe-bearing
    separator with trailing spaces (`"| --- | --- |    "`) was already
    losing the spaces post-`|`-strip due to nothing reading them.
    Don't revert to the narrower trim — it just reintroduces the
    leading-whitespace phantom-column bug.
  - All four table builders share the helper. Currently-passing
    captions never end in `{...}` so the helper is a no-op for them.
    Don't try to special-case "only call this for pipe_table" — keeping
    one path means future caption-attribute support is uniform.

## Earlier session (2026-05-03, Same-line nested LIST marker: parser gate + formatter inline-emit)

- **Pass before → after**: 171 → 172 / 187 (+1 import: #111). The
  Pandoc-dialect gate on the same-line nested LIST emission path
  (`finish_list_item_with_optional_nested` in
  `crates/panache-parser/src/parser/blocks/lists.rs`) was flipped, so
  `- - foo` and `1. - 2. foo` now produce the pandoc-native nested
  shape under `Flavor::Pandoc` (matching the existing CommonMark
  output). The formatter side gained a new arm in
  `format_list_item` for the case where the LIST_ITEM has no
  PLAIN/PARAGRAPH content node and a non-empty leading nested LIST —
  it emits the outer marker without a newline, then formats the
  nested LIST at indent=0 (stripping the leading `\n` that
  `format_list` otherwise injects at top-level). The companion gate
  for same-line BLOCK_QUOTE (`text_to_buffer.starts_with('>')`) is
  intentionally left CommonMark-only — that path needs additional
  formatter work because of `BlockQuote::depth()` double-counting
  ancestors when a nested BQ is rendered to a temp buffer (see
  `Don't redo` for details). CommonMark allowlist green; pandoc
  allowlist green; full parser-crate suite green; full workspace
  tests green; clippy + fmt clean.
- **What landed**:
  - **Parser-shape: ungate same-line nested LIST under Pandoc
    (`crates/panache-parser/src/parser/blocks/lists.rs`)** ---
    removed `dialect_allows_nested &&` from the
    `try_parse_list_marker(&text_to_buffer, config)` branch in
    `finish_list_item_with_optional_nested`. The gate variable is
    still computed because the BLOCK_QUOTE same-line case below
    still uses it. Comment expanded to explain that the LIST path
    is dialect-agnostic but the BLOCK_QUOTE path is still gated
    pending depth-aware blockquote rendering.
  - **Formatter: inline-emit leading nested LIST inside LIST_ITEM
    (`crates/panache-formatter/src/formatter/lists.rs`)** ---
    `format_list_item` previously emitted nothing when there was no
    `PLAIN/PARAGRAPH` content node (so the outer marker was
    silently dropped, then the children loop emitted the nested
    LIST at hanging-indent on a new line). New branch added before
    the `find_content_node` call: if `first_non_blank_child` is a
    non-empty LIST (and `find_content_node` is None), emit the
    outer marker + spaces (no newline), then `format_node_sync` the
    nested LIST at indent=0. `format_list` injects a leading `\n`
    when `indent==0 && !output.ends_with("\n\n")`; the new arm
    detects-and-strips that `\n` post-hoc by saving
    `self.output.len()` before the call and removing the byte at
    that index if it's `\n`. Trailing children (blank lines,
    further nested blocks) are then emitted at the outer's hanging
    indent.
  - **Helper sig change: `first_non_blank_child` borrowed not moved
    (`crates/panache-formatter/src/formatter/lists.rs`)** --- the
    leading-heading branch above the new arm previously moved
    `first_non_blank_child`, blocking the new arm from re-borrowing.
    Updated to `if let Some(leading_heading) =
    first_non_blank_child.as_ref()` and the two existing `==
    leading_heading` comparisons inside that arm reborrowed.
- **Cases unlocked** (+1, allowlisted under `# imported`):
  - 111 (list_nested_same_line_marker_pandoc)
- **Files changed (classified)**:
  - **parser-shape**:
    `crates/panache-parser/src/parser/blocks/lists.rs`
    (gate flip on the `try_parse_list_marker` branch of
    `finish_list_item_with_optional_nested`)
  - **formatter (companion)**:
    `crates/panache-formatter/src/formatter/lists.rs`
    (new leading-nested-LIST arm in `format_list_item`; borrow
    cleanup in the leading-heading arm)
  - **parser snapshot updated**:
    `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_list_nested_same_line_marker_pandoc.snap`
    --- previously pinned the buggy `LIST > LIST_ITEM > LIST_MARKER
    + WS + PLAIN [TEXT "- foo"]` shape (and the legacy
    `1. - 2. foo` flat plain). Now pins the pandoc-native nested
    `LIST > LIST_ITEM > LIST_MARKER + WS + LIST > LIST_ITEM > ...`
    shape, matching the existing CommonMark snapshot byte-for-byte.
  - **formatter golden case (new)**:
    `tests/fixtures/cases/list_nested_same_line_marker/`
    (input: `- - foo\n1. - 2. foo\n`; expected: same with a
    blank line inserted between the two top-level lists since
    they have different marker types). Registered in
    `tests/golden_cases.rs` between
    `list_nested_roman_idempotency_136` and `line_ending_crlf`.
  - **allowlist**:
    `crates/panache-parser/tests/pandoc/allowlist.txt` (+1: 111
    inserted between 110 and 112 under `# imported`)
- **Don't redo**:
  - The same-line BLOCK_QUOTE gate (`text_to_buffer.starts_with('>')`
    branch) is intentionally still CommonMark-only. Flipping it
    requires fixing `BlockQuote::depth()` rendering: when the
    outer BLOCK_QUOTE renders a child LIST to a temp buffer and
    inside the LIST the formatter tries to render a *nested*
    BLOCK_QUOTE, the inner BQ's depth() walks AST ancestors
    (which include the outer BQ) and yields 2 — so it emits
    `> > content`. Then the outer BQ's
    `append_blockquote_prefixed_list_output` doesn't strip those,
    only base-indents lines that already start with `> `. Result:
    triple `>` prefix where pandoc emits double. Verified
    under CommonMark: `> 1. > Blockquote\ncontinued.` formats to
    `>    > > Blockquote continued here.` (4-space gap, double
    inner `>`). Until `format_blockquote` learns to subtract its
    container-context depth (e.g., a `containing_blockquote_depth`
    parameter or output-prefix counting via the existing
    `BlockquoteContext`), don't flip the BLOCK_QUOTE gate. Cases
    #93 and #108 remain blocked on this.
  - The `format_list` leading-`\n` strip is intentionally
    post-hoc, not a flag on `format_list`. Adding a flag would
    propagate through the recursive call chain and is a larger
    refactor; the post-hoc strip is a one-byte fixup that fires
    only when the output we just appended was a leading newline
    (the most common case at indent=0). Don't refactor to a
    plumbed flag without a clear second use.
  - The new leading-nested-LIST arm sits *before* the
    `find_content_node` call so it overrides the wrap path. Don't
    move it after — the wrap path's `lines.is_empty()` branch
    silently emits no marker, which is exactly the bug the new
    arm fixes.
  - The `is_empty_nested_list` short-circuit is preserved: when
    the leading nested LIST is empty (e.g., `- *`), the existing
    `has_only_empty_nested_list` path handles it. The new arm
    explicitly excludes that case via
    `!Self::is_empty_nested_list(leading_list)`.
  - `1. b. WHERE firstName LIKE...` no longer round-trips as
    `- b. WHERE...` — this is **correct**: with the gate flipped,
    `b.` is now recognized as an alphabetic ordered marker (Pandoc
    `fancy_lists` default is on), so the inner LIST shape applies.
    The formatter golden test
    `escaped_double_underscore_in_list_item_stays_idempotent` was
    already exercising this and now passes via the new arm. Don't
    "revert" toward the old plain-text shape — it was a gate
    artifact, not a deliberate behavior.

## Earlier session (2026-05-03, Simple-table short-header zero-width cells)

- **Pass before → after**: 170 → 171 / 187 (+1 import: #94). One projector
  fix: `simple_table_row_cells` was filtering out zero-width `TABLE_CELL`
  nodes as "parser artifacts", but those nodes are actually *meaningful* —
  they represent positionally-empty columns when header words land in only
  some of the dash-defined columns (case 0094 had a 6-column simple table
  with header words populating only columns 2–4, leaving cols 1, 5, 6
  empty). Dropping them collapsed the row to 3 cells, which `cells_to_
  plain_blocks` then padded to 6 by appending empty cells *at the end* —
  putting the empties on the wrong side. Keeping the zero-width cells
  preserves the parser's correct positional ordering. CommonMark
  allowlist green; full parser-crate suite green; full workspace tests
  green; clippy + fmt clean.
- **What landed**:
  - **Projector: keep zero-width simple-table cells
    (`crates/panache-parser/tests/pandoc/native_projector.rs`)** ---
    `simple_table_row_cells` removed the `cell.text_range().is_empty()`
    skip and now maps every `TABLE_CELL` child to its inlines (which
    coalesce to an empty `Vec` for zero-width cells, projected to
    `Cell ... []` by `cells_to_plain_blocks`). The dropped explanatory
    comment is replaced with one explaining *why* zero-width cells are
    preserved.
- **Cases unlocked** (+1, allowlisted under `# imported`):
  - 94 (issue_224_simple_table_short_header_losslessness)
- **Files changed (classified)**:
  - **projector**:
    `crates/panache-parser/tests/pandoc/native_projector.rs`
    (`simple_table_row_cells` body)
  - **allowlist**:
    `crates/panache-parser/tests/pandoc/allowlist.txt` (+1: 94 inserted
    between 92 and 95 under `# imported`)
- **Don't redo**:
  - The parser CST is correct here — zero-width `TABLE_CELL` nodes are
    by design when a header/data row leaves a dash-defined column
    visually empty. Don't try to "fix" the parser to omit them; the
    projector consuming them as empty cells is the right contract.
  - `simple_table_aligns` keeps its own `cell.text_range().is_empty()`
    skip (line ~1556) — that's correct because alignment is derived
    from cells *with content* relative to dash boundaries; an empty
    cell contributes no alignment signal. Don't unify the two filters.
  - `cells_to_plain_blocks` still pads at the end if `cells.len() <
    cols`. That's the right fallback for the case where the parser
    really did produce too few cells (different shape gap). The padded
    empties only land at the end *when the leading positions were
    already filled*, so the new behavior here doesn't conflict with
    the existing fallback.

## Earlier session (2026-05-03, DefinitionList-inside-footnote: term detection + blank-line continuation)

- **Pass before → after**: 168 → 170 / 187 (+2 imports: #66, #67).
  Two parser-shape fixes plus one formatter companion. The footnote body's
  first content line now opens as a `TERM` of a `DEFINITION_LIST` when
  upcoming lines (after blanks, with the footnote's 4-col body indent
  stripped) form a `:`/`~` definition marker — matching pandoc-native's
  treatment of `[^1]: Term\n\n    :   Def`. Separately, the blank-line
  continuation policy now strips outer content-container indent when
  re-detecting whether the next non-blank line is a definition marker, so
  `    \n` blanks between two `:` lines inside a footnote body keep the
  `DefinitionList`/`DefinitionItem` open and let subsequent `:` lines
  become sibling definitions inside the same item (instead of three
  separate one-definition lists). The formatter gained a `DEFINITION_LIST`
  first-child arm for `FOOTNOTE_DEFINITION` so the new term stays on the
  same line as `[^id]:` (matching the prior `[^1]: Footnote text` output).
  CommonMark allowlist green; full parser-crate suite green; full workspace
  tests green; clippy + fmt clean.
- **What landed**:
  - **Parser-shape: footnote-first-line term detection
    (`crates/panache-parser/src/parser/core.rs`)** ---
    `handle_footnote_open_effect` previously always called
    `start_paragraph_if_needed` + `append_paragraph_line` for the
    same-line content. Added a lookahead via the new free helper
    `footnote_first_line_term_lookahead(lines, pos, content_col,
    table_captions_enabled)` that walks forward from `pos+1`, skips
    blank lines, and on the first non-blank line strips
    `content_col=4` cols and runs
    `definition_lists::try_parse_definition_marker`. Returns
    `Some(blank_count)` on success. When set, the effect handler opens
    `DEFINITION_LIST` + `DEFINITION_ITEM`, calls `emit_term`, and
    eagerly emits the consumed blank lines as `BLANK_LINE` nodes
    *inside* the `DEFINITION_ITEM` (mirroring
    `DefinitionPrepared::Term`). The lookahead also reuses the
    existing `is_caption_followed_by_table` gate so a `:` table
    caption inside a footnote doesn't get mistaken for a definition.
  - **Parser-shape: continuation policy strips content-container indent
    when re-detecting `:` markers
    (`crates/panache-parser/src/parser/utils/continuation.rs`)** ---
    `compute_levels_to_keep` precomputes
    `next_is_definition_marker` from the unstripped `next_inner`, so a
    `    :   Def` line inside a footnote (4-col indent) fails the 0-3
    space-marker test and the parent `DefinitionList`/`DefinitionItem`
    closes across blank lines. Added a closure
    `stripped_is_definition_marker(content_indent_so_far)` that strips
    that many cols off `next_inner` and re-tests
    `try_parse_definition_marker`. Wired into the
    `Container::DefinitionItem` and `Container::DefinitionList`
    arms so `content_indent_so_far` (which the loop already
    accumulates as it walks the FootnoteDefinition container) keeps
    those containers open across `    \n` blanks. The
    `Container::Definition` arm is unchanged on purpose --- the
    Definition itself *should* close at the `:` boundary, since each
    `:` opens a new sibling Definition.
  - **Formatter: DEFINITION_LIST as FOOTNOTE_DEFINITION first child
    (`crates/panache-formatter/src/formatter/core.rs`)** --- the
    `FOOTNOTE_DEFINITION` arm has a special "first child can join the
    marker line" branch but only handled `PARAGRAPH`. Added a parallel
    `DEFINITION_LIST` arm: emit a single space, then call
    `format_node_sync(child, child_indent)`. This works because the
    `TERM` formatter emits no leading indent, so the first term sits
    flush against `[^id]: `; subsequent `DEFINITION` children carry
    their own `child_indent=4` indentation via the existing `DEFINITION`
    formatter arm. The `is_compact` logic in `DEFINITION_ITEM` already
    handles the `\n\n` separator between `TERM` and the first
    `DEFINITION` (loose, per the BLANK_LINE between them).
- **Cases unlocked** (+2, allowlisted under `# imported`):
  - 66 (footnote_definition_list)
  - 67 (footnote_def_paragraph)
- **Files changed (classified)**:
  - **parser-shape**:
    `crates/panache-parser/src/parser/core.rs`
    (handle_footnote_open_effect refactor + new
    `footnote_first_line_term_lookahead` helper at module bottom),
    `crates/panache-parser/src/parser/utils/continuation.rs`
    (`stripped_is_definition_marker` closure wired into
    `DefinitionItem`/`DefinitionList` arms)
  - **formatter (companion)**:
    `crates/panache-formatter/src/formatter/core.rs`
    (`DEFINITION_LIST` arm in the `FOOTNOTE_DEFINITION` first-child
    branch)
  - **parser snapshots updated**:
    `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_footnote_def_paragraph.snap`
    (was: PARAGRAPH "Footnote text" + separate DEFINITION_LIST without
    TERM. Now: single DEFINITION_LIST with proper TERM + DEFINITION
    inside DEFINITION_ITEM),
    `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_footnote_definition_list.snap`
    (was: three separate DEFINITION_LIST nodes for the three `:`
    lines, all without TERM. Now: one DEFINITION_LIST > one
    DEFINITION_ITEM > TERM + three sibling DEFINITION children with
    BLANK_LINE separators. Per `.claude/rules/parser.md`, fixed toward
    pandoc-native rather than preserving the legacy bug.)
  - **formatter golden expected unchanged**:
    `tests/fixtures/cases/footnote_def_paragraph/expected.md` and
    `tests/fixtures/cases/footnote_definition_list/expected.md`
    --- the user-visible formatted output is byte-identical before and
    after the change (verified by `cargo test --test golden_cases
    footnote`). The companion formatter arm exists specifically to
    preserve this output under the new CST shape.
  - **allowlist**:
    `crates/panache-parser/tests/pandoc/allowlist.txt` (+2: 66 and 67
    inserted between 65 and 69 under `# imported`)
- **Don't redo**:
  - `footnote_first_line_term_lookahead` is intentionally limited to the
    first non-blank line after the marker. If that line isn't a
    definition marker, we fall through to the existing paragraph path.
    Don't extend it to walk past intermediate non-marker non-blank
    lines --- pandoc only treats the first content line as a term when
    the *immediately* following content (after blanks) is a `:`/`~`
    marker.
  - The `stripped_is_definition_marker` closure walks
    `content_indent_so_far` (the *outer* container's content indent), not
    the marker's own column. Don't conflate with `raw_indent_cols`
    --- raw_indent_cols includes the marker line's leading spaces, which
    is not what we want to strip.
  - The `Container::Definition` arm is intentionally NOT updated: a `:`
    line should close the previous `Definition` and start a new one as
    a sibling. Keeping the previous Definition open would produce a
    nested-definition shape that pandoc never emits.
  - The formatter `DEFINITION_LIST` first-child branch passes
    `child_indent` (=4) to `format_node_sync`. The TERM emits no leading
    indent (so it sits flush against `[^id]: `); the DEFINITION emits
    `indent` cols of leading spaces before its `:` marker, then computes
    `def_indent = indent + 4` for its *body content* lines (the body
    offset, not the marker indent). With `indent=4` we get the desired
    `    :   Definition` shape. Don't read `def_indent = indent + 4` as
    "DEFINITION over-indents its marker" — it's only the body offset.

## Earlier session (2026-05-03, definition with bare leading list marker)

- **Pass before → after**: 167 → 168 / 187 (+1 import: #45). One
  parser-shape fix to relax the `should_start_list_from_first_line`
  guard inside the definition-content emission path so a definition
  whose first content line is a bare `- X` (list marker) opens a
  `BulletList` even when the next line is blank or EOF. The previous
  guard only allowed list-emission when the next line was indented at
  `content_col`, which silently dropped pandoc-shape for the trailing
  single-item case. Pandoc's behavior is uniform: `: - X` always opens
  a `BulletList`, regardless of what (or nothing) follows. The
  formatter already handled the `DEFINITION > LIST` shape correctly
  via the existing list child arm; only the formatter golden fixture
  for `definition_list_pandoc_loose_compact` needed an updated
  expected output (an extra blank line before `:   - List` matches
  pandoc's `pandoc -t markdown` round-trip and now lands as the loose
  formatting that the new shape implies). CommonMark allowlist green;
  full parser-crate suite green; full workspace tests green; clippy +
  fmt clean.
- **What landed**:
  - **Parser-shape: relax bare-leading-list-marker gate inside
    definitions (`crates/panache-parser/src/parser/core.rs`)** ---
    the `should_start_list_from_first_line` closure inside the
    `DefinitionPrepared::Definition` arm previously returned `false`
    when the next line was blank/empty AND defaulted to `false` on
    EOF (`unwrap_or(false)`). Both flipped to `true`. The other
    branch (next line non-blank but with insufficient indent) is
    unchanged --- pandoc *does* treat unindented next-line content as
    lazy continuation of the list's `Plain`, but our parser's
    list-continuation path doesn't yet handle that exact shape, so
    leaving the existing `next_indent_cols >= content_col` check in
    place avoids regressing that case. Verified against pandoc:
    - `Term\n: - List\n` (EOF after list) → `BulletList [Plain
      "List"]` (was `Plain "- List"`).
    - `Term\n: - List\n\nNext\n` (blank line, then unrelated para) →
      `BulletList [Plain "List"]` then top-level `Para "Next"` (was
      `Plain "- List"` then `Para "Next"`).
    - `:   Here comes a list...\n    - A\n    - B\n` (the existing
      `definition_list_plain_does_not_start_list_without_blank_line`
      test) is unaffected --- the first content line starts with
      "Here", not a list marker, so the list-emission path doesn't
      trigger at all.
- **Cases unlocked** (+1, allowlisted under `# imported`):
  - 45 (definition_list_pandoc_loose_compact)
- **Files changed (classified)**:
  - **parser-shape**:
    `crates/panache-parser/src/parser/core.rs`
    (`should_start_list_from_first_line` closure: blank-next-line
    branch flipped `false` → `true`; `unwrap_or(false)` flipped to
    `unwrap_or(true)` for the EOF case)
  - **parser fixture (new)**:
    `crates/panache-parser/tests/fixtures/cases/definition_list_pandoc_bare_leading_list/input.md`
    plus snapshot
    `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_definition_list_pandoc_bare_leading_list.snap`
    pinning the new behavior. Registered in
    `crates/panache-parser/tests/golden_parser_cases.rs` between
    `definition_list_nesting` and `definition_list_pandoc_loose_compact`.
  - **parser snapshot updated**:
    `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_definition_list_pandoc_loose_compact.snap`
    --- previously pinned the buggy `PLAIN [TEXT "- List"]` shape for
    the trailing definition; now pins the pandoc-native
    `LIST [LIST_ITEM [LIST_MARKER, WHITESPACE, PLAIN [TEXT "List"]]]`
    shape. Per `.claude/rules/parser.md`, fixed toward pandoc-native
    rather than preserving the legacy bug.
  - **formatter golden expected updated**:
    `tests/fixtures/cases/definition_list_pandoc_loose_compact/expected.md`
    --- the trailing `Term\n:   - List\n` became
    `Term\n\n:   - List\n` (extra blank line before the loose
    definition). Verified against `pandoc -f markdown -t markdown`:
    pandoc round-trips `Term\n: - List\n` to `Term\n\n:   - List\n`,
    confirming the new format output matches pandoc.
  - **allowlist**:
    `crates/panache-parser/tests/pandoc/allowlist.txt` (+1: 45
    inserted between 42 and 46 under `# imported`)
- **Don't redo**:
  - The relaxation only flips the **blank-line** and **EOF** cases.
    The `next_indent_cols < content_col` (non-blank-but-unindented)
    case is intentionally unchanged. Pandoc *does* treat that as a
    list with lazy continuation (e.g., `Term\n: - List\nNo indent\n`
    → `BulletList` with `Plain [Str "List", SoftBreak, Str "No",
    Space, Str "indent"]`), but our list-continuation machinery
    doesn't currently feed unindented post-`: -` text into the inner
    list's `Plain`. Flipping that case would need parallel work in
    the list-continuation path; left for a future session to avoid
    scope creep.
  - The formatter's `format_list_item` LIST-arm at
    `crates/panache-formatter/src/formatter/lists.rs:836` already
    handles the `DEFINITION > LIST` shape. No formatter code change
    was needed --- only the golden expected output. If you ever
    revisit list-inside-definition formatting, the existing path
    routes through `format_node_sync(&child, list_indent.hanging_
    indent(total_indent))`. That works because LIST is not a direct
    "first content child needs a marker" container --- the LIST's
    own children (LIST_ITEMs) carry their own markers.
  - The companion formatter blank-line is **not** added by the
    parser fix --- it's an artifact of the existing loose-list
    detection that triggers on `DEFINITION_ITEM` containing a
    `BLANK_LINE` between TERM and DEFINITION (added when a
    `DEFINITION > LIST` shape is present). This loose-detection
    behavior was already in place; the new shape just exposes it
    for the bare-leading-list case. Don't try to "tighten" the
    output back to no-blank-line --- that would diverge from
    pandoc.

## Earlier session (2026-05-01, ATX heading inside list-item / definition buffer)

- **Pass before → after**: 166 → 167 / 187 (+1 import: #128). One
  parser-shape fix that detects a leading ATX-heading line in buffered
  list-item / definition content and emits HEADING + PLAIN instead of a
  single PLAIN spanning both lines. Required a small companion formatter
  change so the new HEADING-then-PLAIN shape inside `DEFINITION` renders
  with a blank line between heading and continuation (mirrors pandoc's
  `pandoc -t markdown` round-trip and the existing list-item leading-heading
  path). CommonMark allowlist green; full parser-crate suite green; full
  workspace tests green; clippy + fmt clean.
- **What landed**:
  - **Parser-shape: heading-first list-item buffer
    (`crates/panache-parser/src/parser/utils/list_item_buffer.rs`)** ---
    `ListItemBuffer::emit_as_block` previously only detected a leading
    ATX heading when the entire buffer was a single line ending with
    `\n`. Extended with a multi-line branch: if all segments are `Text`
    and the first `\n`-terminated line parses as ATX, emit
    `emit_atx_heading` for the first-line bytes (`text[..first_nl + 1]`
    so the trailing `\n` is included), then emit the rest as
    `PLAIN`/`PARAGRAPH` via `inline_emission::emit_inlines`. The
    all-`Text`-segments guard avoids interfering with the rare
    `BlockquoteMarker`-bearing buffers from blockquote-inside-list
    parsing. The existing single-line ATX/HR fast path is preserved.
  - **Parser-shape: heading-first definition buffer
    (`crates/panache-parser/src/parser/core.rs`)** --- the same
    multi-line ATX detection was duplicated across two definition
    plain-buffer call sites (the close-Definition arm of
    `close_open_node` at the old line ~189 and `emit_buffered_plain_if_needed`
    at the old line ~257). Extracted into a new free helper
    `emit_definition_plain_or_heading(builder, text, config)` at the
    end of the module which: (a) tries the existing single-line ATX
    fast-path, (b) falls back to multi-line "first line is heading,
    rest is plain", (c) defaults to a single PLAIN. Both call sites
    now just call the helper; the close-Definition arm still handles
    its own buffer-clear / pop / finish_node afterward as before.
  - **Formatter: HEADING child inside DEFINITION
    (`crates/panache-formatter/src/formatter/core.rs`)** --- the
    DEFINITION node child match handled `PLAIN` with an embedded ATX
    heading via the legacy `leading_atx_heading_with_remainder`
    helper, but had no arm for an actual `HEADING` child node. Added
    a new `SyntaxKind::HEADING` arm: emit
    `format_heading(n)` followed by `\n`, then if there are *any*
    non-`BLANK_LINE` siblings after this index AND the *immediate*
    next sibling is not already a `BLANK_LINE` node, push another
    `\n`. The "next-is-blank" guard is what keeps formatting
    idempotent across re-parses (after one pass, a `BLANK_LINE` node
    appears between HEADING and PLAIN, and the BLANK_LINE arm
    already emits the separator). Without this guard the second
    pass appends a duplicate blank line. The list-item path
    (`format_list_item` `leading_heading` branch in
    `crates/panache-formatter/src/formatter/lists.rs`) already
    handled HEADING-first list items correctly; only DEFINITION
    needed the new arm.
- **Cases unlocked** (+1, allowlisted under `# imported`):
  - 128 (nested_headings_in_containers)
- **Files changed (classified)**:
  - **parser-shape**:
    `crates/panache-parser/src/parser/utils/list_item_buffer.rs`
    (multi-line leading-heading branch in `emit_as_block`),
    `crates/panache-parser/src/parser/core.rs`
    (new `emit_definition_plain_or_heading` helper; both Definition
    plain-buffer emission sites refactored to call it; removed the
    inline duplicates)
  - **formatter (companion)**:
    `crates/panache-formatter/src/formatter/core.rs`
    (`SyntaxKind::HEADING` arm in the DEFINITION child match)
  - **parser snapshot updated**:
    `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_nested_headings_in_containers.snap`
    --- previously pinned the buggy single-`PLAIN`-spanning-both-lines
    shape for the list-item and definition cases; now pins the
    pandoc-native HEADING-then-PLAIN shape under both containers
    (the BLOCK_QUOTE case in the same fixture was already correct
    and unchanged). Per `.claude/rules/parser.md`, fixed toward
    pandoc-native rather than preserving the legacy bug.
  - **formatter golden expected unchanged**:
    `tests/fixtures/cases/nested_headings_in_containers/expected.md`
    --- the user-visible formatted output is byte-identical before
    and after the change (verified by re-running `cargo test --test
    golden_cases nested_headings_in_containers`). The companion
    formatter arm exists specifically to preserve this output under
    the new CST shape.
  - **allowlist**:
    `crates/panache-parser/tests/pandoc/allowlist.txt` (+1: 128
    inserted between 127 and 129 under `# imported`)
- **Don't redo**:
  - The list-item buffer fix gates on `segments.iter().all(Text)`.
    This guard is load-bearing: if a `BlockquoteMarker` segment is
    present, the buffer's text is *not* the literal source bytes
    (markers were stripped to feed inline parsing) and slicing
    `text[..first_nl + 1]` to feed `emit_atx_heading` would emit
    bytes that don't exist in the source. Don't drop the guard. If
    you ever need to support heading-first inside a
    blockquote-marker-bearing list item, you'd need to walk the
    `segments` directly and emit per-segment, not via the
    flat-text path.
  - The definition helper covers the case where the first line is a
    heading AND the rest is non-empty. The single-line case (rest
    empty) is covered by the existing
    `strip_suffix("\n")`/`!line.contains('\n')` fast path *before*
    falling through to multi-line. Keep both paths; collapsing them
    would either lose the single-line case (which feeds the entire
    text including trailing `\n` to `emit_atx_heading`) or cause a
    double-emit.
  - The formatter HEADING arm's `next_is_blank_line` check inspects
    the *immediate* sibling at index `i + 1`, not a search through
    later siblings. After one format pass the CST has a single
    `BLANK_LINE` node directly after the HEADING; if a future change
    produces multiple BLANK_LINE nodes back-to-back, this check
    still works (only the first is checked, and BLANK_LINE's own
    arm handles the rest). Don't refactor to `find` / `any` — it
    would re-introduce the idempotency bug if a non-blank node
    happens to appear later in the children list (e.g., a CODE_BLOCK
    after a BLANK_LINE that is not adjacent to the HEADING).
  - The legacy `leading_atx_heading_with_remainder` PLAIN-branch is
    now dead code for the heading-first definition path (the parser
    no longer produces PLAIN-with-embedded-heading for that input),
    but it still serves cases where a PLAIN's text *content*
    happens to start with `# ` due to other parsing paths
    (e.g. paragraph reflow or non-standard inputs). Don't delete it
    — it's a defensive fallback.

## Earlier session (2026-05-01, tex inline trailing-space + unresolved reference-link edge whitespace)

- **Pass before → after**: 165 → 166 / 187 (+1 import: #51).
  Two projector-only fixes that combine to unlock #51
  (`double_backslash_math`). Both shipped in
  `crates/panache-parser/tests/pandoc/native_projector.rs`; CST shapes
  are unchanged so no parser fixture / snapshot updates were needed.
  CommonMark allowlist green; full parser-crate suite green; full
  workspace tests green; clippy + fmt clean.
- **What landed**:
  - **Tex inline trailing-space absorption (projector-only)** — Pandoc's
    raw-tex inline reader absorbs trailing horizontal whitespace into a
    `\letters` command (`\foo bar` → `RawInline tex "\\foo "` + `Str
    "bar"`). It does **not** absorb when the command ends in `}` (i.e.
    has brace args: `\frac{a}{b} bar` → `RawInline tex "\\frac{a}{b}"` +
    `Space` + `Str "bar"`) or in a digit/punct (`\foo123` keeps the run).
    The discriminator is the last byte of the command text: ASCII letter
    → absorb, otherwise → don't. New helper
    `emit_latex_command_with_absorb` checks the next sibling element via
    a peekable iterator; if it's a `TEXT` token starting with one or
    more `' '`/`'\t'` bytes, those bytes are appended to the
    `RawInline` content, the `TEXT` token is consumed, and the
    remainder is re-emitted via `push_text`. `inlines_from` and
    `inlines_from_marked` were both converted to peekable iterators
    that route LATEX_COMMAND through the helper. Verified against
    pandoc:
    - `\foo bar` → `RawInline "\\foo "` (absorb)
    - `\LaTeX bar` → `RawInline "\\LaTeX "` (absorb)
    - `\frac{a}{b} bar` → `RawInline "\\frac{a}{b}"` + `Space` (no
      absorb — ends in `}`)
    - `\LaTeX{} bar` → `RawInline "\\LaTeX{}"` + `Space` (no absorb)
    - `\foo  bar` → `RawInline "\\foo  "` (multi-space absorb)
    - `\foo\n bar` → `RawInline "\\foo"` + `SoftBreak` (no absorb across
      newlines — `NEWLINE` token, not `TEXT`)
  - **Unresolved reference-link keep_edges (projector-only)** —
    `render_link_inline` previously trimmed leading/trailing whitespace
    on the LINK_TEXT inlines for both resolved Links *and* unresolved
    Str-fallback paths via shared `coalesce_inlines(inlines_from(n))`.
    Pandoc strips edges on resolved Links but **preserves source
    whitespace** for unresolved references (`[ foo ]` → `Str "[", Space,
    Str "foo", Space, Str "]"`). Renamed the resolved-path local to
    `resolved_text_inlines` and added a separate
    `unresolved_text_inlines = coalesce_inlines_keep_edges(...)` for the
    Str-fallback branch. Verified: `[ foo ]` matches pandoc; `[foo]`
    still coalesces to `Str "[foo]"` via the parent paragraph's
    coalesce pass merging consecutive Strs.
  - **Combined effect on #51** — the input
    `\\[ \int_0^\infty e^{-x^2} dx = \frac{\sqrt{\pi}}{2} \\]` is
    parsed by panache as `ESCAPED_CHAR \\\\` + `LINK` (unresolved,
    `[ ... ]`) + `LINK_TEXT` containing the math fragments. Both fixes
    are needed: the trailing-space absorption gets `\infty ` correct
    inside LINK_TEXT, and keep_edges preserves the leading `Space`
    between `\\[` and `\int`. After parent-level coalesce merges
    consecutive Strs, the leading `\\` + `[` and trailing `\\` + `]`
    pairs combine to `Str "\\["` and `Str "\\]"`, matching pandoc's
    expected shape. The `\\(...\\)` paragraphs (line 11 and 13 of
    input) work the same way — `\\` is parsed as ESCAPED_CHAR, `(` is
    just text, no LINK involvement, so only the trailing-space rule
    matters.
- **Cases unlocked** (+1, allowlisted under `# imported`):
  - 51 (double_backslash_math)
- **Files changed (classified)**:
  - **projector**:
    `crates/panache-parser/tests/pandoc/native_projector.rs`
    (new `emit_latex_command_with_absorb` helper; `inlines_from` and
    `inlines_from_marked` use peekable iterators routing
    `LATEX_COMMAND` through the helper;
    `render_link_inline` splits `text_inlines` into
    `resolved_text_inlines` (trim) and `unresolved_text_inlines`
    (keep_edges))
  - **allowlist**:
    `crates/panache-parser/tests/pandoc/allowlist.txt` (+1: 51)
- **Don't redo**:
  - The trailing-space absorption is gated on **last byte is ASCII
    letter**, not on "no `{...}` arg". The two are equivalent for
    pandoc's grammar (`\letters` vs `\letters{...}`), but checking the
    last byte is cheaper and avoids needing to parse the command
    structure. Verified against pandoc for `\foo`, `\LaTeX`, `\frac{}`,
    `\LaTeX{}`, `\foo123` — all align with last-byte-letter rule.
  - The absorption is **only horizontal whitespace** (`' '` and
    `'\t'`), not newlines. Pandoc keeps `\foo\n` as `RawInline "\\foo"`
    + `SoftBreak`; the helper peeks at the next token and only matches
    `TEXT` tokens (not `NEWLINE`), so newline absorption can't happen
    by accident. Don't widen `bytes[absorbed] == b' ' || b'\t'` to
    include `b'\n'`.
  - The peekable-iterator pattern is **load-bearing** in
    `inlines_from_marked` because the helper consumes the next iterator
    element when absorbing. Don't refactor back to a
    `for el in parent.children_with_tokens()` for-loop — the helper
    requires `&mut iter`. The existing marker-skip arms still work
    because they're simple non-iter-mutating cases inside the same
    `match`.
  - `unresolved_text_inlines` is rebuilt with `keep_edges` rather than
    re-using `resolved_text_inlines` and re-adding stripped edges. The
    edge-trimming inside `coalesce_inlines` is destructive (it pops the
    leading/trailing `Space`/`SoftBreak`), so we can't recover the
    edges from the trimmed list. Two separate calls is the correct
    pattern.
  - Resolved Links keep using `coalesce_inlines` (with trim). Pandoc
    *does* strip edge whitespace for resolved Links (`[ foo ](url)` →
    `Link [Str "foo"] ("url", "")`). Don't switch resolved Links to
    `keep_edges` — that would regress passing cases.

## Earlier session (2026-05-01, multiline-table inline reparse + `~~` empty-subscript fallback)

- **Date**: 2026-05-01 (multiline-table inline reparse + `~~`
  empty-subscript fallback)
- **Pass before → after**: 163 → 165 / 187 (+2 imports: #126, #56).
  One projector-only fix (multiline-table cells run through the
  inline parser) and one parser-shape fix (the unclosed-`~~`
  strikeout fallback now lands on an empty `Subscript` per pandoc).
  CommonMark allowlist stayed green; full parser-crate suite green;
  workspace tests green; clippy + fmt clean.
- **What landed**:
  - **#126 multiline_table_inline_formatting (projector-only)** ---
    `crates/panache-parser/tests/pandoc/native_projector.rs`. The
    multiline-table cell builder previously used a cheap
    `push_plain_text_inlines` whitespace tokenizer that emitted
    `Str` + `Space` only --- inline markup like `**bold**`,
    `` `code` ``, `[link](url)` inside multiline cells projected as
    raw `Str`. Replaced with `parse_cell_text_inlines`: joins the
    column's per-line trimmed segments with `\n`, calls
    `panache_parser::parse(joined, Some(pandoc_options))`, and
    walks `descendants()` for the first `PARAGRAPH`/`PLAIN` node to
    extract inlines via the existing `inlines_from`. The new helper
    uses `panache_parser::ParserOptions` directly (no shared
    constructor with `pandoc.rs::pandoc_options()` --- the projector
    is a sibling module, and threading a constructor through the
    `project()` API would have meant rewriting every callsite). The
    re-parse goes through `coalesce_inlines` afterward for smart
    quotes, abbreviations, and edge whitespace trim. Empty/all-WS
    cell text returns `Vec::new()` directly; the rest is unchanged.
  - **#56 emphasis_nested_inlines (parser-shape)** ---
    - `crates/panache-parser/src/parser/inlines/subscript.rs`:
      `try_parse_subscript` previously bailed when the second byte
      was `~` (to avoid mis-matching strikeout). Replaced with a
      `Some((2, ""))` early-return: `~~` is consumed as an empty
      `Subscript`, matching pandoc's strikeout-fallback (verified:
      `~~unclosed` → `Subscript [] , Str "unclosed"`,
      `a ~~b` → `Str "a" , Space , Subscript [] , Str "b"`,
      `~~ a ~~` → `Subscript [] , Space , Str "a" , Space ,
      Subscript []`). Single-tilde flow (`~text~`,
      `~text\ with\ escapes~`) is unchanged because the `~~` early
      return only fires when bytes[1] == b'~'.
    - `crates/panache-parser/src/parser/inlines/core.rs`: reordered
      the dispatch so strikeout is tried *before* subscript at
      `~`-bytes. With subscript now accepting `~~` as empty, real
      `~~text~~` strikeouts must match before subscript can claim
      the `~~` opener. The subscript-after-strikeout order is the
      complement of the change: strikeout's `try_parse` already
      requires both an opening `~~` and a closing `~~`, so it
      naturally fails on unclosed forms and lets subscript pick up
      `~~`.
    - Unit tests in `subscript.rs`:
      - `test_empty_content` updated --- `~~` is now `Some((2,
        ""))`, `~ ~` is still `None` (pandoc rejects single-space
        between tildes).
      - `test_not_confused_with_strikeout` renamed to
        `test_double_tilde_unclosed_is_empty_subscript` and
        rewritten --- documents the dispatch-order rationale and
        asserts `~~text~~` and `~~unclosed` both produce
        `Some((2, ""))` standalone (real strikeout matching is the
        dispatcher's job).
  - **New parser fixture** ---
    `crates/panache-parser/tests/fixtures/cases/subscript_unclosed_double_tilde_pandoc/`
    with `parser-options.toml` (`flavor = "pandoc"`) and an
    `input.md` covering five cases: bare `~~unclosed strike`,
    `*text ~~unclosed strike end*` (the #56 driver),
    `~~hello~~` (real strikeout still works under reorder),
    `~text~` (single-tilde subscript unchanged), and `a ~~b`
    (mid-paragraph empty subscript). Snapshot pinned at
    `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_subscript_unclosed_double_tilde_pandoc.snap`.
    Wired into `tests/golden_parser_cases.rs` between
    `standardize_bullets` and `sentence_wrap_basic` (preserving the
    file's loose-alphabetical ordering).
  - **Existing parser fixture snapshot updated** ---
    `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_emphasis_nested_inlines.snap`.
    The unclosed-`~~` paragraph inside `*...*` now contains an empty
    `SUBSCRIPT` between the `text ` and `unclosed strike end` text
    runs (was a single `TEXT@1229..1255 "text ~~unclosed strike end"`
    span). The legacy snapshot pinned the buggy single-text shape;
    per `.claude/rules/parser.md`, fixed toward pandoc-native rather
    than preserving the legacy bug.
  - **Formatter golden updated** ---
    `tests/fixtures/cases/emphasis_nested_inlines/expected.md`. The
    line `*text \~\~unclosed strike end*` (with escaped tildes) is
    now `*text ~~unclosed strike end*` (unescaped). The formatter's
    Subscript renderer emits the markers without escaping, and the
    output is idempotent (re-parses to the same EMPHASIS containing
    SUBSCRIPT empty + TEXT). The legacy expected pinned the
    over-escaped form that came out of the buggy-text-only parse;
    updated to match the corrected parser shape.
- **Cases unlocked** (+2, allowlisted under `# imported`):
  - 56 (emphasis_nested_inlines)
  - 126 (multiline_table_inline_formatting)
- **Files changed (classified)**:
  - **projector**: `crates/panache-parser/tests/pandoc/native_projector.rs`
    (multiline_row_cells_blocks now calls a new
    `parse_cell_text_inlines` helper; removed unused
    `push_plain_text_inlines`)
  - **parser-shape**:
    `crates/panache-parser/src/parser/inlines/subscript.rs`,
    `crates/panache-parser/src/parser/inlines/core.rs`
  - **new parser fixture**:
    `crates/panache-parser/tests/fixtures/cases/subscript_unclosed_double_tilde_pandoc/`
    (`input.md`, `parser-options.toml`),
    `crates/panache-parser/tests/golden_parser_cases.rs` (registration)
  - **parser snapshots**:
    `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_emphasis_nested_inlines.snap`,
    `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_subscript_unclosed_double_tilde_pandoc.snap`
    (new)
  - **formatter golden**:
    `tests/fixtures/cases/emphasis_nested_inlines/expected.md`
  - **allowlist**:
    `crates/panache-parser/tests/pandoc/allowlist.txt` (+2: 56, 126)
- **Don't redo**:
  - The dispatch reorder in `inlines/core.rs` is load-bearing for
    the subscript change. With subscript now accepting `~~` as
    empty, swapping the order back would break every real
    strikeout (`~~text~~` → empty subscript + text + empty
    subscript). If you ever need to gate the empty-subscript form
    further, keep the strikeout-first order and refine the
    early-return condition inside `try_parse_subscript`, not the
    dispatcher.
  - `parse_cell_text_inlines` constructs `ParserOptions` inline
    rather than reusing `pandoc.rs::pandoc_options()`. The two
    files are sibling modules attached via `#[path = ...]` from
    `tests/pandoc.rs`; sharing a constructor would require
    plumbing it through `project()`. The duplication is 5 lines
    and matches a recurring pattern --- don't refactor it for
    its own sake.
  - `parse_cell_text_inlines` walks `descendants()` not just direct
    children. Pandoc cell text is normally a single paragraph at
    the top, but a wider re-parse (e.g. a stray reference def at
    top followed by Para) won't strand the inlines; we still find
    the first PARAGRAPH/PLAIN. Don't switch to direct `.children()`
    --- it'll silently return empty for any document whose first
    block isn't PARAGRAPH/PLAIN.
  - The trim-then-join order in the multiline cell loop is
    intentional. Per-line slice → trim → push to `col_lines[i]`
    drops both leading-pad whitespace (column boundaries are not
    word boundaries; the slice may start mid-word, but the parser
    handles that for strict slices). Final join with `\n` makes
    each segment a separate paragraph line that the inline parser
    sees as a SoftBreak boundary. Don't switch to a `\n\n` join
    --- pandoc emits `SoftBreak` between cell lines, not a
    paragraph break.
  - `~~` empty-subscript fallback is gated on
    `config.extensions.subscript`. CommonMark/GFM disable subscript
    by default, so the fallback never fires there (verified
    against pandoc: under `-f commonmark`/`-f gfm`, `~~unclosed`
    stays as `Str "~~unclosed"`). Don't widen the gate to all
    flavors.
  - The fixture's `a ~~b` line tests mid-paragraph empty-subscript
    (no preceding `*`). It's there to pin that the fallback works
    at *any* `~~` position, not just inside emphasis. Pandoc:
    `[ Str "a" , Space , Subscript [] , Str "b" ]`. Don't drop
    that test --- it guards a different code path than the
    `*...*` case (the inline parser's outer dispatch, not the
    emphasis recursion).
  - The formatter golden change for `emphasis_nested_inlines`
    flipped `\~\~` → `~~` in the unclosed-strikeout line. This is
    a side effect of the parser change: the old text-only shape
    triggered the formatter's text-escape pass on `~`; the new
    SUBSCRIPT-node shape uses the subscript renderer which emits
    bare `~`. Don't add an explicit `\~` formatting rule --- the
    new output is idempotent and matches pandoc-native shape.

## Earlier session (2026-05-01, single-char upper Roman period 2-space gate)

- **Pass before → after**: 162 → 163 / 187 (+1 import: #115). One
  parser-shape fix: pandoc requires single-character uppercase Roman
  numerals followed by `.` (the seven values `I, V, X, L, C, D, M`)
  to have **two** spaces after the period, mirroring its existing
  rule for single-letter alpha markers (`A.`, `B.`). Without this
  gate, panache greedily parsed `I. First item\nII. Second item\n...`
  as an UpperRoman list, but pandoc rejects the whole thing as a
  paragraph because the *first* marker `I.` (single space) fails the
  initials-disambiguation check. Multi-character romans like `II.`,
  `III.` only need 1 space; right-paren form `I)` is unaffected.
  CommonMark allowlist stayed green; full parser-crate suite green;
  full workspace tests green; clippy + fmt clean.
- **What landed**:
  - **Parser fix** —
    `crates/panache-parser/src/parser/blocks/lists.rs` (uppercase
    Roman branch around line 524). Added
    `min_spaces = if delim == b'.' && len == 1 { 2 } else { 1 }`
    plus an `effective_cols` measurement against `min_spaces` (mirrors
    the existing UpperAlpha branch's pattern). The check fires
    *before* the `ListMarkerMatch` returns, so a failing `I.` falls
    through to the lowercase-letter branch (which won't match upper)
    and finally returns None — paragraph dispatch takes over. Pandoc's
    rule (`pandoc/src/Text/Pandoc/Readers/Markdown.hs::orderedListStart`,
    lines 879-882): `delim == Period && (style == UpperAlpha ||
    (style == UpperRoman && num ∈ [1,5,10,50,100,500,1000]))` requires
    `lookAhead (newline <|> spaceChar)` — i.e. at least one extra
    space after the consumed-space. `len == 1` is the right
    discriminator because panache's `try_parse_roman_numeral` already
    accepts only `I/V/X` as single-character romans (`L/C/D/M` get
    rejected at the single-char gate and fall through to UpperAlpha,
    which already had the 2-space rule). So the seven pandoc-cared-for
    values map to: I/V/X handled here (Roman branch), L/C/D/M handled
    by the existing UpperAlpha branch.
  - **New parser fixture** —
    `crates/panache-parser/tests/fixtures/cases/lists_fancy_uppercase_roman_period_pandoc/`
    with `parser-options.toml` (`flavor = "pandoc"`) and an
    `input.md` covering: single-space `I. First\nII. Second` (Para),
    two-space `I.  First\nII. Second` (List, the `II.` only needs 1
    space because multi-char), `V. Five\nX. Ten` (both single-char,
    Para), `I) right paren\nII) Second` (List, paren form is
    unaffected). Snapshot pinned at
    `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_lists_fancy_uppercase_roman_period_pandoc.snap`.
    Wired into `tests/golden_parser_cases.rs`.
  - **Existing parser fixture snapshot updated** —
    `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_lists_fancy.snap`.
    The `## Uppercase Roman Numerals\n\nI. First item\nII. Second
    item\n...VII. Seventh item\n` block now projects as a single
    `PARAGRAPH` with `TEXT` + `NEWLINE` runs instead of seven
    `LIST_ITEM` nodes. The legacy snapshot pinned the wrong
    behavior; per `.claude/rules/parser.md`, fixed toward
    pandoc-native rather than preserving the legacy shape.
  - **Existing unit test updated + new test added** —
    `crates/panache-parser/src/parser/blocks/tests/lists.rs`.
    `fancy_list_upper_roman_period` now uses two spaces after `I.`
    (`"I.  first\nII. second\nIII. third\n"`) and still asserts a
    3-item list. Added
    `fancy_list_upper_roman_period_single_char_one_space_rejected`
    that asserts no LIST is found for the single-space form (paragraph
    fallback). Both tests cite the pandoc rationale inline.
  - **Formatter golden updated** —
    `tests/fixtures/cases/lists_fancy/expected.md`. The
    right-aligned 7-row list block in the upper-Roman section is
    replaced by the reflowed paragraph `I. First item II. Second item
    III. Third item IV. Fourth item V. Fifth item VI.\nSoftBreak
    Sixth item VII. Seventh item` (default 80-col reflow). The
    formatter `lists_fancy` golden case kept the buggy behavior
    pinned; updated to match the corrected parser shape. Idempotency
    holds: the formatted output round-trips through parse+format
    unchanged (the suite verifies this for every golden case).
- **Cases unlocked** (+1, allowlisted under `# imported`):
  - 115 (lists_fancy)
- **Files changed (classified)**:
  - **parser-shape**:
    `crates/panache-parser/src/parser/blocks/lists.rs`,
    `crates/panache-parser/src/parser/blocks/tests/lists.rs`
  - **new parser fixture**:
    `crates/panache-parser/tests/fixtures/cases/lists_fancy_uppercase_roman_period_pandoc/`
    (`input.md`, `parser-options.toml`),
    `crates/panache-parser/tests/golden_parser_cases.rs` (registration)
  - **parser snapshots**:
    `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_lists_fancy.snap`,
    `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_lists_fancy_uppercase_roman_period_pandoc.snap`
    (new)
  - **formatter golden**:
    `tests/fixtures/cases/lists_fancy/expected.md`
  - **allowlist**:
    `crates/panache-parser/tests/pandoc/allowlist.txt` (+1: 115)
- **Don't redo**:
  - The 2-space gate is intentionally narrow — only `delim == b'.'`
    AND `len == 1`. Right-paren form (`I)`) does *not* require 2
    spaces (verified against pandoc: `I) Right paren style` is
    accepted as `OrderedList ( 1 , UpperRoman , OneParen )` with
    one space). Paren form is structurally unambiguous (no
    confusion with sentence-end abbreviations), so pandoc's rule
    skips it. Don't broaden to all delim forms.
  - `len == 1` (single-char Roman) is the correct discriminator.
    Pandoc's actual rule is `num ∈ [1,5,10,50,100,500,1000]`, the
    single-character Romans. Panache's `try_parse_roman_numeral`
    already rejects `L/C/D/M` as single-char Romans (only
    `I/V/X` pass the single-char gate); `L/C/D/M` fall through to
    UpperAlpha which has its own existing 2-space rule. So
    `len == 1` covers I/V/X, and the rest are covered by the
    existing UpperAlpha path. No need to enumerate the seven
    Roman values explicitly.
  - The `effective_cols` check (against `min_spaces`) measures
    leading whitespace including tab-stop expansion. Don't
    simplify to `after_marker.starts_with("  ")` — a tab can
    legitimately satisfy 2-col-width even though it's a single
    char. Mirrors the UpperAlpha branch's identical pattern.
  - The legacy `lists_fancy` parser fixture had the buggy behavior
    pinned (LIST for single-space `I.`). Per `.claude/rules/parser.md`
    "an existing fixture matching the legacy output is NOT a
    guarantee of correctness" — fix toward pandoc-native. Updated
    the snapshot rather than carving out a "legacy mode" arm.
  - The formatter expected.md change was a side effect of the parser
    change, not an intentional formatter behavior shift. The
    paragraph reflow comes from the default `wrap=reflow` mode
    operating on the now-correctly-parsed Para. Don't add
    `wrap = preserve` to the fixture's panache.toml — the reflow
    behavior is what users get out of the box and is what the
    fixture should pin.

## Earlier session (2026-05-01, heading-then-indented-code + super/sub whitespace)

- **Pass before → after**: 161 → 162 / 187 (+1 import: #82). First
  parser-touching session in a while — two related parser-shape fixes
  landed (one unlocks #82, the other improves correctness for #51 but
  doesn't fully unlock it because the case has additional gaps in TeX
  inline trailing-space handling and unresolved-reference-link
  projection of `\\[ ... \\]`). CommonMark allowlist stayed green; full
  parser-crate suite green; full workspace tests green; clippy + fmt
  clean.
- **What landed**:
  - **#82 indented_code_after_atx_heading_pandoc** —
    `crates/panache-parser/src/parser/block_dispatcher.rs`
    `IndentedCodeBlockParser::detect_prepared` previously gated Pandoc
    dialect strictly on `has_blank_before_strict`. Pandoc actually
    allows an indented code block to immediately follow a complete
    one-liner block (ATX heading or HR) at the current blockquote
    depth without an intervening blank line. Added
    `prev_line_is_terminal_one_liner(lines, line_pos, expected_bq_depth)`
    helper at the bottom of the dispatcher: looks at `lines[line_pos -
    1]`, strips `expected_bq_depth` blockquote markers (rejects if the
    prev line's bq depth differs — that's a lazy-continuation case),
    then checks if the trimmed inner is a `try_parse_atx_heading` or
    `try_parse_horizontal_rule` match. Used in the existing pandoc-arm:
    `ctx.has_blank_before_strict || prev_line_is_terminal_one_liner(_lines, line_pos, ctx.blockquote_depth)`.
    The fixture and snapshot for `indented_code_after_atx_heading_pandoc`
    already existed (with the broken behavior pinned); updated the
    snapshot to the corrected `HEADING + CODE_BLOCK` shape (matches the
    `_commonmark` sibling). No new fixture added — the parser-shape was
    already pinned, the fix made it correct.
  - **Superscript / Subscript internal-whitespace gate (correctness, not
    a #51 unlock alone)** —
    `crates/panache-parser/src/parser/inlines/{superscript,subscript}.rs`
    `try_parse_superscript` and `try_parse_subscript` previously
    accepted `^foo bar^` / `~foo bar~` as a single Superscript/Subscript
    with internal whitespace. Pandoc rejects unescaped whitespace inside
    the carets/tildes (verified: `^x y^` is plain text; `^x\ y^` is
    `Superscript [Str "x\160y"]`). Added a `contains_unescaped_whitespace`
    helper in each module that walks bytes, skipping `\X` pairs as
    escaped chars; if any unescaped whitespace char remains, return
    `None`. Updated each module's `test_spaces_inside_are_ok` to
    `test_internal_whitespace_rejected` (asserts None for `^some text^`
    plus accepts `^some\ text^`). This is a correctness improvement —
    #51 still fails because it has independent additional gaps (TeX
    inline trailing-space inclusion: pandoc emits `RawInline tex
    "\\infty "` with the trailing space; panache emits `RawInline tex
    "\\infty"` + `Space`. And `\\[ E = mc^2 \\]` is parsed as an
    unresolved reference Link `\\[E = mc^2 \\]` whose projection drops
    the leading space inside the link text).
  - **Test updates** —
    - `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_indented_code_after_atx_heading_pandoc.snap`
      updated to the corrected CST.
    - `tests/format/subscript.rs`: replaced
      `subscript_with_multiple_words` (asserted incorrect "tildes
      preserved" behavior — they're now escaped to `\~`) with
      `subscript_with_unescaped_internal_whitespace_is_not_subscript`
      and a paired `_with_escaped_internal_whitespace_is_subscript`
      test.
    - `tests/format/superscript.rs`: same shape — renamed
      `superscript_with_multiple_words` to
      `..._with_unescaped_internal_whitespace_is_not_superscript` (the
      assertion happened to still pass coincidentally because `^` isn't
      escaped in plain-text output, but the test name now matches what
      it actually verifies) and added a paired escaped-space test.
- **Cases unlocked** (+1, allowlisted under `# imported`):
  - 82 (indented_code_after_atx_heading_pandoc)
- **Files changed (classified)**:
  - **parser-shape**:
    `crates/panache-parser/src/parser/block_dispatcher.rs` (#82),
    `crates/panache-parser/src/parser/inlines/superscript.rs` (correctness),
    `crates/panache-parser/src/parser/inlines/subscript.rs` (correctness)
  - **parser snapshot**:
    `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_indented_code_after_atx_heading_pandoc.snap`
  - **formatter integration tests**:
    `tests/format/subscript.rs`, `tests/format/superscript.rs`
  - **allowlist**: `crates/panache-parser/tests/pandoc/allowlist.txt`
    (+1 imported ID)
- **Don't redo**:
  - The `prev_line_is_terminal_one_liner` heuristic only checks ATX
    heading + HR. Pandoc *also* allows indented code after a fenced
    code closer or setext underline (`Heading\n=====\n    foo` →
    Header + CodeBlock). Those weren't in the failing-cases bucket
    this session (no remaining failing case requires them), so
    extending the heuristic was deferred. If a future session needs
    one of those, add the corresponding parse check to the same
    helper — don't re-architect to track previously-emitted block
    kinds in Parser state. The dispatch-time lookback is cheap (O(1)
    per dispatch) and avoids per-emission state-update touch points
    scattered across the parser.
  - The `expected_bq_depth` argument to
    `prev_line_is_terminal_one_liner` is the *current* line's
    blockquote depth (passed via `ctx.blockquote_depth`). Mismatched
    prev-line depth → return false. This is what correctly rejects
    `>     foo\n    bar` lazy-continuation: the prev line `>     foo`
    has bq_depth=1 but the current line `    bar` has bq_depth=0
    (well, the dispatcher sees ctx.blockquote_depth=1 because the
    container is still open from the prev line — but then the prev
    line's stripped inner `    foo` doesn't parse as ATX or HR
    either). Don't simplify the bq-depth check away; it guards
    against future cases where prev-line is at a different depth.
  - The whitespace check in superscript/subscript walks bytes, skipping
    `\X` pairs as escaped (advances by 2). Don't switch to a regex or
    use Rust's `chars()` — the byte-level walk is faster and matches
    pandoc's lexer-level whitespace check. The check fires AFTER the
    existing leading/trailing-whitespace and trim-empty checks, so it
    only sees content that's already non-empty and non-edge-padded.
  - The new `contains_unescaped_whitespace` helper is duplicated in
    both `superscript.rs` and `subscript.rs` (private to each
    module). It's 14 lines; deduping into a shared `parser/inlines/utils`
    module was considered but rejected because the function is too
    small to justify a new utils module entry, and both call sites
    need to retain the precise pandoc-spec rationale comment in their
    file (where future maintainers will look first).
  - #51 (`double_backslash_math`) still fails after the superscript
    fix. Two independent remaining gaps:
    1. **TeX inline trailing-space**: pandoc's `\\infty ` (with
       trailing space) → `RawInline tex "\\infty "`; panache emits
       `RawInline tex "\\infty"` + separate `Space` token. Lives in
       parser inlines/latex.rs (or similar). Not addressed here.
    2. **`\\[ ... \\]` parsed as unresolved reference Link**: the
       `[` after `\\` opens a LINK node whose LINK_TEXT contains the
       `E = mc^2 \\` content. The unresolved-link projector path
       drops the leading space inside LINK_TEXT (or the coalescer
       does), producing `Str "\\[E"` instead of `Str "\\[" + Space +
       Str "E"`. Either the parser shouldn't open a LINK after
       `\\[`, or the projector's unresolved-link path needs to
       preserve the leading space. Not addressed here.
- **Next**: Citations (#38) is still the largest single-fix unlock
  (heavy projector). Among parser-shape gaps, the most leverage
  remaining is the **blockquote/list/definition-list nesting cluster**
  (#34, #91, #93, #96, #108, #111 — 6 cases sharing parser-shape
  root causes around lazy continuation and same-line marker
  containers). Smaller individual targets:
  - **#56 emphasis_nested_inlines** — single edge case where unclosed
    `~~` inside emphasis should emit `Subscript []` (pandoc parses
    `~~` as two `~` tokens, the first opens an empty Subscript
    closed by the second; remaining text follows). Niche but tiny
    inline-parser change.
  - **#128 nested_headings_in_containers** — the parser doesn't
    recognize `# Heading` on the first line of a list item or
    definition item. The blockquote case already works, so the gap
    is specifically in lists/definition-lists initial-content
    dispatch. Parser-shape work.
  - **#115 lists_fancy** — parser too permissive on uppercase
    alphabetic markers (`I.` with single space accepted as a list
    marker; pandoc requires double space for single capital
    letters). Parser fix in `lists.rs::try_parse_list_marker`.
  - **#79 ignore_directives** — pandoc keeps trailing `<!-- ... -->`
    inline-html as a `RawInline` at the end of the surrounding
    paragraph; panache splits each comment into a separate
    `RawBlock`. Parser-shape gap in HTML_BLOCK boundary detection.

## Previous session (2026-05-01, tables)

- **Date**: 2026-05-01 (tables)
- **Pass before → after**: 152 → 161 / 187 (+9 imports). All wins are
  **projector-only** — no parser code was touched. CommonMark allowlist
  stayed green; full parser-crate suite green; clippy + fmt clean.
- **What landed (all in `tests/pandoc/native_projector.rs`)**:
  - **`SIMPLE_TABLE` projector** (`simple_table`,
    `simple_table_dash_runs`, `simple_table_aligns`,
    `simple_table_row_cells`, `simple_table_row_is_all_dashes`).
    Walks `TABLE_HEADER` / `TABLE_SEPARATOR` / `TABLE_ROW` /
    `TABLE_CAPTION` children. Column boundaries from dash runs in the
    separator. Alignment derivation via the *flushness* rule: each
    cell's visible (whitespace-trimmed) `(start_col, end_col)` vs the
    column's dash-run `(start, end)` — both flush → AlignDefault,
    left-only → AlignLeft, right-only → AlignRight, neither →
    AlignCenter (mirrors pandoc's `alignType` in
    `Markdown.hs::simpleTableHeader`). Headerless variant: derives
    alignment from the first data row, and drops a trailing all-dashes
    `TABLE_ROW` (parser-shape quirk: the closing `------` in headerless
    tables is currently emitted as a row of dash cells). All
    simple-table widths are `ColWidthDefault` per pandoc
    (`useDefaultColumnWidths` in `simpleTable`).
  - **`MULTILINE_TABLE` projector** (`multiline_table`,
    `multiline_row_cells_blocks`, `char_slice`,
    `push_plain_text_inlines`). Two `TABLE_SEPARATOR`s (top border +
    column separator) when a header is present, one when headerless;
    the column separator is the canonical one for column boundaries.
    Row content is sliced from raw row text via column ranges and
    joined with `SoftBreak` between source lines (multi-line cells).
    Width computation per pandoc `widthsFromIndices`:
    `width[i] = (col_start[i+1] - col_start[i])` for non-last cols,
    `width[last] = dashes[last] + 2` (the `indices'` last-index `+1`
    bump). Normalize by `max(sum(widths), 72)`. Alignment uses the
    same flushness rule as simple tables (header line 1 only —
    sufficient for current cases).
  - **`GRID_TABLE` projector** (`grid_table`, `grid_dash_widths`,
    `grid_separator_aligns`, `grid_segment_align`). Column widths
    follow pandoc's `fractionalColumnWidths`:
    `raw[i] = dashes[i] + 1`, `norm = max(sum(raw) + count - 2, 72)`,
    `width[i] = raw[i] / norm`. Alignment from the first
    `:`-bearing separator using the pipe-table rule (left:, :right,
    :center: → AlignLeft/AlignRight/AlignCenter; otherwise
    AlignDefault). Each row's cells are taken directly from
    `TABLE_CELL` children (parser already splits them at `|`
    boundaries).
  - **`TableData.widths`** field added to the projector AST: each
    column carries `Option<f64>` (None → `ColWidthDefault`,
    Some(w) → `ColWidth w`). `write_table` renders the appropriate
    form. Pipe-table widths populate as all `None`.
  - **`show_double` helper**. Renders `f64` like Haskell's `show`:
    decimal in `[0.1, 1e7)`, scientific outside. Always emits a
    fractional component (`1.0` not `1`) and a `.0` mantissa for
    whole-number scientific (`1.0e8`). Matches pandoc's pretty-print
    of `ColWidth N` exactly (probed: `1/12 = 8.333333333333333e-2`,
    `1/8 = 0.125`, `11/72 = 0.1527777777777778`).
- **Cases unlocked** (+9, all allowlisted under `# imported`):
  - 69 (grid_table_caption_before)
  - 72 (headerless_table)
  - 122 (multiline_table_basic)
  - 123 (multiline_table_caption)
  - 124 (multiline_table_caption_after)
  - 125 (multiline_table_headerless)
  - 127 (multiline_table_single_row)
  - 169 (simple_table)
  - 172 (table_with_caption)
- **Files changed (classified)**:
  - **projector** (single file): `tests/pandoc/native_projector.rs`
  - **allowlist**: `tests/pandoc/allowlist.txt` (+9 imported IDs)
- **Don't redo**:
  - `simple_table_aligns` operates on the cell's *trimmed-content*
    range, not the raw `TABLE_CELL` byte range. Multiline-table cells
    include padding whitespace within the column slice (e.g.
    `TABLE_CELL@62..73 " Centered  "`); simple-table cells don't
    (parser splits leading/trailing WHITESPACE out, so cell range
    already equals trimmed range). Both paths feed the same
    visible-range computation. Don't switch to raw-range — case 122
    (`Centered Default Right Left`) regresses to all-Default.
  - For multiline-table widths, pandoc's `widthsFromIndices` produces
    `width[last] = dashes[last] + 2` (the indices' last-index `+1`
    bump compensates for trailing-spaces being excluded). Non-last
    cols use `dashes[i] + spaces_after[i] = col_start[i+1] -
    col_start[i]`. The `+2` for last is *not* the same as `dashes+1`;
    for typical 1-space-between-cols layouts they happen to coincide
    for non-last cols only by accident. Keep the explicit branch on
    `i + 1 < cols.len()`.
  - For grid-table widths: norm = `max(sum_raw + count - 2, 72)`,
    where `sum_raw = sum(dashes+1)` and `count` = number of cols.
    This is *not* `max(line_length, 72)` and *not* `max(sum_dashes +
    count, 72)`. The `- 2` term is from pandoc's code (line 205 of
    `GridTable.hs`); without it widths come out a few percent too
    small for wide tables. Verified vs probed pandoc output for the
    0070 nordics layout (`24/82 = 0.2926...`).
  - Headerless-simple-table parser quirk: trailing `-------` emitted
    as a `TABLE_ROW` of dash cells. The projector skips the last row
    when its non-empty cells are all-dashes
    (`simple_table_row_is_all_dashes`). Don't try to fix this in the
    parser as part of a conformance session — it'd require a separate
    parser-fixture-first change.
  - Inline content of multiline-table cells goes through
    `push_plain_text_inlines`, a cheap whitespace-tokenizer that emits
    `Str` + `Space` only — *no* markdown re-parsing. This is why
    **#126** (`multiline_table_inline_formatting`) still fails on
    `**bold**`/`` `code` ``/`[link](url)`. Re-parsing requires feeding
    the cell text through panache's inline parser; not done in this
    session.
  - `show_double` sticks to `format!("{x}")` for the decimal branch
    and `format!("{x:e}")` for scientific. Rust's default f64 Display
    happens to match Haskell `show` for `[0.1, 1e7)` and the
    shortest-round-trip scientific form matches outside that range.
    Don't switch to a fixed-precision formatter (`{:.16}`); that
    over-pads and breaks expected output.

## Previous session (2026-05-01, later)

- **Pass before → after**: 147 → 152 / 187 (+5 imports). All wins are
  **projector-only** again --- no parser code was touched this session.
  CommonMark allowlist stayed green; full parser-crate suite green; clippy + fmt
  clean.
- **What landed (all in `tests/pandoc/native_projector.rs`)**:
  - **Inline-code whitespace normalization** (`strip_inline_code_padding`).
    Pandoc's `Markdown.hs::code` does `\n` → space then `trim`, with no
    preservation of edge whitespace beyond what `trim` keeps. The previous
    "strip a single leading/trailing space if both ends have a space" rule
    under-stripped (`\` a
    \``would keep edge spaces).     Replaced with`chars().map(\|c\| if c == '\n'
    { ' ' } else { c
    })`then`.trim()`. Internal multi-space runs are still preserved     (probed:`\`
    a b \``→`Code "a b"`). Unlocked **#63** ---     Quarto fence at column 0 (`
    \```{r} `) is parsed by pandoc as     inline code (no language ID after `\`\`\``),
    and the body `{r}\na <- 1\n` had to collapse to `"{r} a <- 1"`.
  - **`PANDOC_TITLE_BLOCK`→ drop**. Pandoc's `% Title\n% Authors\n%     Date`
    populates Meta and emits *no body block*. Added the
    `SyntaxKind::PANDOC_TITLE_BLOCK => None` arm in the dispatcher (mirrors the
    existing `YAML_METADATA => None`). Unlocked **#130**.
  - **Link/Image attribute attachment** (`extract_attr_from_node`). Parser
    already attaches a child `ATTRIBUTE` node/token to LINK and IMAGE_LINK for
    `[text](url){.cls #id key=val}` form, but `render_link_inline` /
    `render_image_inline` were ignoring it and passing `Attr::default()`. Added
    an `extract_attr_from_node` helper that reads the `ATTRIBUTE` child and
    parses via the existing `parse_attr_block`, then applied it to all four code
    paths (inline link/image with paren dest, reference link/ image both
    ref-resolved and heading-id-resolved). Unlocked **#101**.
  - **Example-list document-level numbering pre-pass** (`RefsCtx` additions:
    `example_label_to_num`, `example_list_start_by_offset`). Mirrors the
    heading-id pre-pass shape. `collect_example_numbering` walks every `LIST` in
    document order; for each Example list (detected via `list_is_example`
    reading the first `LIST_MARKER`), records the start counter for the list's
    offset and increments a shared counter per item. Labeled items (`(@label)`)
    populate `example_label_to_num`. `ordered_list_attrs` consults the offset
    map for Example lists so each subsequent list starts where the last left off
    (rather than restarting at 1). Inline `@label` refs are routed through a new
    `render_citation_inline` that looks up the label in `example_label_to_num`
    and emits `Inline::Str(N.to_string())` (just the digits --- surrounding
    parens come from adjacent source `(`/`)` text and our coalescing pass merges
    them with the digits into a single Str). Unrecognized citations still emit
    `Unsupported "CITATION"` to keep general citation work visible. Unlocked
    **#114**.
  - **`Figure`block** for `+implicit_figures` (`figure_block`, `Block::Figure`).
    Parser already produces a `FIGURE` block when a paragraph is exactly one
    image; the projector previously fell through to `Unsupported "FIGURE"`.
    Added `Block::Figure(Attr,     caption_blocks, body_blocks)` with
    `Caption Nothing     [Plain alt-inlines]` shape, body re-inserts the Image
    as a `Plain` block. Image alt becomes the Figure caption. Image attrs (id
    only --- pandoc keeps classes/kvs on the Image) migrate to the Figure attr.
    Unlocked **#81**.
- **Cases unlocked** (+5, all allowlisted under `# imported`):
  - 63 (fenced_code) --- inline-code newline-collapse + trim
  - 81 (images) --- Figure block for image-only paragraph
  - 101 (links) --- `{.cls key=val}` attribute attachment
  - 114 (lists_example) --- document-level Example numbering + `@label`
    reference resolution
  - 130 (pandoc_title_block) --- drop title block (Meta-bound)
- **Files changed (classified)**:
  - **projector** (single file): `tests/pandoc/native_projector.rs`
  - **allowlist**: `tests/pandoc/allowlist.txt` (+5 imported IDs)
- **Don't redo**:
  - The inline-code normalization rule is **not** the CommonMark "strip exactly
    one leading and one trailing space if both ends are spaces" rule. Pandoc
    fully `trim`s after `\n`→space (probed: `\` a \``→`Code "a"`,`\` a b
    \``→`Code "a b"\`). Don't restore the strip-1-each-side logic --- it
    under-strips.
  - `extract_attr_from_node` reads ATTRIBUTE as either a child *node* or *token*
    (parser attaches it both ways depending on syntax). Mirrors the heading-attr
    extraction. Don't switch to a single-form lookup; both shapes exist in the
    wild.
  - The Example-list inline reference (`@label` → `Str "N"`) emits *only the
    digits*. Pandoc's surrounding parens (`(@good)` → `Str "(1)"`) come from
    adjacent `(` / `)` *source text* that our text-coalescing pass merges with
    the digit Str. If you try to emit `Str "(N)"` directly you'll double up the
    parens for `(@good)` and break the bare-`@good` form (which renders as
    `Str "1"` with no parens).
  - The Example pre-pass uses one global counter across the entire document.
    Pandoc tracks Example numbering at document scope; do *not* reset per
    OrderedList. The counter increments per LIST_ITEM, not per LIST, so
    multi-item lists get sequential numbers (1,2,3) and the next list picks up
    at 4.
  - `list_is_example` keys off the *first* item's marker only --- pandoc decides
    list style from the first marker. Don't scan every item; mismatched markers
    (e.g. first `(@)` then `1.`) are accepted by pandoc as one Example list.
  - `Figure` body re-wraps the Image as `Plain [Image]` (not bare `Image`).
    Pandoc-native shape: `Figure attr caption [Plain [     Image ... ]]`. The
    Image's classes/kvs stay on the Image; only the id (when present) moves to
    the Figure attr.
  - The `render_citation_inline` fallback path emits `Unsupported "CITATION"`
    for non-Example labels. Keep this --- it keeps real-citation cases visibly
    failing in `report.txt` so they're easy to find when proper Cite support
    lands. Don't silently drop unrecognized citations.

## Previous session (2026-05-01)

- **Pass before → after**: 134 → 147 / 187 (+13 imports). All wins are
  **projector-only** again --- no parser code was touched this session.
  CommonMark allowlist stayed green; full parser-crate suite green; clippy + fmt
  clean.
- **What landed (all in `tests/pandoc/native_projector.rs`)**:
  - **Pandoc abbreviations (`+abbreviations` extension).** Added a fixed list of
    pandoc's default abbrevs (verbatim from `pandoc/data/abbreviations`, \~80
    entries) and an `apply_abbreviations` post-pass run after
    `smart_quote_pairs` inside `coalesce_inlines_inner`. Rule: a `Str` ending in
    an abbrev followed by `Space` has the `Space` replaced by `\u{a0}` (NBSP)
    appended to the `Str`, and the next `Str` (if any) merged in. The match is
    suffix-anchored: the abbrev must end the Str and be preceded by either
    start-of-Str or a non-letter, non-dot char (matches pandoc's parser behavior
    where the abbrev is parsed as an isolated token before coalescing). Recurses
    into `Quoted` content because `Quoted` is built inside `smart_quote_pairs`
    and bypasses the per-marker `coalesce_inlines_keep_edges` recursion.
    Unlocked **#152, #157**.
  - **OrderedList style/delim classifier.** Replaced the always-`Decimal/Period`
    stub with `classify_ordered_marker` that mirrors pandoc's
    `anyOrderedListMarker` parser: try `decimal` → `exampleNum` (`@label`) →
    `defaultNum` (`#`) → `romanOne` (single `i`/`I`) → single-letter alpha →
    multi-char roman, in that order. Added `roman_to_int` for roman parsing.
    Delimiters derived from the marker punctuation: `(X)` → `TwoParens`, `X)` →
    `OneParen`, `X.` → `Period`. `#` style forces `DefaultDelim` regardless of
    punctuation (per pandoc's `inPeriod`). Unlocked **#117** and contributed to
    **#116**.
  - **Task-list checkbox glyph.** `list_item_blocks` now reads the
    `TASK_CHECKBOX` token from the `LIST_ITEM` and prepends `Str "\u{2610}"` (☐)
    or `Str "\u{2612}"` (☒) plus a `Space` to the first non-empty
    `PLAIN`/`PARAGRAPH` content. The checkbox only applies to the first
    inline-content block per item; later blocks are unchanged. Unlocked **#118,
    #120, #121**.
  - **Code-block language normalization.** Added `normalize_lang_id` mirroring
    pandoc's `toLanguageId`: lowercase, `c++` → `cpp`, `objective-c` →
    `objectivec`. Applied at both attribute-block and shortcut paths in
    `code_block_attr`. Unlocked **#113** (in combination with the offset fix).
  - **Nested-list-item content offset includes leading WHITESPACE.**
    `list_item_content_offset` previously only counted
    `LIST_MARKER + WHITESPACE-after-marker`. Nested list items also carry
    leading WHITESPACE *before* the marker (the outer item's content offset).
    Including those spaces makes the cumulative offset correct for stripping
    nested fenced/indented code-block bodies. The CODE_BLOCK arm in
    `list_item_blocks` now routes *both* fenced and indented code through
    `indented_code_block_with_extra_strip` so the offset gets stripped
    uniformly.
  - **Definition-item loose vs. tight.** `definition_blocks` now takes a `loose`
    flag set by `is_loose_definition_item`. The rule: a `DEFINITION_ITEM` is
    loose iff there is a `BLANK_LINE` between its `TERM` and the first
    `DEFINITION` (per-item, not per-definition). When loose, all `PLAIN`
    children become `Para`; when tight, they stay `Plain`. Unlocked **#139** and
    **#179**.
  - **Raw block via `{=format}` info string.** Added `code_block_raw_format`
    that detects the pandoc raw-attribute form (info string of the shape
    `{=fmt}`, no other attrs). When matched, `code_block` and
    `indented_code_block_with_extra_strip` return `RawBlock(fmt, content)`
    instead of `CodeBlock`. Unlocked **#40, #140**.
  - **Tab expansion in code blocks.** Pandoc tab-expands code-block bodies to
    4-col tab stops *before* any indent stripping. Added `expand_tabs_to_4` and
    applied it: in `strip_indented_code_indent` (before the 4-col strip), in
    `indented_code_block_with_extra_strip` (before the leading-space strip), and
    in `code_block` for fenced bodies. Unlocked **#83**. Also added
    `advance_col` so `definition_content_offset` measures in *columns* (with
    tab-rounding) rather than chars --- without this, `:\t` was reading as
    offset 2 instead of the correct column 4.
- **Cases unlocked** (+13, all allowlisted under `# imported`):
  - 40 (code_blocks_raw) --- `{=format}` → RawBlock
  - 83 (indented_code_mixed_tab_space) --- tab expansion
  - 113 (lists_code) --- c++→cpp, nested code offset
  - 116 (lists_nested) --- fell out from list classifier + offset
  - 117 (lists_ordered) --- `#.` DefaultStyle
  - 118 (lists_task) --- task checkbox glyphs
  - 120 (lists_wrapping_nested) --- task checkbox in nested
  - 121 (lists_wrapping_simple) --- task checkbox in simple
  - 139 (plain_continuation_edge_cases) --- definition loose/tight
  - 140 (raw_blocks) --- `{=format}` → RawBlock
  - 152 (sentence_wrap_abbreviations) --- abbreviation NBSP
  - 157 (sentence_wrap_inline_code_sentence_end) --- abbreviation NBSP
  - 179 (writer_definition_lists_multiblock) --- definition loose
- **Files changed (classified)**:
  - **projector** (single file): `tests/pandoc/native_projector.rs`
  - **allowlist**: `tests/pandoc/allowlist.txt` (+13 imported IDs)
- **Don't redo**:
  - The `PANDOC_ABBREVIATIONS` list is a verbatim copy of
    `pandoc/data/abbreviations`. When pandoc updates that file, refresh --- but
    don't try to derive abbreviations from heuristics (e.g. "ends with `.`").
    Pandoc rejects `etc.` and `X.Y.Z.` despite both ending with a dot --- the
    explicit allowlist is load-bearing.
  - The abbreviation match requires the char preceding the abbrev inside the Str
    to be neither alphanumeric nor `.`. The `.` exclusion is critical: `a.M.D.`
    must NOT match (pandoc rejects because its tokenizer parses the whole thing
    as one Str token, then the result `a.M.D.` isn't in the abbrev set). Don't
    relax to `!is_alphanumeric()` alone.
  - `apply_abbreviations` recurses into `Quoted` because Quoted content is built
    inside `smart_quote_pairs` *after* its source has been coalesced --- the
    parent's abbrev pass won't see Quoted contents. Other inline wrappers
    (Emph/Strong/Link/Image/Note) have their content built via their own
    `coalesce_inlines_*` call, so they get the abbrev pass for free. Don't add
    explicit recursion for those --- it'd run twice.
  - The ordered-list classifier follows pandoc's *parser order*: try decimal
    first, then example, then default, then romanOne, then single-letter alpha,
    then multi-char roman. Critical: `i.` becomes `LowerRoman` (not
    `LowerAlpha`) because `romanOne` runs before `lowerAlpha` in pandoc. Don't
    reorder. Multi-char lowercase non-roman (e.g. `ab.`) won't reach the
    classifier because the parser wouldn't accept it as a list marker --- the
    fallback `Decimal` arm exists only to keep the projector rendering rather
    than panicking on parser-permissive markers.
  - The task-checkbox glyph is `\u{2610}` (BALLOT BOX) for `[ ]` and `\u{2612}`
    (BALLOT BOX WITH X) for `[x]` / `[X]`. Pandoc emits them as a single-char
    `Str` followed by `Space`; do *not* fold the glyph + space into one `Str`
    (`\u{2612} foo`). Pandoc keeps them separate so it can reflow.
  - `expand_tabs_to_4` uses 4-column tab stops measured from column 0 of each
    line. The CST already strips outer container offsets *visually*, but the
    body line text is raw. Don't adjust the starting column --- tabs in source
    columns N still expand based on the real column N, which equals the byte
    column once we're in CODE_CONTENT (the parser doesn't shift content
    columns).
  - `definition_content_offset` returns *columns* (tab-aware), not chars. The
    strip in `indented_code_block_with_extra_strip` operates on tab-*expanded*
    body, so the offset must be in columns to match. Don't switch to
    char-counting; it'll silently over/under-strip on tab-indented definitions.
  - `code_block_raw_format` requires the info string to be exactly `{=fmt}` with
    no spaces, classes, ids, or kvs. If pandoc accepts `{=html .extra}` etc. in
    some future version, this is where to relax --- but probe first; current
    pandoc rejects.
- **Next**: same as before --- **Citations (\~14 remaining)** is the largest
  single-fix unlock but heavy. Smaller leverage targets now:
  - **#114 lists_example** --- needs document-level Example numbering (counter
    across all OrderedList(_, Example, _) in the doc) plus `(@label)` reference
    resolution. The `heading_id_by_offset` pre-pass is the right template.
    Single-case unlock once both pieces land.
  - **#43/#44/#45 definition list** (3 cases) --- multiple parser + projector
    gaps; #44 in particular has the nested-list-inside- definition offset
    propagation issue (LIST has leading WHITESPACE sibling that
    `list_item_content_offset` doesn't see).
  - **Tables (\~13 across simple/headerless/multiline/grid)** --- all still
    gated on parser-shape and projector buildout.
  - **#115 lists_fancy** --- parser too permissive on uppercase markers (`I.`
    with single space accepted as list).
  - **Footnotes #66/#67** --- definition-list-inside-Note parser shape.
  - **HTML block coalescence (#78/#181)** --- parser splits each `<tag>` line
    into separate raw blocks under pandoc; we coalesce.
  - **Misc remaining**: #51 double-backslash math (parser-shape: `\(`/`\[`
    shouldn't trigger inline parsing), #79 ignore_directives, #82
    indented-code-after-heading, #128 nested-headings-in-containers (parser).

## Earlier session (2026-05-01, first)

- **Pass before → after**: 123 → 134 / 187 (+11 imports). All wins are
  **projector-only** again --- no parser code was touched this session.
  CommonMark allowlist stayed green; full parser-crate suite green; clippy + fmt
  clean.
- **What landed (all in `tests/pandoc/native_projector.rs`)**:
  - **Misc small fixes from recap-#9.**
    - **#92 (HTML span attrs).** `<span class="rtl">...</span>` was emitted as
      `Unsupported "BRACKETED_SPAN"`. The parser CST shape was already correct;
      the projector just needed to (a) read `SPAN_ATTRIBUTES` via
      `children_with_tokens()` (it's a *token* for HTML form, but a *node* for
      `[text]{.cls}` form), and (b) parse HTML-style `class="x" id="y" key="z"`
      attributes via a new `parse_html_attrs` helper. `class` splits on
      whitespace.
    - **#29 (autolink scheme allowlist).** `<m:abc>` was projected as a uri
      Link, but pandoc rejects the autolink (scheme `m` is too short / not in
      pandoc's known-schemes set) and falls back to
      `RawInline (Format "html") "<m:abc>"`. Added the full pandoc schemes list
      (sorted, \~280 entries from `pandoc/src/Text/Pandoc/URI.hs`) and an
      `is_known_uri_scheme` check. Anything that isn't email *and* isn't a known
      scheme is now projected as RawInline html.
    - **#41 (all-space inline code).** `strip_inline_code_padding` wasn't
      stripping all-whitespace inline code to empty. Pandoc does (`( )` →
      `Code "" ""`). Added a fast path before the surround-pair-strip arm.
    - **#87/#88 (link dest URL escaping).** `parse_link_dest` was truncating at
      the first space (so `[link](/my uri)` lost `uri`) and not stripping
      angle-bracket wrappers (so `[link](<foo(and(bar)>)` kept the `<...>`).
      Rewrote to (a) strip `<...>` wrapping, (b) split URL/title only when the
      trailing whitespace is followed by `"`/`'`/`(`, (c) percent-escape per
      pandoc's `escapeURI` set: ASCII whitespace + \`<>\|"{}\[\]^\`\`. Backslash
      and Unicode are preserved (pandoc-tested).
  - **Heading-id pre-pass (#167).** `***\n---\n` projects as a setext H2 with
    content `***`, but our slugifier returned `""` (no alphanum), so the id was
    empty. Pandoc's auto-id falls back to `section` and disambiguates duplicates
    against ALL prior auto-generated ids (but explicit `{#x}` ids are kept
    verbatim even on conflict --- pandoc only warns). Replaced
    `fixup_empty_heading_ids` (which only handled bare-marker headings with
    empty *inlines*) with a `RefsCtx` pre-pass that walks every HEADING in
    document order, classifies as explicit/auto via
    `heading_id_with_explicitness`, applies `section`/disambiguation only to
    auto, and stores the final id in `heading_id_by_offset`. `heading_block` now
    consults that map instead of slugifying inline.
  - **Loose-list "blank between blocks of one item" (#105/#107/#158).**
    `is_loose_list` only checked for blanks *between items* and items containing
    a `PARAGRAPH`. It missed CommonMark's other half: a list is also loose if
    any single item directly contains a blank line between two of its
    block-level children. Added `has_internal_blank_between_blocks` --- but with
    a critical caveat surfaced by #61 (regressed mid-session): bare-marker lines
    emit an *empty* PLAIN node (NEWLINE only), and pandoc does *not* count that
    as the "first block". Added `child_is_empty_plain` to skip those. Verified
    vs `-\n\n  foo` (tight, Plain) and `-     bar\n\n  foo` (loose, Para) by
    probing pandoc directly.
  - **List-item content offset for indented code (#107/#106).** Indented code
    blocks inside list items are doubly indented in the CST (item-content offset +
    the 4-space code-block indent). `list_item_blocks` now computes
    `list_item_content_offset` and routes non-fenced code through
    `indented_code_block_with_extra_strip`. The offset rule (verified against
    pandoc): bare-marker line (no WHITESPACE after LIST_MARKER) → offset =
    marker width; marker followed by space(s) → offset = marker_width +
    ws_width.
  - **Definition body content offset for fenced code (#176).** Same shape as
    list items: a fenced code block inside a `: ...` definition has the body's
    indent on each content line. Added `definition_content_offset` and threaded
    it through `definition_blocks`; also generalized
    `indented_code_block_with_extra_strip` to skip `strip_indented_code_indent`
    when fenced (the offset strip is sufficient --- no extra 4-space removal).
- **Cases unlocked** (+11, all allowlisted under `# imported`):
  - 29 (autolink_strict_validation_pandoc) --- known-schemes allowlist
  - 41 (code_spans) --- all-space inline code → empty
  - 87 (inline_link_dest_angle_brackets_with_parens) --- `<...>` strip
  - 88 (inline_link_dest_strict_pandoc) --- space → %20
  - 92 (issue_175_native_span_unicode_panic) --- HTML span attrs
  - 105 (list_item_blank_line_inside) --- internal-blank → loose
  - 106 (list_item_empty_marker_indented_code_next_line) --- bare marker offset
  - 107 (list_item_indented_code) --- list-item code-block strip
  - 158 (sentence_wrap_lazy_continuation) --- fell out from #105 (loose-list
    rule)
  - 167 (setext_text_thematic_break_pandoc) --- `section` fallback
  - 176 (unicode) --- definition fenced-code offset strip
- **Files changed (classified)**:
  - **projector** (single file): `tests/pandoc/native_projector.rs`
  - **allowlist**: `tests/pandoc/allowlist.txt` (+11 imported IDs)
- **Don't redo**:
  - The pandoc URI-scheme allowlist (`PANDOC_KNOWN_SCHEMES`) is a verbatim copy
    of pandoc's `Text.Pandoc.URI.schemes` (sorted alphabetically for
    `binary_search`). When pandoc adds/removes a scheme, refresh this list ---
    but don't try to derive it from `Network.URI` parsing rules. The test for
    "is this a URI autolink?" is *not* RFC3986 conformance; it's "is this scheme
    in pandoc's allowlist?".
  - `parse_html_attrs` is intentionally minimal and does *not* handle
    attribute-value-less keys (e.g. `<input disabled>`). Pandoc's HTML-span
    reader doesn't need them --- adding support would require a different code
    path. Leave it narrow.
  - The percent-escape set in `escape_link_dest` is exactly
    ```isSpace || c \in "<>|\"{}[]^\``" --- copied from pandoc's```escapeURI`. Backslash is *not* in the set, even though it     would be a syntax-significant char in raw URLs. Don't add     backslash without re-probing pandoc ---`[a](foo\\bar)`→`"foo\\bar"\`,
    preserved.
  - The auto-id pre-pass uses `text_range().start()` as the map key (a `u32`
    since rowan's `TextSize` is u32-based). Don't change the key type ---
    explicit `usize` would conflict with rowan's type. Heading offsets are
    unique per document.
  - `child_is_empty_plain` only counts `NEWLINE`/`WHITESPACE` tokens as "empty".
    Don't broaden to count, e.g., comment-only PLAIN nodes --- that's not what
    pandoc considers empty.
  - The list-item content offset is *measured*, not assumed: the bare-marker
    rule (offset = marker_width, no `+1`) is verified against pandoc and matches
    its behavior, contradicting the naive CommonMark §5.2 reading. Don't
    refactor to a "marker width + 1" universal rule.
  - `indented_code_block_with_extra_strip` now branches on `is_fenced` to skip
    the legacy `strip_indented_code_indent` pass when the block is fenced. The
    offset strip alone is sufficient for fenced; layering both produces
    over-strip in nested `:` + ```` ``` ```` cases.
- **Next**: same as before --- **Citations (\~14 remaining)** is the largest
  single-fix unlock but heavy. Smaller leverage targets now:
  - **#43/#44/#45 definition list** (3 cases) --- multiple parser
    - projector gaps; #44 in particular has a fenced code with tabs that lose
      tab-stops in the projector.
  - **Tables (\~18 across simple/headerless/multiline/grid)** --- all still
    gated on parser-shape and projector buildout.
  - **Lists (#113/#114/#115/#116/#117/#118 etc.)** --- fancy/example/ ordered
    styles still need `LowerRoman`/`UpperAlpha`/`OneParen`/ etc. projector
    entries.
  - **Footnotes #66/#67** --- definition-list-inside-Note parser shape.
  - **HTML block coalescence (#78/#181)** --- parser splits each `<tag>` line
    into separate raw blocks under pandoc; we coalesce.
  - **Misc remaining**: #51 double-backslash math (parser-shape: `\(`/`\[`
    shouldn't trigger inline parsing), #79 ignore_directives (block-level
    `<!-- -->` comment is RawBlock in pandoc but our `<!--...-->` inside lists
    projects as RawBlock-with-leading-spaces).
