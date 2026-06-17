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
- **A new HTML wrapper retag (`HTML_BLOCK_RAW`, `HTML_BLOCK_DIV`, …)
  must be added to EVERY consumer that matches the old kind**, or the
  block silently mis-formats / drops. For `HTML_BLOCK_RAW` (Phase 7a)
  that meant: formatter match arms (`formatter/core.rs` ×3,
  `formatter/lists.rs` ×4, `utils.rs` block-kind set), the list-item
  lift gate (`list_item_buffer.rs` — both the single-child and
  2-child `matches!`), LSP `folding_ranges.rs`, linter
  `html_entities.rs` excluded-ancestors, and BOTH `directives.rs`
  copies (`src/` + `crates/panache-formatter/src/`). Grep the old
  kind across `crates/` + `src/` before assuming coverage. The retag
  fires under `Dialect::Pandoc`, so Quarto/RMarkdown docs see it too —
  not just the conformance harness.

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
- **Indented `isInlineTag` demotes to `Para [RawInline]`** under
  Pandoc — the same set as `cannot_interrupt` (Comment, PI,
  `<style>` o+c, `</script>`, math-tex `<script>`, Type7, inline-
  block matched-pair, void block tags). Parser-side gate in
  `HtmlBlockParser::detect_prepared` returns `None` when
  `leading_spaces(ctx.content) > list_indent_info.content_col`,
  so paragraph parsing picks up the line and emits `RawInline`.
  Trap: `ctx.content` retains list-item content_col indent
  (NOT auto-stripped). Blockquote markers ARE stripped from
  `ctx.content` — bq cases work transparently.
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
- **Formatter non-idempotency for tab-indented list items.**
  `-\t<div>\n\thello\n\t</div>` parses correctly as
  `Div [Para [Str "hello"]]` but the formatter normalizes
  `-\t` to `- ` while keeping body tabs — round-trip then
  re-parses as `Div [CodeBlock "hello"]` (tab exceeds new
  content_col 2). Formatter bug, not html-conformance.
  Parser fixtures + conformance pin parser side only; no
  formatter goldens for tab-indented list-item HTML shapes.
  Fix likely in `formatter/lists.rs`.

### Latent projector panic on unstructural HTML_BLOCK_DIV

`pandoc_ast.rs::html_div_block` `debug_assert!`s on
`HTML_BLOCK_DIV` lacking structural inner shape ("parser
regression"). Any future parser change that retags
`HTML_BLOCK_DIV` MUST guarantee structural lift; otherwise the
dispatcher's retag without a matching body lift will panic at
projection time. Prefer "fall back to opaque HTML_BLOCK" over
silently emitting a one-child HTML_BLOCK_DIV. `div_has_structural_inner`
accepts a missing close tag (unclosed `<div>` projects as `Div`
with implicit close per pandoc-native warning) as of 2026-05-15,
so 1 HTML_BLOCK_TAG (clean open) + structural body + no
HTML_BLOCK_CONTENT is sufficient. Same-line lift gate
(`probe_same_line_lift`) is depth-aware (walks via
`matched_close_offset`) as of 2026-05-15 — accepts nested same-
tag opens and unmatched trailing closes alongside the single-close
shape. Same-line body split now uses
`try_split_close_line_depth_aware` (NOT `try_split_close_line`);
the latter remains for paths where the strict `(0, 1)` count is
intentional. The multi-line close-line lift (`<div>\nfoo</div>\n…`
path) also uses the depth-aware split + `split_close_marker_end`
trailing graft as of 2026-05-15. **Multi-line open + matched close
in `pre_content`** (`<div\n  id="x">foo</div>` and depth-aware
variants — nested, trailing close, trailing text — both at top
level and inside a blockquote) lifts via a branch BEFORE the
legacy `same_line_closed` block, gated on
`multiline_open_end.is_some() && depth_aware_tag.is_some() &&
depth <= 0 && lift_mode && (bq_depth == 0 ||
bq_multiline_close_lift_tag.is_some()) && !pre_content.is_empty()`.
Uses the same `try_split_close_line_depth_aware` +
`split_close_marker_end` + `graft_document_children` pattern as
the single-line same-line path, just returns `end_line_idx + 1`
instead of `start_pos + 1`. The bq variant inherits the bq prefix
from the open's last line (already re-emitted by
`emit_multiline_open_tag_with_attrs` for lines past `start_pos`),
so the body and close graft with `bq: &mut None` —
`emit_html_block_body_lifted` suffices, no BqPrefixState plumbing
needed for this shape.

### Projector-as-second-stage-parser smell (architectural)

`pandoc_ast.rs` is the public `to_pandoc_ast` API; linter / salsa
/ LSP / formatter walk the CST, not the projector. Phases 1/5
landed structural retags (`HTML_BLOCK_DIV`, `INLINE_HTML_SPAN`);
Phase 6 lifted inner content of `<div>` / non-div strict-block /
inline-block matched-pair shapes (non-bq + bq) into CST children.
Vestigial `<div>` byte walkers (`try_div_html_block`, etc.)
pruned 2026-05-11. **Phase 7a (2026-06-17)** retagged single-construct
opaque shapes (comment / PI / verbatim `<pre>`/`<script>`/`<style>`/
`<textarea>`) to a new `HTML_BLOCK_RAW` wrapper under Pandoc; projector
routes by kind via `html_raw_block` (→ one `RawBlock`), so
`emit_html_block`'s leading-byte-sniff early-return arm
(`trimmed.starts_with("<!--")` etc.) is now **dead for Pandoc** and
survives only for CommonMark `HTML_BLOCK` + the CommonMark-only
`<![CDATA[`/`<!` declaration branches. Retag mechanism mirrors
`HTML_BLOCK_DIV`: `wrapper_kind` stays `HTML_BLOCK` (all internal lift
gates unchanged, child tokens byte-identical), only the node kind at the
two `start_node` sites changes via `html_block_node_kind`. Load-bearing
remainder: `split_html_block_by_tags` (opaque HTML_BLOCKs only — Buckets
B/C/D: standalone close/void tags, single open + trailing, multi-tag
interleave), `parse_pandoc_blocks` (inter-tag text reparse via
`flush_html_block_text` / `flush_html_block_tail_text`),
`collect_html_block_text_skip_bq_markers` (now also used by
`html_raw_block` for verbatim-in-bq), table-cell reparses.
`html_div_block` `debug_assert!`s on unlifted HTML_BLOCK_DIV.

### Structural lift (Fix #3 / Fix #4 family)

- **Recursive parse uses `parse_with_refdefs`, not `parse`.**
  `parse` re-runs `populate_refdef_labels` on JUST the inner
  text, hiding outer refdefs from inner reference links. Thread
  outer config's `refdef_labels` through.
- **Line-consumption boundary trap** (Comment / PI trailing split,
  2026-05-13). `parse_html_block_with_wrapper`'s `lines: &[&str]`
  is the WHOLE document, not just the current container's
  content. Returning `lines.len()` from inside a fenced div /
  list item / blockquote consumes container close markers
  (`:::`, `> `, list-marker indent). Sibling-emit helpers
  (`graft_document_children` after `builder.finish_node()`)
  should only consume the current line; the outer dispatcher
  resumes at `close_line + 1` to keep container boundaries
  intact. Trade-off: multi-line softbreak continuation
  (`<!-- --> A\nB` → `Para [A, SoftBreak, B]`) breaks because
  the outer dispatcher starts a fresh paragraph for `B` —
  blocked.txt entry 0390 tracks the gap.
- **`graft_document_children` works as a sibling-emit helper**,
  not just an inside-HTML_BLOCK helper. Call it AFTER
  `builder.finish_node()` on HTML_BLOCK and it grafts children
  at the parent (DOCUMENT / container) level — that is what
  the Comment / PI trailing-split uses.
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
  just calls `format_node_sync` with content_node — it does
  NOT emit the marker.

### Bq-in-listitem first-line dispatch (option (a) landed 2026-05-18)

- **`lists::add_list_item` returns `ListItemFinish`.** When the
  list-item line is `- > <content>`, the bq branch of
  `finish_list_item_with_optional_nested` opens an inner
  `BLOCK_QUOTE` then returns `ListItemFinish::BqDispatch{content}`
  carrying the post-`> ` content. All `add_list_item` call sites
  + `start_nested_list` must capture the result and feed it to
  `Parser::dispatch_bq_after_list_item(finish)`. The helper calls
  `parse_inner_content(&content, Some(&content))` and decrements
  `self.pos` by 1 to absorb the caller's mandatory `lines_consumed
  += 1` (parse_inner_content advances pos by N; outer adds 1; net
  N is the target). Discarding the result silently loses line 0
  content — there is no eager-paragraph fallback inside the
  function anymore.
- **HTML-block dispatcher uses raw `lines[line_pos]`, not stripped
  content.** When `parse_inner_content` is invoked from the bq-in-
  listitem dispatch helper, the HTML-block dispatcher's
  `pandoc_html_open_tag_closes` still reads raw
  `lines[line_pos]` and strips `bq_depth` markers via
  `strip_n_blockquote_markers`. The list-marker prefix bytes
  (`- ` etc.) are NOT stripped, so the gate fails for
  bq-in-listitem first line. `parse_inner_content` falls back to
  paragraph; 0452/0453 stay broken in the same family. Headings,
  HRs, fenced-code-fail, etc. dispatch correctly because their
  parsers use `ctx.content` (already stripped before the helper
  is invoked). Fix shape (deferred): thread `list_content_col` from
  `ctx.list_indent_info` through `pandoc_html_open_tag_closes`,
  `parse_html_block_with_wrapper`, `find_multiline_open_end`,
  `count_tag_balance`, and the bq body-lift paths
  (`emit_html_block_body_lifted_bq*`). Watch losslessness — line-
  start `WHITESPACE` for list indent needs the same re-injection
  treatment as `BqPrefixState` / `LinePrefixState`.
- **`find_content_node` skips PLAIN/PARAGRAPH trailing a leading
  `HTML_BLOCK`/`HTML_BLOCK_DIV`.** Without the guard, the
  formatter picks the trailing PLAIN (from the comment/PI
  trailing-text-split list-item shape `- <!-- hi --> trailing`)
  as the wrap source, emits `- trailing` on the marker line,
  then drops the HTML_BLOCK below — producing the broken
  non-idempotent `- trailing\n<!-- hi -->`. With the guard the
  function returns None for this shape; the HTML_BLOCK arm
  handles the marker line and the trailing PLAIN runs through
  the continuation-paragraph path, yielding the idempotent
  `- <!-- hi -->\n  trailing`. The guard also returns None for
  any non-PLAIN/PARAGRAPH/BLANK_LINE first-real child after the
  marker (the wrap source must be the FIRST PLAIN/PARAGRAPH, or
  there's no wrap source).

--------------------------------------------------------------------------------

## Phase progress

| Phase | Description | Status |
|-------|-------------|--------|
| 1 | `<div>` block lift (HTML_BLOCK_DIV + HTML_ATTRS structural) | **Wrapper retag landed** (2026-05-08) — issue #263 closed; `<DIV>` losslessness fix landed. **Inner content NOT yet lifted into CST children** — still raw `HTML_BLOCK_CONTENT` TEXT tokens; projector reparses them. |
| 2 | `<span>` inline lift (INLINE_HTML_SPAN) | **Wrapper retag landed** (2026-05-08). Inner inlines mostly trivial (no recursive reparse needed). |
| 3 | Sectioning + verbatim corpus pin; `eitherBlockOrInline` lift | **Conformance landed** — non-void (2026-05-09); void (`<embed>`/`<area>`/`<source>`/`<track>`) (2026-05-10). Implementation leans on projector-side `inline_pending` tracking + byte walker; CST still opaque for split/matched-pair shapes. |
| 4 | Comments, PIs, declarations, CDATA projection | **Conformance landed** (2026-05-08); type-4 CM lowercase still gappy. CST opaque (these constructs project as RawBlock / RawInline). |
| 5 | `markdown_in_html_blocks` interaction edge cases | **Conformance landed** — depth-aware nested div, Plain/Para promotion, refs inheritance, **projector-level splitter** (`split_html_block_by_tags` byte walker + `parse_pandoc_blocks` recursive reparse), outer-matched-pair-abandons-on-void-interior. **The structural CST lift was deferred** — Phase 5's mechanism is the projector reparsing bytes, not the parser emitting structure. |
| 7a | Single-construct opaque lift — comment / PI / verbatim retag to `HTML_BLOCK_RAW` so the projector routes by kind. | **Landed 2026-06-17.** New `HTML_BLOCK_RAW` wrapper applied under `Dialect::Pandoc` via `html_block_node_kind` at the two `start_node` sites in `parse_html_block_with_wrapper` (incl. the comment/PI trailing-split head); `wrapper_kind` stays `HTML_BLOCK` so all lift gates + child tokens are unchanged (byte-lossless, `HTML_BLOCK_DIV` precedent). Projector `collect_block` → `html_raw_block` → one `RawBlock` (trailing-trim + 1-3 leading-space strip via `html_raw_block_text`); `emit_html_block` byte-sniff arm now dead for Pandoc. All consumers updated (formatter ×~8, list-item lift gate, folding, html_entities, both directives copies). Conformance **flat** (CST-fidelity refactor — report.txt byte-identical); 6 paired parser goldens + 2 formatter goldens added. **Remaining (7b-7e roadmap, NOT done): standalone single-tag (close/void), single open + trailing, void sequences, multi-tag interleave (D3) — `split_html_block_by_tags` + `parse_pandoc_blocks` still serve those.** |
| 6 (new) | Lift inner HTML block content into structural CST children — `HTML_BLOCK_DIV` / `HTML_BLOCK` get `PARAGRAPH` / `LIST` / etc. as direct children; projector byte walkers become vestigial; `PARAGRAPH→PLAIN` retag at adjacent-HTML-block boundary. | **All non-bq + bq shapes lifted for `<div>` and non-div Pandoc strict-block tags.** Shapes covered: clean multi-line, open-trailing, butted-close, indented-close, same-line, same-line + trailing-text-after-close, empty / blank-only, multi-line open (clean and trailing), depth-aware nested same-tag (`<div><div>x</div></div>` and trailing variants), multi-close trailing (`<div>foo</div></div>` and variants — projects as `Div + RawBlock` per pandoc-native), unclosed `<div>` (projects as `Div [...]` with implicit close), multi-line open + matched close in `pre_content` (single-close, nested, trailing-close, trailing-text — `<div\n  id="x">foo</div>` / `<div\n  id="x">foo</div></div>` / `<div\n  id="x"><div>x</div></div>` / `<div\n  id="x">foo</div>bar` and strict-block `<form\n  id="x">foo</form>`, **at top level and inside a blockquote** via `bq_multiline_close_lift_tag`). Inline-block matched-pair abandons when body begins with a void block tag (Plain via OnlyIfLast). Bq via four discriminator gates (`bq_clean_lift`, `same_line_bq_lift_tag`, `bq_messy_lift_tag`, `bq_multiline_close_lift_tag`). Dispatcher's `HTML_BLOCK_DIV` retag gate uses `pandoc_html_open_tag_closes` AND requires `is_closing: false`. Same-line / multi-line close-line lift paths use depth-aware split (`matched_close_offset` + `try_split_close_line_depth_aware`) + `split_close_marker_end` + trailing graft. `div_has_structural_inner` accepts unclosed div (1 HTML_BLOCK_TAG + structural body, no close). List items: same-line / fully-contained lift via `ListItemBuffer::emit_as_block` reparse + graft (formatter `format_list_item` HTML_BLOCK arm); multi-line lift via close-form dispatcher gate (`BlockContext::list_item_unclosed_html_block_tag` + `ListItemBuffer::unclosed_pandoc_matched_pair_tag`); indent normalization via `strip_list_item_indent` + `LinePrefixState` re-injection (projector `walk_skip_bq_markers` line-start-WS strip). List-item Comment/PI trailing-text via 2-child `try_emit_html_block_lift` branch + formatter `find_content_node` PLAIN-after-HTML_BLOCK guard. Inline-block matched-pair multi-line-open + same-line close (`<video\n  src="x">body</video>` / `<iframe\n  ...>...</iframe>` and bq variants) works transparently via the existing parser-side structural lift (open `HTML_BLOCK_TAG` + PLAIN body + close `HTML_BLOCK_TAG`, no HTML_BLOCK_DIV retag), pinned by 0448-0451. **Bq-in-listitem first-line dispatch landed 2026-05-18** via `ListItemFinish::BqDispatch` + `Parser::dispatch_bq_after_list_item` helper — fixes headings/HRs/etc. on `- > # heading` etc. (pinned by corpus 0454/0455 in `block` section). **Pass count history: 105 → 257** (current). **Known gap: bq-in-listitem first-line HTML block** (`- > <div>...` shapes — corpus 0452, 0453 blocked because the html-block dispatcher's `pandoc_html_open_tag_closes` walks raw `lines[line_pos]` without list-marker stripping; see Persistent traps for fix shape). |

--------------------------------------------------------------------------------

## Latest session — 2026-06-17 (Phase 7a — single-construct opaque lift)

Conformance: **flat** — html 259 / 1 fail (the fail is pre-existing
blocked 0390); `report.txt` + `pandoc-report.json` byte-identical, as
expected for a CST-fidelity refactor. Workspace 0 → 0 failures.

### What landed

- New `SyntaxKind::HTML_BLOCK_RAW` for single-construct opaque HTML
  blocks (comment, PI, verbatim `<pre>`/`<script>`/`<style>`/
  `<textarea>`). Applied under `Dialect::Pandoc` only; CommonMark keeps
  `HTML_BLOCK`.
- New `html_block_node_kind(wrapper_kind, block_type, dialect)` in
  `html_blocks.rs` decides the emitted node kind at the two
  `builder.start_node` sites (the main path in
  `parse_html_block_with_wrapper` + the comment/PI trailing-split head
  in `try_parse_comment_pi_with_trailing_split`). Key design choice:
  `wrapper_kind` stays `HTML_BLOCK` everywhere as the behavior gate, so
  none of the internal lift branches change and child tokens stay
  byte-identical (losslessness verified). Only the wrapper `u16`
  changes — exact `HTML_BLOCK_DIV` precedent.
- Projector: `collect_block` routes `HTML_BLOCK_RAW` → new
  `html_raw_block` → one `RawBlock "html"` via new `html_raw_block_text`
  helper (trailing-ASCII-WS trim + 1-3 leading-space strip;
  `collect_html_block_text_skip_bq_markers` still strips bq `> ` for
  the verbatim-in-bq shape). `block_from` routes it to `html_block`
  (single-block / table-cell context). `emit_html_block`'s leading-byte
  sniff early-return is now **dead for Pandoc** (kept for CommonMark +
  `<![CDATA[`/`<!` declarations).
- All CST consumers updated to accept `HTML_BLOCK_RAW` alongside
  `HTML_BLOCK`: formatter (`core.rs` ×3, `lists.rs` ×4, `utils.rs`),
  list-item lift gate (`list_item_buffer.rs`, both `matches!`), LSP
  `folding_ranges.rs`, linter `html_entities.rs`, both `directives.rs`
  copies. Verified directive comments (`panache-ignore-format-*`) still
  honored under Pandoc.
- 6 paired parser goldens (`html_block_raw_{comment,pre,pre_blockquote}_
  {pandoc,commonmark}`) pinning RAW vs HTML_BLOCK divergence + 2
  formatter idempotency goldens. ~30 existing Pandoc snapshots
  retagged HTML_BLOCK → HTML_BLOCK_RAW (every changed line verified as
  the benign retag only; no structural diffs).

### Files in committable diff

- `crates/panache-parser/src/syntax/kind.rs`,
  `parser/blocks/html_blocks.rs`, `parser/utils/list_item_buffer.rs`,
  `src/pandoc_ast.rs`.
- `crates/panache-formatter/src/{formatter/core.rs,formatter/lists.rs,
  utils.rs,directives.rs}`; `src/{directives.rs,
  linter/rules/html_entities.rs,lsp/handlers/folding_ranges.rs}`.
- `crates/panache-parser/tests/fixtures/cases/` (6 new dirs +
  snapshots, ~30 updated snapshots); `golden_parser_cases.rs`.
- `tests/fixtures/cases/html_block_raw_{comment,pre}/` +
  `golden_cases.rs`.

### Suggested next sub-targets

1. **Phase 7b — standalone single-tag opaque** (close tags
   `</section>`/`</p>`, single void `<embed>`): route an `HTML_BLOCK`
   whose only non-trivia child is one `HTML_BLOCK_TAG` to a structural
   single RawBlock (reuse `open_tag_raw_block_text`). Removes the easy
   majority of remaining `split_html_block_by_tags` hits.
2. **HTML-block dispatcher list-marker awareness** — unblocks 0452 +
   0453 (`pandoc_html_open_tag_closes` doesn't skip list-marker prefix
   bytes; thread `list_content_col`). Carried over from 2026-05-18.
3. **Phase 7c-7e** — single open + trailing, void sequences, multi-tag
   interleave (D3). D3's inter-tag reparse relocates into the parser
   rather than disappearing; the walker is not fully deletable.
4. **Softbreak continuation** (0390) — unchanged; conflicts with
   `close_line + 1` boundary preservation.

### New trap

Folded into Persistent traps ("A new HTML wrapper retag must be added
to EVERY consumer" under Parser shape & losslessness).

--------------------------------------------------------------------------------

## Earlier sessions (compact log)

Newest first. One line per session: date — phase/sub-target — pass
count delta — root cause / lever.

- 2026-05-18 — bq-in-listitem dispatch (option (a)) — block 15 → 17, html flat — `ListItemFinish::BqDispatch` + `Parser::dispatch_bq_after_list_item` hand post-`> ` content to caller instead of eager paragraph; 0452/0453 HTML-block stay blocked (dispatcher walks raw `lines[line_pos]` without list-marker strip).

- 2026-05-17 — Negative-space pin (`<video\n…>body</video>`, `<iframe\n…>` and bq variants) + bq-in-listitem first-line diagnosis (0452/0453) — html 253 → 257 — already-correct parser-side structural lift pinned; eager-paragraph at `finish_list_item_with_optional_nested` line 1499 identified as the root cause.
- 2026-05-15 — Phase 6 — bq + multi-line + same-line close lift (`> <div\n>   id="x">foo</div>` and depth-aware variants) — html 248 → 253 — new gate `bq_multiline_close_lift_tag` in `parse_html_block_with_wrapper` joins `lift_mode`/`lift_trailing`; close-line lift gate widens to `bq_depth == 0 || bq_multiline_close_lift_tag.is_some()`; body and close inherit bq prefix from open's last line via `emit_multiline_open_tag_with_attrs`.
- 2026-05-15 — Phase 6 — multi-line open + same-line close lift on `pre_content` (`<div\n  id="x">foo</div>` and depth-aware variants — non-bq only) — html 243 → 248 — new branch BEFORE legacy `same_line_closed`, gated on `multiline_open_end.is_some() && depth <= 0 && lift_mode && bq_depth == 0 && !pre_content.is_empty()`; same depth-aware split + `split_close_marker_end` + `graft_document_children` pattern; consumes `end_line_idx + 1` lines.
- 2026-05-15 — Phase 6 — depth-aware same-line + multi-line close-line lift; unclosed `<div>` projects as `Div [...]` — html 235 → 243 — `matched_close_offset` helper; `probe_same_line_lift` switched to depth-aware; `try_split_close_line_depth_aware` mirror + same-line/multi-line lift body switch; multi-line close-line path widened to depth-aware split + `split_close_marker_end` + trailing graft; `div_has_structural_inner` accepts missing close tag.
- 2026-05-15 — Phase 6 — same-line `<div>foo</div>bar` / `<form>foo</form>bar` trailing-text lift (top-level, bq, list-item, with-id); negative-space pins for `>   <!-- hi --> trailing` and bq-nested variants — html 226 → 235 — `probe_same_line_lift` widened (ends_with → contains, still `(0, 1)`); `split_close_marker_end` quote-aware close-marker split + sibling graft via `graft_document_children`; list-item buffer 2-child branch widened to HTML_BLOCK_DIV + PARAGRAPH.
- 2026-05-13 — Phase 6 wave (multiple subtargets) — html 142 → 226 — Combined: list-item Comment/PI trailing-text split via 2-child `try_emit_html_block_lift` branch + formatter `find_content_node` PLAIN-after-HTML_BLOCK guard; indented `isInlineTag` demotion in `HtmlBlockParser::detect_prepared` (Comment, PI, `<style>` o+c, `</script>`, math-tex `<script>`, Type7, inline-block matched-pair, void) when `leading_spaces > content_col`; top-level / bq Comment/PI trailing-text split via `try_parse_comment_pi_with_trailing_split` + `emit_bq_prefix_tokens` + first-line indent strip; list-item indent normalization via `strip_list_item_indent` + `LinePrefixState` (mirrors `BqPrefixState`) + projector `walk_skip_bq_markers` line-start-WS strip; multi-line list-item HTML lift via close-form dispatcher gate (`BlockContext::list_item_unclosed_html_block_tag` + `ListItemBuffer::unclosed_pandoc_matched_pair_tag`).
- 2026-05-11/12 — Phase 6 — non-div strict-block + bq + list-item structural lift wave — html 142 → 171 — `is_pandoc_lift_eligible_block_tag`, `LastParaDemote::{OnlyIfLast,SkipTrailingBlanks,Never}`, `parse_with_refdefs` graft, `emit_multiline_open_tag_with_attrs`; three bq discriminator gates + `BqPrefixState`; `ListItemBuffer::try_emit_html_block_lift` + formatter LIST_MARKER arm; pruned vestigial `try_div_html_block`.
- 2026-05-08/11 — Phases 1-5 seed — html 0 → 142 — `HTML_BLOCK_DIV`/`INLINE_HTML_SPAN` retag, `HTML_ATTRS`, projector `inline_pending`, CM/Pandoc blockHtmlTags split, `closes_at_open_tag`, `pandoc_html_open_tag_closes`, `PANDOC_VOID_BLOCK_TAGS`, PARAGRAPH→PLAIN retag at YesCanInterrupt, `is_closing` field, pandoc `isInlineTag` (issue #10643).
