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
- **`<style>` (open+close), `</script>`, PIs, comments, and
  `<script type="math/tex...">` cannot interrupt a paragraph under
  Pandoc**; `<pre>`/`<script>` open without math/tex AND
  `<textarea>` DO interrupt. Mirrors pandoc's `isInlineTag`
  (`Readers/HTML.hs`, issue #10643). `<style>` and `</script>` are
  special-cased always-inline; `<script>` open is inline only when
  `type` prefix `math/tex` (case-insensitive). Pandoc's
  `blockHtmlTags` does include `style`/`textarea`; the difference
  is `isInlineTag`'s special cases, not tag-set membership. Fix:
  `cannot_interrupt` in `HtmlBlockParser::detect_prepared`
  includes `ProcessingInstruction`, `tag_name == "style"`,
  `is_closing && tag_name == "script"`, and `!is_closing &&
  tag_name == "script" && is_math_tex_script_open(ctx.content)`
  under `Dialect::Pandoc`. The math/tex helper inspects single-line
  opens only; multi-line `<script\n type="math/tex">` not yet
  exercised. Required `is_closing: bool` field on
  `HtmlBlockType::BlockTag` (carries through every literal site).

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
- **Projector RawBlock emission of lifted open `HTML_BLOCK_TAG`
  must canonicalize when structural `HTML_ATTRS` are present.**
  Multi-line open tags carry literal source bytes
  (`<form\n  id="x"\n  class="y">`) — feeding these to RawBlock
  diverges from pandoc-native's canonical single-line form
  (`<form id="x" class="y">`). `normalize_native` preserves
  whitespace inside `"..."` string literals, so the conformance
  harness sees the divergence. Use `open_tag_raw_block_text` in
  `pandoc_ast.rs`: walk `children_with_tokens`, take the leading
  `<tagname` TEXT, append each `HTML_ATTRS`'s trimmed text
  separated by single spaces, append `>`. Single-line opens
  without HTML_ATTRS (`<form>`, `</form>`) keep their literal
  text. Don't substitute `node.text()` on the open-tag emission
  path even if the literal looks correct — it diverges on
  multi-line and on non-canonical whitespace within attribute
  regions.

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
  salsa walk picks up `<div id>` / `<span id>` automatically. As of
  2026-05-11 (Fix #4 shape-lift extension) the same walk also picks up
  non-div strict-block tag ids (`<section id="x">`, `<form id="x">`,
  `<p id="x">`, etc.) because the parser now emits `HTML_ATTRS` for
  those tags too. **Fix #5 followup (2026-05-11):** the bq case
  (`> <section id="x">`) also tokenizes HTML_ATTRS via
  `bq_strict_attr_emit_tag_name` — single-line opens only; the
  multi-line open inside bq still goes through the legacy whole-line
  TEXT path (`multiline_open_end` is gated on `bq_depth == 0`).
  Diverges from pandoc-native (which keeps them as RawBlock without
  lifting attrs), but matches user intent for anchor-link
  resolution — the linter no longer false-positives
  `undefined-anchor` against `<section id>` either outside or inside
  bq. No parallel salsa walk for HTML attrs.

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
of all non-bq `<div>` shapes (clean / messy / empty) AND non-div
Pandoc strict-block tags in the clean shape (`<form>...</form>`,
`<section>...</section>`, `<table>...</table>`, etc.) into CST
children. The projector still re-runs the markdown parser on opaque
`HTML_BLOCK` bodies for the remaining shapes (same-line / open-
trailing / butted-close / bq-wrapped non-div, matched-pair inline-
block `<video>...</video>`, multi-line opens) via
`parse_pandoc_blocks` / `split_html_block_by_tags` /
`flush_html_block_*` / `try_div_html_block`. **The path forward is
parser work** — extend the lift to the remaining shapes (butted-
close → same-line → bq-wrapped → matched-pair inline-block) and
then prune the byte walkers. Defensible reparses (table cells via
`parse_grid_cell_text` / `parse_cell_text_inlines`) mirror pandoc
and stay.

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
  (covers both div and non-div). `div_has_structural_inner`
  (HTML_BLOCK_DIV) and `html_block_has_structural_lift`
  (HTML_BLOCK) both require: exactly two `HTML_BLOCK_TAG`
  children, both clean (open ends at `>`, close starts with
  `</`), no `HTML_BLOCK_CONTENT`. `HTML_BLOCK_CONTENT` persists
  only on still-opaque bodies (bq-wrapped, same-line non-div,
  open-trailing non-div, butted-close non-div, multi-line opens,
  matched-pair inline-block). Empty / blank-only bodies (no
  `HTML_BLOCK_CONTENT`, just optional `BLANK_LINE`s between tags)
  count as lifted.
- **`html_block_open_tag_is_clean` accepts "TEXT ends in `>`"**
  (2026-05-11), not "TEXT exactly equals `>`". Covers both div's
  split-`>` emission (`TEXT("<div") + ... + TEXT(">")`) and
  non-div's whole-line emission (`TEXT("<form>")`). Trailing
  content after `>` produces a TEXT NOT ending in `>` and
  correctly fails. The lift gate's
  `probe_clean_open_tag_line` rejects open-trailing at parse
  time, so the predicate's leniency only sees clean shapes.
- **Three Plain/Para demotion semantics via `LastParaDemote`
  enum** on `graft_document_children`:
  - `Never` — clean `<div>\nfoo\n</div>` and unbalanced bodies:
    trailing `Para` preserved.
  - `SkipTrailingBlanks` — `<div>` close-butted shape
    (`<div>foo</div>` / `<div>\nfoo</div>` / `<div>foo\n</div>`
    with content-then-`</div>`): demote LAST `PARAGRAPH` even
    past trailing `BLANK_LINE`s.
  - `OnlyIfLast` — non-div strict-block close: demote ONLY when
    the inner doc's last child is a `PARAGRAPH` (no trailing
    `BLANK_LINE`). Mirrors pandoc's top-level Para→Plain
    adjacency rule between a paragraph and the close-tag
    `RawBlock`.
- **Multi-line open tags emit multiple `HTML_ATTRS` regions** for
  `<div>`. `<div\n  id="x"\n  class="y">` produces one
  `HTML_ATTRS` per attribute line. Helpers that read via
  `.children().find(HTML_ATTRS)` see only the FIRST. Iterate and
  join with `" "` before parsing (`cst_div_open_tag_attr`).
- **All `<div>` shapes outside blockquotes lift** (clean
  multi-line, trailing-on-open, butted-close, indented-close,
  same-line, empty / blank-only). Same-line is gated on
  `probe_same_line_div_lift`; nested-close shapes fall back.
  **Non-div strict-block tags lift ONLY the clean multi-line
  shape** as of 2026-05-11 — same-line / open-trailing /
  butted-close fall through to opaque `HTML_BLOCK_CONTENT` and
  the projector byte walker.
- **Parser-side structural lift inside blockquote — clean shape
  + same-line shape.** Clean multi-line goes through the
  `bq_clean_lift` gate (close-line; demote Never for `<div>`,
  OnlyIfLast for non-div / matched-pair, abandons on void-interior).
  Same-line goes through `same_line_bq_lift_tag` (open-line; gated
  on `bq_depth > 0` + `depth_aware_tag` + `multiline_open_end =
  None` + `probe_same_line_lift`). Same-line uses
  `emit_html_block_body_lifted` with `bq: &mut None` because the
  body has no inner newlines — the `> ` prefix sits on the outer
  BLOCK_QUOTE, outside the HTML_BLOCK[_DIV] span. Clean shape
  uses `emit_html_block_body_lifted_bq` + `BqPrefixState` to
  re-inject `BLOCK_QUOTE_MARKER + WHITESPACE` at each inner line
  start. Other bq shapes (butted-close, open-trailing) still go
  through `collect_html_block_text_skip_bq_markers` + projector
  byte walker. Deeper bq (`> > <div>`) works transparently via
  the clean gate (prefix capture is depth-agnostic).
- **Bq prefix re-injection: both `NEWLINE` and the `BLANK_LINE`
  *token* (kind, not node) advance `line_idx`.** The inner parse
  puts a `BLANK_LINE` token (text `"\n"`) inside a `BLANK_LINE`
  node; treating only `NEWLINE` as a line-end mis-aligns prefixes
  for any body containing a blank line — losslessness violation
  that doesn't surface until `>` (blank) precedes a content line.
- **`bq_clean_lift` gate:** HTML_BLOCK_DIV OR (HTML_BLOCK + tag in
  `is_pandoc_lift_eligible_block_tag`) + bq_depth > 0 +
  single-line open + `pre_content.is_empty()` +
  `open_no_nl.trim_end…ends_with('>')` +
  `close_no_nl.trim_start…starts_with("</")` + (for inline-block)
  not `inline_block_void_interior_abandons`. Anything else falls
  through to the opaque path.
- **`same_line_bq_lift_tag` gate:** bq_depth > 0 +
  `multiline_open_end.is_none()` + `depth_aware_tag.is_some()` +
  `depth <= 0` + `probe_same_line_lift(line_no_nl, tag_name)`.
  Triggers for HTML_BLOCK_DIV (any case) and HTML_BLOCK with tag
  in `is_pandoc_lift_eligible_block_tag`. When matched, replaces
  the `bq_strict_attr_emit_tag_name(.., false)` open-tag emission
  with `emit_open_tag_tokens(.., true)` so trailing bytes land in
  `pre_content`, and the `same_line_closed` branch grafts the
  body. Same demote policies as non-bq same-line (div =
  SkipTrailingBlanks, non-div/matched-pair = OnlyIfLast).
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
| 6 (new) | Lift inner HTML block content into structural CST children — `HTML_BLOCK_DIV` gets `PARAGRAPH` / `LIST` / etc. as direct children; `split_html_block_by_tags` / `flush_html_block_*` / `parse_pandoc_blocks` collapse into trivial CST walks; `PARAGRAPH→PLAIN` retag at adjacent-HTML-block boundary. | **All `<div>` shapes outside blockquotes lift structurally (2026-05-11)**: clean multi-line, trailing-on-open, butted-close, indented-close, same-line, empty / blank-only. **Fix #4 (non-div strict-block body lift) — clean multi-line + shape sweep + multi-line open landed 2026-05-11** for `<form>`, `<section>`, `<header>`, `<nav>`, `<aside>`, `<article>`, `<footer>`, `<p>`, `<table>`, `<tr>`, `<td>`, … (butted-close, open-trailing, same-line, multi-line open). **Inline-block matched-pair body lift landed (2026-05-11)** for `<video>`, `<iframe>`, `<button>`, `<audio>`, `<object>`, etc. — same shape coverage as non-div when body's first non-blank content is NOT a void block tag. **Fix #5 (bq-wrapped clean-shape lift) landed (2026-05-11)** for the clean multi-line shape inside `>` blockquote — `bq_clean_lift` gate covers `<div>` (Para), non-div strict-block tags (Plain via OnlyIfLast), and inline-block matched-pair (Plain, abandons on void-interior). `BqPrefixState` re-injects per-line bq markers around recursive parse's children so CST stays byte-equal; projector `open_tag_raw_block_text` strips bq markers from close-tag RawBlock emission. Deeper bq (`> > <div>`) works transparently. **Same-line bq lift landed (2026-05-11)** via the `same_line_bq_lift_tag` open-tag-side gate — covers `<div>` / non-div strict-block / inline-block matched-pair when `> <tag …>body</tag>` is on one line; routes through `emit_html_block_body_lifted` with `bq: &mut None` (body has no inner newlines). Remaining bq shapes (butted-close, open-trailing) still take the projector marker-strip path. Pass count progression: 132 → 137 → 140 → 141 → 142 → 145 → 148 → 151 → 154 → 157 → 159 (stable at 159 — purely structural, no new corpus cases). |

--------------------------------------------------------------------------------

## Latest session — 2026-05-11 (Phase 6 / Fix #7 — same-line bq lift across div / non-div strict-block / inline-block matched-pair)

Extended structural body lift inside `>` blockquotes to cover the
**same-line** shape (`> <tag>body</tag>` all on one source line).
Previously this fell back to opaque `HTML_BLOCK[_DIV]` with the
whole inner content inside a single `HTML_BLOCK_TAG`; the projector
byte walker (`split_html_block_by_tags`, `flush_html_block_*`,
`try_div_html_block`) reparsed bytes with bq marker stripping. With
this session the parser emits structural children — `HTML_BLOCK_TAG`
(open) + `PLAIN` (grafted body) + `HTML_BLOCK_TAG` (close) — for
`<div>`, non-div Pandoc strict-block tags (`<form>`, `<section>`, …),
and inline-block matched-pair tags (`<video>`, `<iframe>`, …).

The trick: same-line means there are no inner source newlines, so
the body lift uses `emit_html_block_body_lifted` with `bq: &mut
None`. The `> ` prefix sits on the outer `BLOCK_QUOTE`, outside the
HTML_BLOCK[_DIV] span — no re-injection needed. This is simpler
than the clean multi-line bq lift (Fix #5), which threads
`BqPrefixState` to re-emit per-line prefixes.

CST is byte-identical to source. Projector output already matched
pandoc-native via the byte walker; this session swaps in the
structural CST path with no projection change. Conformance stable
at 159 html / 352 total.

**Workspace test count**: 3061 → 3062 (+1 linter regression).
Consumer-facing win is downstream salsa/LSP/formatter — the body's
emphasis/strong/links now live as structural CST children instead
of being recovered via byte reparse.

### What landed

- New gate `same_line_bq_lift_tag` in `html_blocks.rs`: tag-name
  resolver firing when `bq_depth > 0` + `multiline_open_end =
  None` + `depth_aware_tag = Some` + `depth <= 0` +
  `probe_same_line_lift`. Returns `"div"` for HTML_BLOCK_DIV; tag
  name for HTML_BLOCK + `is_pandoc_lift_eligible_block_tag`.
- `lift_mode` now ORs in `same_line_bq_lift_tag.is_some()`.
- Open-tag emission's `bq_strict_attr_emit_tag_name` arm now
  passes `lift_trailing = same_line_bq_lift_tag == Some(name)`
  and forwards the returned trailing bytes into `pre_content` —
  matches the non-bq strict-block path's behavior.
- `same_line_closed` dispatch extends with a fourth `else if`
  for `same_line_bq_lift_tag`. Existing demote policies apply
  (div = SkipTrailingBlanks, non-div / matched-pair = OnlyIfLast).
- 6 new paired parser fixtures + snapshots:
  `html_block_div_blockquote_same_line_{pandoc,commonmark}`,
  `html_block_strict_block_blockquote_same_line_{pandoc,commonmark}`,
  `html_block_inline_block_blockquote_same_line_{pandoc,commonmark}`.
  Pandoc shapes pin the lifted CST (HTML_BLOCK_TAG + PLAIN +
  HTML_BLOCK_TAG); CommonMark shapes pin the opaque legacy shape.
- Linter regression
  `resolves_explicit_id_on_html_strict_block_same_line_inside_blockquote`
  pinning `> <section id="sec-a">body</section>` + `[link](#sec-a)`.

### Files in committable diff

- `crates/panache-parser/src/parser/blocks/html_blocks.rs` —
  `same_line_bq_lift_tag` gate + `lift_mode` extension +
  open-tag emission `lift_trailing` thread + `same_line_closed`
  dispatch arm (~60 net ins).
- `crates/panache-parser/tests/golden_parser_cases.rs` — 6 new
  fixture registrations.
- `crates/panache-parser/tests/fixtures/cases/` — 6 new fixture
  dirs (input.md + parser-options.toml for CommonMark cases).
- `crates/panache-parser/tests/snapshots/` — 6 new CST snapshots.
- `src/linter/rules/undefined_anchor.rs` — 1 new regression test.

### Suggested next sub-targets

1. **Bq butted-close + open-trailing lift** (medium-hard,
   unchanged). `> <div>foo` / `> foo</div>` / `> <div>\n> foo</div>`
   shapes for `<div>`, non-div strict-block, and inline-block
   matched-pair. Each has different `BqPrefixState` setup: the
   inner body lines need per-line bq-marker re-injection (closer
   to clean-shape lift) while the open / close tag wrapping
   captures the bq markers that the body lift skips.
2. **Multi-line open tag inside bq**. `multiline_open_end` is
   gated on `bq_depth == 0`, so `> <section\n>   id="x">\n` falls
   back to opaque per-line TEXT. Rare in practice; defer.
3. **Prune projector byte walkers** once (1) lands.
   `collect_html_block_text_skip_bq_markers`, `try_div_html_block`,
   the matched-pair `split_html_block_by_tags` branch, and
   `interior_starts_with_void_block_tag` become dead code.

### New trap

Folded into Persistent traps under "Structural lift" (the
`same_line_bq_lift_tag` gate description) and under "Parser-side
structural lift inside blockquote" (the same-line-uses-`bq: &mut
None` insight — same-line has no inner newlines so no prefix
re-injection is needed, distinguishing it from the clean
multi-line lift's `BqPrefixState` path).

--------------------------------------------------------------------------------

## Earlier sessions (compact log)

Newest first. One line per session: date — phase/sub-target — pass
count delta — root cause / lever.

- 2026-05-11 — Phase 6 / Fix #5 followup HTML_ATTRS in bq for non-div strict-block tags — html stable 159 — `bq_strict_attr_emit_tag_name` helper, open-tag attribute tokenization at `bq_depth > 0` (single-line only), salsa anchor walk picks up `<section id>` inside `>` quotes.
- 2026-05-11 — Phase 6 / Fix #5 bq lift + generalization across `<div>` / non-div strict-block / inline-block matched-pair + depth > 1 — html stable 159 — `bq_clean_lift` gate, `BqPrefixState` re-injection, `inline_block_void_interior_abandons`, `open_tag_raw_block_text` bq-prefix strip, depth-2 fixture.
- 2026-05-11 — Phase 6 / Fix #4 non-div strict-block shape sweep + multi-line open-tag lift — html 142 → 159 — `is_pandoc_lift_eligible_block_tag`, `html_block_has_structural_lift`, `LastParaDemote::{OnlyIfLast,SkipTrailingBlanks,Never}`, `parse_with_refdefs` graft, `emit_multiline_open_tag_with_attrs`, `open_tag_raw_block_text` canonicalizer.
- 2026-05-10 → 2026-05-11 — Phase 6 cannot_interrupt + Fix #1/#2 — html 132 → 142 — PARAGRAPH→PLAIN retag at YesCanInterrupt; `is_closing` field; `is_math_tex_script_open`; pandoc `isInlineTag` (issue #10643).
- 2026-05-10 — Strict-block/verbatim closing-form lift, multi-line void open-tag, incomplete-open recursion fix, Phase 3 void `eitherBlockOrInline` — html 105 → 132 — `closes_at_open_tag`, `pandoc_html_open_tag_closes` gate, `PANDOC_VOID_BLOCK_TAGS`.
- 2026-05-09 — Phase 3 + Phase 5 (non-void eitherBlockOrInline; HTML5 sectioning; `<DIV>` losslessness; Plain/Para; multi-line attrs; refs inheritance) — html 62 → 105 — projector `inline_pending` + parser `cannot_interrupt`; CM/Pandoc blockHtmlTags split; `build_refs_ctx_inherited`.
- 2026-05-08 — Phases 1-5 seed (issue #263 closed) — html 0 → 62 — `HTML_BLOCK_DIV`/`INLINE_HTML_SPAN` retag, `HTML_ATTRS` tokenization, sectioning/verbatim corpus pin, depth-aware nested `<div>`.
