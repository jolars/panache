# Pandoc-AST projector audit (2026-05-10)

Inventory of every place `crates/panache-parser/src/pandoc_ast.rs`
encodes a structural decision the CST should have encoded itself, plus
the places that are *appropriately* projector-side. Produced before
any Phase 6 code changes so the next session has a ranked pick rather
than guessing scope.

The projector is the public `panache_parser::to_pandoc_ast` API
(also `panache parse --to pandoc-ast`). It is not purely a
test-only diagnostic, but the original critique still stands: the
consumers of structural HTML decisions (linter, salsa, LSP,
formatter) walk the CST, not the projector. Compensation in the
projector is invisible to them.

This audit covers `pandoc_ast.rs` only (5,696 lines as of this
session). Conformance count at audit start: 324/324 total, 132/132
html. No code changed.

--------------------------------------------------------------------------------

## 1. Reparse sites — full-parser re-runs on bytes already in the CST

Calls that take a `&str` carved out of CST text and feed it back
through `crate::parse(...)` (or a sub-parse routine).

### 1a. `parse_pandoc_blocks(text)` — line 1600

Re-runs the full panache parser on a byte slice and returns its
top-level blocks. Builds an inner `RefsCtx` via `mem::take` swap so
heading auto-ids / refs / footnote defs inside the recursive parse
resolve against inner offsets while still inheriting outer
refs/footnotes.

**Callers:**

- `try_div_html_block` line 1587 — `<div>` interior.
- `flush_html_block_text` line 1336 — inter-tag text inside an
  `HTML_BLOCK` body (with Para→Plain demotion).
- `flush_html_block_tail_text` line 1356 — tail text inside an
  `HTML_BLOCK` body (preserves Para).

**Classification: CST gap.**

The parser sees these bytes as raw `HTML_BLOCK_CONTENT` TEXT tokens.
Pandoc parses `<div>` body in a single pass and emits structural
blocks inline; we emit a single TEXT-bearing CONTENT and re-parse
later. Once inner blocks are CST children, the projector becomes a
trivial `collect_block` walk over those children. The `RefsCtx` swap
also goes away (one-pass parse means one resolution pass).

### 1b. `parse_grid_cell_text(text)` — line 2776

Re-runs panache on a grid-table cell's extracted text, then demotes
top-level `Para`→`Plain` (pandoc's grid-table cell rule).

**Caller:** `grid_table` line 2639 (one call per cell).

**Classification: defensible.** Pandoc's grid-table reader
(`Text/Pandoc/Parsing/GridTable.hs`) sub-parses cell content as full
markdown blocks. The grid-layout walker (`find_grid_cell` at line
2697) computes a logical cell text by joining char-grid slices that
do not correspond to a single CST sub-tree (cells span row separators
in ways the parser's TABLE_CELL nodes don't capture). Pandoc itself
does this two-stage layout-then-reparse.

A cleaner shape would have the parser's grid-table parse produce
TABLE_CELL nodes whose content was already CST-structural — but
that's a parser refactor of comparable cost with no obvious downstream
payoff (linter/salsa/LSP don't currently consume grid-cell internal
structure).

### 1c. `parse_cell_text_inlines(text)` — line 3016

Re-runs panache to extract inlines from a text fragment. Used in two
places:

- `multiline_row_cells_blocks` line 3002 — multiline-table cells.
- `parse_cite_affix_inlines` line 3851 — citation prefix / suffix raw
  text (with a `Z ` sentinel wrap to dodge the parser's
  alpha-list-marker recognition of `p. `).

**Classification: half-defensible / half-CST gap.**

- *Multiline tables*: the parser holds raw TEXT for lines past the
  first inside TABLE_CELL — a cleaner shape would have uniform
  inlines across all cell lines. This is a CST gap.
- *Citation affixes*: the CITATION node already has CITATION_CONTENT
  children that are *raw text* (the parser snapshots bytes between
  brackets without inline-parsing them). Pandoc's citeproc reader
  likewise sub-parses affixes — defensible.

### 1d. `parse_html_attrs(s)` — line 1771

Parses HTML-style `key="value"` strings into `Attr`.

**Callers:** `try_div_html_block` line 1565,
`inline_html_span_inline` line 1991, the `bracketed_span_inline`
neighborhood line 2014.

**Classification: defensible.** Pandoc itself parses attributes from
source text at AST emission. The bytes the projector consumes already
live in a structural `HTML_ATTRS` node (Phase 1's invariant). No CST
gap — this is the appropriate place for the key=value parse.

--------------------------------------------------------------------------------

## 2. Byte walkers inside opaque CST nodes

Functions that walk bytes looking for syntax (tags, attributes, depth
balance) the parser already saw at parse time.

### 2a. `split_html_block_by_tags(content, out)` — line 1155

Walks `HTML_BLOCK`'s text bytes, looking for HTML open/close tags via
`parse_open_tag` / `parse_close_tag`, splitting at every block-level
tag boundary, threading `inline_pending` state across the walk.
Re-tokenizes bytes the parser already saw (the parser's
`try_parse_html_block_start` matched the type-6 HTML block; it knows
where the open tag ends).

**Classification: CST gap.** The parser should emit `HTML_BLOCK`
children that the projector can walk: alternating `HTML_BLOCK_TAG`
(one per recognized block tag in the body) and `HTML_BLOCK_CONTENT`
(text chunks between tags). Currently the entire body lives as a
single TEXT-bearing `HTML_BLOCK_CONTENT`, forcing the projector to
redo the tag scan.

**Map to fix:** extend the block parser's HTML state machine to
recognize and emit one `HTML_BLOCK_TAG` per block-level tag inside the
body, keeping intervening bytes inside `HTML_BLOCK_CONTENT` siblings.
The projector then walks children and emits `RawBlock` per tag without
scanning bytes.

### 2b. `try_div_html_block(content)` — line 1546

Re-tokenizes the open tag (`<div ATTRS>`), calls `parse_html_attrs`
on the extracted attribute substring, locates the closing `</div>`
from bytes, calls `parse_pandoc_blocks` on the inner.

Phase 1 retagged the wrapper to `HTML_BLOCK_DIV` and lifted attributes
into `HTML_ATTRS`, but `try_div_html_block` is **still on the path**
(called from `html_block`, `html_div_block`, `emit_html_block`,
`split_html_block_by_tags`).

**Classification: CST gap (partially landed, partially deferred).**
The attribute byte re-scan can already be replaced with an
`AttributeNode::cast` walk on the open `HTML_BLOCK_TAG`'s `HTML_ATTRS`
child (Phase 1 wired this in salsa; the projector hasn't followed).
The inner-content reparse is the bigger structural lift (Phase 6).

**Map to fix (two-stage):**

- *Small, immediate*: rewrite `html_div_block` to walk the structural
  CST children (open `HTML_BLOCK_TAG` → attribute node walk → middle
  children → close `HTML_BLOCK_TAG`), eliminating the byte
  re-tokenize for the open-tag attrs. The inner reparse still falls
  through to `parse_pandoc_blocks` until the medium fix.
- *Medium, follow-up (Phase 6)*: lift inner blocks into structural
  CST children — `HTML_BLOCK_DIV` gets `PARAGRAPH`, `LIST`, etc. as
  direct children. `try_div_html_block` collapses to a `collect_block`
  walk over those children.

### 2c. `flush_html_block_text` / `flush_html_block_tail_text` — lines 1331 / 1352

Re-runs the parser on inter-tag / tail text inside an `HTML_BLOCK`.
Differs only in the Para→Plain demotion rule.

**Classification: CST gap.** Same root as 2a — once `HTML_BLOCK` body
is structurally split into TAG vs CONTENT children, the projector
walks the CONTENT children (which can hold parsed
PARAGRAPH/LIST/etc.) instead of re-running the parser on text. The
two helpers collapse into a single child-walk.

### 2d. `interior_starts_with_void_block_tag(content, interior_start)` — line 1372

Bytes-after-`<video>` peek: skip whitespace/newlines, look for a
`<void-tag>` to decide whether to abandon the matched-pair lift.

**Classification: CST gap (small).** This entire decision is "what's
the kind of the next block-level child after `<video>`?" — trivial if
`<video>` and its interior are children of a structural matched-pair
`INLINE_HTML_BLOCK_PAIR` node. Currently it's a byte peek because the
parser emits a single TEXT-bearing HTML_BLOCK and the projector
reconstructs structure.

### 2e. `find_matching_html_close_with_start` / `find_matching_html_close` — lines 1414 / 1459

Depth-aware byte scans for the matching `</tag>`. Used by
`split_html_block_by_tags` to find the close of a `<div>` or
matched-pair inline-block tag.

**Classification: CST gap.** If the parser did the depth-aware scan
once at parse time and emitted a structural matched-pair wrapper
(`HTML_BLOCK_DIV` is already this for `<div>`; an
`INLINE_HTML_BLOCK_PAIR` would be the analog for
`<video>`/`<button>` etc.), the projector reads child boundaries
trivially.

`HTML_BLOCK_DIV` already has the matched-close baked in at parse
time. `find_matching_html_close` only fires when no `HTML_BLOCK_DIV`
was retagged — i.e. `split_html_block_by_tags`'s recursive sub-scan
looking for a *nested* `<div>` inside a sibling HTML_BLOCK.

### 2f. `extract_html_tag_name(tag_text)`, `is_raw_text_element_open(s)` — lines 1393 / 1500

Lightweight byte slicers: tag-name extraction, raw-text element
detection.

**Classification: helper utilities.** Leaf functions called by other
byte walkers. They go away when their callers go away (via the
structural lifts above).

--------------------------------------------------------------------------------

## 3. Context-dependent decisions made at projection time

Sites that pick a pandoc-AST node based on surrounding context the
parser could record structurally.

### 3a. `inline_pending` flag in `split_html_block_by_tags` — line 1164

Decides per inline-block tag whether to split (fresh-block position)
or pass through (inside running text). Resets on consecutive newlines
(`\n\n`).

**Classification: CST gap.** This *is* a parser decision — pandoc
determines fresh-block-ness during block parsing, not at AST
emission. The CST should encode "this `<video>` started a fresh
block" vs "this `<video>` is mid-inline" via different parse-time
decisions: the inline-block tag at fresh-block is its own block
(`HTML_BLOCK` family); the same tag mid-paragraph is `INLINE_HTML`
inside a `PARAGRAPH`. Currently the parser groups *everything* into
one `HTML_BLOCK` body and the projector re-derives the split.

**Map to fix:** parser-side. The current `cannot_interrupt`
machinery already tracks the dual at top level
(inline-block-mid-paragraph is `INLINE_HTML`, not block-recognized).
The remaining gap is in *multi-tag* HTML blocks where the body
contains sequences like `<form>foo<embed>` — currently one
`HTML_BLOCK`, should be one HTML_BLOCK + Para-with-inline-RawHTML or
similar shape.

### 3b. `close_butted` Plain/Para rule in `try_div_html_block` — line 1585

Decides whether the LAST inner block is `Plain` or `Para` based on
whether the closing `</div>` is butted against content
(`<div>foo</div>` or `<div>foo\n   </div>`) or sits column-0
(`<div>\nfoo\n</div>`).

**Classification: CST gap (small).** Once inner content is lifted
into structural CST children (Phase 6), the parser already knows
which blocks are `PARAGRAPH` vs `PLAIN` (it makes this decision as
part of normal markdown parse). The projector just maps each kind. The
`close_butted` byte peek goes away.

Caveat: pandoc's own rule here is at AST-emission time, but it's a
one-block decision — the entire `<div>` body parses uniformly as Para
under pandoc's markdown reader, then the renderer demotes the
trailing Para to Plain when adjacent to the close tag. So this *might*
be defensible, but it's still avoidable.

### 3c. Para→Plain demotion at top-level HTML-block boundary

Pandoc emits `[Plain[foo], RawBlock<p>]`, `[Plain[foo],
RawBlock</p>]`, `[Plain[foo], Div(...)]` when a strict-block /
verbatim HTML construct (open OR close direction) follows the
paragraph immediately. Panache emits `[Para[foo], …]`. **A 2026-05-10
attempt put this in the projector and was reverted** — the fix
belongs in the parser.

**Classification: CST gap.** Belongs in the parser as a
`PARAGRAPH → PLAIN` retag. The CST already has both kinds (`PLAIN`
is a real SyntaxKind used by lists); the projector trivially maps
each.

**Map to fix:** in the block parser, when terminating a paragraph
because the next line begins an HTML strict-block / verbatim block,
emit `PLAIN` instead of `PARAGRAPH`. Small, focused, single-site
change. Risk surface: formatter idempotency must round-trip
`PLAIN`-followed-by-HTML-block correctly.

### 3d. Demote Para→Plain in `flush_html_block_text` (vs preserve in `flush_html_block_tail_text`) — line 1340

Same root as 3c, but at inter-tag boundaries inside an `HTML_BLOCK`
body. The split between `_text` (demotes) and `_tail_text`
(preserves) is purely positional context.

**Classification: CST gap.** Once the parser splits the HTML_BLOCK
body into structural children (2a), each chunk is just a `PARAGRAPH`
whose adjacency is visible from the CST. Same retag fix as 3c, applied
to bodies of HTML blocks.

### 3e. `emit_citation_with_absorb` cross-CST sibling absorb — line 3450

Bare `@key` followed by `[locator]` (with `LINK` or
`UNRESOLVED_REFERENCE` shape) gets absorbed into a single `Cite` with
the locator as suffix.

**Classification: defensible.** Pandoc's citation reader does the
same absorb at parse time; ours does it at projection time. The CST
keeps both nodes intact (CITATION + LINK), which is good for
cross-tool consumers (linter can flag a malformed locator
separately). Could become a parser-side `CITE_WITH_SUFFIX` kind, but
blast radius is high and benefit is low.

### 3f. `emit_latex_command_with_absorb` trailing-space absorb — line 3513

`\foo bar` → `RawInline tex "\\foo "` + `Str "bar"`; `\frac{a}{b}
bar` keeps the space outside.

**Classification: defensible.** Pandoc's tex inline reader does the
same absorb. Small scope, no downstream consumer cares.

### 3g. `coalesce_inlines` / `smart_quote_pairs` / `apply_abbreviations` — lines 4424 / 4561 / 4526

`coalesce_inlines` collapses runs of Str/Space/SoftBreak;
`smart_quote_pairs` builds `Quoted` nodes from straight-quote pairs;
`apply_abbreviations` swaps Space for NBSP after known abbrevs.

**Classification: defensible.** Pandoc applies these at AST
construction. CST keeps the raw straight quotes / individual Str
tokens / regular Space tokens, which is correct for losslessness and
lets the formatter round-trip. The projector applies the same
transforms pandoc would.

### 3h. `autolink_inline` URI scheme classification — line 4213

Decides URI autolink vs email autolink vs raw HTML based on body
content + scheme allowlist.

**Classification: defensible (borderline).** Could be split into
`AUTOLINK_URI` / `AUTOLINK_EMAIL` CST kinds at parse time, but the
parser would need the scheme allowlist too. Current split keeps the
parser scheme-agnostic. Borderline because consumers (e.g. a linter
rule that flags broken autolinks) might want to know.

### 3i. List loose/tight + Para/Plain item content — lines 3249 / 3351

In `list_item_blocks`: if `loose`, emit `Para`; if tight, emit
`Plain`. Classification computed in `is_loose_list` (line 3351) which
walks list children.

**Classification: defensible.** Pandoc computes loose/tight from the
same surface signals (blank lines between items / inside items).
Could be a CST flag on `LIST` set at parse time, but the formatter's
tight/loose round-trip already depends on the existing CST shape.
Status quo is fine.

### 3j. `figure_block` `implicit_figures` lift — line 804

PARAGRAPH containing only an IMAGE_LINK is lifted to `Block::Figure`
at projection time.

**Classification: defensible.** Pandoc's `implicit_figures` extension
makes this decision at the AST layer too. Keeping it in the projector
means the formatter's CST round-trip stays simple.

--------------------------------------------------------------------------------

## 4. Defensible projector logic (do NOT remove)

Computations the projector should keep doing, because pandoc itself
does the same at AST emission rather than at parse time, OR because
moving them parser-side has no downstream payoff.

- **Table cell sub-parses**: `parse_grid_cell_text`,
  `parse_cell_text_inlines` (multiline-table arm).
- **Attribute parsing from extracted source text**: `parse_html_attrs`,
  `parse_attr_block`, `parse_div_info`, `extract_attr_from_node`.
- **Inline coalescing & smart transforms**: `coalesce_inlines`,
  `coalesce_inlines_keep_edges`, `smart_quote_pairs`,
  `apply_abbreviations`, `smart_intraword_apostrophe`,
  `smart_dashes_and_ellipsis`.
- **Adjacent-element absorbs**: `emit_citation_with_absorb`,
  `emit_latex_command_with_absorb`.
- **Tight/loose list classification**: `is_loose_list`,
  `has_internal_blank_between_blocks` and the Para/Plain choice in
  `list_item_blocks`.
- **Auto-id / heading-id resolution via `RefsCtx`**: `build_refs_ctx`,
  `build_refs_ctx_inherited`, `collect_refs_and_headings`,
  `heading_id_with_explicitness`, `pandoc_slugify`. Canonical place
  for document-wide auto-id with disambiguation.
- **`figure_block` `implicit_figures` lift**.
- **Code-block info parsing**: `code_block_attr`,
  `code_block_raw_format`, `normalize_lang_id`.
- **List marker classification**: `ordered_list_attrs`,
  `classify_ordered_marker`, `roman_to_int`, `task_checkbox_for_item`.
- **Table separator math**: `pipe_separator_aligns`,
  `grid_separator_aligns`, `grid_dash_widths`, `grid_segment_align`,
  `simple_table_aligns`, `simple_table_dash_runs`.
- **Code-content tab/indent normalization**: `expand_tabs_to_4`,
  `strip_indented_code_indent`, `indented_code_block_with_extra_strip`,
  `strip_leading_spaces_per_line`. CST keeps source indentation
  (correct for losslessness); projector strips for AST emission.
- **List-item content offset arithmetic**: `list_item_content_offset`,
  `parent_list_leading_ws` (for stripping nested code-block indent).
- **Citation builder**: `CitationBuilder`, `parse_cite_affix_inlines`,
  `literal_inlines`.
- **URI scheme allowlist**: `is_known_uri_scheme`,
  `PANDOC_KNOWN_SCHEMES`.

--------------------------------------------------------------------------------

## 5. Findings → ranked parser-side fixes

| # | Fix | Size | Leverage | Blast radius |
|---|-----|------|----------|--------------|
| 1 | `PARAGRAPH→PLAIN` retag at top-level HTML strict-block / verbatim adjacency (3c) | Small | Unblocks several conformance cases; collapses two flush helpers (`_text` vs `_tail_text`) when later applied within HTML body too | Formatter must round-trip `PLAIN`+HTML-block (likely already works — PLAIN is a real kind already used by lists). |
| 2 | Walk structural CST in `html_div_block` for the open-tag attrs (eliminate one of `try_div_html_block`'s two byte walks) (2b small fix) | Small | Pure projector simplification on top of Phase 1's structural shape; sets the precedent for fix #3. | None — pure projector cleanup. **LANDED 2026-05-10** — `html_div_block` reads attrs from `cst_div_open_tag_attr` (`HTML_BLOCK_TAG → HTML_ATTRS` walk); inner-content + `close_butted` shared via `extract_div_inner_and_butted` + `assemble_div_block` helpers. `try_div_html_block` (byte-only) kept for `HTML_BLOCK` callers that lack a structural HTML_ATTRS lift. |
| 3 | Lift `<div>` (`HTML_BLOCK_DIV`) inner block content into structural CST children — direct PARAGRAPH/LIST/etc. children (1a, 2b medium, 2c, 3b) | Medium | Collapses `try_div_html_block`'s inner reparse + Plain/Para `close_butted` rule + cross-boundary `RefsCtx` swap. Linter/salsa/LSP can finally see structural children of a `<div>`. | Touches block dispatcher: when entering an HTML_BLOCK_DIV body, parse contents as nested markdown rather than capturing as TEXT. Formatter round-trip is the risk surface. |
| 4 | Split `HTML_BLOCK` body into alternating TAG / CONTENT structural children (2a, 2c, 2d, 2e, 2f, 3a, 3d) | Medium-large | Eliminates `split_html_block_by_tags`, both flush helpers, `interior_starts_with_void_block_tag`, `find_matching_html_close*`, `extract_html_tag_name`, `is_raw_text_element_open`, the `inline_pending` flag, all in one swing. Projector becomes a child walk. | Largest blast radius — the parser's current HTML_BLOCK shape is byte-for-byte text in CONTENT; structural split changes how everything from the formatter to the linter reads HTML blocks. |
| 5 | Multi-line HTML open-tag structural lift cleanup (mostly already landed for `<div>` and void elements; tighten remaining gaps) | Small | Removes a couple of fallback paths in `html_blocks.rs`. | Low. |

The `inline_pending` semantics in #4 specifically map to the parser's
`cannot_interrupt` machinery: where the projector currently
re-derives the split, the parser would emit either a fresh HTML_BLOCK
for the inline-block tag at column 0, or leave the tag inside an
INLINE_HTML inside a PARAGRAPH when mid-text. The current architecture
already has both kinds — what's missing is using them inside an
*existing* HTML_BLOCK body rather than treating that body as opaque
text.

--------------------------------------------------------------------------------

## 6. Recommended sequencing

1. **#1 (PARAGRAPH→PLAIN retag).** Smallest, highest-leverage
   parser-side fix. Replicates the (reverted) projector demotion as
   a parser decision. Adds 1–6 conformance cases and removes the
   largest "obviously needs to be parser-side" item from the
   projector.
   - Fixture-first: paired parser golden for `foo\n<p>bar</p>` and
     `foo\n</p>` (Pandoc) — pin PARAGRAPH→PLAIN when the next sibling
     is HTML_BLOCK with a strict-block/verbatim opener-or-closer.
   - Parser change: at paragraph termination in
     `block_dispatcher::detect_prepared`, when the terminating block
     is an HTML_BLOCK (any of the strict-block / verbatim categories
     that can interrupt running paragraphs), retag the just-emitted
     PARAGRAPH as PLAIN.
   - Projector change: trivial — `block_from` already maps PLAIN to
     `Block::Plain`. Remove any nascent compensation if present.
   - Formatter check: golden round-trip for `foo\n<p>bar</p>` and
     similar.

2. **#2 (`html_div_block` walks structural CST for the open tag).**
   Pure projector cleanup on top of Phase 1's structure. No
   conformance impact (already passes), but removes the byte
   re-tokenize that the audit flagged. Sets the precedent for #3
   (the inner-content lift).

3. **#3 (lift `<div>` inner blocks into structural CST children).**
   Phase 6 proper. Larger lift — start with a focused case-set
   (`<div>foo</div>`, `<div>\n# heading\n</div>`,
   `<div>\n- list\n</div>`) and pin parser fixtures + formatter
   goldens before touching the projector.

4. **Defer #4 (full HTML_BLOCK structural split) until #3 lands and
   the pattern is proven.** It's the largest blast radius and the
   leverage is huge — but doing it before validating the pattern on
   `<div>` first risks a costly stall.
