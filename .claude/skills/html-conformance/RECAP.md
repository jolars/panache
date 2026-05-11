# HTML conformance — running session recap

Rolling, terse handoff between sessions of the `html-conformance`
skill. Read at the start of a session for phase status, persistent
traps, and the latest session's "Suggested next sub-targets". At the
end of a session, **rewrite** the Latest session entry, add a
one-line entry to the Earlier sessions log, and merge any
still-relevant trap into the Persistent traps section. Keep the file
short — see `SKILL.md`'s "Session recap" section for length budget.

--------------------------------------------------------------------------------

## Persistent traps & invariants (cross-session)

These survive across sessions. Add to this list when a trap is
re-relevant (i.e. you'd warn a future session about it); fold it
back into a session entry only if it's purely historical.

### Disk + tooling

- **Disk lint cache at `~/.cache/panache/`** serves stale
  `undefined-anchor` (and other linter rule) results even after
  `cargo build`. Symptoms: unit tests pass, `panache lint` keeps
  emitting old diagnostics, `eprintln!` from changed code never
  fires. Fix: `rm -rf ~/.cache/panache/` (or
  `cache.enabled = false` in `panache.toml`). Validate via unit
  tests first; treat CLI as downstream.
- **Conformance comparison is whitespace-insensitive**:
  `normalize_native` collapses pandoc's pretty-printed multi-line
  block output to single-line. Visual diffs are misleading.

### Parser shape & losslessness

- **HTML_ATTRS is the structural pattern; never add synthetic
  tokens.** Expose attributes by tokenizing existing source bytes at
  finer granularity (split TEXT into
  `TEXT + WHITESPACE + HTML_ATTRS{TEXT} + TEXT`). Synthetic tokens
  break the tree-text-equals-input invariant.
- **Use source-byte slices, never literal strings, when emitting
  TEXT tokens** for HTML. `"<div"` literal vs `&rest[..4]` was the
  root of the `<DIV>` losslessness regression. Case-insensitive
  prefix matches give a false sense of byte-identity.
- **Same-line `<div>foo</div>` is ONE `HTML_BLOCK_TAG`**, not open
  + content + close. The close `</div>` lives inside a TEXT child
  of the open tag. Any naive `strip_suffix('>')` grabs the wrong
  `>`. Scan to the first **unquoted** `>` (see
  `parse_html_tag_attributes`).
- **Quoted attribute values can hide `<` and `>`.** Tag-bracket
  scanners must thread quote state across line boundaries; don't
  reset per-line. `count_tag_balance`, `find_multiline_open_end`,
  `pandoc_html_open_tag_closes` do this right.
- **Multi-line open-tag close branches diverge by tag class** —
  void-tag multi-line opens get an early-exit returning
  `end_line_idx + 1` BEFORE the close-marker loop (no `</tag>` to
  find). `same_line_closed` short-circuit must guard
  `multiline_open_end.is_none()`.
- **Incomplete open tags (`<embed\n`, `<div\n`, no `>` anywhere)
  caused projector infinite recursion.** Pandoc-native treats them
  as paragraph text. Fix: gate Pandoc BlockTag recognition on
  `pandoc_html_open_tag_closes(lines, line_pos, bq_depth)` in
  `block_dispatcher.rs::detect_prepared`. CommonMark stays liberal
  — `<table\n` is a valid CM type-6 RawBlock.
- **Self-closing `<tag/>` doesn't bump depth.** Depth-aware close
  matchers must check `bytes[j-1] == b'/'` at the closing `>`.
- **`input.lines()` strips newlines**; for losslessness-asserting
  parser tests use
  `crate::parser::utils::helpers::split_lines_inclusive` to build
  `lines: Vec<&str>`.
- **`HtmlBlockType::BlockTag` is `Box<dyn Any>`-roundtripped via
  the block dispatcher.** Adding a field works automatically;
  cargo's E0063 errors point at every literal site that needs
  updating.

### Pandoc tag categorization

- **Pandoc has THREE tag sets, not one**: strict block
  (`PANDOC_BLOCK_TAGS`), inline-block non-void
  (`PANDOC_INLINE_BLOCK_TAGS`), inline-block void
  (`PANDOC_VOID_BLOCK_TAGS`). Each requires distinct handling — the
  strict set always splits, the non-void set follows
  `inline_pending` and lifts as matched-pair, the void set follows
  `inline_pending` and emits a single RawBlock per instance. Source
  of truth: `pandoc/src/Text/Pandoc/Readers/HTML/TagCategories.hs`
  + `Readers/HTML.hs::isBlockTag`/`isInlineTag`.
- **`eitherBlockOrInline` is context-dependent.** Mirroring needs
  BOTH parser-side `cannot_interrupt` (don't break running paragraph)
  AND projector-side `inline_pending` tracking (don't split mid-text).
  Either alone is insufficient.
- **CommonMark and Pandoc `blockHtmlTags` lists differ in BOTH
  directions** by ~15 tags. Don't merge them. The parser's
  `is_commonmark` flag gates which list runs; the projector only
  runs under Pandoc and uses `is_pandoc_block_tag_name` directly.
- **Closing forms of strict-block, verbatim, inline-block, and void
  tags ALL ARE block starts under Pandoc** (`htmlBlock isBlockTag`
  matches both directions for `blockHtmlTags ∪ verbatimTags ∪
  eitherBlockOrInline`). Each emits `BlockTag { closes_at_open_tag:
  true }`. Dispatcher's `cannot_interrupt` keys on inline-block +
  void names only — strict-block and verbatim closes get
  `YesCanInterrupt` (matches pandoc); inline-block / void closes
  stay inline inside running paragraphs.
- **Verbatim tags (`<pre>`/`<script>`/`<style>`/`<textarea>`) fire
  before inline-block / strict-block arms** — script membership in
  `eitherBlockOrInline` and style/textarea in `blockHtmlTags` is
  harmless because `VERBATIM_TAGS` matches first.
- **Pandoc `isInlineTag` special cases (issue #10643):** `<style>`
  (open+close), `</script>`, PIs, comments, and `<script
  type="math/tex…">` (case-insensitive, single-line opens only)
  cannot interrupt a paragraph. `<pre>` / non-math-tex `<script>`
  open / `<textarea>` DO interrupt. Implemented in
  `HtmlBlockParser::detect_prepared`'s `cannot_interrupt`. Requires
  `is_closing: bool` field on `HtmlBlockType::BlockTag`.

### Projector tag splitting

- **`split_html_block_by_tags` walks bytes, not tokens.** It is
  context-tracked via `inline_pending`; runs for opaque HTML_BLOCKs
  only (comments, PI, verbatim, void tags, unmatched strict /
  inline-block tags). Matched-pair `<div>` is parser-lifted now.
- **Matched-pair lift for `<video>...</video>` must abandon when
  interior opens with a void block tag at column 0** (pandoc emits
  per-tag, not a balanced lift). Helper
  `interior_starts_with_void_block_tag` / `inline_block_void_interior_abandons`
  peeks past leading newlines/whitespace; indentation doesn't save
  the lift. Inline-block open with no matched close must ALSO emit
  as RawBlock — falling through to `inline_pending=true` causes
  stack overflow via trailing tail-text reparse recursion.
- **`inline_pending` resets on consecutive newlines (≥ 2);
  inter-tag text demotes Para→Plain when butted against next tag;
  tail text does NOT demote.** Use `flush_html_block_text` vs
  `flush_html_block_tail_text` correctly — uniform demotion breaks
  `<form>\nfoo\n` and `<embed> trailing` shapes.
- **HTML blocks inside blockquotes need `collect_html_block_text_skip_bq_markers`
  on remaining byte-walker paths.** Parser keeps `BLOCK_QUOTE_MARKER
  + WHITESPACE` as structural tokens; passing `node.text()` to
  `split_html_block_by_tags` / `parse_pandoc_blocks` re-recognizes
  `> ` as nested bq. Most paths now route through the structural
  lift; the remaining caller is `emit_html_block` (for verbatim
  tags inside bq, e.g. `<pre>` in a `>` block).
- **Projector `open_tag_raw_block_text` canonicalizes multi-line
  open tags.** When `HTML_ATTRS` are present, the literal source
  (`<form\n  id="x"\n  class="y">`) diverges from pandoc-native's
  canonical single-line form. `normalize_native` preserves
  whitespace inside `"..."` so the divergence is visible. Helper
  walks `children_with_tokens`, takes leading `<tagname` TEXT,
  joins `HTML_ATTRS` trimmed texts with single spaces, appends
  `>`. Single-line opens without HTML_ATTRS keep their literal
  text. Don't substitute `node.text()` here.

### Refs / footnotes / heading-id resolution

- **`parse_pandoc_blocks` swaps in an inner `RefsCtx`** for the
  recursive `<div>` reparse (and any other call site). The swap
  belongs in `parse_pandoc_blocks` itself, not at call sites.
- **`build_refs_ctx` mutates `REFS_CTX` mid-build** (stages
  cite-num/example-num maps before the heading pre-pass). When
  swapping for an inner reparse, save outer FIRST (`mem::take`),
  THEN call `build_refs_ctx`, THEN install the result.
- **`heading_id_by_offset` is offset-keyed, not slug-keyed.** The
  inner CST's offsets are zero-based and don't intersect the
  outer's offset space. Tempting wrong fix: copy outer
  `heading_ids` into inner. Right fix: build a fresh inner ctx and
  optionally inherit cross-boundary refs/footnotes via
  `build_refs_ctx_inherited`.
- **`fenced_div` does NOT use `parse_pandoc_blocks`** — it walks
  the structural CST via `collect_block`. Fenced divs already
  resolve through the outer ctx; don't generalize the swap to
  fenced divs.
- **`AttributeNode::can_cast` accepts `HTML_ATTRS`**; the existing
  salsa walk picks up `<div id>` / `<span id>` and (since
  2026-05-11) non-div strict-block tag ids (`<section id="x">`,
  `<form id="x">`, `<p id="x">`, etc.) automatically, both outside
  and inside `>` quotes (single-line opens; multi-line-inside-bq
  still TEXT). Diverges from pandoc-native (which keeps them as
  RawBlock without lifting attrs) but matches user intent for
  anchor-link resolution. No parallel salsa walk for HTML attrs.

### Out of scope / known divergences

- **`<!ENTITY x "y">` projects `Str "\"y\">"`** where pandoc emits
  `Quoted DoubleQuote [Str "y"]`. Smart-quote / Quoted feature
  gap; not html-conformance.
- **Outer-wins-over-inner ref-conflict**: pandoc's rule is
  document-order-first; we have inner-wins. No corpus exercises
  this; deferred.
- **Cross-boundary cite numbering** for `<div>` recursive reparse
  similarly deferred.
- **Top-level Para→Plain demotion at HTML strict-block / verbatim
  adjacency** is parser-side
  (`Parser::close_paragraph_as_plain_if_open` +
  `html_block_demotes_paragraph_to_plain`, wired at
  YesCanInterrupt in `core.rs`). CST emits `PLAIN`; projector
  trivially maps. Don't reintroduce projector-side demotion.

### Projector-as-second-stage-parser smell (architectural)

`pandoc_ast.rs` is the public `panache_parser::to_pandoc_ast` API;
linter / salsa / LSP / formatter walk the CST, not the projector.
Phases 1/5 landed structural retags (`HTML_BLOCK_DIV`,
`INLINE_HTML_SPAN`); Phase 6 lifted inner content of all non-bq
`<div>` / non-div strict-block / inline-block matched-pair shapes
AND all bq shapes (clean, same-line, messy) of those tags into
CST children. **The vestigial `<div>` byte walkers
(`try_div_html_block`, `parse_div_open_tag_attrs_from_bytes`,
`extract_div_inner_and_butted`, `assemble_div_block`,
`find_matching_html_close`, the matched-pair-div branch of
`split_html_block_by_tags`, and the `html_div_block` byte
fallback) were pruned 2026-05-11.** What remains is genuinely
load-bearing: the splitter (`split_html_block_by_tags` for opaque
HTML_BLOCKs — comments, PI, verbatim, void tags, unmatched
strict-block tags), `parse_pandoc_blocks` (called from
`flush_html_block_text` / `flush_html_block_tail_text` for
inter-tag text reparse), `collect_html_block_text_skip_bq_markers`
(needed by the one `<pre>` verbatim-inside-bq case +
multi-line-open-inside-bq fallback), table-cell reparses via
`parse_grid_cell_text` / `parse_cell_text_inlines`. `html_div_block`
now `debug_assert!`s on an unlifted HTML_BLOCK_DIV — that would be
a parser bug.

### Structural lift (Fix #3 / Fix #4 family)

- **Recursive parse uses `parse_with_refdefs`, not `parse`.** When
  doing an inner recursive parse for a structural lift, call
  `crate::parser::parse_with_refdefs(inner_text, opts, outer_refdefs)`
  (or thread the outer config's `refdef_labels` through). `parse`
  re-runs `populate_refdef_labels` on JUST the inner text, hiding
  outer refdefs from inner reference links.
- **Lifted HTML_BLOCK / HTML_BLOCK_DIV MUST route to the structural
  walk, never the byte path.** `collect_block` routes
  `HTML_BLOCK_DIV` to `html_div_block` (not `emit_html_block`);
  `emit_html_block` internally routes lifted HTML_BLOCKs to
  `emit_html_block_structural` (not `split_html_block_by_tags`).
  The byte path's `parse_pandoc_blocks` reparse builds a fresh
  inner `RefsCtx` and re-disambiguates heading auto-ids — running
  it on a body whose headings ALREADY participate in the outer
  ctx's disambiguation produces `heading-1`/`subheading-1`
  instead of `heading`/`subheading`. Symptom: stray `-1` suffix
  on inner heading ids in pandoc-ast output.
- **Body-lifted signal is "no `HTML_BLOCK_CONTENT` child"**
  (covers div + non-div + matched-pair). `div_has_structural_inner`
  / `html_block_has_structural_lift` require exactly two
  `HTML_BLOCK_TAG` children, both clean, no `HTML_BLOCK_CONTENT`.
  Empty / blank-only bodies count as lifted.
  `html_block_open_tag_is_clean` accepts "TEXT ends in `>`" (covers
  both split-`>` and whole-line emissions); trailing content
  produces a TEXT NOT ending in `>` and correctly fails.
- **`LastParaDemote` enum** on `graft_document_children`:
  `Never` (clean / unbalanced — Para preserved), `SkipTrailingBlanks`
  (div close-butted shapes — demote LAST PARAGRAPH past trailing
  BLANK_LINEs), `OnlyIfLast` (non-div strict-block close — demote
  only when last child is PARAGRAPH with no trailing BLANK_LINE).
- **Multi-line open tags emit multiple `HTML_ATTRS` regions** —
  one per attribute line. Helpers reading via `.children().find()`
  see only the FIRST; iterate and join with `" "`
  (`cst_div_open_tag_attr`).
- **All non-bq `<div>` shapes lift** (clean multi-line, open-
  trailing, butted-close, indented-close, same-line, empty /
  blank-only) and as of 2026-05-11 all non-bq shapes for non-div
  strict-block + inline-block matched-pair tags lift too.
- **Parser-side structural lift inside blockquote covers clean +
  same-line + messy shapes** (all three gates documented below).
  Open-line `> ` is consumed by outer BLOCK_QUOTE; subsequent
  source lines' `> ` are re-injected into the grafted CST via
  `BqPrefixState`. Deeper bq (`> > <div>`) works transparently —
  prefix capture is depth-agnostic. Multi-line open tag inside bq
  still falls back to opaque per-line TEXT
  (`multiline_open_end` gated on `bq_depth == 0`).
- **Bq prefix re-injection: both `NEWLINE` and the `BLANK_LINE`
  *token* (kind, not node) advance `line_idx`.** The inner parse
  puts a `BLANK_LINE` token (text `"\n"`) inside a `BLANK_LINE`
  node; treating only `NEWLINE` as a line-end mis-aligns prefixes
  for any body containing a blank line — losslessness violation
  that doesn't surface until `>` (blank) precedes a content line.
- **Three bq lift gates by `depth` after open line.** All three
  require `bq_depth > 0` + `multiline_open_end.is_none()` +
  `depth_aware_tag.is_some()` and accept HTML_BLOCK_DIV or
  HTML_BLOCK with tag in `is_pandoc_lift_eligible_block_tag`.
  Inline-block matched-pair additionally gates on NOT
  `inline_block_void_interior_abandons`. The discriminator is
  the depth state plus shape:
  - `same_line_bq_lift_tag` — `depth <= 0` after open (open
    balances). Routes through the `same_line_closed` branch;
    uses `emit_html_block_body_lifted` with `bq: &mut None`
    (body has no inner newlines). Demote: div =
    SkipTrailingBlanks, non-div / matched-pair = OnlyIfLast.
  - `bq_clean_lift` — `depth > 0` after open + close line
    `trim_start…starts_with("</")` (clean close) +
    `pre_content.is_empty()` (clean open). Close-marker site
    calls `emit_html_block_body_lifted_bq` with `BqPrefixState`
    built from each content line's captured prefix. Demote: div
    = Never (Para preserved), non-div / matched-pair = OnlyIfLast.
  - `bq_messy_lift_tag` — `depth > 0` after open + NOT clean
    (open-trailing or butted-close or both). Open-tag emission
    lifts trailing into `pre_content`; close-marker site
    bq-STRIPS the close line then `try_split_close_line` →
    `(leading, close_part)`. Calls
    `emit_html_block_body_lifted_bq_messy` with prefixes vec
    [empty for pre_content, content-line prefixes,
    close-line-prefix for leading]. Demote: div is keyed on
    close-butted-ness (Never when leading empty,
    SkipTrailingBlanks otherwise), non-div / matched-pair =
    OnlyIfLast.
- **Bq messy-lift duplicate-prefix trap.**
  `emit_html_block_body_lifted_bq_messy` injects the close
  line's bq prefix in front of `leading` via BqPrefixState — so
  the close `HTML_BLOCK_TAG` MUST NOT re-emit
  `emit_bq_prefix_tokens(close_prefix)` when `leading` is
  non-empty (doubles the `> ` bytes; surfaces as `+2 byte`
  losslessness mismatch). Only emit before close tag when
  `leading.is_empty()`.
- **Projector `open_tag_raw_block_text` strips bq markers.** Bq-
  wrapped close tags (`> </form>`) carry `BLOCK_QUOTE_MARKER +
  WHITESPACE` as leading tokens inside the close `HTML_BLOCK_TAG`
  for losslessness. Pandoc-native's `RawBlock` text is the tag
  bytes only — the helper walks tokens skipping each
  `BLOCK_QUOTE_MARKER` plus the immediately-following
  `WHITESPACE`. Without this, lifted bq RawBlock emissions render
  as `"> </form>"` instead of `"</form>"`. The HTML_ATTRS branch
  (multi-line open canonicalization) is unaffected — those opens
  don't have bq prefix tokens since they appear outside bq today.

--------------------------------------------------------------------------------

## Phase progress

| Phase | Description | Status |
|-------|-------------|--------|
| 1 | `<div>` block lift (HTML_BLOCK_DIV + HTML_ATTRS structural) | **Wrapper retag landed** (2026-05-08) — issue #263 closed; `<DIV>` losslessness fix landed. **Inner content NOT yet lifted into CST children** — still raw `HTML_BLOCK_CONTENT` TEXT tokens; projector reparses them. |
| 2 | `<span>` inline lift (INLINE_HTML_SPAN) | **Wrapper retag landed** (2026-05-08). Inner inlines mostly trivial (no recursive reparse needed). |
| 3 | Sectioning + verbatim corpus pin; `eitherBlockOrInline` lift | **Conformance landed** — non-void (2026-05-09); void (`<embed>`/`<area>`/`<source>`/`<track>`) (2026-05-10). Implementation leans on projector-side `inline_pending` tracking + byte walker; CST still opaque for split/matched-pair shapes. |
| 4 | Comments, PIs, declarations, CDATA projection | **Conformance landed** (2026-05-08); type-4 CM lowercase still gappy. CST opaque (these constructs project as RawBlock / RawInline). |
| 5 | `markdown_in_html_blocks` interaction edge cases | **Conformance landed** — depth-aware nested div, Plain/Para promotion, refs inheritance, **projector-level splitter** (`split_html_block_by_tags` byte walker + `parse_pandoc_blocks` recursive reparse), outer-matched-pair-abandons-on-void-interior. **The structural CST lift was deferred** — Phase 5's mechanism is the projector reparsing bytes, not the parser emitting structure. |
| 6 (new) | Lift inner HTML block content into structural CST children — `HTML_BLOCK_DIV` / `HTML_BLOCK` get `PARAGRAPH` / `LIST` / etc. as direct children; projector byte walkers become vestigial; `PARAGRAPH→PLAIN` retag at adjacent-HTML-block boundary. | **All shapes lifted as of 2026-05-11** for `<div>`, non-div Pandoc strict-block tags, and inline-block matched-pair tags. Non-bq shapes: clean multi-line, open-trailing, butted-close, indented-close, same-line, empty / blank-only, multi-line open (where applicable). Inline-block matched-pair abandons when body begins with a void block tag (Plain via OnlyIfLast). Bq shapes via three gates by `depth` after open line: clean (`bq_clean_lift`), same-line (`same_line_bq_lift_tag`), messy (`bq_messy_lift_tag`); `BqPrefixState` re-injects per-line bq markers. Multi-line open inside bq still falls back to opaque per-line TEXT (`multiline_open_end` gated on `bq_depth == 0`). **2026-05-11 cleanup**: vestigial `<div>` byte walkers pruned from `pandoc_ast.rs` (-170 net lines); `html_div_block` `debug_assert!`s on unlifted input. Pass count: 132 → 159. |

--------------------------------------------------------------------------------

## Latest session — 2026-05-11 (Phase 6 cleanup — prune vestigial `<div>` byte walkers)

Removed dead `<div>` projector byte walkers in `pandoc_ast.rs`. The
audit instrumented each candidate fallback with `eprintln!` markers
and ran the full workspace; zero hits on `html_block` singular's
`try_div_html_block` check, on `html_div_block`'s byte fallback, or
on `split_html_block_by_tags`'s matched-pair-div branch. The one
case still exercising bq-marker stripping is the `<pre>`-inside-bq
verbatim fixture (`0339-html-block-pre-verbatim-inside-blockquote`).

Conformance + workspace stable: 159 html / 352 total, all green.
Pure deletion — no behavior change. Single-file diff:
`crates/panache-parser/src/pandoc_ast.rs`, `-207/+37` (~170 net lines
removed).

### What landed

- Deleted `try_div_html_block`, `parse_div_open_tag_attrs_from_bytes`,
  `extract_div_inner_and_butted`, `assemble_div_block`,
  `find_matching_html_close`.
- Deleted `html_div_block`'s byte fallback (the
  `collect_html_block_text_skip_bq_markers` + `extract_div...` +
  `assemble_div_block` chain); replaced with `debug_assert!` +
  `Block::Div(attr, Vec::new())` defensive return — an unlifted
  `HTML_BLOCK_DIV` would be a parser bug.
- Deleted the matched-pair-`<div>` branch inside
  `split_html_block_by_tags` (lines that did
  `find_matching_html_close("div")` then `try_div_html_block` for
  the matched span). Strict-block tags inside an opaque HTML_BLOCK
  now uniformly emit per-tag RawBlocks.
- Simplified `html_block` (singular, used by grid-cell reparse) to
  emit a single `RawBlock` — its `try_div_html_block` check was
  dead.
- Updated doc comments on `collect_block` and `emit_html_block` to
  drop references to the removed byte paths.

### Suggested next sub-targets

1. **Multi-line open tag inside bq**. `multiline_open_end` is gated
   on `bq_depth == 0`, so `> <section\n>   id="x">\n` still falls
   back to opaque per-line TEXT. Rare in practice; defer unless a
   real corpus / linter case demands it. If pursued, the lift
   would need a threaded bq-aware multi-line opener — non-trivial.
2. **Formatter goldens for bq messy shapes** (deferred from Fix #8).
   Probe first — likely unchanged since the rendered text is the
   same, but worth pinning idempotency for the new CST shape.
3. **Audit `collect_html_block_text_skip_bq_markers` further**.
   Only one test (the `<pre>`-in-bq verbatim case) exercises real
   bq-marker stripping. The fallback for multi-line-open-inside-bq
   would also use it, but no test covers that combo today. The
   helper is small (~25 lines), so the cost of keeping it is low.

### New trap

None — pruned paths were truly dead.

--------------------------------------------------------------------------------

## Earlier sessions (compact log)

Newest first. One line per session: date — phase/sub-target — pass
count delta — root cause / lever.

- 2026-05-11 — Phase 6 bq lift arc (Fix #5 clean + HTML_ATTRS-in-bq followup, Fix #7 same-line, Fix #8 messy = open-trailing/butted-close/both) across div / non-div strict-block / inline-block matched-pair — html stable 159 — three discriminator gates (`bq_clean_lift`, `same_line_bq_lift_tag`, `bq_messy_lift_tag`), `BqPrefixState` re-injection, `inline_block_void_interior_abandons`, `bq_strict_attr_emit_tag_name`, `open_tag_raw_block_text` bq-prefix strip, `leading.is_empty()` close-tag guard.
- 2026-05-11 — Phase 6 / Fix #4 non-div strict-block shape sweep + multi-line open-tag lift — html 142 → 159 — `is_pandoc_lift_eligible_block_tag`, `html_block_has_structural_lift`, `LastParaDemote::{OnlyIfLast,SkipTrailingBlanks,Never}`, `parse_with_refdefs` graft, `emit_multiline_open_tag_with_attrs`, `open_tag_raw_block_text` canonicalizer.
- 2026-05-10 → 2026-05-11 — Phase 6 cannot_interrupt + Fix #1/#2 — html 132 → 142 — PARAGRAPH→PLAIN retag at YesCanInterrupt; `is_closing` field; `is_math_tex_script_open`; pandoc `isInlineTag` (issue #10643).
- 2026-05-10 — Strict-block/verbatim closing-form lift, multi-line void open-tag, incomplete-open recursion fix, Phase 3 void `eitherBlockOrInline` — html 105 → 132 — `closes_at_open_tag`, `pandoc_html_open_tag_closes` gate, `PANDOC_VOID_BLOCK_TAGS`.
- 2026-05-09 — Phase 3 + Phase 5 (non-void eitherBlockOrInline; HTML5 sectioning; `<DIV>` losslessness; Plain/Para; multi-line attrs; refs inheritance) — html 62 → 105 — projector `inline_pending` + parser `cannot_interrupt`; CM/Pandoc blockHtmlTags split; `build_refs_ctx_inherited`.
- 2026-05-08 — Phases 1-5 seed (issue #263 closed) — html 0 → 62 — `HTML_BLOCK_DIV`/`INLINE_HTML_SPAN` retag, `HTML_ATTRS` tokenization, sectioning/verbatim corpus pin, depth-aware nested `<div>`.
