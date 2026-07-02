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
- **Coverage: all non-bq shapes** lift for `<div>` + non-div strict-
  block + inline-block matched-pair (clean multi-line, open-trailing,
  butted-close, indented-close, same-line, empty/blank, multi-line
  open + trailing). **Bq: clean + same-line + messy + multi-line-open-
  clean.** Line-0 `> ` owned by outer BLOCK_QUOTE; later lines'
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

### List-item HTML structural lift

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
| 1 | `<div>` block lift (HTML_BLOCK_DIV + HTML_ATTRS structural) | **Wrapper retag landed** (2026-05-08) — issue #263 closed; `<DIV>` losslessness fix landed. **Inner content NOT yet lifted into CST children** — still raw `HTML_BLOCK_CONTENT` TEXT tokens; projector reparses them. |
| 2 | `<span>` inline lift (INLINE_HTML_SPAN) | **Wrapper retag landed** (2026-05-08). Inner inlines mostly trivial (no recursive reparse needed). |
| 3 | Sectioning + verbatim corpus pin; `eitherBlockOrInline` lift | **Conformance landed** — non-void (2026-05-09); void (`<embed>`/`<area>`/`<source>`/`<track>`) (2026-05-10). Implementation leans on projector-side `inline_pending` tracking + byte walker; CST still opaque for split/matched-pair shapes. |
| 4 | Comments, PIs, declarations, CDATA projection | **Conformance landed** (2026-05-08); type-4 CM lowercase still gappy. CST opaque (these constructs project as RawBlock / RawInline). |
| 5 | `markdown_in_html_blocks` interaction edge cases | **Conformance landed** — depth-aware nested div, Plain/Para promotion, refs inheritance, **projector-level splitter** (`split_html_block_by_tags` byte walker + `parse_pandoc_blocks` recursive reparse), outer-matched-pair-abandons-on-void-interior. **The structural CST lift was deferred** — Phase 5's mechanism is the projector reparsing bytes, not the parser emitting structure. |
| 7b | Standalone-tag split — single line of ≥ 2 standalone block-level tags (close + void) emits one `HTML_BLOCK_TAG` per tag. | **Landed 2026-06-29; bq extension landed 2026-07-02.** Parser early-branch `try_parse_standalone_block_tags_split` (Pandoc, `closes_at_open_tag: true`, line parses as ≥ 2 tags). Projector routes via `html_block_is_standalone_tag_sequence` → `emit_html_block_structural` (no new byte walking). **Bq extension:** dropped the `bq_depth == 0` guard — the `> ` prefix is a sibling of `HTML_BLOCK`, so `strip_line_0_for_emission` yields clean tags and `> </p></div>` splits structurally (corpus 0467-0469). Single tags stay baked; if the prefix strip leaves non-tag bytes the segment scan bails to the byte walker. CommonMark keeps baked shape. html 259 → 262 → 271. |
| 7c | Open-only body lift — non-div strict-block / inline-block open tag + trailing body + no matching close (`<section>foo`, `<video>bar\nbaz`) lifts the body into structural CST children. | **Landed 2026-07-02; bq extension landed 2026-07-02 (same day).** `emit_html_block_body` gained a non-div `HTML_BLOCK` + `bq_depth == 0` lift arm (mirrors the existing unbalanced-`<div>` arm, `LastParaDemote::Never`) → open `HTML_BLOCK_TAG` + recursively-parsed body (`PARAGRAPH`/`HEADING`/`LIST`/…) as siblings, no `HTML_BLOCK_CONTENT`. Projector routes via new `html_block_has_open_only_structural_lift` predicate (1 clean open tag, no HTML_BLOCK_CONTENT, ≥ 1 block child) → `emit_html_block_structural`. Matches pandoc-native `RawBlock "<section>"` + `Para [foo]` (open = lone RawBlock, body = fresh siblings, Para preserved). **Bq extension:** the same shape inside a blockquote (`> <section>foo\n> bar`) now lifts too — `emit_html_block_body` takes `prefix: &ContainerPrefix` + an `open_only` flag; the no-close path with `bq_depth > 0` routes through `emit_html_block_body_lifted_bq_messy` (empty leading/close-prefix) so continuation-line `> ` markers re-inject at line start. CommonMark keeps baked shape. html 262 → 265 → 268. |
| 7e | Multi-tag interleave (tags + inter-tag text on one block). | **Same-line non-div matched-pair inter-tag text FIXED 2026-07-02** (option b). `same_line_trailing_forces_opaque` suppresses the same-line strict-block lift when the after-first-close trailing (ws-trimmed) starts with non-`<` text AND holds a later matched-pair tag → whole line kept opaque `HTML_BLOCK` → projector `split_html_block_by_tags` produces the flat RawBlock/Plain split. `<p>foo</p> bar <p>baz</p>` (0472), no-ws (0475), tail→Para (0476), consecutive-stay-lifted (0477). CommonMark unaffected (always type-6 opaque). **Still open:** `<div>` inter-tag (needs Div-per-tag, risks HTML_BLOCK_DIV panic), and the general paragraph-LEADS-tag inline boundary (`foo <p>bar</p>`). Neighbors 0473-0474 still pinned. |
| 7a | Single-construct opaque lift — comment / PI / verbatim retag to `HTML_BLOCK_RAW` so the projector routes by kind. | **Landed 2026-06-17.** New `HTML_BLOCK_RAW` wrapper applied under `Dialect::Pandoc` via `html_block_node_kind` at the two `start_node` sites in `parse_html_block_with_wrapper` (incl. the comment/PI trailing-split head); `wrapper_kind` stays `HTML_BLOCK` so all lift gates + child tokens are unchanged (byte-lossless, `HTML_BLOCK_DIV` precedent). Projector `collect_block` → `html_raw_block` → one `RawBlock` (trailing-trim + 1-3 leading-space strip via `html_raw_block_text`); `emit_html_block` byte-sniff arm now dead for Pandoc. All consumers updated (formatter ×~8, list-item lift gate, folding, html_entities, both directives copies). Conformance **flat** (CST-fidelity refactor — report.txt byte-identical); 6 paired parser goldens + 2 formatter goldens added. **Remaining (7b-7e roadmap, NOT done): standalone single-tag (close/void), single open + trailing, void sequences, multi-tag interleave (D3) — `split_html_block_by_tags` + `parse_pandoc_blocks` still serve those.** |
| 6 (new) | Lift inner HTML block content into structural CST children — `HTML_BLOCK_DIV` / `HTML_BLOCK` get `PARAGRAPH` / `LIST` / etc. as direct children; projector byte walkers become vestigial; `PARAGRAPH→PLAIN` retag at adjacent-HTML-block boundary. | **All non-bq + bq shapes lifted for `<div>` and non-div Pandoc strict-block tags.** Shapes covered: clean multi-line, open-trailing, butted-close, indented-close, same-line, same-line + trailing-text-after-close, empty / blank-only, multi-line open (clean and trailing), depth-aware nested same-tag (`<div><div>x</div></div>` and trailing variants), multi-close trailing (`<div>foo</div></div>` and variants — projects as `Div + RawBlock` per pandoc-native), unclosed `<div>` (projects as `Div [...]` with implicit close), multi-line open + matched close in `pre_content` (single-close, nested, trailing-close, trailing-text — `<div\n  id="x">foo</div>` / `<div\n  id="x">foo</div></div>` / `<div\n  id="x"><div>x</div></div>` / `<div\n  id="x">foo</div>bar` and strict-block `<form\n  id="x">foo</form>`, **at top level and inside a blockquote** via `bq_multiline_close_lift_tag`). Inline-block matched-pair abandons when body begins with a void block tag (Plain via OnlyIfLast). Bq via four discriminator gates (`bq_clean_lift`, `same_line_bq_lift_tag`, `bq_messy_lift_tag`, `bq_multiline_close_lift_tag`). Dispatcher's `HTML_BLOCK_DIV` retag gate uses `pandoc_html_open_tag_closes` AND requires `is_closing: false`. Same-line / multi-line close-line lift paths use depth-aware split (`matched_close_offset` + `try_split_close_line_depth_aware`) + `split_close_marker_end` + trailing graft. `div_has_structural_inner` accepts unclosed div (1 HTML_BLOCK_TAG + structural body, no close). List items: same-line / fully-contained lift via `ListItemBuffer::emit_as_block` reparse + graft (formatter `format_list_item` HTML_BLOCK arm); multi-line lift via close-form dispatcher gate (`BlockContext::list_item_unclosed_html_block_tag` + `ListItemBuffer::unclosed_pandoc_matched_pair_tag`); indent normalization via `strip_list_item_indent` + `LinePrefixState` re-injection (projector `walk_skip_bq_markers` line-start-WS strip). List-item Comment/PI trailing-text via 2-child `try_emit_html_block_lift` branch + formatter `find_content_node` PLAIN-after-HTML_BLOCK guard. Inline-block matched-pair multi-line-open + same-line close (`<video\n  src="x">body</video>` / `<iframe\n  ...>...</iframe>` and bq variants) works transparently via the existing parser-side structural lift (open `HTML_BLOCK_TAG` + PLAIN body + close `HTML_BLOCK_TAG`, no HTML_BLOCK_DIV retag), pinned by 0448-0451. **Bq-in-listitem first-line dispatch landed 2026-05-18** via `ListItemFinish::BqDispatch` + `Parser::dispatch_bq_after_list_item` helper — fixes headings/HRs/etc. on `- > # heading` etc. (pinned by corpus 0454/0455 in `block` section). **Pass count history: 105 → 257.** bq-in-listitem first-line HTML block (`- > <div>...`, corpus 0452/0453) was **unblocked** by a later (unrecorded-in-RECAP) `ContainerPrefix` session — see the `# html-block (bq-in-listitem first-line HTML…)` allowlist comment near line 580; both now pass + allowlisted, and the stale `blocked.txt` 452/453 entry was removed 2026-06-29. |

--------------------------------------------------------------------------------

## Latest session — 2026-07-02 (7e — same-line matched-pair inter-tag text fix)

Conformance: **html 275 → 279** (0472 flipped to passing + 3 new
regression corpus 0475-0477). 477 total, 1 fail (blocked 0390 only —
0472 removed from blocked.txt). Workspace: only the pre-existing
`r_air_formats_equals_spacing_in_quarto_r_block` external-formatter
failure (fails on clean baseline too — unrelated). Allowlists,
clippy, fmt, full parser suite all green.

Took the prior #1 target (0472). The blocked note called both fixes
out-of-scope, but probing showed the divergence is NARROWER than
recorded: only non-div matched-pair strict-block tags with same-line
inter-tag TEXT diverge, and option (b) (keep line opaque → projector
splitter) is a small, well-gated parser change — not the broad
inline-parser rewrite the note feared.

### What landed

- **Parser fix (bucket: parser-shape).** New
  `same_line_trailing_forces_opaque(line, tag)` +
  `trailing_contains_matched_pair_tag` in `html_blocks.rs`; wired as a
  `&& !…` guard into `same_line_strict_lift_safe`. When it fires,
  `strict_block_lift` → false → `lift_mode` false → open tag emits as
  one opaque TEXT token → single opaque `HTML_BLOCK` → existing
  projector `split_html_block_by_tags` produces pandoc's flat
  RawBlock/Plain split (inter-tag → Plain, tail-after-last → Para).
- Gate is precise: fires only when after-first-close trailing
  (ws-trimmed) starts with non-`<` text AND contains a later
  matched-pair tag. Consecutive / ws-only (`<p>a</p><p>b</p>`) trim to
  `<`-leading → keep clean sibling-lift; void-only (`<embed>`, 0474)
  → no matched-pair tag → tail Para. Verified all 7 hand-probed
  variants match pandoc-native; formatter idempotent.
- Non-div only. `<div>` inter-tag left divergent (would need Div-per-
  tag; risks the HTML_BLOCK_DIV-without-structural-inner panic).
- Fixtures: 2 paired parser goldens (pandoc + commonmark, identical
  opaque CST), 1 formatter golden (idempotency), 3 corpus guards
  (0475 no-ws, 0476 tail→Para, 0477 consecutive).

### Files in committable diff

- `crates/panache-parser/src/parser/blocks/html_blocks.rs` (helpers +
  gate).
- `crates/panache-parser/tests/` (2 parser goldens + snapshots,
  golden_parser_cases list, corpus 0475-0477, allowlist 472/475/476/
  477 + reworded 0473/0474 header, blocked.txt, report{.txt,.json}).
- `tests/` (1 formatter golden + golden_cases list).
- `.claude/skills/html-conformance/RECAP.md`.

### Suggested next sub-targets

1. **`<div>` inter-tag** (`<div>x</div> y <div>z</div>` → `Div`/`Plain`/
   `Div`) — the div analog of what just landed. Needs the splitter (or
   a structural lift) to emit `Div` per tag; watch the HTML_BLOCK_DIV
   panic. Not in corpus yet — seed it first.
2. **Softbreak (0390)** — Comment/PI trailing-text softbreak
   continuation (`<!-- hi --> trailing\nmore`); needs open-paragraph
   carry across the HTML-block boundary or projector-side Para fusion.
   Last remaining corpus failure.
3. **General inline-parser boundary** (`foo <p>bar</p>` where a
   paragraph LEADS the tag) — the broad case the 0472 note feared;
   still unaddressed, needs its own phasing.

### New trap

Folded into Persistent traps (Pandoc tag categorization) — updated the
block-level-tag-mid-paragraph bullet to record the same-line fix and
the two remaining sub-cases.

--------------------------------------------------------------------------------

## Earlier sessions (compact log)

Newest first. One line per session: date — phase/sub-target — pass
count delta — root cause / lever.

- 2026-07-02 — 7e same-line matched-pair inter-tag text triage — html 273 → 275 — no code; found only same-line non-div matched-pair diverges; pinned neighbors 0473/0474; blocked 0472 (later fixed same day).
- 2026-07-02 — Void strict-block tags (`<hr>`/`<col>`/`<meta>` → sibling `HTML_BLOCK`s, not nested) — html 271 → 273 — new `PANDOC_VOID_STRICT_BLOCK_TAGS`; `closes_at_open_tag: true` under Pandoc; excluded from lift/matched-pair but kept out of `cannot_interrupt`; resolves the 7c `<hr>`-nesting quirk; corpus 0470-0471.
- 2026-07-02 — Phase 7c blockquote open-only lift (`> <section>foo\n> bar` → structural PARAGRAPH/HEADING child) — html 265 → 268 — `emit_html_block_body` takes `&ContainerPrefix` + `open_only` flag; no-close bq path routes through `emit_html_block_body_lifted_bq_messy`; projector unchanged; corpus 0464-0466.
- 2026-07-02 — Phase 7b blockquote standalone-tag split (`> </p></div>` → one `HTML_BLOCK_TAG` per tag) — html 268 → 271 — dropped `try_parse_standalone_block_tags_split`'s `bq_depth == 0` guard (`> ` prefix is an `HTML_BLOCK` sibling, strip yields clean tags, non-tag bytes bail to byte walker); projector unchanged; corpus 0467-0469. Also disproved prior target #4 (`<div id>` anchor false-positive was a stray `/tmp/panache.toml` `flavor=myst`, not a bug).
- 2026-07-02 — Phase 7c open-only body lift (non-div strict-block / inline-block open + trailing body, no close → structural CST body) — html 262 → 265 — `emit_html_block_body` non-div `HTML_BLOCK` + `bq_depth == 0` lift arm; projector `html_block_has_open_only_structural_lift`; bq stayed opaque; `<hr>`-nesting side effect pinned by `writer_html_blocks`.
- 2026-06-29 — Phase 7b standalone-tag split (single line of ≥ 2 standalone close/void tags → one `HTML_BLOCK_TAG` each) — html 259 → 262 — parser early-branch `try_parse_standalone_block_tags_split` + `split_line_into_standalone_tags`; projector `html_block_is_standalone_tag_sequence` → `emit_html_block_structural` (no new byte walking); single-tag + multi-line + bq stay legacy; also removed stale `blocked.txt` 452/453.
- 2026-06-17 — Phase 7a single-construct opaque lift (comment/PI/verbatim → `HTML_BLOCK_RAW`) — html flat (CST-fidelity refactor) — `html_block_node_kind` retags wrapper at the two `start_node` sites; `wrapper_kind` stays `HTML_BLOCK` as behavior gate (byte-identical children); projector `html_raw_block` routes by kind; all ~8 consumers updated.
- 2026-05-18 — bq-in-listitem dispatch (option (a)) — block 15 → 17, html flat — `ListItemFinish::BqDispatch` + `Parser::dispatch_bq_after_list_item` hand post-`> ` content to caller instead of eager paragraph; 0452/0453 HTML-block stay blocked (dispatcher walks raw `lines[line_pos]` without list-marker strip).
- 2026-05-17 — Negative-space pin (`<video\n…>body</video>`, `<iframe\n…>` and bq variants) + bq-in-listitem first-line diagnosis (0452/0453) — html 253 → 257 — already-correct parser-side lift pinned; eager-paragraph root cause identified.
- 2026-05-11→15 — Phase 6 structural-lift waves (all traps in Persistent) — html 142 → 253 — non-div strict-block + bq + list-item lift; depth-aware same-line/multi-line close-line lift + multi-line-open+same-line-close (`pre_content`) + bq variants; same-line trailing-text lift; Comment/PI trailing split + indented `isInlineTag` demotion + list-item indent normalization. Levers: `is_pandoc_lift_eligible_block_tag`, `LastParaDemote`, `graft_document_children`, `matched_close_offset` + `try_split_close_line_depth_aware` + `split_close_marker_end`, `bq_multiline_close_lift_tag`, `BqPrefixState`/`LinePrefixState`, `try_emit_html_block_lift`; pruned vestigial `try_div_html_block`.
- 2026-05-08/11 — Phases 1-5 seed — html 0 → 142 — `HTML_BLOCK_DIV`/`INLINE_HTML_SPAN` retag, `HTML_ATTRS`, projector `inline_pending`, CM/Pandoc blockHtmlTags split, `closes_at_open_tag`, `pandoc_html_open_tag_closes`, `PANDOC_VOID_BLOCK_TAGS`, PARAGRAPH→PLAIN retag at YesCanInterrupt, `is_closing` field, pandoc `isInlineTag` (issue #10643).
