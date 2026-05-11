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
- **Body-lifted signal is "no `HTML_BLOCK_CONTENT` child"**
  (2026-05-11). `div_has_structural_inner` was inverted from
  "requires a non-(TAG/BLANK_LINE/CONTENT) body child" to "rejects
  any node with an `HTML_BLOCK_CONTENT` child". This admits empty
  (`<div></div>`) and blank-only (`<div>\n\n</div>`) divs into
  the structural walk while still excluding bq-wrapped divs (those
  carry `HTML_BLOCK_CONTENT` with marker-prefixed TEXT). Fix #4
  generalizes this invariant: when the parser lifts non-`<div>`
  bodies, `HTML_BLOCK_CONTENT` will only persist on still-opaque
  blocks, and the projector's child walk can rely on its absence
  as a "body is structural" guarantee.

--------------------------------------------------------------------------------

## Phase progress

| Phase | Description | Status |
|-------|-------------|--------|
| 1 | `<div>` block lift (HTML_BLOCK_DIV + HTML_ATTRS structural) | **Wrapper retag landed** (2026-05-08) — issue #263 closed; `<DIV>` losslessness fix landed. **Inner content NOT yet lifted into CST children** — still raw `HTML_BLOCK_CONTENT` TEXT tokens; projector reparses them. |
| 2 | `<span>` inline lift (INLINE_HTML_SPAN) | **Wrapper retag landed** (2026-05-08). Inner inlines mostly trivial (no recursive reparse needed). |
| 3 | Sectioning + verbatim corpus pin; `eitherBlockOrInline` lift | **Conformance landed** — non-void (2026-05-09); void (`<embed>`/`<area>`/`<source>`/`<track>`) (2026-05-10). Implementation leans on projector-side `inline_pending` tracking + byte walker; CST still opaque for split/matched-pair shapes. |
| 4 | Comments, PIs, declarations, CDATA projection | **Conformance landed** (2026-05-08); type-4 CM lowercase still gappy. CST opaque (these constructs project as RawBlock / RawInline). |
| 5 | `markdown_in_html_blocks` interaction edge cases | **Conformance landed** — depth-aware nested div, Plain/Para promotion, refs inheritance, **projector-level splitter** (`split_html_block_by_tags` byte walker + `parse_pandoc_blocks` recursive reparse), outer-matched-pair-abandons-on-void-interior. **The structural CST lift was deferred** — Phase 5's mechanism is the projector reparsing bytes, not the parser emitting structure. |
| 6 (new) | Lift inner HTML block content into structural CST children — `HTML_BLOCK_DIV` gets `PARAGRAPH` / `LIST` / etc. as direct children; `split_html_block_by_tags` / `flush_html_block_*` / `parse_pandoc_blocks` collapse into trivial CST walks; `PARAGRAPH→PLAIN` retag at adjacent-HTML-block boundary. | **All `<div>` shapes outside blockquotes now lift structurally (2026-05-11)**: clean multi-line, trailing-on-open, butted-close, indented-close, same-line, **and empty / blank-only (2026-05-11)** — `<div></div>`, `<div>\n</div>`, `<div>\n\n</div>` all take the structural walk now that `div_has_structural_inner` only rejects nodes that carry an unlifted `HTML_BLOCK_CONTENT` body. **HTML blocks (any tag) inside blockquotes now project correctly via projector marker-strip (2026-05-11)**: BLOCK_QUOTE_MARKER+WHITESPACE prefix tokens are stripped at the projector before byte-reparse. Parser-side structural lift inside bq still deferred. Earlier sub-targets: PARAGRAPH→PLAIN retag at YesCanInterrupt; `<style>` / PI / `</script>` / `<script type="math/tex…">` cannot_interrupt. Pass count progression: 132 → 137 → 140 → 141 → 142 → 145 → 148 → 151. Fix #4 from AUDIT.md (full HTML_BLOCK body structural split) still pending. |

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

## Latest session — 2026-05-11 (Phase 6: empty / blank-only `<div>` structural lift)

Trimmed the byte-reparse fallback for empty `<div></div>`,
`<div>\n</div>`, and `<div>\n\n</div>`. The parser already emits
clean `HTML_BLOCK_TAG + (optional BLANK_LINEs) + HTML_BLOCK_TAG`
children for these, but `div_has_structural_inner` required at
least one non-(TAG/BLANK_LINE/CONTENT) body child, so empties fell
through to `extract_div_inner_and_butted` / `parse_pandoc_blocks`
and re-derived `Div("",[],[]) []` from bytes. Output was already
correct; the change is a small architectural cleanup.

**Implementation** (single-site in
`crates/panache-parser/src/pandoc_ast.rs`): inverted the body
predicate from "any non-(TAG/BLANK_LINE/CONTENT) child" to "no
`HTML_BLOCK_CONTENT` children". `HTML_BLOCK_CONTENT` only appears
on still-opaque bodies (bq-wrapped divs today; non-div HTML blocks
until Fix #4 lands); its absence means the body is structurally
lifted (or empty). Same-line `<div></div>` also already produces
two clean HTML_BLOCK_TAGs and flips onto the structural walk.

**Pass count**: html 148 → 151, total 341 → 344. New corpus cases
0342/0343/0344 (same-line / newline-only / blank-only) plus paired
parser fixture `html_block_div_empty_{pandoc,commonmark}`.
CommonMark snapshot exposes the dialect divergence — type-6
HTML_BLOCK with TAG + CONTENT for `<div>\n</div>` (CM closes at
blank line, not by matching tag) vs Pandoc's HTML_BLOCK_DIV with
clean TAG+TAG.

**Payoff**: removes one "byte path still fires" smell. Establishes
the invariant that *absence* of `HTML_BLOCK_CONTENT` is the signal
for a fully lifted body — directly reusable in Fix #4 when the
parser lifts non-`<div>` bodies.

### Suggested next sub-targets

1. **Fix #4 from AUDIT.md — full HTML_BLOCK body structural split**
   (medium-large). Largest remaining lever. Parser already emits
   TAG+CONTENT+TAG for multi-line non-div HTML blocks
   (`<form>\nbody\n</form>` etc.) but inner block tags stay as
   TEXT inside CONTENT. Lifting non-`<div>` bodies the same way
   `<div>` already does (recursive `parse_with_refdefs` + graft,
   then this session's "no HTML_BLOCK_CONTENT = lifted" predicate)
   would let `emit_html_block` walk children structurally and
   eliminate `split_html_block_by_tags`, both flush helpers,
   `interior_starts_with_void_block_tag`, `find_matching_html_close*`,
   and the `inline_pending` flag. Start with strict-block tags
   (`<form>`, `<header>`, `<footer>`, `<section>`, `<article>`,
   `<nav>`, `<aside>`, …) with matching open/close; defer matched-
   pair inline-block lift (`<video>`, `<iframe>`).
2. **Parser-side structural lift for `<div>` inside blockquotes** —
   still deferred. Needs BLOCK_QUOTE_MARKER threading around
   grafted children. Wait for Fix #4's pattern.
3. **Multi-line `<script\n type=...>`** corpus pin — only if a real
   case appears.

### Files in committable diff

- `crates/panache-parser/src/pandoc_ast.rs` — `div_has_structural_inner`
  predicate inversion + comment update.
- `crates/panache-parser/tests/fixtures/pandoc-conformance/corpus/`
  — three new cases `0342–0344`.
- `crates/panache-parser/tests/fixtures/cases/html_block_div_empty_{pandoc,commonmark}/`
  + `tests/golden_parser_cases.rs` + two new snapshots — paired
  parser fixture covering all three shapes in one input.
- `crates/panache-parser/tests/pandoc/allowlist.txt` — new
  `# html-block (empty `<div></div>` …)` section + ids 342–344.
- Regenerated `tests/pandoc/report.txt` and
  `docs/development/pandoc-report.json`.

### New traps

Folded into Persistent traps under "Structural lift (Fix #3
family)": absence of `HTML_BLOCK_CONTENT` is now the projector's
signal for a fully lifted body.

--------------------------------------------------------------------------------

## Earlier sessions (compact log)

Newest first. One line per session: date — phase/sub-target — pass
count delta — root cause / lever.

- 2026-05-11 — Phase 6 bq-wrapped HTML blocks projector marker-strip — html 142 → 148 — `collect_html_block_text_skip_bq_markers` drops BLOCK_QUOTE_MARKER+WHITESPACE before byte-reparse.
- 2026-05-11 — Phase 6 Fix #3 `<div>` shape lifts (clean multi-line, trailing-on-open, butted-close, indented-close, same-line) — html 142 → 142 — recursive `parse_with_refdefs` + `graft_subtree`; cleanness predicates; `probe_same_line_div_lift`.
- 2026-05-10 → 2026-05-11 — Phase 6 cannot_interrupt (`<style>`, PI, `</script>`, `<script type=math/tex>`) + Fix #1/#2 — html 132 → 142 — PARAGRAPH→PLAIN retag at YesCanInterrupt; `is_closing` field; `is_math_tex_script_open`; pandoc `isInlineTag` (issue #10643).
- 2026-05-10 — Strict-block/verbatim closing-form lift, multi-line void open-tag, incomplete-open recursion fix, Phase 3 void `eitherBlockOrInline` — html 105 → 132 — close-tag branches, `closes_at_open_tag`, `pandoc_html_open_tag_closes` gate, `PANDOC_VOID_BLOCK_TAGS`.
- 2026-05-09 — Phase 3 + Phase 5 (non-void eitherBlockOrInline; HTML5 sectioning; `<DIV>` losslessness; Plain/Para; multi-line attrs; refs inheritance) — html 62 → 105 — projector `inline_pending` + parser `cannot_interrupt`; CM/Pandoc blockHtmlTags split; `build_refs_ctx_inherited`.
- 2026-05-08 — Phases 1-5 seed (issue #263 closed) — html 0 → 62 — `HTML_BLOCK_DIV`/`INLINE_HTML_SPAN` retag, `HTML_ATTRS` tokenization, sectioning/verbatim corpus pin, depth-aware nested `<div>`.
