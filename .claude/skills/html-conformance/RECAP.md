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

- **Disk lint cache at `~/.cache/panache/`** serves stale linter
  results even after `cargo build` (unit tests pass, `panache lint`
  emits old diagnostics, `eprintln!` never fires). Fix:
  `rm -rf ~/.cache/panache/` or `cache.enabled = false`. Validate via
  unit tests first.
- **Conformance comparison is whitespace-insensitive** —
  `normalize_native` collapses multi-line block output to one line;
  visual diffs mislead.
- **Config is discovered by walking up from the INPUT FILE's dir, not
  just CWD.** A stray `/tmp/panache.toml` (`flavor = "myst"`, CommonMark-
  based → no `<div>` lift) shadows any test file under `/tmp/…`, so
  `<div id>` anchor quick-repros there falsely emit `undefined-anchor`.
  This masqueraded as a "salsa anchor false-positive" bug across
  sessions; it is NOT a Panache bug. Reproduce anchor cases under a
  clean dir (the repo's `target/`), where `<div id>` resolves correctly.
  Prior session's ranked target #4 was this artifact — dropped.

### Parser shape & losslessness

- **HTML_ATTRS is the structural pattern; never add synthetic tokens.**
  Expose attrs by tokenizing existing bytes (`TEXT + WS +
  HTML_ATTRS{TEXT} + TEXT`). Use source-byte slices (`&rest[..4]`),
  never literals, for case-insensitive prefix matches.
- **Same-line `<div>foo</div>` is ONE `HTML_BLOCK_TAG`** — close lives
  in a TEXT child of the open; scan to first **unquoted** `>` (naive
  `strip_suffix('>')` grabs the wrong one). Quoted attr values hide
  `<`/`>`; bracket scanners thread quote state across lines
  (`count_tag_balance`, `find_multiline_open_end`,
  `pandoc_html_open_tag_closes`).
- **Multi-line open-tag close branches diverge by tag class** — void
  multi-line opens early-exit `end_line_idx + 1` BEFORE the close loop;
  `same_line_closed` must guard `multiline_open_end.is_none()`.
- **Incomplete opens (`<embed\n`, no `>`) caused projector infinite
  recursion** — gate Pandoc BlockTag recognition on
  `pandoc_html_open_tag_closes` in `detect_prepared` (CommonMark liberal).
- **Self-closing `<tag/>` doesn't bump depth** — depth matchers check
  `bytes[j-1] == b'/'` at the closing `>`.
- **`input.lines()` strips newlines** — losslessness tests use
  `split_lines_inclusive`.
- **`HtmlBlockType::BlockTag` is `Box<dyn Any>`-roundtripped** — adding
  a field works automatically; E0063 points at every literal site.
- **Baked multi-tag TEXT vs structural split.** The parser bakes
  consecutive standalone tags on one line into a SINGLE `HTML_BLOCK_TAG`
  TEXT token (`</p></div>`), indistinguishable from a genuine single
  tag. Phase 7b's `try_parse_standalone_block_tags_split` emits one
  tag each for the single-line case (top-level AND blockquote — the
  `> ` prefix is a sibling of `HTML_BLOCK`, so `strip_line_0_for_
  emission` yields clean tags; if the strip leaves non-tag bytes the
  segment scan bails to the byte walker), so the projector predicate
  `html_block_is_standalone_tag_sequence` (≥ 2 `HTML_BLOCK_TAG`, no
  `HTML_BLOCK_CONTENT`) is SAFE. Still baked (byte-walker): single tags,
  multi-line standalone (each tag already its own separate `HTML_BLOCK`
  as of current parser — 0304 emits two clean blocks, NOT a buried 2nd
  tag as an older RECAP claimed). Do NOT loosen the predicate to
  single-`HTML_BLOCK_TAG` (would merge baked-multi).
- **A new HTML wrapper retag (`HTML_BLOCK_RAW`, `HTML_BLOCK_DIV`, …)
  must be added to EVERY consumer that matches the old kind**, else the
  block silently mis-formats/drops. `HTML_BLOCK_RAW` touched: formatter
  arms (`formatter/core.rs`, `lists.rs`, `utils.rs`), list-item lift
  gate (`list_item_buffer.rs` single- + 2-child `matches!`), LSP
  `folding_ranges.rs`, linter `html_entities.rs`, BOTH `directives.rs`
  copies (`src/` + `crates/panache-formatter/src/`). Grep the old kind
  across `crates/` + `src/`. Retag fires under `Dialect::Pandoc`, so
  Quarto/RMarkdown see it too, not just the harness.

### Pandoc tag categorization

- **Pandoc has THREE tag sets**: strict block (`PANDOC_BLOCK_TAGS`),
  inline-block non-void (`PANDOC_INLINE_BLOCK_TAGS`), inline-block
  void (`PANDOC_VOID_BLOCK_TAGS`). Strict always splits; non-void
  follows `inline_pending` + matched-pair lift; void follows
  `inline_pending` + emits single RawBlock. Source:
  `pandoc/.../TagCategories.hs` + `Readers/HTML.hs::isBlockTag` /
  `isInlineTag`. CommonMark and Pandoc `blockHtmlTags` lists differ
  in both directions (~15 tags); don't merge. Parser gates on
  `is_commonmark`; projector runs Pandoc only.
- **`eitherBlockOrInline` is context-dependent** — needs BOTH
  parser-side `cannot_interrupt` (don't break running paragraph) AND
  projector-side `inline_pending` (don't split mid-text).
- **Block-level tags mid-paragraph force a block boundary in pandoc;
  panache inlines them.** `foo <p>bar</p>` → pandoc `Plain [foo]` +
  `RawBlock "<p>"`/…; panache keeps one `Para` with `RawInline "<p>"`.
  Same-line inter-tag text between NON-DIV matched-pair strict-block
  tags (`<p>foo</p> bar <p>baz</p>`, 0472/0475/0476/0477) is FIXED via
  option (b): `same_line_trailing_forces_opaque` suppresses the
  same-line strict-block lift when the after-first-close trailing
  (ws-trimmed) starts with non-`<` text AND holds a later matched-pair
  tag → whole line kept opaque `HTML_BLOCK` → projector
  `split_html_block_by_tags` produces the flat RawBlock/Plain split
  (tail-after-last-tag stays `Para`). Gate is narrow: consecutive /
  ws-only (`<p>a</p><p>b</p>`, `<p>a</p> <p>b</p>`) trim to `<`-leading
  → keep clean sibling-lift; void-only trailing (`<embed>`, 0474) has
  no matched-pair tag → tail `Para`. Still divergent: (a) `<div>`
  inter-tag (`<div>x</div> y <div>z</div>` → pandoc `Div`/`Plain`/`Div`;
  splitter emits RawBlock not Div, risks the HTML_BLOCK_DIV-panic) —
  not in corpus; (b) the general inline-parser boundary for
  `foo <p>bar</p>` where a paragraph LEADS the tag — broad, unaddressed.
- **Closing forms of all matched-pair sets ARE block starts** — emit
  `BlockTag { closes_at_open_tag: true }`. Dispatcher's
  `cannot_interrupt` keys on inline-block + void only (strict-block +
  verbatim closes get `YesCanInterrupt`).
- **Verbatim tags fire first** — `VERBATIM_TAGS` checked before
  inline-block/strict-block arms; the overlap is harmless.
- **Pandoc `isInlineTag` special cases (issue #10643):** `<style>` o+c,
  `</script>`, PIs, comments, `<script type="math/tex…">` (ci, single-
  line) cannot interrupt a paragraph; `<pre>` / non-math `<script>` /
  `<textarea>` DO. In `detect_prepared`'s `cannot_interrupt`; needs
  `is_closing: bool` on `HtmlBlockType::BlockTag`.
- **Indented `isInlineTag` demotes to `Para [RawInline]`** (same set as
  `cannot_interrupt`) — `detect_prepared` returns `None` when
  `leading_spaces(ctx.content) > list_indent_info.content_col`. Trap:
  `ctx.content` retains list-item content_col indent (bq markers ARE
  stripped, so bq works transparently).
- **`HtmlBlockType::BlockTag.is_closing` — match guards pivoting on
  tag identity MUST check it.** `pandoc_html_open_tag_closes`
  returns true for both `<div>` and `</div>` (scans for first `>`).
  Gates firing on `tag_name == "div"` alone wrongly retag close
  forms. `HTML_BLOCK_DIV` retag destructures `is_closing: false`;
  `</div>` without matched open keeps opaque HTML_BLOCK → single
  RawBlock per pandoc-native.

### Projector tag splitting

- **`split_html_block_by_tags` walks bytes, not tokens.**
  Context-tracked via `inline_pending`; runs for opaque
  HTML_BLOCKs only (comments, PI, verbatim, void tags, unmatched
  strict / inline-block tags). Matched-pair `<div>` is parser-
  lifted now. `<video>...</video>` matched-pair lift abandons
  when interior opens with void block tag at col 0
  (`inline_block_void_interior_abandons`). Inline-block open with
  no matched close also emits RawBlock — falling through to
  `inline_pending=true` causes stack overflow via tail-text
  reparse recursion.
- **`inline_pending` resets on consecutive newlines (≥ 2).**
  Inter-tag text demotes Para→Plain when butted against next tag;
  tail text does NOT demote. Use `flush_html_block_text` vs
  `flush_html_block_tail_text`.
- **HTML blocks inside blockquotes need
  `collect_html_block_text_skip_bq_markers`** on remaining
  byte-walker paths — parser keeps `BLOCK_QUOTE_MARKER + WS` as
  structural tokens; passing `node.text()` re-recognizes `> ` as
  nested bq. Remaining caller: `emit_html_block` for verbatim in
  bq.
- **`walk_skip_bq_markers` also strips leading line-start
  `WHITESPACE`** (token NOT preceded by a `BLOCK_QUOTE_MARKER` on the
  same line) — covers the list-item indent re-injected by
  `strip_list_item_indent`/`LinePrefixState`. Safe because the parser
  never emits leading line-start WS inside HTML_BLOCK_CONTENT/_TAG
  outside the lift path (top-level indent stays in one TEXT token).
  Threads `skip_next_ws` (bq pairs) + `at_line_start` (reset after
  NEWLINE/BLANK_LINE).
- **Projector `open_tag_raw_block_text` canonicalizes multi-line opens**
  — with `HTML_ATTRS`, walk `children_with_tokens`, take leading
  `<tagname` TEXT, join HTML_ATTRS trimmed texts with single spaces,
  append `>`. Single-line opens without HTML_ATTRS keep literal text.

### Refs / footnotes / heading-id resolution

- **`parse_pandoc_blocks` swaps in an inner `RefsCtx`** for recursive
  reparse — swap belongs IN it, not at call sites. `build_refs_ctx`
  mutates `REFS_CTX` mid-build: save outer FIRST via `mem::take`, THEN
  `build_refs_ctx`, THEN install.
- **`heading_id_by_offset` is offset-keyed** (inner offsets zero-based;
  don't copy outer `heading_ids` in). Build fresh inner ctx, inherit
  cross-boundary refs/footnotes via `build_refs_ctx_inherited`.
- **`fenced_div` walks CST via `collect_block`**, not
  `parse_pandoc_blocks` — don't generalize the swap to fenced divs.
- **`AttributeNode::can_cast` accepts `HTML_ATTRS`** — salsa walk picks
  up `<div id>`/`<span id>`/`<section id>` ids automatically. Diverges
  from pandoc-native (RawBlock without lifting attrs) but matches
  anchor-link intent. No parallel salsa walk.

### Out of scope / known divergences

- **HTML entity decoding is unimplemented** (verified 2026-07-02, out
  of html-tag-parsing scope → general pandoc-conformance). pandoc
  decodes named (`&amp;`→`&`, `&copy;`→`©`, `;` required) + numeric
  (`&#65;`/`&#x41;`→`A`, null/surrogate/oob→U+FFFD, single-pass) refs in
  text, heading text (**auto-ids change**), link/image URLs+titles, and
  autolinks; NOT in code spans/blocks or raw HTML. panache keeps all
  literal. Reusable `panache_parser::entities::ENTITIES` table already
  vendored. A projector-side pass touching Str/heading-id/link/autolink
  emission; broad, its own effort.
- **`<!ENTITY x "y">` projects `Str "\"y\">"`** vs pandoc's `Quoted
  DoubleQuote [Str "y"]` — smart-quote/Quoted gap, not html-conformance.
- **Ref-conflict + cross-boundary cite numbering** for `<div>` recursive
  reparse: pandoc is document-order-first, we're inner-wins. No corpus;
  deferred.
- **Top-level Para→Plain demotion at HTML strict-block/verbatim adjacency
  is parser-side** (`Parser::close_paragraph_as_plain_if_open` +
  `html_block_demotes_paragraph_to_plain`, wired at YesCanInterrupt in
  `core.rs`; CST emits `PLAIN`). Don't reintroduce projector-side demote.
- **Formatter non-idempotency for tab-indented list items** —
  `-\t<div>\n\thello\n\t</div>` parses as `Div [Para]` but formatter
  normalizes `-\t`→`- ` while keeping body tabs → re-parses as
  `Div [CodeBlock]`. Formatter bug (likely `formatter/lists.rs`), not
  html-conformance; parser fixtures pin the parser side only.

### Latent projector panic on unstructural HTML_BLOCK_DIV

- `html_div_block` `debug_assert!`s when `HTML_BLOCK_DIV` lacks a
  structural inner shape. Any parser change that retags
  `HTML_BLOCK_DIV` MUST guarantee the body lift, else projection
  panics. Prefer "fall back to opaque HTML_BLOCK" over emitting a
  one-child HTML_BLOCK_DIV. `div_has_structural_inner` accepts a
  missing close tag (unclosed `<div>` → `Div` with implicit close):
  1 clean open HTML_BLOCK_TAG + structural body + no
  HTML_BLOCK_CONTENT suffices.
- Same-line and multi-line close-line lifts are depth-aware:
  `probe_same_line_lift` + `matched_close_offset` +
  `try_split_close_line_depth_aware` + `split_close_marker_end`
  (accept nested same-tag opens and unmatched trailing closes).
  `try_split_close_line` (strict `(0,1)`) survives only where that
  count is intentional.
- Multi-line open + matched close in `pre_content`
  (`<div\n  id="x">foo</div>` and depth-aware variants, top-level +
  bq) lifts via a branch BEFORE `same_line_closed`, gated on
  `multiline_open_end.is_some() && depth_aware_tag.is_some() &&
  depth <= 0 && lift_mode && (bq_depth == 0 ||
  bq_multiline_close_lift_tag.is_some()) && !pre_content.is_empty()`;
  same split+graft pattern, returns `end_line_idx + 1`. Bq variant
  inherits the prefix from the open's last line
  (`emit_multiline_open_tag_with_attrs`), so `bq: &mut None` suffices.

### Projector-as-second-stage-parser smell (architectural)

`pandoc_ast.rs` is the public `to_pandoc_ast` API; linter/salsa/LSP/
formatter walk the CST, not the projector — so byte-walking there is a
smell to shrink over time. Progress: Phases 1/5 retags
(`HTML_BLOCK_DIV`, `INLINE_HTML_SPAN`); Phase 6 lifted `<div>` /
non-div strict-block / inline-block matched-pair bodies (non-bq + bq);
vestigial `<div>` byte walkers (`try_div_html_block`) pruned. **7a**
retagged single-construct opaque shapes (comment/PI/verbatim) to
`HTML_BLOCK_RAW` (routes via `html_raw_block` → one `RawBlock`;
`emit_html_block`'s byte-sniff early-return now dead for Pandoc,
alive for CommonMark + `<![CDATA[`/`<!`). **7b** split standalone
tag sequences. **7c** removed non-div unmatched-open + trailing
via `html_block_has_open_only_structural_lift` — first `bq_depth == 0`,
then extended to `bq_depth > 0` (bq open-only body re-injects `> `
prefixes via `emit_html_block_body_lifted_bq_messy`).

Retag mechanism (7a/7c share the `HTML_BLOCK_DIV` precedent):
`wrapper_kind` stays `HTML_BLOCK` (lift gates + child tokens
byte-identical), only the node kind at the two `start_node` sites
flips via `html_block_node_kind`.

Load-bearing byte-walker remainder (opaque HTML_BLOCKs only):
`split_html_block_by_tags` — bq-wrapped open+trailing, single open
tag with NO body (`<section>` alone → 0 block children → byte walker,
one RawBlock, correct), void/self-contained sequences, multi-tag
interleave; `parse_pandoc_blocks` (inter-tag text reparse via
`flush_html_block_text` / `flush_html_block_tail_text`);
`collect_html_block_text_skip_bq_markers` (also `html_raw_block`
verbatim-in-bq); table-cell reparses.

- **Void strict-block tags (`col`, `hr`, `meta`) close on the open
  line.** These are HTML void elements that live in `PANDOC_BLOCK_TAGS`
  (strict block: always split, DO interrupt a paragraph) but have no
  close form. `PANDOC_VOID_STRICT_BLOCK_TAGS` (2026-07-02) makes the
  parser emit `closes_at_open_tag: true` / `depth_aware: false` for
  them under Pandoc, so `<hr>\n<hr>\n<hr>` is three sibling
  `HTML_BLOCK`s, NOT nested (fixes the pre-7c matched-pair-body-lift
  quirk). They're excluded from `is_pandoc_lift_eligible_block_tag` +
  `is_pandoc_matched_pair_tag` and stay OUT of the dispatcher's
  `cannot_interrupt` void set (distinct from `PANDOC_VOID_BLOCK_TAGS` =
  `area`/`embed`/`source`/`track`, which DON'T interrupt). Attrs are no
  longer split into HTML_ATTRS (`<hr id>` stays opaque — pandoc lifts no
  anchor there). CommonMark keeps type-6 (block until blank line). Don't
  add `col`/`hr`/`meta` to `PANDOC_VOID_BLOCK_TAGS` — that would wrongly
  make them non-interrupting.

### Structural lift (Fix #3 / Fix #4 family)

- **Recursive parse uses `parse_with_refdefs`, not `parse`** —
  `parse` re-runs `populate_refdef_labels` on just the inner text,
  hiding outer refdefs. Thread outer `refdef_labels` through.
- **Line-consumption boundary trap** (Comment/PI trailing split).
  `parse_html_block_with_wrapper`'s `lines` is the WHOLE document.
  Returning `lines.len()` from inside a fenced div / list item / bq
  eats container close markers (`:::`, `> `, indent). Sibling-emit
  helpers only consume the current line; outer dispatcher resumes at
  `close_line + 1`. Trade-off: multi-line softbreak continuation
  (`<!-- --> A\nB`) breaks (fresh Para for `B`) — blocked 0390.
- **`graft_document_children` is a sibling-emit helper** — call it
  AFTER `builder.finish_node()` on HTML_BLOCK to graft children at
  the parent (DOCUMENT/container) level (Comment/PI trailing-split).
  Its `LastParaDemote` arg: `Never` (clean/unbalanced — Para kept),
  `SkipTrailingBlanks` (div close-butted — demote last PARAGRAPH
  past trailing BLANK_LINEs), `OnlyIfLast` (non-div strict-block
  close — demote only when last child is PARAGRAPH, no trailing
  BLANK_LINE).
- **Comment/PI trailing softbreak fusion is parser-side, gated by the
  `SoftbreakFusion` enum** (`ToDocEnd` / `ToFencedDivClose` /
  `ToBlockquoteEnd` / `None`), computed at the dispatcher and threaded
  into `parse_html_block_with_wrapper` →
  `try_parse_comment_pi_with_trailing_split`. `<!-- hi --> trailing\nmore`
  fuses the continuation into ONE `Para` (pandoc): reparse `trailing +
  lines[close+1 .. fusion_end]`, graft ONLY the first block, map
  `first.text_range().end()` back to a consumed-line count (bytes ≤
  `trailing.len()` = close line already counted; add each fully-covered
  later line, capped at `fusion_end`). `fusion_end` = `lines.len()` at the
  outermost level (`ToDocEnd`); the enclosing fenced div's close `:::`
  line inside a **plain** fenced div (`ToFencedDivClose`,
  `fenced_div_body_end` scans from `close+1` — PAST the comment close so a
  `:::` in the comment body doesn't stop early — tracking div nesting
  depth); the blockquote boundary inside a **pure blockquote**
  (`ToBlockquoteEnd`, `blockquote_body_end` = first line whose
  `count_blockquote_markers` depth `< bq_depth`). Excluding the close
  marker from the fragment is what keeps a bare `:::` from fusing into the
  para (verified: bare `:::` IS swallowed by a fresh-doc para, pandoc
  too). **Blockquote continuation lines are prefix-handled inside the
  fusion reparse**: strip each continuation line's `> ` prefix into the
  fragment, capture it in a `prefix_lines` vec (line 0 = the `trailing`
  bytes gets an EMPTY prefix — its `> ` was already emitted by the outer
  dispatcher), reparse, then `graft_subtree` with a `ContainerPrefixState`
  re-injecting each prefix at line start (byte-lossless). The
  consumed-line mapping accumulates STRIPPED line lengths (fragment
  coords), not raw. For `bq_depth == 0` (ToDocEnd/ToFencedDivClose) the
  strip is a no-op, every prefix is empty, and the state collapses to
  `None` — identical to the pre-existing raw-line behavior. List /
  content-indent / directive containers still stay `None` (they'd need
  list-indent stripping too — deferred). Lazy paragraph continuation (a
  bare, `>`-less line after a bq) stays divergent — that's the broader
  blockquote-continuation gap, NOT softbreak fusion; `blockquote_body_end`
  deliberately stops at the non-`>` line. CommonMark keeps opaque
  HTML_BLOCK (whole first line). Corpus 0390 (outermost), 0481 (fenced
  div), 0482 (blockquote).
- **`HTML_BLOCK_DIV` retag at dispatcher is two-pronged** — fires iff
  `probe_open_tag_line_has_close_gt(ctx.content,"div")` (single-line)
  OR `pandoc_html_open_tag_closes(...)` (multi-line). Incomplete
  opens (`<div\n` no `>`) keep opaque HTML_BLOCK. Multi-line +
  trailing on close-`>` line: `emit_multiline_open_tag_with_attrs`
  captures trailing into `pre_content` via `lift_trailing=true`.
- **`emit_html_block_body` has two call sites; only the no-close one
  may bq-lift.** The function takes `open_only: bool` — the close-found
  site (2095) passes `false`, the end-of-input no-close site (2121)
  passes `true`. The `HTML_BLOCK` + `bq_depth > 0` open-only lift arm
  (routes through `emit_html_block_body_lifted_bq_messy`, empty
  leading/close-prefix) is gated on `open_only`, so a bq shape with a
  close tag (whose body clean/messy lifts didn't fire) keeps the legacy
  opaque path. `bq_depth` comes from the threaded `prefix:
  &ContainerPrefix`.
- **Lifted HTML_BLOCK[_DIV] MUST route structural, not byte path.**
  `collect_block` → `html_div_block`; `emit_html_block` → lifted →
  `emit_html_block_structural` (NOT `split_html_block_by_tags`, whose
  `parse_pandoc_blocks` builds a fresh inner `RefsCtx` → stray `-1`
  auto-id suffix). Body-lifted signal: no `HTML_BLOCK_CONTENT` child;
  `html_block_open_tag_is_clean` accepts TEXT ending in `>`.
- **Multi-line open tags emit one `HTML_ATTRS` region per attr line**
  — iterate + join with `" "` (`cst_div_open_tag_attr`);
  `.children().find()` only sees the first.
- **Coverage** (shape list in Phase-6 table cell): all non-bq +
  bq shapes for `<div>` + non-div strict-block + inline-block
  matched-pair. Line-0 `> ` owned by outer BLOCK_QUOTE; later lines'
  `> ` re-injected via `BqPrefixState` (both NEWLINE *and* BLANK_LINE
  tokens advance `line_idx`, else losslessness breaks).
  `find_multiline_open_end` + `emit_multiline_open_tag_with_attrs/
  _simple` thread `bq_depth` and re-emit prefix tokens past line 0.
- **Three bq lift gates by post-open `depth`** — all require
  `bq_depth > 0` + `depth_aware_tag.is_some()` +
  `is_pandoc_lift_eligible_block_tag`; inline-block also gates on NOT
  `inline_block_void_interior_abandons`:
  - `same_line_bq_lift_tag` (`depth <= 0`, single-line) → via
    `same_line_closed`, `emit_html_block_body_lifted` `bq:&mut None`.
    Demote div=SkipTrailingBlanks, non-div=OnlyIfLast.
  - `bq_clean_lift` (`depth > 0`, close `trim_start.starts_with("</")`
    + clean open) → `emit_html_block_body_lifted_bq`. Demote div=Never,
    non-div=OnlyIfLast.
  - `bq_messy_lift_tag` (`depth > 0`, not clean; multi-line+trailing
    uses `lift_trailing`; close site bq-strips then
    `try_split_close_line`) → `emit_html_block_body_lifted_bq_messy`.
    Demote div=close-butted-keyed, non-div=OnlyIfLast.
- **`<div>` inter-tag peel (`graft_same_line_div_peel`).** For
  `<div>x</div> y <div>z</div>` the after-first-close trailing peels each
  further matched `<div>...</div>` pair into a sibling `HTML_BLOCK_DIV`,
  with interstitial text grafted as demoted `Plain` (butted between
  blocks) and tail text after the last div kept as `Para`. Reparses each
  segment as a fresh document (`emit_html_block_body_lifted`), so col-0
  opens lift and losslessness holds: only the FINAL segment carries the
  source line's newline; whitespace-only gaps parse to `BLANK_LINE` (no
  block, bytes preserved, projector skips). `find_next_matched_pair`
  reuses `locate_open_tag_close_gt` + `matched_close_offset`. Wired at
  BOTH the single-line `same_line_closed` path and the multi-line-first-
  div content-scan close-line path (gated `HTML_BLOCK_DIV` + `bq_depth
  == 0`). **Still divergent** (deferred, not in corpus): a multi-line
  SECOND div (`</div> y <div>\n z\n</div>`) — the content-scan depth
  model treats `</div> ... <div>` on the close line as depth-neutral
  (close then re-open), so the whole span stays ONE div; pandoc treats
  the later `<div>` as a fresh block. Needs the depth-model rework.
- **Unclosed fenced div in a `<div>` body suspends the `</div>` close.**
  Pandoc parses the div body as markdown first, so an unclosed `:::x`
  greedily swallows the following `</div>` as a `RawBlock`
  (`<div>\n:::x\n</div>` -> `Div[Div(x)[RawBlock "</div>"]]`, outer div
  implicitly closed at EOF). `parse_html_block`'s body-scan loop tracks a
  `body_fence_depth` (bumped by `try_parse_div_fence_open`, dropped by
  `is_div_closing_fence`) and returns `line_closes = false` for the
  depth-aware close while `body_fence_depth > 0` WITHOUT advancing `depth`,
  so the whole body lifts on the no-close implicit-close path
  (`emit_html_block_body` -> `emit_html_block_body_lifted`). Interleaved
  `<div>\n:::x\n:::\n</div>` still pairs normally (fence closes first).
  Gated `Dialect::Pandoc && extensions.fenced_divs && depth_aware_tag`;
  CommonMark keeps opaque `HTML_BLOCK`. Corpus 0478.
- **`try_split_close_line` whitespace-only `leading` = close indent,
  not body.** For `   </article>`: pass `body_leading=""`, emit
  leading as `WHITESPACE` in close `HTML_BLOCK_TAG`, keep demote keyed
  on original `leading.is_empty()`.
- **Bq messy-lift duplicate-prefix trap** —
  `emit_html_block_body_lifted_bq_messy` injects the close line's bq
  prefix before `leading`; close `HTML_BLOCK_TAG` MUST NOT re-emit
  `emit_bq_prefix_tokens` when `leading` is non-empty (doubles `> `).
- **Projector `open_tag_raw_block_text` strips bq markers AND leading
  1-3 space indent** before the accumulator's first non-WS token
  (pandoc RawBlock text is tag bytes only). HTML_ATTRS branch
  (multi-line canonicalization) unaffected.

### List-item / definition-body HTML structural lift

- **Footnote-body marker-line HTML (`[^1]: <html>`) dispatches via
  `Parser::try_dispatch_footnote_html_block`** (added to
  `handle_footnote_open_effect` before the `start_paragraph_if_needed`
  fallback; mirrors the def version — throwaway-builder probe, single-line-
  close lift, byte-lossless marker-line emit, `content_col = 4`). CRITICAL
  DIVERGENCE from def bodies: only tags that CAN interrupt a paragraph lift.
  Pandoc keeps comments, PIs, `<span>`, `<style>`, math-`<script>`, and void
  inline-block tags (`<embed>`/`<area>`/`<source>`/`<track>`) INLINE inside
  footnote bodies (`Para [RawInline]`), unlike def bodies where a leading
  comment lifts to `RawBlock`. Gate: `!html_block_cannot_interrupt(&bt,
  content, true)` — the dispatcher's `isInlineTag` predicate, extracted to
  `pub(crate) block_dispatcher::html_block_cannot_interrupt`. So `<div>`/
  `<p>`/`<section>`/`<pre>`/`<hr>`/`<table>` lift; the inline set stays
  `Para`. Gated `Dialect::Pandoc` (GFM/CommonMark footnotes byte-identical).
  `<div id>` registers in the anchor index (issue #263 in footnote bodies).
  Shares `marker_line_html_block_wrapper_kind` (renamed from
  `definition_html_block_wrapper_kind`). Multi-line-open + softbreak fusion
  deferred (mirror Phase D).
- **`format_node_sync`'s `FOOTNOTE_DEFINITION` arm drops the marker's
  trailing WHITESPACE for non-`PARAGRAPH` first children.** The marker
  string trims trailing WS (`marker.trim_end()`) and the space is re-added
  only in the `first`-child `PARAGRAPH`/`DEFINITION_LIST` branches. A lifted
  HTML block as first child (`[^1]: <div>x</div>`) collapsed onto the colon
  (`[^1]:<div>...`). Fix: the `first` branch now also special-cases
  `HTML_BLOCK | HTML_BLOCK_RAW | HTML_BLOCK_DIV` (push a space, inline
  `format_node_sync`, `continue`). Same class as the `format_list_item`
  `LIST_MARKER`-drop trap — any new footnote first-child-as-block shape MUST
  add the same pattern or the marker space vanishes.
- **Definition-body marker-line HTML (`:   <html>`) dispatches via
  `Parser::try_dispatch_definition_html_block`** (added to the def
  first-content-line cascade in `handle_definition_list_effect`, alongside
  the bq/list/fence arms). The marker line otherwise flows into
  `emit_definition_plain_or_heading`, which only special-cases ATX headings
  → raw HTML would parse as inline `RawInline`-in-`Para`. HTML on a *later*
  def-body line dispatches via the normal container path for the
  single-line shape; the multi-line shape there still drops indent (parses
  body as `CodeBlock`) — a SEPARATE normal-container bug, deferred. The
  dispatch probes the block's line extent via a THROWAWAY `GreenNodeBuilder`
  (line 0 = post-marker bytes; continuation lines = `content_prefix.strip`ped
  raw lines) and branches on `probe_consumed`:
  - **`== 1` (single-line close):** real emit uses ONLY the marker-line
    bytes (byte-lossless — no continuation consumed), `fusion = None`.
  - **`> 1` (multi-line body opens on the marker line, closes later):**
    reuses the list-item lift `list_item_buffer::try_emit_html_block_lift`
    (now `pub(crate)`) — builds the block text from `content_line` +
    raw continuation lines up to `probe_consumed`, strips `content_col`
    indent before the inner reparse (so `<div>\n    x\n    </div>` body
    parses as `Para [x]`, NOT an indented `CodeBlock`), re-injects the
    stripped indent via `ContainerPrefixState` during graft (byte-lossless).
    Gated `Dialect::Pandoc && bq_depth == 0` (the lift hardcodes the Pandoc
    HTML grammar and doesn't strip `> ` markers, so def-in-blockquote
    multi-line falls through to the pre-existing losslessness gap). `use_
    paragraph` = blank-line-before-marker (loose→`Para`, tight→`Plain`);
    only matters for the 2-child trailing-split shape. Corpus 0487.
  `marker_line_html_block_wrapper_kind` mirrors the dispatcher's
  `HTML_BLOCK_DIV` retag gate. Continuation softbreak fusion of a following
  content-indented line into the trailing block stays deferred (needs a
  content-indent `SoftbreakFusion` variant + Para/Plain looseness demotion,
  which the fresh-reparse lift can't bound to the def body).
- **`ListItemBuffer::emit_as_block` lifts same-line / fully-contained
  HTML via `try_emit_html_block_lift`.** Strict gate:
  `try_parse_html_block_start` recognizes line 0, inner reparse yields
  exactly ONE top-level `HTML_BLOCK`/`HTML_BLOCK_DIV` consuming every
  buffer byte, and `HTML_BLOCK_DIV` needs ≥ 2 `HTML_BLOCK_TAG`
  children. Multi-line shapes lift via the close-form dispatcher gate.
- **Close-form dispatcher gate (multi-line list-item HTML)** — close
  recognition (`</div>`, …) is gated on the enclosing LIST_ITEM buffer
  NOT having an unclosed matched-pair open of that tag.
  `BlockContext::list_item_unclosed_html_block_tag` (populated via
  `Parser::list_item_unclosed_html_block_tag` →
  `ListItemBuffer::unclosed_pandoc_matched_pair_tag`, walking buffer
  segments with `count_tag_balance`); `detect_prepared` returns `None`
  for matching close-forms so the buffer accumulates the full pair.
  `count_tag_balance` / `is_pandoc_lift_eligible_block_tag` /
  `is_pandoc_matched_pair_tag` are `pub(crate)`. Pandoc only.
- **List-item indent normalization: `strip_list_item_indent` +
  `LinePrefixState`.** `emit_as_block` threads `content_col`; when
  `> 0`, strip up to `content_col` leading spaces from lines past 0,
  reparse stripped, re-inject each prefix as line-start `WHITESPACE`
  (mirrors `BqPrefixState`; tab = col+4, refuse overshoot). Without
  it, `- <div>\n  body\n  </div>` mis-demotes and `<pre>` keeps
  indent. Injected WS inside opaque HTML_BLOCK_* is stripped by
  projector `walk_skip_bq_markers`; inside lifted PARAGRAPH/PLAIN it's
  a leading `Inline::Space` dropped by `coalesce_inlines` edge-trim.
- **`format_list_item` drops `LIST_MARKER` when the item has no
  PLAIN/PARAGRAPH content_node** (marker-emit is tied to the wrapping
  flow). Per-kind arms emit it when `no_content_emitted &&
  is_first_real_child` (HORIZONTAL_RULE, HTML_BLOCK | HTML_BLOCK_DIV).
  Any new list-item-as-block lift (HEADING-only, BLOCK_QUOTE-only, …)
  MUST add the same pattern or the marker silently vanishes (the `_`
  fallback does NOT emit it).

### Bq-in-listitem first-line dispatch (option (a), 2026-05-18)

- **`lists::add_list_item` returns `ListItemFinish`.** For `- > <x>`,
  the bq branch of `finish_list_item_with_optional_nested` opens an
  inner BLOCK_QUOTE and returns `ListItemFinish::BqDispatch{content}`.
  ALL `add_list_item` call sites + `start_nested_list` must feed it to
  `Parser::dispatch_bq_after_list_item`, which calls
  `parse_inner_content` and decrements `self.pos` by 1 (to absorb the
  caller's `lines_consumed += 1`). Discarding it silently loses line 0
  — no eager-paragraph fallback remains.
- **HTML-block dispatcher reads raw `lines[line_pos]`, not stripped.**
  From the bq-in-listitem helper, `pandoc_html_open_tag_closes` strips
  `bq_depth` markers but NOT the list-marker prefix (`- `), so the
  gate fails and falls to paragraph (0452/0453 family) — headings/HRs
  dispatch fine (they use pre-stripped `ctx.content`). Deferred fix:
  thread `list_content_col` through `pandoc_html_open_tag_closes`,
  `parse_html_block_with_wrapper`, `find_multiline_open_end`,
  `count_tag_balance`, `emit_html_block_body_lifted_bq*`; watch
  losslessness (list-indent WS needs BqPrefixState/LinePrefixState-
  style re-injection). *(0452/0453 later unblocked by a `ContainerPrefix`
  session — see the allowlist comment.)*
- **`find_content_node` skips PLAIN/PARAGRAPH trailing a leading
  HTML_BLOCK[_DIV].** Without it, the formatter picks the trailing
  PLAIN (`- <!-- hi --> trailing` shape) as wrap source → non-
  idempotent `- trailing\n<!-- hi -->`. With it, returns None so the
  HTML_BLOCK arm handles the marker line and the trailing PLAIN wraps
  as continuation (`- <!-- hi -->\n  trailing`). Also returns None for
  any non-PLAIN/PARAGRAPH/BLANK_LINE first-real child.

--------------------------------------------------------------------------------

## Phase progress

| Phase | Description | Status |
|-------|-------------|--------|
| 1 | `<div>` block lift (`HTML_BLOCK_DIV` + `HTML_ATTRS`) | **Landed** (2026-05-08) — issue #263; `<DIV>` losslessness fixed. Inner content lifted structurally in Phase 6. |
| 2 | `<span>` inline lift (`INLINE_HTML_SPAN`) | **Landed** (2026-05-08). Inner inlines trivial. |
| 3 | Sectioning + verbatim pin; `eitherBlockOrInline` lift | **Landed** — non-void (05-09), void `<embed>`/`<area>`/`<source>`/`<track>` (05-10). |
| 4 | Comments, PIs, declarations, CDATA | **Landed** (2026-05-08); type-4 CM lowercase still gappy. |
| 5 | `markdown_in_html_blocks` edge cases | **Landed** — depth-aware nested div, Plain/Para promotion, refs inheritance. Mechanism was the projector splitter; superseded by Phase 6's structural lift. |
| 7a | Single-construct opaque lift → `HTML_BLOCK_RAW` | **Landed 2026-06-17.** Comment/PI/verbatim retag routes projector by kind (`html_raw_block`). Detail in Persistent traps. |
| 7b | Standalone-tag split (≥ 2 tags/line → one `HTML_BLOCK_TAG` each) | **Landed 2026-06-29; bq 2026-07-02.** `try_parse_standalone_block_tags_split`; projector `html_block_is_standalone_tag_sequence`. |
| 7c | Open-only body lift (open tag + trailing body, no close) | **Landed 2026-07-02 (+bq same day).** `emit_html_block_body` non-div lift arm; projector `html_block_has_open_only_structural_lift`. |
| 7e | Multi-tag interleave (inter-tag text) | **Non-div same-line FIXED 2026-07-02** (`same_line_trailing_forces_opaque`, corpus 0472/0475-0477). **para-leads-tag still deferred** (see blocked.txt). |
| B | `<div>` inter-tag structural lift (`<div>x</div> y <div>z</div>`) | **Landed 2026-07-02.** `graft_same_line_div_peel` peels each pair into a sibling `HTML_BLOCK_DIV`; interstitial→`Plain`, tail→`Para`. Single-line + multi-line-first-div. Corpus 0479 (ws), **0480 (no-ws, pinned 2026-07-08)**. Multi-line-SECOND-div deferred (depth model). Detail in Persistent traps. |
| 6 | Lift inner HTML content into structural CST children | **All non-bq + bq shapes lifted** for `<div>` + non-div Pandoc strict-block tags (clean, open-trailing, butted/indented-close, same-line, empty, multi-line-open, depth-aware nested, multi-close, unclosed, multi-line-open+matched-close top-level+bq; inline-block matched-pair). List items + bq-in-listitem covered. `PARAGRAPH→PLAIN` retag at adjacency. All mechanisms in Persistent traps. **Pass count 105 → 257.** |
| A | fenced-div-in-html-div (`<div>\n:::x\n</div>`) | **Landed 2026-07-02.** `body_fence_depth` suspends the `</div>` close while an inner `:::` fence is open; body lifts with implicit EOF close. Corpus 0478. Detail in Persistent traps. First of the remaining-gaps roadmap (A fenced-div-in-div, B `<div>` inter-tag, C 0390 softbreak, D paragraph-leads-tag). |
| C | Comment/PI trailing softbreak fusion (`<!-- hi --> trailing\nmore`) | **Landed 2026-07-02; fenced-div container 2026-07-08; blockquote container 2026-07-08.** `SoftbreakFusion` enum bounds the reparse: `ToDocEnd` (outermost, corpus 0390) + `ToFencedDivClose` (plain fenced div, corpus 0481, `fenced_div_body_end`) + `ToBlockquoteEnd` (pure blockquote, corpus 0482, `blockquote_body_end` + `> `-prefix strip/re-inject via `ContainerPrefixState`). Grafts first block, maps `text_range().end()` to consumed lines. **List/content-indent containers stay `None` (need list-indent strip); lazy `>`-less bq continuation stays divergent (broader bq-continuation gap). Both deferred.** Detail in Persistent traps. |
| D | Definition-body marker-line HTML dispatch (`:   <div>x</div>`, `:   <div>\n x\n</div>`) | **Landed 2026-07-08; multi-line-open body 2026-07-08.** `try_dispatch_definition_html_block` arm in the def first-content-line cascade (mirrors bq/list/fence). Throwaway-builder probe branches on `probe_consumed`: single-line close → byte-lossless marker-line emit; multi-line body → reuse `list_item_buffer::try_emit_html_block_lift` (strip `content_col` + reparse dedented + re-inject) so `Div [Para x]` matches pandoc (fixed a **losslessness** failure that reordered open/close tags). `<div id>` registers in the anchor index. Gated Pandoc + `bq_depth==0`. Corpus 0483/0484/**0487**. Continuation softbreak fusion + def-in-bq multi-line + later-line multi-line (separate normal-container path) deferred. Detail in Persistent traps. |
| E | Footnote-body marker-line HTML dispatch (`[^1]: <div>x</div>`) | **Landed 2026-07-08.** `try_dispatch_footnote_html_block` in `handle_footnote_open_effect`, mirroring Phase D but gated on `!html_block_cannot_interrupt` (extracted shared predicate) — footnotes keep comments/PIs/`<span>`/void-inline-block INLINE (unlike def bodies). `<div id>` registers in anchor index. Formatter marker-space drop fixed. Corpus 0485/0486. Gated Pandoc dialect. Multi-line-open + softbreak fusion deferred. Detail in Persistent traps. |

--------------------------------------------------------------------------------

## Latest session — 2026-07-08 (Phase D extension — multi-line HTML body on a definition marker line)

Conformance: **html 289 → 290 passing** (290 total, **0 fail — html
100%**); total **486 → 487 / 487 (100%)**. Parser + formatter session:
fixed a **losslessness failure** — a raw HTML block whose body OPENS on a
definition marker line and CLOSES on a later content-indented line
(`:   <div id="d">\n    x\n    </div>`) was reordering the open/close tags
(the marker line fell into the buffered-plain path and parsed as inline
text). This was the previous session's ranked next-target #2.

### What landed

- `:   <div id="d">\n    x\n    </div>` → `Div ("d",[],[]) [Para [x]]`
  (0487), byte-lossless, matching pandoc-native. Also verified: loose vs
  tight, multi-line `<section>` strict-block (RawBlock/Plain/RawBlock),
  multi-paragraph div body — all structurally correct.
- `try_dispatch_definition_html_block` now branches on the probe's line
  count instead of a `consumed == 1` bool. Multi-line bodies reuse the
  list-item lift (`list_item_buffer::try_emit_html_block_lift`, made
  `pub(crate)`): strip `content_col` before the inner reparse (body parses
  as markdown, not an indented `CodeBlock`) + re-inject the stripped indent
  during graft. Single-line path unchanged.
- Gated `Dialect::Pandoc && bq_depth == 0`. The lift hardcodes the Pandoc
  HTML grammar and doesn't strip `> `, so def-in-blockquote multi-line falls
  through to the pre-existing losslessness gap (NOT a new regression).
- **Diagnosed but deferred** (own future sessions, not touched): (a)
  def-body comment/PI **trailing softbreak fusion** (`:   <!-- --> t\n
  more`) — panache's plain path already fuses + gets looseness right, but
  the HTML trailing-split makes a standalone block; needs a content-indent
  `SoftbreakFusion` variant bounded to the def body. (b) HTML on a *later*
  def-body line multi-line (`\n    <div>\n    x\n    </div>`) still drops
  indent (separate normal-container dispatch path).

### Files in committable diff

- `parser/core.rs` (`try_dispatch_definition_html_block` multi-line branch);
  `parser/utils/list_item_buffer.rs` (`try_emit_html_block_lift` →
  `pub(crate)`).
- Corpus `0487-…-multiline/`; allowlist (new html-block section) +
  report.{txt,json}.
- Paired parser goldens `html_block_div_definition_body_multiline_{pandoc,
  commonmark}` (+ snapshots); formatter golden
  `html_block_div_definition_body_multiline`.
- This RECAP.

### Suggested next sub-targets (ranked)

1. **Def-body comment/PI trailing softbreak fusion** (`:   <!-- --> t\n
   more` → `RawBlock, Plain/Para [t, SoftBreak, more]`). panache's plain
   def-body path already fuses AND gets Plain/Para looseness right; only the
   HTML trailing-split diverges (standalone unfused block). Needs a
   content-indent `SoftbreakFusion` variant bounded to the def-body content
   extent + looseness-aware demotion. Footnote bodies are ALREADY correct
   (comment stays inline). Medium.
2. **Later-line multi-line HTML in def/list body** (`\n    <div>\n    x\n
   </div>` on a non-marker def-body line → `Div [Para x]`, currently drops
   indent → `Div [CodeBlock x]` + losslessness fail). Separate
   normal-container dispatch path from the marker-line arm; the def
   container doesn't content-indent-normalize HTML blocks the way list
   items do. Medium.
3. **Multi-line-second-div inter-tag** (`<div>a</div> y <div>\nz\n</div>`).
   Content-scan depth model must treat a `</div> … <div>` close line as
   close-then-fresh-block. Completes the `<div>` inter-tag family. Not in
   corpus.
4. **Phase (paragraph-leads-tag)** (`foo <div>bar</div> baz`). Deep; inline
   layer has no block-tag classifier + line-anchored block model.
   Exploratory decision session first.

### New trap

Folded into Persistent traps ("List-item / definition-body HTML structural
lift") — the `probe_consumed`-branch multi-line dispatch reusing
`try_emit_html_block_lift`, the `bq_depth == 0` gate, and the deferred
later-line / def-in-bq / trailing-fusion gaps.

--------------------------------------------------------------------------------

## Earlier sessions (compact log)

Newest first. One line per session: date — phase/sub-target — pass
count delta — root cause / lever.

- 2026-07-08 — Phase E footnote-body marker-line HTML dispatch (`[^1]: <div>x</div>`) — html 287 → 289 (total 486/486) — `try_dispatch_footnote_html_block` in `handle_footnote_open_effect` mirrors Phase D but gated on `!html_block_cannot_interrupt` (extracted shared `isInlineTag` predicate) — footnotes keep comments/PIs/`<span>`/void-inline-block INLINE (unlike def bodies); `<div id>` anchor-index registration; formatter `FOOTNOTE_DEFINITION` marker-space drop fixed; corpus 0485/0486.
- 2026-07-08 — Phase D definition-body marker-line HTML dispatch (`:   <div>x</div>`, `:   <!-- hi -->`) — html 285 → 287 (total 484/484) — `try_dispatch_definition_html_block` arm in the def first-content-line cascade (mirrors bq/list/fence); throwaway-builder probe lifts only marker-line-closing blocks → byte-lossless single-line emit; `<div id>` anchor-index registration; multi-line-open + softbreak fusion deferred; corpus 0483/0484.
- 2026-07-08 — Phase C container softbreak fusion (fenced-div + blockquote; `<!-- --> t\nmore` fuses to one `Para`) — html 283 → 285 (html 100%, total 482/482) — `SoftbreakFusion` enum (`ToDocEnd`/`ToFencedDivClose`/`ToBlockquoteEnd`/`None`); `fenced_div_body_end` / `blockquote_body_end` bound the reparse at the container close; bq strips+re-injects each continuation `> ` prefix via `ContainerPrefixState`; list/content-indent still `None`; corpus 0481/0482. Mechanisms in Persistent.
- 2026-07-08 — corpus pin `<div>` inter-tag no-ws (`<div>foo</div>bar<div>baz</div>` → `Div`/`Plain`/`Div`) — html 282 → 283 — Phase B's `graft_same_line_div_peel` already handled the no-ws form; pinned as corpus 0480 + paired parser/formatter goldens; corrected stale `blocked.txt` item (1) that still claimed both ws+no-ws divergent. No parser change.
- 2026-07-02 — Phases A/B/C + 7e (single day cluster) — html 262 → 282 (total 479/479) — Phase C comment/PI trailing softbreak `at_outermost_level` (corpus 0390); Phase B `<div>` inter-tag `graft_same_line_div_peel` (0479); Phase A fenced-div-in-html-div `body_fence_depth` (0478); 7e `same_line_trailing_forces_opaque` for `<p>foo</p> bar <p>baz</p>` (0472/0475-0477); void strict-block `PANDOC_VOID_STRICT_BLOCK_TAGS` `<hr>`/`<col>`/`<meta>` sibling-not-nested (0470-0471). All levers in Persistent.
- 2026-07-02 — Phase 7b/7c blockquote lifts (`> </p></div>` standalone split 0467-0469; `> <section>foo\n> bar` open-only lift 0464-0466; non-div open-only body lift) — html 262 → 271 — dropped `bq_depth==0` guards; `emit_html_block_body` `open_only` + `&ContainerPrefix`; `emit_html_block_body_lifted_bq_messy`; projector `html_block_has_open_only_structural_lift`. Also disproved a `<div id>` anchor false-positive (stray `/tmp/panache.toml` `flavor=myst`, not a bug).
- 2026-06-29 — Phase 7b standalone-tag split (single line of ≥ 2 standalone close/void tags → one `HTML_BLOCK_TAG` each) — html 259 → 262 — parser early-branch `try_parse_standalone_block_tags_split` + `split_line_into_standalone_tags`; projector `html_block_is_standalone_tag_sequence` → `emit_html_block_structural` (no new byte walking); single-tag + multi-line + bq stay legacy; also removed stale `blocked.txt` 452/453.
- 2026-06-17 — Phase 7a single-construct opaque lift (comment/PI/verbatim → `HTML_BLOCK_RAW`) — html flat (CST-fidelity refactor) — `html_block_node_kind` retags wrapper at the two `start_node` sites; `wrapper_kind` stays `HTML_BLOCK` as behavior gate (byte-identical children); projector `html_raw_block` routes by kind; all ~8 consumers updated.
- 2026-05-18 — bq-in-listitem dispatch (option (a)) — block 15 → 17, html flat — `ListItemFinish::BqDispatch` + `Parser::dispatch_bq_after_list_item` hand post-`> ` content to caller instead of eager paragraph; 0452/0453 HTML-block stay blocked (dispatcher walks raw `lines[line_pos]` without list-marker strip).
- 2026-05-17 — Negative-space pin (`<video\n…>body</video>`, `<iframe\n…>` and bq variants) + bq-in-listitem first-line diagnosis (0452/0453) — html 253 → 257 — already-correct parser-side lift pinned; eager-paragraph root cause identified.
- 2026-05-11→15 — Phase 6 structural-lift waves (all traps in Persistent) — html 142 → 253 — non-div strict-block + bq + list-item lift; depth-aware same-line/multi-line close-line lift + multi-line-open+same-line-close (`pre_content`) + bq variants; same-line trailing-text lift; Comment/PI trailing split + indented `isInlineTag` demotion + list-item indent normalization. Levers: `is_pandoc_lift_eligible_block_tag`, `LastParaDemote`, `graft_document_children`, `matched_close_offset` + `try_split_close_line_depth_aware` + `split_close_marker_end`, `bq_multiline_close_lift_tag`, `BqPrefixState`/`LinePrefixState`, `try_emit_html_block_lift`; pruned vestigial `try_div_html_block`.
- 2026-05-08/11 — Phases 1-5 seed — html 0 → 142 — `HTML_BLOCK_DIV`/`INLINE_HTML_SPAN` retag, `HTML_ATTRS`, projector `inline_pending`, CM/Pandoc blockHtmlTags split, `closes_at_open_tag`, `pandoc_html_open_tag_closes`, `PANDOC_VOID_BLOCK_TAGS`, PARAGRAPH→PLAIN retag at YesCanInterrupt, `is_closing` field, pandoc `isInlineTag` (issue #10643).
