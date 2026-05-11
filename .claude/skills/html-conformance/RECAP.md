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

The pandoc-AST projector at `crates/panache-parser/src/pandoc_ast.rs`
is a **test-only diagnostic** for CST shape, not a runtime artifact.
Phases 1/5 landed structural retags (`HTML_BLOCK_DIV`,
`INLINE_HTML_SPAN`); Phase 6's Fix #3 (first cut, 2026-05-11) lifted
inner content of CLEAN multi-line `<div>...</div>` shapes into
structural CST children. The projector still re-runs the markdown
parser on the remaining HTML block bodies via `parse_pandoc_blocks` /
`split_html_block_by_tags` / `flush_html_block_text` /
`flush_html_block_tail_text` / `try_div_html_block` — applied for
opaque `HTML_BLOCK`, same-line `<div>foo</div>`, trailing-content-on-
open, butted-close, and div-inside-blockquote. **The path forward is
parser work** (lift inner blocks into CST children, retag
PARAGRAPH→PLAIN when appropriate, etc.); each lift collapses a chunk
of projector compensation into a trivial CST walk. Defensible
reparses (table cells via `parse_grid_cell_text` /
`parse_cell_text_inlines`) match how pandoc itself sub-parses cell
content and can stay.

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
- **Structural lift covers ONLY "clean" tags.** The
  `html_block_{open,close}_tag_is_clean` predicates gate the
  structural walk: open tag must end with `>` + NEWLINE only (no
  trailing content), close tag's first TEXT token must start with
  `</`. Trailing-content-on-open (`<div>foo\nbar\n</div>`) and
  butted-close (`<div>\nfoo\nbar</div>`) keep content INSIDE
  `HTML_BLOCK_TAG`; the structural walk would silently drop it.
  Messy shapes fall through to the byte reparse path. Same-line
  `<div>foo</div>` has only ONE `HTML_BLOCK_TAG` and likewise
  falls through. Don't relax the cleanness predicates without
  parser-side handling for the missing content.
- **Parser-side structural lift is gated on `bq_depth == 0`.**
  Inside blockquotes, content lines carry `> ` markers that
  `emit_html_block_line` knows to split into BLOCK_QUOTE_MARKER +
  WHITESPACE + TEXT. The recursive parse would feed the markers
  back into the inner parser and produce a nested blockquote.
  Strip markers first before extending coverage.

--------------------------------------------------------------------------------

## Phase progress

| Phase | Description | Status |
|-------|-------------|--------|
| 1 | `<div>` block lift (HTML_BLOCK_DIV + HTML_ATTRS structural) | **Wrapper retag landed** (2026-05-08) — issue #263 closed; `<DIV>` losslessness fix landed. **Inner content NOT yet lifted into CST children** — still raw `HTML_BLOCK_CONTENT` TEXT tokens; projector reparses them. |
| 2 | `<span>` inline lift (INLINE_HTML_SPAN) | **Wrapper retag landed** (2026-05-08). Inner inlines mostly trivial (no recursive reparse needed). |
| 3 | Sectioning + verbatim corpus pin; `eitherBlockOrInline` lift | **Conformance landed** — non-void (2026-05-09); void (`<embed>`/`<area>`/`<source>`/`<track>`) (2026-05-10). Implementation leans on projector-side `inline_pending` tracking + byte walker; CST still opaque for split/matched-pair shapes. |
| 4 | Comments, PIs, declarations, CDATA projection | **Conformance landed** (2026-05-08); type-4 CM lowercase still gappy. CST opaque (these constructs project as RawBlock / RawInline). |
| 5 | `markdown_in_html_blocks` interaction edge cases | **Conformance landed** — depth-aware nested div, Plain/Para promotion, refs inheritance, **projector-level splitter** (`split_html_block_by_tags` byte walker + `parse_pandoc_blocks` recursive reparse), outer-matched-pair-abandons-on-void-interior. **The structural CST lift was deferred** — Phase 5's mechanism is the projector reparsing bytes, not the parser emitting structure. |
| 6 (new) | Lift inner HTML block content into structural CST children — `HTML_BLOCK_DIV` gets `PARAGRAPH` / `LIST` / etc. as direct children; `split_html_block_by_tags` / `flush_html_block_*` / `parse_pandoc_blocks` collapse into trivial CST walks; `PARAGRAPH→PLAIN` retag at adjacent-HTML-block boundary. | **Fix #1 landed (2026-05-10)** — `PARAGRAPH→PLAIN` retag at YesCanInterrupt for HTML BlockTag under Pandoc; +5 (132 → 137 html). **`<style>` + PI sub-target landed (2026-05-10)** — `cannot_interrupt` under Pandoc; +3 (137 → 140). **Fix #2 landed (2026-05-10)** — `html_div_block` reads open-tag attrs via `HTML_BLOCK_TAG → HTML_ATTRS` CST walk; pure projector cleanup, no delta. **`</script>` close cannot_interrupt landed (2026-05-10)** — `is_closing` field added to `HtmlBlockType::BlockTag`; +1 (140 → 141). **`<script type="math/tex…">` open cannot_interrupt landed (2026-05-11)** — `is_math_tex_script_open` helper inspects `ctx.content` attrs; +1 (141 → 142). **Fix #3 first cut landed (2026-05-11)** — clean multi-line `<div>` now lifts inner content into structural CST children (`PARAGRAPH`, `HEADING`, nested `HTML_BLOCK_DIV`, etc.) via recursive `parse_with_refdefs` + graft; `collect_block` routes `HTML_BLOCK_DIV` to `html_div_block` (structural walk) and `cst_div_open_tag_attr` concatenates multi-line attribute regions; pass count unchanged (142, all messy shapes still byte-reparse). Fix #4 from AUDIT.md still pending. |

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

## Latest session — 2026-05-11 (Phase 6 Fix #3 first cut: `<div>` inner-block structural lift)

Landed the parser-side structural lift for clean multi-line
`<div>...</div>` shapes. The inner content lines (between separate
open and close `HTML_BLOCK_TAG` siblings) now parse recursively at
parse time; the resulting top-level blocks (`PARAGRAPH`, `HEADING`,
nested `HTML_BLOCK_DIV`, `LIST`, `BLANK_LINE`, …) become direct CST
children of `HTML_BLOCK_DIV` instead of opaque `HTML_BLOCK_CONTENT`
TEXT. The projector's `html_div_block` now walks those children
when the open/close tags are "clean"; the byte path stays for
shapes the first cut intentionally doesn't lift.

**Implementation**:
- `parse_html_block_with_wrapper` (`parser/blocks/html_blocks.rs`)
  takes `config: &ParserOptions` so the recursive parse inherits
  the outer's refdef labels. New `emit_html_block_body` chooses
  between the legacy `HTML_BLOCK_CONTENT` capture and the new lift;
  lift fires when `wrapper_kind == HTML_BLOCK_DIV && bq_depth == 0`
  and `content_lines` is non-empty.
- `emit_recursively_parsed_blocks` calls
  `crate::parser::parse_with_refdefs(inner_text, ...)` on the
  joined content lines and grafts the document children into the
  current builder via a small `graft_subtree` recursion (token-by-
  token, byte-equal).
- Projector: `collect_block` (`pandoc_ast.rs`) now routes
  `HTML_BLOCK_DIV` to `html_div_block` (was: `emit_html_block` for
  both opaque and lifted variants). `html_div_block` walks
  structural children when `div_has_structural_inner` passes —
  i.e. exactly one open + one close `HTML_BLOCK_TAG`, both
  "clean" (open ends with `>` + newline only; close starts with
  `</`). All other shapes fall through to `extract_div_inner_and_butted`
  + `assemble_div_block` (byte reparse).
- `cst_div_open_tag_attr` now joins ALL `HTML_ATTRS` regions of
  the open tag (multi-line opens have one per attribute line),
  fixing classes/kvs loss for `<div\n  id="x"\n  class="y">`.

**Pass count**: html 142 → 142 (no delta — all messy shapes still
go through the byte reparse, which already handled them). The
payoff is structural: inner headings, paragraphs, and emphasis
inside `<div>` are now visible to salsa / LSP / linter via the
normal CST walk instead of being hidden behind an opaque
`HTML_BLOCK_CONTENT` blob. Six existing parser-golden snapshots
accepted with the richer shape; new paired fixtures
`html_block_div_inner_blocks_{pandoc,commonmark}` pin the
structural difference between dialects.

**Cleanup not done**: The byte-reparse helpers
(`try_div_html_block`, `extract_div_inner_and_butted`,
`assemble_div_block`, `parse_pandoc_blocks`) are still on the path
for messy shapes (same-line `<div>foo</div>`, trailing content on
open tag `<div>foo\nbar\n</div>`, butted close `<div>\nfoo\nbar</div>`,
div-inside-blockquote). Eliminating them needs the parser to also
fold trailing-content-on-open and butted-close content back into
the recursive parse input, which is the next sub-target.

### Suggested next sub-targets

1. **Lift messy `<div>` shapes** (small-medium). Concretely:
   trailing-content-on-open (`<div>foo\nbar\n</div>`) and butted-
   close (`<div>\nfoo\nbar</div>`) currently keep content inside
   `HTML_BLOCK_TAG`. Extract that content at parse time and prepend/
   append to the recursive parse input so the structural lift
   covers them too. Eliminates `extract_div_inner_and_butted` +
   `assemble_div_block` + the `close_butted` Plain/Para rule (the
   parser produces `PARAGRAPH` or `PLAIN` directly). Same-line
   `<div>foo</div>` is a separate trickier sub-target (no
   separate close `HTML_BLOCK_TAG` exists; needs parser-side
   recognition).
2. **Fix #4 — full HTML_BLOCK body structural split** (large;
   defer until #1 above settles the messy-`<div>` pattern).
   Eliminates `split_html_block_by_tags`, both flush helpers,
   `interior_starts_with_void_block_tag`,
   `find_matching_html_close*`, `inline_pending` flag.
3. **Lift inside blockquotes** — the structural lift gates on
   `bq_depth == 0` for simplicity. Strip blockquote markers from
   inner content lines before recursive parse to extend coverage.
4. **Multi-line `<script\n type=...>`** corpus pin — only if a
   real case appears.

### Files in committable diff

- `crates/panache-parser/src/parser/blocks/html_blocks.rs` —
  `parse_html_block_with_wrapper` takes `&ParserOptions`; new
  `emit_html_block_body` / `emit_recursively_parsed_blocks` /
  `graft_document_children` / `graft_subtree` helpers. Unit tests
  threaded with `ParserOptions::default()`.
- `crates/panache-parser/src/parser/block_dispatcher.rs` —
  call site passes `ctx.config`.
- `crates/panache-parser/src/pandoc_ast.rs` — `collect_block`
  routes `HTML_BLOCK_DIV` to `html_div_block`; `html_div_block`
  walks structural children when `div_has_structural_inner`
  passes; new `html_block_open_tag_is_clean` /
  `html_block_close_tag_is_clean` cleanness predicates;
  `cst_div_open_tag_attr` joins multi-line `HTML_ATTRS` regions.
- `crates/panache-parser/tests/fixtures/cases/html_block_div_inner_blocks_{pandoc,commonmark}/`
  + `tests/golden_parser_cases.rs` — paired parser goldens pinning
  the dialect-divergent shape (Pandoc: `PARAGRAPH` child;
  CommonMark: opaque `HTML_BLOCK_CONTENT`).
- Six accepted snapshot updates for existing div fixtures
  (`html_block_div_with_id_pandoc`, `html_block_div_uppercase_pandoc`,
  `html_block_div_multiline_open_pandoc`, `html_block_div_nested_pandoc`,
  `html_block`, `html_block_commonmark_type6_type7_pandoc`) — all
  show inner `HTML_BLOCK_CONTENT` TEXT replaced by structural
  `PARAGRAPH` / `EMPHASIS` / nested `HTML_BLOCK_DIV` / `BLANK_LINE`.
- `.claude/skills/html-conformance/RECAP.md` — Phase 6 row gains a
  "Fix #3 first cut" entry; this Latest session.

### New traps

All folded into Persistent traps under "Structural lift (Fix #3
family)": recursive parse must use `parse_with_refdefs`;
`collect_block` must route `HTML_BLOCK_DIV` to `html_div_block`;
multi-line open tags emit multiple `HTML_ATTRS` regions; structural
lift covers only "clean" tags; parser-side lift gated on
`bq_depth == 0`.

--------------------------------------------------------------------------------

## Earlier sessions (compact log)

Newest first. One line per session: date — phase/sub-target — pass
count delta — root cause / lever.

- 2026-05-11 — Phase 6 `<script type="math/tex…">` open
  cannot_interrupt — html 141 → 142 — `is_math_tex_script_open`
  helper inspects single-line open's `type` attr; pandoc's
  `isInlineTag` math/tex special case.
- 2026-05-10 — Phase 6 `</script>` close cannot_interrupt + tag-set
  audit — html 140 → 141 — added `is_closing: bool` to
  `HtmlBlockType::BlockTag`; root-caused the non-interrupt set to
  `isInlineTag` special cases (pandoc issue #10643), not tag-set
  membership. Corrected earlier `<style>` rationalization.
- 2026-05-10 — Phase 6 Fix #2 (`html_div_block` open-tag attrs from
  CST walk) — 140 → 140 — pure projector cleanup; shared
  `extract_div_inner_and_butted` + `assemble_div_block` helpers.
- 2026-05-10 — Phase 6 `<style>` + PI cannot_interrupt — 137 → 140.
- 2026-05-10 — Phase 6 Fix #1: PARAGRAPH→PLAIN retag at HTML
  strict/verbatim adjacency — 132 → 137 —
  `close_paragraph_as_plain_if_open` at YesCanInterrupt.
- 2026-05-10 — Projector audit + course correction (reverted
  projector Para→Plain demotion) — 132 → 132 — AUDIT.md ranks
  fixes #1-#4; "What this skill is NOT" added to SKILL.md.
- 2026-05-10 — Strict-block/verbatim closing-form lift, multi-line
  void open-tag, incomplete-open-tag recursion fix, Phase 3 void
  `eitherBlockOrInline` — 105 → 132 — close-tag branches,
  `closes_at_open_tag`, `pandoc_html_open_tag_closes` gate,
  `PANDOC_VOID_BLOCK_TAGS`, split flush helpers.
- 2026-05-09 — Phase 3 lifts (non-void eitherBlockOrInline; HTML5
  sectioning; `<DIV>` losslessness; Phase 5 Plain/Para, multi-line
  attrs, refs inheritance) — 62 → 105 — context-aware projector
  `inline_pending` + parser `cannot_interrupt`; CM/Pandoc
  blockHtmlTags split; `build_refs_ctx_inherited`.
- 2026-05-08 — Phases 1-5 seed (issue #263 closed) — 0 → 62 —
  `HTML_BLOCK_DIV`/`INLINE_HTML_SPAN` retag, `HTML_ATTRS`
  tokenization, sectioning/verbatim corpus pin, depth-aware
  nested `<div>`.
