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
  tokens.** Expose attributes by tokenizing existing source bytes
  (split TEXT into `TEXT + WS + HTML_ATTRS{TEXT} + TEXT`).
  Synthetic tokens break the tree-text-equals-input invariant.
  Use source-byte slices (`&rest[..4]`), never literals (`"<div"`)
  for case-insensitive prefix matches.
- **Same-line `<div>foo</div>` is ONE `HTML_BLOCK_TAG`** — close
  lives inside a TEXT child of the open. Naive `strip_suffix('>')`
  grabs wrong `>`; scan to first **unquoted** `>`. Quoted attribute
  values hide `<` / `>`; tag-bracket scanners thread quote state
  across line boundaries (`count_tag_balance`,
  `find_multiline_open_end`, `pandoc_html_open_tag_closes`).
- **Multi-line open-tag close branches diverge by tag class** —
  void multi-line opens early-exit returning `end_line_idx + 1`
  BEFORE close-marker loop. `same_line_closed` short-circuit must
  guard `multiline_open_end.is_none()`.
- **Incomplete open tags (`<embed\n`, no `>` anywhere) caused
  projector infinite recursion.** Pandoc treats as paragraph text.
  Gate Pandoc BlockTag recognition on `pandoc_html_open_tag_closes`
  in `block_dispatcher::detect_prepared`. CommonMark stays liberal.
- **Self-closing `<tag/>` doesn't bump depth.** Depth-aware close
  matchers check `bytes[j-1] == b'/'` at closing `>`.
- **`input.lines()` strips newlines**; for losslessness-asserting
  parser tests use `split_lines_inclusive`.
- **`HtmlBlockType::BlockTag` is `Box<dyn Any>`-roundtripped via
  block dispatcher.** Adding a field works automatically; E0063
  points at every literal site.

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
- **`eitherBlockOrInline` is context-dependent.** Mirror needs BOTH
  parser-side `cannot_interrupt` (don't break running paragraph) AND
  projector-side `inline_pending` (don't split mid-text).
- **Closing forms of all matched-pair tag sets ARE block starts
  under Pandoc** — each emits `BlockTag { closes_at_open_tag: true }`.
  Dispatcher's `cannot_interrupt` keys on inline-block + void only:
  strict-block + verbatim closes get `YesCanInterrupt`; inline-block
  / void closes stay inline in running paragraphs.
- **Verbatim tags fire before inline-block / strict-block arms** —
  `VERBATIM_TAGS` checked first; script-in-eitherBlockOrInline +
  style/textarea-in-blockHtmlTags overlap is harmless.
- **Pandoc `isInlineTag` special cases (issue #10643):** `<style>`
  open+close, `</script>`, PIs, comments, `<script
  type="math/tex…">` (case-insensitive, single-line) cannot
  interrupt paragraph. `<pre>` / non-math-tex `<script>` /
  `<textarea>` DO interrupt. Implemented in
  `HtmlBlockParser::detect_prepared`'s `cannot_interrupt`;
  requires `is_closing: bool` on `HtmlBlockType::BlockTag`.
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
  `WHITESPACE`** when the token is NOT preceded by a
  `BLOCK_QUOTE_MARKER` on the same line. This covers the
  list-item indent re-injected by
  `strip_list_item_indent`/`LinePrefixState` (see "List-item
  HTML structural lift" section). The rule is unambiguous: the
  parser never emits a leading line-start `WHITESPACE` inside
  `HTML_BLOCK_CONTENT` or `HTML_BLOCK_TAG` outside the lift
  path — top-level indented HTML keeps the leading indent in a
  single `TEXT` token. The walker threads two flags
  (`skip_next_ws` for bq pairs, `at_line_start` for line-start
  WS) and flips `at_line_start` to `true` after each NEWLINE /
  BLANK_LINE token.
- **Projector `open_tag_raw_block_text` canonicalizes multi-line
  open tags.** With `HTML_ATTRS`, literal source diverges from
  pandoc's canonical single-line form (`normalize_native`
  preserves WS inside `"..."`). Helper walks
  `children_with_tokens`, takes leading `<tagname` TEXT, joins
  HTML_ATTRS trimmed texts with single spaces, appends `>`.
  Single-line opens without HTML_ATTRS keep literal text.

### Refs / footnotes / heading-id resolution

- **`parse_pandoc_blocks` swaps in an inner `RefsCtx`** for
  recursive reparse. Swap belongs IN `parse_pandoc_blocks`, not
  at call sites. `build_refs_ctx` mutates `REFS_CTX` mid-build —
  when swapping save outer FIRST via `mem::take`, THEN call
  `build_refs_ctx`, THEN install.
- **`heading_id_by_offset` is offset-keyed, not slug-keyed.**
  Inner CST's offsets are zero-based; don't copy outer
  `heading_ids` into inner. Build fresh inner ctx and inherit
  cross-boundary refs/footnotes via `build_refs_ctx_inherited`.
- **`fenced_div` walks structural CST via `collect_block`** —
  doesn't use `parse_pandoc_blocks`. Don't generalize the swap
  to fenced divs.
- **`AttributeNode::can_cast` accepts `HTML_ATTRS`**; the salsa
  walk picks up `<div id>` / `<span id>` and non-div strict-block
  tag ids (`<section id="x">`, etc.) automatically. Diverges
  from pandoc-native (which keeps them as RawBlock without
  lifting attrs) but matches user intent for anchor-link
  resolution. No parallel salsa walk.

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

`pandoc_ast.rs` is the public `to_pandoc_ast` API; linter / salsa
/ LSP / formatter walk the CST, not the projector. Phases 1/5
landed structural retags (`HTML_BLOCK_DIV`, `INLINE_HTML_SPAN`);
Phase 6 lifted inner content of `<div>` / non-div strict-block /
inline-block matched-pair shapes (non-bq + bq) into CST children.
Vestigial `<div>` byte walkers (`try_div_html_block`, etc.)
pruned 2026-05-11. Load-bearing remainder: `split_html_block_by_tags`
(opaque HTML_BLOCKs only), `parse_pandoc_blocks` (inter-tag text
reparse via `flush_html_block_text` /
`flush_html_block_tail_text`), `collect_html_block_text_skip_bq_markers`
(one `<pre>` verbatim-in-bq case + multi-line-open-in-bq
fallback), table-cell reparses. `html_div_block` `debug_assert!`s
on unlifted HTML_BLOCK_DIV.

### Structural lift (Fix #3 / Fix #4 family)

- **Recursive parse uses `parse_with_refdefs`, not `parse`.**
  `parse` re-runs `populate_refdef_labels` on JUST the inner
  text, hiding outer refdefs from inner reference links. Thread
  outer config's `refdef_labels` through.
- **`HTML_BLOCK_DIV` retag at dispatcher is two-pronged.** Retag
  fires iff `probe_open_tag_line_has_close_gt(ctx.content, "div")`
  (single-line) OR `pandoc_html_open_tag_closes(lines, line_pos,
  bq_depth)` (multi-line). Incomplete opens (`<div\n` no `>`
  anywhere) keep opaque HTML_BLOCK so projector treats as
  paragraph text. Multi-line + trailing on close-`>` line:
  `emit_multiline_open_tag_with_attrs` captures trailing into
  `pre_content` via `lift_trailing=true` so open `HTML_BLOCK_TAG`
  ends cleanly with `TEXT(">")`.
- **Lifted HTML_BLOCK / HTML_BLOCK_DIV MUST route structural,
  not byte path.** `collect_block` routes `HTML_BLOCK_DIV` →
  `html_div_block`; `emit_html_block` routes lifted HTML_BLOCK →
  `emit_html_block_structural` (not `split_html_block_by_tags`).
  Byte path's `parse_pandoc_blocks` builds fresh inner `RefsCtx`
  → re-disambiguates heading auto-ids, producing stray `-1`
  suffix. Body-lifted signal: no `HTML_BLOCK_CONTENT` child;
  `html_block_open_tag_is_clean` accepts TEXT ending in `>`.
- **`LastParaDemote` enum** on `graft_document_children`:
  `Never` (clean/unbalanced — Para preserved), `SkipTrailingBlanks`
  (div close-butted — demote LAST PARAGRAPH past trailing
  BLANK_LINEs), `OnlyIfLast` (non-div strict-block close —
  demote only when last child is PARAGRAPH with no trailing
  BLANK_LINE).
- **Multi-line open tags emit multiple `HTML_ATTRS` regions** —
  one per attribute line. Iterate + join with `" "` (see
  `cst_div_open_tag_attr`); `.children().find()` only sees first.
- **All non-bq shapes lift** for `<div>` and non-div Pandoc
  strict-block + inline-block matched-pair tags: clean
  multi-line, open-trailing, butted-close, indented-close,
  same-line, empty/blank-only, multi-line open + trailing.
- **Bq lift covers clean + same-line + messy + multi-line-open-
  clean.** Open-line `> ` consumed by outer BLOCK_QUOTE;
  subsequent lines' `> ` re-injected via `BqPrefixState`. Deeper
  bq (`> > <div>`) works transparently. `find_multiline_open_end`
  + `emit_multiline_open_tag_with_attrs/_simple` thread `bq_depth`
  and re-emit `BLOCK_QUOTE_MARKER + WHITESPACE` prefix tokens for
  lines past `start_pos` (line 0's prefix is owned by outer BQ).
- **Bq prefix re-injection: both `NEWLINE` *and* `BLANK_LINE`
  token (kind, not node) advance `line_idx`.** Inner parse puts
  `BLANK_LINE` token (text `"\n"`) inside `BLANK_LINE` node;
  treating only NEWLINE mis-aligns prefixes — losslessness
  violation when blank line precedes content line in body.
- **Three bq lift gates by `depth` after open line.** All require
  `bq_depth > 0` + `depth_aware_tag.is_some()` + tag in
  `is_pandoc_lift_eligible_block_tag`. Inline-block matched-pair
  also gates on NOT `inline_block_void_interior_abandons`.
  Discriminators:
  - `same_line_bq_lift_tag` — `depth <= 0`, single-line. Routes
    through `same_line_closed` branch; uses
    `emit_html_block_body_lifted` with `bq: &mut None`.
    Demote: div=SkipTrailingBlanks, non-div=OnlyIfLast.
  - `bq_clean_lift` — `depth > 0` + close line is `trim_start
    .starts_with("</")` + clean open (`pre_content.is_empty()`).
    Accepts single + multi-line opens. Calls
    `emit_html_block_body_lifted_bq`. Demote: div=Never (Para
    preserved), non-div=OnlyIfLast.
  - `bq_messy_lift_tag` — `depth > 0` + NOT clean. Accepts both
    open shapes; multi-line + trailing uses `lift_trailing` so
    trailing → `pre_content`. Close-marker site bq-strips then
    `try_split_close_line`. Calls
    `emit_html_block_body_lifted_bq_messy`. Demote: div keyed on
    close-butted (Never when `leading` empty, else
    SkipTrailingBlanks); non-div=OnlyIfLast.
- **`try_split_close_line` whitespace-only `leading` is close-tag
  indent, not body content.** For `   </article>`, classify
  whitespace-only via `leading.bytes().all(|b| b == b' ' || b ==
  b'\t')`, pass `body_leading=""` to recursive parse, emit
  leading bytes as `WHITESPACE` inside close `HTML_BLOCK_TAG`.
  Keep demote policy keyed on **original** `leading.is_empty()`.
- **Bq messy-lift duplicate-prefix trap.**
  `emit_html_block_body_lifted_bq_messy` injects close line's bq
  prefix in front of `leading` via BqPrefixState; close
  `HTML_BLOCK_TAG` MUST NOT re-emit `emit_bq_prefix_tokens`
  when `leading` is non-empty (doubles `> ` bytes).
- **Projector `open_tag_raw_block_text` strips bq markers AND
  leading 1-3 space indent.** Bq-wrapped close `> </form>`
  carries `BLOCK_QUOTE_MARKER + WHITESPACE` leading tokens;
  open-line `  <article>` carries standalone `WHITESPACE` before
  tag-name TEXT. Pandoc-native `RawBlock` text is tag bytes only
  — helper skips bq prefix pairs AND leading `WHITESPACE` before
  the accumulator collects its first non-WS token. HTML_ATTRS
  branch (multi-line open canonicalization) unaffected.

### List-item HTML structural lift

- **`ListItemBuffer::emit_as_block` lifts same-line / fully-
  contained HTML blocks via `try_emit_html_block_lift`.** Gate is
  strict: `try_parse_html_block_start` must recognize the first
  line, the inner reparse must produce exactly ONE top-level child
  of kind `HTML_BLOCK` / `HTML_BLOCK_DIV`, the child must consume
  every byte of the buffer text, and `HTML_BLOCK_DIV` requires
  ≥ 2 `HTML_BLOCK_TAG` children (matched open+close). Multi-line
  shapes (`- <section>\n  hello\n  </section>`, `- <video>\n  body\n
  </video>`) also lift as of 2026-05-13 — see "Close-form
  dispatcher gate" trap.
- **Close-form dispatcher gate (multi-line list-item HTML).** The
  dispatcher's HTML-block close-form recognition (`</div>`,
  `</section>`, `</pre>`, …) is gated on the enclosing LIST_ITEM
  buffer NOT having an unclosed matched-pair open of the same
  tag. Mechanism: `BlockContext::list_item_unclosed_html_block_tag:
  Option<String>` is populated in `parse_line` via
  `Parser::list_item_unclosed_html_block_tag` → `ListItemBuffer::
  unclosed_pandoc_matched_pair_tag` → which inspects the first
  buffer text segment with `try_parse_html_block_start`, checks
  it's a `BlockTag { is_closing: false }` matching
  `is_pandoc_matched_pair_tag`, then walks all buffer text
  segments calling `count_tag_balance`. When opens > closes,
  returns the tag name; `HtmlBlockParser::detect_prepared`
  returns `None` for close-forms whose tag matches the field.
  The buffer then accumulates the full matched-pair text, and
  `try_emit_html_block_lift` reparses + grafts. `count_tag_balance`,
  `is_pandoc_lift_eligible_block_tag`, and new
  `is_pandoc_matched_pair_tag` are now `pub(crate)`. The gate
  only fires under Pandoc dialect.
- **List-item indent normalization via `strip_list_item_indent`
  + `LinePrefixState` re-injection.** `emit_as_block` threads
  `Container::ListItem::content_col` to
  `try_emit_html_block_lift`. When `> 0`,
  `strip_list_item_indent` strips up to `content_col`
  leading-space bytes from each line after line 0 (line 0's
  leading is owned by the list marker), returns per-line
  prefix vector. Inner reparse runs on stripped text; graft
  re-injects each prefix as a `WHITESPACE` token at line start
  via `LinePrefixState` (mirrors `BqPrefixState`). Without
  this, `- <div>\n  body\n  </div>` triggers indented-close
  demote (Plain not Para) and `<pre>` keeps indent in RawBlock.
  Tab handling: advance col by 4 on `\t`, refuse to split a
  tab that would overshoot. Injected WHITESPACE inside opaque
  `HTML_BLOCK_CONTENT` / `HTML_BLOCK_TAG` is stripped by
  projector's `walk_skip_bq_markers` line-start rule; inside
  lifted PARAGRAPH/PLAIN it becomes leading `Inline::Space`
  and `coalesce_inlines` edge-trim drops it.
- **`format_list_item` silently drops `LIST_MARKER` when the
  list item has NO `PLAIN`/`PARAGRAPH` content_node.** The
  marker-emit pass is wired to the wrapping flow which produces
  no output without a content_node. Per-kind arms in the
  nested-blocks loop emit the marker when
  `no_content_emitted && is_first_real_child`: existing
  `HORIZONTAL_RULE` arm, added `HTML_BLOCK | HTML_BLOCK_DIV` arm
  for the same-line HTML lift. Any new structural lift that
  produces a list-item-as-block CST shape (HEADING-only,
  BLOCK_QUOTE-only, FENCED_DIV-only, etc.) MUST update
  `format_list_item` with the same pattern or the marker
  silently disappears. The `_` fallback at the end of the loop
  just calls `format_node_sync` with content_indent — it does
  NOT emit the marker.

--------------------------------------------------------------------------------

## Phase progress

| Phase | Description | Status |
|-------|-------------|--------|
| 1 | `<div>` block lift (HTML_BLOCK_DIV + HTML_ATTRS structural) | **Wrapper retag landed** (2026-05-08) — issue #263 closed; `<DIV>` losslessness fix landed. **Inner content NOT yet lifted into CST children** — still raw `HTML_BLOCK_CONTENT` TEXT tokens; projector reparses them. |
| 2 | `<span>` inline lift (INLINE_HTML_SPAN) | **Wrapper retag landed** (2026-05-08). Inner inlines mostly trivial (no recursive reparse needed). |
| 3 | Sectioning + verbatim corpus pin; `eitherBlockOrInline` lift | **Conformance landed** — non-void (2026-05-09); void (`<embed>`/`<area>`/`<source>`/`<track>`) (2026-05-10). Implementation leans on projector-side `inline_pending` tracking + byte walker; CST still opaque for split/matched-pair shapes. |
| 4 | Comments, PIs, declarations, CDATA projection | **Conformance landed** (2026-05-08); type-4 CM lowercase still gappy. CST opaque (these constructs project as RawBlock / RawInline). |
| 5 | `markdown_in_html_blocks` interaction edge cases | **Conformance landed** — depth-aware nested div, Plain/Para promotion, refs inheritance, **projector-level splitter** (`split_html_block_by_tags` byte walker + `parse_pandoc_blocks` recursive reparse), outer-matched-pair-abandons-on-void-interior. **The structural CST lift was deferred** — Phase 5's mechanism is the projector reparsing bytes, not the parser emitting structure. |
| 6 (new) | Lift inner HTML block content into structural CST children — `HTML_BLOCK_DIV` / `HTML_BLOCK` get `PARAGRAPH` / `LIST` / etc. as direct children; projector byte walkers become vestigial; `PARAGRAPH→PLAIN` retag at adjacent-HTML-block boundary. | **All non-bq + bq shapes lifted for `<div>` and non-div Pandoc strict-block tags as of 2026-05-12.** Shapes covered: clean multi-line, open-trailing, butted-close, indented-close, same-line, empty / blank-only, multi-line open (clean and trailing). Inline-block matched-pair abandons when body begins with a void block tag (Plain via OnlyIfLast). Bq via three discriminator gates (`bq_clean_lift`, `same_line_bq_lift_tag`, `bq_messy_lift_tag`) — see "Three bq lift gates" trap. Dispatcher's `HTML_BLOCK_DIV` retag gate uses `pandoc_html_open_tag_closes` AND requires `is_closing: false`. Vestigial `<div>` byte walkers pruned 2026-05-11. **As of 2026-05-12** same-line / fully-contained HTML blocks lift inside list items (`ListItemBuffer::emit_as_block` reparse + graft path); formatter's `format_list_item` gets a `HTML_BLOCK / HTML_BLOCK_DIV` arm to emit the marker for these. **As of 2026-05-13** multi-line HTML blocks lift inside list items for non-div strict-block + inline-block + verbatim matched-pair tags via a close-form dispatcher gate (`BlockContext::list_item_unclosed_html_block_tag` + `ListItemBuffer::unclosed_pandoc_matched_pair_tag`); **same day**, list-item indent normalization (`strip_list_item_indent` + `LinePrefixState`) closes the `<div>` Plain→Para gap and the verbatim-tag (`<pre>`, `<style>`, `<script>`, `<textarea>`) RawBlock-indent gap. Projector's `walk_skip_bq_markers` strips leading line-start `WHITESPACE` to make the parser-side re-injection invisible to opaque-HTML projection. **Pass count history: 105 → 181** (current). Open shape gaps tracked in latest session's "Suggested next sub-targets". |

--------------------------------------------------------------------------------

## Latest session — 2026-05-13 (list-item indent normalization for `<div>` / verbatim lift)

Top-ranked sub-target from previous session: list-item indent
normalization for the HTML-block first-line structural lift so
multi-line `<div>` in a list projects as `Div [Para [body]]`
(not `Plain`) and verbatim tags (`<pre>`, `<style>`,
`<script>`, `<textarea>`) emit RawBlock text without the
list-item leading indent baked in.

Implementation took the planned fix path from the previous
session's recap. `ListItemBuffer::emit_as_block` now accepts
`content_col: usize`, threaded from the enclosing
`Container::ListItem::content_col` at both call sites in
`parser/core.rs`. `try_emit_html_block_lift` calls a new
helper `strip_list_item_indent(text, content_col)` that
strips up to `content_col` leading-space bytes from each line
after the first and returns a per-line prefix vector. The
inner reparse runs on the stripped text; the gate validates
against the stripped length. A new `LinePrefixState` struct
(mirroring the existing `BqPrefixState` pattern in
`html_blocks.rs`) drives per-line WHITESPACE re-injection
during graft, keeping the CST byte-equal to source.

On the projection side, `walk_skip_bq_markers` (used by
`collect_html_block_text_skip_bq_markers` for opaque HTML_BLOCK
projection — verbatim tags, comments, PIs, etc.) gains a
second flag `at_line_start` and strips a leading `WHITESPACE`
token at the start of each source line when no
`BLOCK_QUOTE_MARKER` precedes it on the same line. The rule
is unambiguous: the parser never emits a leading line-start
`WHITESPACE` inside `HTML_BLOCK_CONTENT` outside the lift
path (top-level indented HTML keeps the indent in a single
TEXT token), and structural `HTML_BLOCK_TAG` already had a
leading-WS strip via `open_tag_raw_block_text`. For lifted
`<div>` body, the injected `WHITESPACE` lives inside
PARAGRAPH / PLAIN; inline projection converts it to a leading
`Inline::Space` which `coalesce_inlines`' edge-trim rule
drops automatically.

Probed corpus shapes (all match pandoc-native after fix):
- `- <div>\n  body\n  </div>` → `Div [Para [Str "body"]]` ✓
  (was `Div [Plain [body]]`).
- `- <pre>\n  body\n  </pre>` → `RawBlock "<pre>\nbody\n</pre>"`
  ✓ (was `"<pre>\n  body\n  </pre>"`).
- `- <style>\n  body { color: red; }\n  </style>` → indent
  stripped ✓.
- `- <script>\n  alert(1);\n  </script>` → ✓.
- `- <textarea>\n  hello\n  </textarea>` → ✓.

Conformance: html 176 → 181, total 369 → 374 (+5).
Parser-crate 382 → 386 (added 4 paired fixtures: div +
pre, Pandoc + CommonMark each).

### What landed

- `parser/utils/list_item_buffer.rs`: `emit_as_block` gains
  `content_col` parameter. `try_emit_html_block_lift`
  threads it through to a new helper
  `strip_list_item_indent` (tab-aware, refuses to split a
  tab that overshoots `content_col`). New `LinePrefixState`
  struct + `emit_grafted_token` drive per-line `WHITESPACE`
  re-injection during graft (mirroring `BqPrefixState` in
  `html_blocks.rs`). Existing `graft_node` signature
  extended to take `&mut Option<LinePrefixState>`.
- `parser/core.rs`: both `emit_as_block` call sites
  (`close_containers_to` ListItem branch +
  `emit_list_item_buffer_if_needed`) destructure
  `Container::ListItem { content_col, .. }` and pass it
  through.
- `pandoc_ast.rs`: `collect_html_block_text_skip_bq_markers`
  + `walk_skip_bq_markers` gain `at_line_start` tracking;
  leading `WHITESPACE` at line start (not preceded by
  `BLOCK_QUOTE_MARKER`) is stripped. Doc-comment updated to
  describe both prefix-stripping rules.
- Parser fixtures (paired):
  `list_item_html_div_multiline_para_{pandoc,commonmark}`,
  `list_item_html_pre_multiline_{pandoc,commonmark}`.
  Pandoc snapshots show the structural lift with injected
  `WHITESPACE` tokens for the stripped indent; CommonMark
  snapshots show the existing inline-HTML-in-Plain behavior
  (no lift, no stripping).
- Formatter goldens:
  `list_item_html_div_multiline_para`,
  `list_item_html_pre_multiline` — pin idempotent
  round-trip.
- Corpus 0370 – 0374 pin pandoc-native for div + 4 verbatim
  tags as list-item content.
- Snapshot
  `parser_cst_list_item_html_section_multiline_pandoc`
  updated: PLAIN now has `WHITESPACE@12..14 "  " +
  TEXT@14..19 "hello"` instead of `TEXT@12..19 "  hello"`
  (same byte range — TEXT-coalescence diff per parser.md,
  but it's actually structurally adding the WHITESPACE
  token for indent re-injection; pre-fix it was a single
  TEXT, post-fix it's the structural split).

### Files in committable diff

- `crates/panache-parser/src/parser/utils/list_item_buffer.rs`
- `crates/panache-parser/src/parser/core.rs`
- `crates/panache-parser/src/pandoc_ast.rs`
- `crates/panache-parser/tests/fixtures/{cases,pandoc-conformance/corpus}/`
  + snapshots + `golden_parser_cases.rs`
- `tests/fixtures/cases/list_item_html_{div_multiline_para,pre_multiline}/`
  + `tests/golden_cases.rs`
- `crates/panache-parser/tests/pandoc/{allowlist.txt,report.txt}`
  + `docs/development/pandoc-report.json`

### Suggested next sub-targets

1. **Comment + trailing-text split.** `<!-- comment --> body`
   (same-line) and multi-line variants emit a single
   `RawBlock` containing both bytes; pandoc splits into
   `RawBlock "<!-- comment -->"` + `Para "body"`. The trailing
   text also joins subsequent non-blank lines into one
   paragraph. Requires either parser-side truncation (HTML
   block ends at `-->`, trailing bytes go back to paragraph
   parsing — substantial: dispatcher currently consumes whole
   lines) or projector-side split-and-merge (detect adjacent
   PARAGRAPH and merge bytes). Same for type-3 PIs
   (`<?...?>`). High leverage; many corpus gaps possible.
2. **`<span>` (and other inline-block) lift inside paragraph
   text, mid-line.** Probed `text <span id="x">body</span>
   more text` mid-paragraph already matches pandoc; worth
   pinning in corpus to lock the IR-migration trap (emphasis
   must not pair into span content). Low risk, low effort —
   pure corpus expansion.
3. **Multi-line `<div>` open in list-item.** `- <div\n  id="x">\n  body\n  </div>`
   — multi-line open tag inside a list item. Currently
   `try_parse_html_block_start` looks only at the first line.
   Probably falls back to opaque HTML_BLOCK. Probe + classify
   bucket.
4. **Tab-indented list items.** `strip_list_item_indent`
   advances 4 cols per tab but refuses to split a tab. Probe
   tab-indented list items with HTML blocks to confirm the
   stripping does the right thing.

### New traps

Folded into Persistent traps:
- "List-item indent normalization via `strip_list_item_indent`
  + `LinePrefixState`" — replaces the old "indent normalization
  gap" entry under "List-item HTML structural lift".
- "`walk_skip_bq_markers` also strips leading line-start
  `WHITESPACE`" — new bullet under "Projector tag splitting".

--------------------------------------------------------------------------------

## Earlier sessions (compact log)

Newest first. One line per session: date — phase/sub-target — pass
count delta — root cause / lever.

- 2026-05-13 — Phase 6 — multi-line list-item HTML lift via close-form dispatcher gate (`- <section>...`, `- <video>...`, `- <iframe>...`, `- <span>...`) — html 171 → 176 — `BlockContext::list_item_unclosed_html_block_tag` + `ListItemBuffer::unclosed_pandoc_matched_pair_tag`; `count_tag_balance` / `is_pandoc_lift_eligible_block_tag` / new `is_pandoc_matched_pair_tag` promoted to `pub(crate)`; close-form `</tag>` dispatch suppressed when the enclosing LIST_ITEM has an unclosed matched-pair open. Indent gap for `<div>` body and verbatim content deferred to next session.
- 2026-05-12 — Phase 6 wave (same-line list-item lift, butted-close WS routing, projector leading-indent strip, `</div>` standalone retag fix, multi-line-open trailing lift, bq multi-line open lift) — html 159 → 171 — `ListItemBuffer::try_emit_html_block_lift`, formatter LIST_MARKER arm, `open_tag_raw_block_text` leading-WS strip, dispatcher `is_closing: false` retag gate, `emit_multiline_open_tag_with_attrs` `lift_trailing` + `pre_content`, `find_multiline_open_end` bq_depth, `bq_lift_tag`/`bq_messy_lift_tag` drop `multiline_open_end.is_none()`.
- 2026-05-11 — Phase 6 bq lift arc (Fix #5 clean + HTML_ATTRS-in-bq, Fix #7 same-line, Fix #8 messy) + `<div>` byte-walker prune in `pandoc_ast.rs` (~170 net lines) — html stable 159 — three discriminator gates (`bq_clean_lift`, `same_line_bq_lift_tag`, `bq_messy_lift_tag`), `BqPrefixState` re-injection, `inline_block_void_interior_abandons`, `bq_strict_attr_emit_tag_name`, `open_tag_raw_block_text` bq-prefix strip; `html_div_block` `debug_assert!`s on unlifted HTML_BLOCK_DIV.
- 2026-05-11 — Phase 6 / Fix #4 non-div strict-block shape sweep + multi-line open-tag lift — html 142 → 159 — `is_pandoc_lift_eligible_block_tag`, `html_block_has_structural_lift`, `LastParaDemote::{OnlyIfLast,SkipTrailingBlanks,Never}`, `parse_with_refdefs` graft, `emit_multiline_open_tag_with_attrs`, `open_tag_raw_block_text` canonicalizer.
- 2026-05-10 → 2026-05-11 — Phase 6 cannot_interrupt + Fix #1/#2 — html 132 → 142 — PARAGRAPH→PLAIN retag at YesCanInterrupt; `is_closing` field; `is_math_tex_script_open`; pandoc `isInlineTag` (issue #10643).
- 2026-05-10 — Strict-block/verbatim closing-form lift, multi-line void open-tag, incomplete-open recursion fix, Phase 3 void `eitherBlockOrInline` — html 105 → 132 — `closes_at_open_tag`, `pandoc_html_open_tag_closes` gate, `PANDOC_VOID_BLOCK_TAGS`.
- 2026-05-08 → 2026-05-09 — Phases 1-5 seed + projector-side lift (issue #263 closed; non-void eitherBlockOrInline; HTML5 sectioning; `<DIV>` losslessness; Plain/Para; multi-line attrs; refs inheritance) — html 0 → 105 — `HTML_BLOCK_DIV`/`INLINE_HTML_SPAN` retag, `HTML_ATTRS` tokenization, sectioning/verbatim corpus pin, depth-aware nested `<div>`, projector `inline_pending` + parser `cannot_interrupt`, CM/Pandoc blockHtmlTags split, `build_refs_ctx_inherited`.
