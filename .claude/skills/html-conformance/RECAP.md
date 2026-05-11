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
- **Multi-line open-tag close branches diverge by tag class.** The
  `same_line_closed` short-circuit assumes single-line; void-tag
  multi-line opens take a separate early-exit returning
  `end_line_idx + 1` BEFORE the regular close-marker loop. Without
  the explicit branch the parser would scan content lines for a
  closing tag that doesn't exist (void tags have none) and run
  off the document. Likewise `same_line_closed` must guard
  `multiline_open_end.is_none()`.
- **Incomplete open tags caused projector infinite recursion.**
  `<embed\n`, `<div\n`, `<table\n` etc. (no `>` anywhere) were
  recognized as `RawBlock` under Pandoc, but pandoc-native treats
  them as paragraph text. The projector's `flush_html_block_tail_text`
  then reparsed the same bytes and re-emitted the same HTML_BLOCK,
  recursing forever. Fix: gate Pandoc BlockTag recognition on
  `pandoc_html_open_tag_closes(lines, line_pos, bq_depth)` in
  `block_dispatcher.rs::detect_prepared`. Multi-line opens still
  work because the helper scans subsequent lines (across blank
  lines, threading quotes) for an unquoted `>`. CommonMark must
  remain liberal: `<table\n` (no `>`) is a valid CM type-6
  RawBlock.
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
  tags ALL ARE block starts under Pandoc.** Pandoc's `htmlBlock
  isBlockTag` matches both directions for any tag in
  `blockHtmlTags ∪ verbatimTags ∪ eitherBlockOrInline`. Routing in
  the parser: each category emits `BlockTag { closes_at_open_tag:
  true }` so the block ends on the open line. The dispatcher's
  `cannot_interrupt` gate keys ONLY on inline-block + void tag
  names — strict-block (`</p>`, `</nav>`, `</section>`) and verbatim
  (`</pre>`, `</style>`, `</script>`, `</textarea>`) closes get
  `YesCanInterrupt` and DO interrupt running paragraphs (matches
  pandoc). Inline-block / void closes follow `cannot_interrupt`
  semantics and stay inline inside running paragraphs
  (`foo\n</video>` → `Para[foo, SB, RI</video>]`). Earlier recap
  claims that "closing forms must be excluded" were wrong on all
  counts.
- **`<script>` is in `eitherBlockOrInline` AND `blockHtmlTags`.**
  Verbatim handling fires first via `VERBATIM_TAGS`; don't add
  `script` to `PANDOC_INLINE_BLOCK_TAGS`. Likewise `<pre>`,
  `<style>`, `<textarea>` membership in `PANDOC_BLOCK_TAGS` is
  harmless — the verbatim arm fires first.
- **`<style>`, PIs, `</script>`, and `<script type="math/tex…">`
  cannot interrupt a paragraph under Pandoc; `<pre>`/`<script>` open
  without math/tex/`<textarea>` DO** (LANDED 2026-05-10 / 2026-05-11).
  The non-interrupt set mirrors pandoc's `isInlineTag` predicate
  (`pandoc/src/Text/Pandoc/Readers/HTML.hs`):
  - `<style>` open AND close are SPECIAL-CASED to always be inline
    (commit fixing pandoc issue #10643).
  - `</script>` close is similarly special-cased to always be inline.
  - `<script>` open is inline ONLY when the `type` attribute starts
    with `math/tex` (case-insensitive prefix; e.g. `math/tex`,
    `math/tex; mode=display`). Every other `<script>` open is a
    `RawBlock`.
  - PIs (`<? … ?>`) match `T.take 1 name == "?"`.
  - Comments are always inline.
  - Pandoc's `eitherBlockOrInline` set (audio, button, iframe, …,
    plus void area/embed/source/track) returns True from
    `isInlineTag` because those tags are NOT in `blockTags`.
  Earlier RECAP entries claimed `<style>` was "the lone verbatim
  tag NOT in `blockHtmlTags` (verbatimHtmlBlocks only)" — wrong;
  pandoc's `blockHtmlTags` does include `style` and `textarea`. The
  behavior difference comes from `isInlineTag`'s special cases, not
  tag-set membership. Fix: `cannot_interrupt` in
  `HtmlBlockParser::detect_prepared` includes
  `HtmlBlockType::ProcessingInstruction`, `BlockTag`s where
  `tag_name == "style"`, `BlockTag`s where
  `is_closing && tag_name == "script"`, and `BlockTag`s where
  `!is_closing && tag_name == "script" && is_math_tex_script_open(ctx.content)`
  under `Dialect::Pandoc`. The math/tex helper inspects only
  `ctx.content` (single-line opens); multi-line `<script\n type="math/tex">`
  opens are an edge case not yet exercised by the corpus. Required
  adding an `is_closing: bool` field to `HtmlBlockType::BlockTag`
  (carries through every literal site). CommonMark stays liberal —
  paired CM/Pandoc parser fixtures pin any divergence.

### Projector tag splitting

- **`split_html_block_by_tags` walks bytes, not tokens.** It is
  depth-unaware (Phase 5 work for the few cases that need it) and
  context-tracked via `inline_pending`. Don't try to "merge" with
  `find_matching_close` (the smart-quote bracket scanner) — same
  name, different inputs.
- **Matched-pair lift for `<video>...</video>` must abandon when
  interior opens with a void block tag at column 0.** Pandoc-native
  emits per-tag (`<video>` RB, `<source>` RB, Para[fallback, SB,
  RawInline</video>]) — not a balanced lift. Helper
  `interior_starts_with_void_block_tag` peeks past leading
  newlines/whitespace; on hit, the open tag emits as a single
  RawBlock and the closing `</video>` falls into the trailing
  paragraph reparse as RawInline. Indentation before the void tag
  doesn't save the lift (pandoc abandons even with 4-space indent).
- **Inline-block open with no matched close must emit as RawBlock
  at fresh-block.** Falling through to `inline_pending=true` causes
  the trailing tail-text reparse to recurse on the same `<video>...`
  bytes (parser still recognizes the open tag, projector splits it
  again, …) → stack overflow. The same `interior_starts_with_void`
  bail and the no-match bail share the single-tag emit path.
- **`inline_pending` resets on consecutive newlines (≥ 2).** A
  blank line restarts pandoc's block parser; in our byte walker
  that's `\n\n`. Don't substitute "byte == whitespace" — single
  trailing whitespace shouldn't reset.
- **Inter-tag text demotes Para→Plain when butted against the next
  tag**; tail text does NOT demote. Use `flush_html_block_text`
  (inter-tag) vs `flush_html_block_tail_text` (end-of-block).
  Uniform demotion silently breaks `<form>\nfoo\n` and
  `<embed src="x"> trailing` shapes.
- **Plain/Para signal for `<div>` recursive reparse is
  `</div>`-side, not `<div>`-side**: `close_butted = byte_at(close_start - 1) != '\n'`.
  Demotion applies to the LAST block only, regardless of how many
  precede it.
- **`try_div_html_block` requires the WHOLE content to be a single
  `<div>...</div>`** with optional surrounding whitespace. Pass an
  exact `<div>...</div>` slice when calling on a sub-range.
- **HTML blocks (any tag) inside blockquotes need projector marker-
  strip.** The parser keeps `BLOCK_QUOTE_MARKER + WHITESPACE` as
  structural CST tokens inside `HTML_BLOCK_CONTENT` and the close
  `HTML_BLOCK_TAG` for losslessness. Calling `node.text()` on
  `HTML_BLOCK` / `HTML_BLOCK_DIV` returns those markers as literal
  bytes; feeding them to `parse_pandoc_blocks` /
  `split_html_block_by_tags` / `try_div_html_block` re-recognizes
  the `> ` prefixes as a nested blockquote (or for verbatim
  `<pre>...</pre>`, leaves literal `>` chars in the emitted
  RawBlock). Always use `collect_html_block_text_skip_bq_markers`
  in `html_div_block` and `emit_html_block` (the two byte-reparse
  entry points). Walker collapses each `BLOCK_QUOTE_MARKER` plus
  one immediately-following `WHITESPACE` token; handles arbitrary
  nesting depth (`> > <div>`). Don't reintroduce `node.text()` on
  these paths until the parser-side structural lift inside bq lands.

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
  salsa walk picks up `<div id>` / `<span id>` automatically. No
  parallel salsa walk for HTML attrs.

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
  adjacency: LANDED 2026-05-10.** Parser-side fix in
  `Parser::close_paragraph_as_plain_if_open` +
  `html_block_demotes_paragraph_to_plain`, wired at the
  YesCanInterrupt branch in `core.rs`. Gated on `Dialect::Pandoc` +
  `parser_name == "html_block"` + `HtmlBlockType::BlockTag`. CST
  emits `PLAIN` instead of `PARAGRAPH`; projector trivially maps
  each. Don't reintroduce the projector-side demotion (reverted
  earlier the same day).

### Projector-as-second-stage-parser smell (architectural)

The projector at `crates/panache-parser/src/pandoc_ast.rs` is the
public `panache_parser::to_pandoc_ast` API; consumers of structural
HTML decisions (linter, salsa, LSP, formatter) walk the CST, not
the projector. Phases 1/5 landed structural retags
(`HTML_BLOCK_DIV`, `INLINE_HTML_SPAN`); Phase 6 lifted inner content
of all non-bq `<div>` shapes into CST children. The projector still
re-runs the markdown parser on opaque `HTML_BLOCK` bodies and on
all HTML inside blockquotes (via `parse_pandoc_blocks` /
`split_html_block_by_tags` / `flush_html_block_*` /
`try_div_html_block`). **The path forward is parser work** — Fix #4
in AUDIT.md (full HTML_BLOCK body structural split) is the largest
remaining lever. Defensible reparses (table cells via
`parse_grid_cell_text` / `parse_cell_text_inlines`) mirror pandoc
and stay.

### Structural lift (Fix #3 family)

- **Recursive parse uses `parse_with_refdefs`, not `parse`.** When
  the parser does an inner recursive parse during a structural
  lift, call `crate::parser::parse_with_refdefs(inner_text, opts,
  outer_refdefs)` (or thread the outer config's `refdef_labels`
  through). `parse` re-runs `populate_refdef_labels` on JUST the
  inner text, hiding outer refdefs from inner reference links.
- **`collect_block` must route `HTML_BLOCK_DIV` to `html_div_block`,
  NOT `emit_html_block`.** `emit_html_block`'s byte path calls
  `try_div_html_block` → `parse_pandoc_blocks` which rebuilds an
  inner `RefsCtx` and re-runs heading-id disambiguation against
  it. If the outer CST already has those inner headings as
  children (post-Fix-#3), the outer `build_refs_ctx` already
  disambiguated them; running disambiguation again in
  `parse_pandoc_blocks` bumps `seen_ids` for the same base name
  and produces `heading-1`/`subheading-1` instead of
  `heading`/`subheading`. Symptom: inner heading ids in pandoc-ast
  output gain a stray `-1` suffix.
- **Multi-line open tags emit multiple `HTML_ATTRS` regions.** A
  `<div\n  id="x"\n  class="y">` produces one `HTML_ATTRS` per
  attribute line, all as direct children of the open
  `HTML_BLOCK_TAG`. Helpers that read attrs via
  `.children().find(... HTML_ATTRS)` see only the FIRST and
  silently drop the rest (classes lost). Iterate and join
  attribute texts with `" "` before parsing
  (`cst_div_open_tag_attr`).
- **All non-bq `<div>` shapes lift structurally (2026-05-11).**
  `<div>foo\n</div>`, `<div>\nfoo\nbar</div>`, `<div>a\n   </div>`,
  `<div>foo</div>` all produce a clean open `HTML_BLOCK_TAG` (ends
  at `>`), a clean close `HTML_BLOCK_TAG` (starts with `</`), with
  lifted content as `PARAGRAPH` / `PLAIN` / `BLANK_LINE` children
  between them. Mechanism: `try_split_close_line` extracts close-
  line leading bytes; `emit_html_block_body_lifted` builds inner
  text from `pre_content` + `content_lines` + `post_content` and
  grafts the recursive parse; `graft_subtree_as(..., PLAIN)` retags
  the LAST `PARAGRAPH` (skipping trailing BLANK_LINEs) when
  `post_content` is non-empty (butted close → Para→Plain). Same-
  line is gated on `probe_same_line_div_lift` (line begins `<div`,
  open `>` quote-aware, trailing ends with `</div>`,
  `count_tag_balance` gives `(0,1)`). Rejected shapes
  (trailing-content-after-close, nested same-line) fall back to
  opaque single-`HTML_BLOCK_TAG` + projector byte path.
  `try_split_close_line` only handles simple closes (one `</tag>`,
  zero opens); nested-close shapes fall back to non-lift.
- **Parser-side structural lift is gated on `bq_depth == 0`.**
  Inside blockquotes, content lines carry BLOCK_QUOTE_MARKER +
  WHITESPACE prefixes the parser keeps for losslessness. The
  projector handles bq context via `collect_html_block_text_skip_bq_markers`
  (see "Projector tag splitting"); a parser-side structural lift
  would need to re-attach markers around grafted children to stay
  lossless and is deferred until Fix #4 lands.
- **`div_has_structural_inner` cleanness predicates stayed the
  same** through the messy-shape lift. Because the parser now
  emits clean open + close `HTML_BLOCK_TAG`s for all lifted
  shapes, `html_block_open_tag_is_clean` (ends with `>` followed
  only by NEWLINEs) and `html_block_close_tag_is_clean` (first
  TEXT starts with `</`) trivially pass — no projector changes
  needed. Don't relax these predicates; they still correctly
  reject same-line `<div>foo</div>` (one `HTML_BLOCK_TAG`, fails
  the two-tags requirement) and bq-wrapped divs (close tag has
  leading `> ` markers, not `</`).

--------------------------------------------------------------------------------

## Phase progress

| Phase | Description | Status |
|-------|-------------|--------|
| 1 | `<div>` block lift (HTML_BLOCK_DIV + HTML_ATTRS structural) | **Wrapper retag landed** (2026-05-08) — issue #263 closed; `<DIV>` losslessness fix landed. **Inner content NOT yet lifted into CST children** — still raw `HTML_BLOCK_CONTENT` TEXT tokens; projector reparses them. |
| 2 | `<span>` inline lift (INLINE_HTML_SPAN) | **Wrapper retag landed** (2026-05-08). Inner inlines mostly trivial (no recursive reparse needed). |
| 3 | Sectioning + verbatim corpus pin; `eitherBlockOrInline` lift | **Conformance landed** — non-void (2026-05-09); void (`<embed>`/`<area>`/`<source>`/`<track>`) (2026-05-10). Implementation leans on projector-side `inline_pending` tracking + byte walker; CST still opaque for split/matched-pair shapes. |
| 4 | Comments, PIs, declarations, CDATA projection | **Conformance landed** (2026-05-08); type-4 CM lowercase still gappy. CST opaque (these constructs project as RawBlock / RawInline). |
| 5 | `markdown_in_html_blocks` interaction edge cases | **Conformance landed** — depth-aware nested div, Plain/Para promotion, refs inheritance, **projector-level splitter** (`split_html_block_by_tags` byte walker + `parse_pandoc_blocks` recursive reparse), outer-matched-pair-abandons-on-void-interior. **The structural CST lift was deferred** — Phase 5's mechanism is the projector reparsing bytes, not the parser emitting structure. |
| 6 (new) | Lift inner HTML block content into structural CST children — `HTML_BLOCK_DIV` gets `PARAGRAPH` / `LIST` / etc. as direct children; `split_html_block_by_tags` / `flush_html_block_*` / `parse_pandoc_blocks` collapse into trivial CST walks; `PARAGRAPH→PLAIN` retag at adjacent-HTML-block boundary. | **All `<div>` shapes outside blockquotes now lift structurally (2026-05-11)**: clean multi-line `<div>...</div>` (first cut); messy shapes — trailing-on-open, butted-close, indented-close, same-line. **HTML blocks (any tag) inside blockquotes now project correctly via projector marker-strip (2026-05-11)**: `<div>`, `<p>`, `<pre>`, `<video>`, `<iframe>`, … inside `>` blockquotes get their BLOCK_QUOTE_MARKER+WHITESPACE prefix tokens stripped at the projector before byte-reparse, so the inner pandoc reparse doesn't re-recognize them as nested blockquotes. CST stays lossless (markers preserved as structural tokens); parser-side structural lift inside bq deferred (would need to thread markers around grafted children). Earlier sub-targets: PARAGRAPH→PLAIN retag at YesCanInterrupt; `<style>` / PI / `</script>` / `<script type="math/tex…">` cannot_interrupt. Pass count progression: 132 → 137 → 140 → 141 → 142 → 145 → 148. Fix #4 from AUDIT.md (full HTML_BLOCK body structural split) still pending. |

Multi-line `<div>` open-tag structural HTML_ATTRS lift landed
(2026-05-09). Multi-line void open-tag now lifts via
`find_multiline_open_end` + simple per-line TEXT/NEWLINE emission
(2026-05-10). Inline-block / void closing forms (`</video>`,
`</embed>`) start single-line `RawBlock`s under Pandoc (2026-05-10).
Strict-block / verbatim closing forms (`</p>`, `</nav>`, `</section>`,
`</pre>`) likewise lift under Pandoc, with `closes_at_open_tag: true`
and CAN interrupt a running paragraph (no `cannot_interrupt` gate)
(2026-05-10).

--------------------------------------------------------------------------------

## Latest session — 2026-05-11 (Phase 6: bq-wrapped HTML blocks projector marker-strip)

Closed the last shape gap for HTML blocks inside blockquotes: any
HTML block under `> ...` markers (`<div>`, `<p>`, `<pre>`, `<video>`,
`<iframe>`, …) now projects to the same pandoc-AST as
`pandoc -f markdown -t native`. Previously the projector's
byte-reparse path called `node.text()` on `HTML_BLOCK` /
`HTML_BLOCK_DIV`, which included the BLOCK_QUOTE_MARKER + WHITESPACE
prefix tokens that the parser keeps as structural CST tokens for
losslessness. Feeding those bytes to the recursive parser
re-recognized the `> ` prefixes as a nested blockquote (or, for
verbatim `<pre>...</pre>`, kept literal `>` characters in the
emitted RawBlock). The fix is purely projector-side; the parser-side
structural lift (replacing the byte-reparse with CST children)
inside `bq_depth > 0` is still deferred — it would need to thread
markers around grafted children to keep losslessness intact.

**Implementation** (all in
`crates/panache-parser/src/pandoc_ast.rs`):
- New `collect_html_block_text_skip_bq_markers(node)` walker plus
  `walk_skip_bq_markers` recursive helper. Concatenates the node's
  token text but drops every `BLOCK_QUOTE_MARKER` token AND the
  immediately-following `WHITESPACE` token (one space). Handles
  arbitrary nesting depth (`> > <div>`) because each line's marker
  pairs are collapsed independently.
- `html_div_block` switched from `node.text().to_string()` to the
  new helper; `extract_div_inner_and_butted` + `assemble_div_block`
  then operate on clean bytes.
- `emit_html_block` (the opaque `HTML_BLOCK` projector) switched the
  same way; `try_div_html_block` / `split_html_block_by_tags` /
  `flush_html_block_text` now see clean bytes too. Inter-tag
  Para→Plain demotion follows automatically because `Inner.` is no
  longer wrapped in a spurious BlockQuote that hides it from
  `flush_html_block_text`.

**Parser**: no changes. The CST shape (BLOCK_QUOTE_MARKER + WHITESPACE
inside HTML_BLOCK_CONTENT and the close HTML_BLOCK_TAG) is unchanged
and stays lossless. Two new paired parser fixtures
(`html_block_div_inside_blockquote_{pandoc,commonmark}`) pin the
shape so the projector-only fix has a stable contract.

**Pass count**: html 142 → 148, total 335 → 341. Six new corpus
cases (336–338 div, 339–341 pre/video/p) all flipped to passing in
one shot. No existing snapshots updated; no parser fixtures changed.

**Payoff**: bq-wrapped HTML blocks now match pandoc-native at the
public `to_pandoc_ast` API. Salsa anchor walk for `<div id>` inside
a blockquote was already correct (HTML_ATTRS lives in the open
HTML_BLOCK_TAG, never carries bq markers); the linter / LSP
consumers see no change.

### Suggested next sub-targets

1. **Fix #4 from AUDIT.md — full HTML_BLOCK body structural split**
   (large). Eliminates `split_html_block_by_tags`, both flush helpers,
   `interior_starts_with_void_block_tag`,
   `find_matching_html_close*`, `inline_pending` flag. Each lifts a
   chunk of byte-walker compensation into a CST walk. Largest blast
   radius remaining.
2. **Parser-side structural lift for `<div>` inside blockquotes** —
   the projector marker-strip is a stopgap. A real lift would need
   to: (a) build inner_text from content_lines with markers stripped
   per line, (b) recursively parse + graft, (c) re-attach markers
   around the grafted children to stay lossless, OR keep markers in
   sibling tokens (which makes downstream walks awkward). Defer
   until Fix #4 lands; the bq-context decision is part of the same
   architecture.
3. **Multi-line `<script\n type=...>`** corpus pin — only if a real
   case appears (the `is_math_tex_script_open` helper currently only
   inspects single-line `ctx.content`).
4. **Empty `<div></div>` projector trim**: relax
   `div_has_structural_inner` to accept clean-tag-only shapes, so
   the byte-reparse fallback isn't hit on empty divs. Cosmetic
   (output is identical).

### Files in committable diff

- `crates/panache-parser/src/pandoc_ast.rs` — new
  `collect_html_block_text_skip_bq_markers` + `walk_skip_bq_markers`
  helpers; `html_div_block` and `emit_html_block` switched to use
  them.
- `crates/panache-parser/tests/fixtures/pandoc-conformance/corpus/`
  — six new cases: `0336…0338` (div in bq, with attrs, with block
  content); `0339…0341` (pre verbatim, video matched-pair, p strict-
  block).
- `crates/panache-parser/tests/fixtures/cases/html_block_div_inside_blockquote_{pandoc,commonmark}/`
  + `tests/golden_parser_cases.rs` + two new snapshots — paired
  parser fixture pinning the bq+div CST shape (BLOCK_QUOTE_MARKER
  tokens inside HTML_BLOCK_CONTENT and the close HTML_BLOCK_TAG;
  HTML_ATTRS structurally lifted on the open tag).
- `crates/panache-parser/tests/pandoc/allowlist.txt` — new
  `# html-block (HTML blocks inside a blockquote …)` section header
  + ids 336–341.
- Regenerated `crates/panache-parser/tests/pandoc/report.txt` and
  `docs/development/pandoc-report.json`.
- `.claude/skills/html-conformance/RECAP.md` — this session.

### New traps

Folded into Persistent traps under "Projector tag splitting":
projector marker-strip pattern for HTML blocks inside blockquotes
(don't reintroduce `node.text()` in `html_div_block` / `emit_html_block`).

--------------------------------------------------------------------------------

## Earlier sessions (compact log)

Newest first. One line per session: date — phase/sub-target — pass
count delta — root cause / lever.

- 2026-05-11 — Phase 6 Fix #3 same-line `<div>foo</div>` lift — html 142 → 142 — `probe_same_line_div_lift` gates the lift; rejects nested/trailing shapes; close always butted → PLAIN.
- 2026-05-11 — Phase 6 Fix #3 messy + first-cut `<div>` lifts (clean multi-line, trailing-on-open, butted-close, indented-close) — html 142 → 142 — recursive `parse_with_refdefs` + `graft_subtree`; cleanness predicates; `html_div_block` routed structurally.
- 2026-05-10 → 2026-05-11 — Phase 6 cannot_interrupt (`<style>`, PI, `</script>`, `<script type=math/tex>`) + Fix #1/#2 — html 132 → 142 — PARAGRAPH→PLAIN retag at YesCanInterrupt; `is_closing` field; `is_math_tex_script_open`; pandoc `isInlineTag` (issue #10643) drives the set.
- 2026-05-10 — Strict-block/verbatim closing-form lift, multi-line void open-tag, incomplete-open recursion fix, Phase 3 void `eitherBlockOrInline` — html 105 → 132 — close-tag branches, `closes_at_open_tag`, `pandoc_html_open_tag_closes` gate, `PANDOC_VOID_BLOCK_TAGS`.
- 2026-05-09 — Phase 3 + Phase 5 (non-void eitherBlockOrInline; HTML5 sectioning; `<DIV>` losslessness; Plain/Para; multi-line attrs; refs inheritance) — html 62 → 105 — projector `inline_pending` + parser `cannot_interrupt`; CM/Pandoc blockHtmlTags split; `build_refs_ctx_inherited`.
- 2026-05-08 — Phases 1-5 seed (issue #263 closed) — html 0 → 62 — `HTML_BLOCK_DIV`/`INLINE_HTML_SPAN` retag, `HTML_ATTRS` tokenization, sectioning/verbatim corpus pin, depth-aware nested `<div>`.
