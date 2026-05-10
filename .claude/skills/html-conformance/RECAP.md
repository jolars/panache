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
- **Closing forms of inline-block / void tags ARE block starts
  under Pandoc.** `</video>`, `</button>`, `</embed>` standalone
  emit as single-line `RawBlock`s in pandoc-native; the parser
  routes them via `closes_at_open_tag: true` so the block ends on
  the open line. `cannot_interrupt` (gated on tag-name membership
  in the dispatcher) keeps them from breaking running paragraphs.
  Strict-block closes (`</p>`, `</nav>`) and verbatim closes
  (`</pre>`) still fall through to inline (separate gap, tracked).
  pandoc's `htmlTag isBlockTag` matches BOTH directions — earlier
  recap claims that "closing forms must be excluded" were wrong.
- **`<script>` is in `eitherBlockOrInline` AND `blockHtmlTags`.**
  Verbatim handling fires first via `VERBATIM_TAGS`; don't add
  `script` to `PANDOC_INLINE_BLOCK_TAGS`. Likewise `<pre>`,
  `<style>`, `<textarea>` membership in `PANDOC_BLOCK_TAGS` is
  harmless — the verbatim arm fires first.

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

--------------------------------------------------------------------------------

## Phase progress

| Phase | Description | Status |
|-------|-------------|--------|
| 1 | `<div>` block lift (HTML_BLOCK_DIV + HTML_ATTRS structural) | **Complete** (2026-05-08) — issue #263 closed; `<DIV>` losslessness fix landed |
| 2 | `<span>` inline lift (INLINE_HTML_SPAN) | **Complete** (2026-05-08) |
| 3 | Sectioning + verbatim corpus pin; `eitherBlockOrInline` lift | **Complete** — non-void (2026-05-09); void (`<embed>`/`<area>`/`<source>`/`<track>`) (2026-05-10) |
| 4 | Comments, PIs, declarations, CDATA projection | **Complete** (2026-05-08); type-4 CM lowercase still gappy |
| 5 | `markdown_in_html_blocks` interaction edge cases | **Complete** — depth-aware nested div, Plain/Para promotion, refs inheritance, projector-level splitter, outer-matched-pair-abandons-on-void-interior all landed (last gap closed 2026-05-10) |

Multi-line `<div>` open-tag structural HTML_ATTRS lift landed
(2026-05-09). Multi-line void open-tag now lifts via
`find_multiline_open_end` + simple per-line TEXT/NEWLINE emission
(2026-05-10). Inline-block / void closing forms (`</video>`,
`</embed>`) also start single-line `RawBlock`s under Pandoc
(2026-05-10).

--------------------------------------------------------------------------------

## Latest session — 2026-05-10 (inline-block close lift + matched-pair abandon)

**html pass count: 122 → 126** (+4 new corpus cases). **Workspace:
0 → 0 failing.** **Total pandoc conformance: 314/314 → 318/318.**
**New parser fixtures: 3** (paired close-standalone Pandoc/CommonMark
+ video-source-fallback Pandoc).

### What landed

Closed Phase 5's last gap: `<video>\n<source src="x">\nfallback\n
</video>` projected as `[RawBlock<video>, RawBlock<source>,
Plain[fallback], RawBlock</video>]` instead of pandoc-native's
`[RawBlock<video>, RawBlock<source>, Para[Str fallback, SoftBreak,
RawInline</video>]]`. Two related parser-shape gaps surfaced and
were fixed in the same session:

1. **Parser** (`html_blocks.rs::try_parse_html_block_start`):
   accept closing forms (`</video>`, `</button>`, `</embed>`, …)
   under Pandoc dialect for both `PANDOC_INLINE_BLOCK_TAGS` and
   `PANDOC_VOID_BLOCK_TAGS`, routed via `closes_at_open_tag: true`
   so the block ends on the open-tag line. `cannot_interrupt`
   (gated on tag-name membership in the dispatcher) keeps closing
   tags from breaking running paragraphs — `foo\n</video>` still
   stays as `Para[foo, SoftBreak, RawInline</video>]`.
2. **Projector** (`pandoc_ast.rs::split_html_block_by_tags`,
   inline-block arm): new helper
   `interior_starts_with_void_block_tag` peeks past leading
   newlines/whitespace; when the interior of `<video>...</video>`
   opens with a void block tag at column 0, the matched-pair lift
   abandons and emits the open tag as a single `RawBlock`. The
   trailing `</video>` ends up as `RawInline` inside the
   reparsed-paragraph tail.
3. **Projector**: same arm now also emits a single `RawBlock` for
   inline-block opens with no matched close (e.g. `<video>\nfoo\n`)
   and for closing tags at fresh-block positions. Without this, the
   tail-text reparse re-recognized the same open tag and recursed
   to a stack overflow.

4 new corpus cases (0315–0318): `<video>\n<source>\nfallback\n
</video>` (the original divergence); `</video>\n` standalone close;
`<video>\nfoo\n` open-without-close; `</button>\n` standalone
inline-block close. 3 new paired parser fixtures
(`html_block_video_close_standalone_{pandoc,commonmark}` and
`html_block_video_source_fallback_pandoc`); the close-standalone
pair pins the dialect-divergent CST — Pandoc → HTML_BLOCK
(`closes_at_open_tag`) + PARAGRAPH; CommonMark → single HTML_BLOCK
(type 7, blank-line-terminated).

### Files in committable diff

- Parser-shape: `parser/blocks/html_blocks.rs` (+33/−14 to
  `try_parse_html_block_start` + 2 unit-test edits).
- Projector: `pandoc_ast.rs` (+44/−16 to inline-block arm of
  `split_html_block_by_tags`; new `interior_starts_with_void_block_tag`
  helper).
- Corpus: 4 new dirs under `corpus/0315..0318-…/`.
- Parser fixtures: 3 new under
  `crates/panache-parser/tests/fixtures/cases/`, registered in
  `golden_parser_cases.rs`; snapshots emitted.
- Allowlist + report regenerated.

No salsa, formatter, linter, LSP, or other host-side changes.

### Suggested next sub-targets, ranked

1. **Strict-block / verbatim closing-form lift**: `</p>`, `</nav>`,
   `</pre>` standalone — pandoc-native still emits as `RawBlock`
   but our parser leaves them as `Para[RawInline]`. Mirror this
   session's inline-block close path under the strict-block /
   verbatim arms. Risk: the strict-block close-as-block can
   interrupt running paragraphs (no `cannot_interrupt`), which may
   shift existing fixtures (`foo\n</p>` is currently
   `Para[foo, SB, RI</p>]`; pandoc emits `Plain[foo] +
   RawBlock</p>`). Triage hits before landing.
2. **Strict-block multi-line open structural cleanup**
   (`<table\n  border="1">`). Output already correct; the CST
   still splits open-tag bytes between HTML_BLOCK_TAG (line 0) and
   HTML_BLOCK_CONTENT (later lines). Reuse
   `find_multiline_open_end` + a parameterized emitter. Low value
   (no behavior change); skip unless a corpus case exercises a
   shape that depends on the cleaner CST.
3. **Audit `parse_html_attrs` and `find_matching_html_close` for
   literal-byte hazards** (still on the list from earlier sessions).
4. **Outer-wins-on-conflict for inherited refs/footnotes** (still
   deferred — no corpus exercises it).

### New traps (folded into Persistent traps)

- Closing forms of inline-block / void tags ARE block starts under
  Pandoc — earlier "exclude closing forms" rule was wrong. Folded
  into Pandoc tag categorization.
- Matched-pair lift in the projector must abandon when interior
  starts with a void block tag at column 0, AND inline-block opens
  with no matched close must emit as `RawBlock` (otherwise tail-text
  reparse recurses to stack overflow). Folded into Projector tag
  splitting.

--------------------------------------------------------------------------------

## Earlier sessions (compact log)

Newest first. One line per session: date — phase/sub-target — pass
count delta — root cause / lever.

- 2026-05-10 — Inline-block close lift + matched-pair-abandons-on-void-interior
  (`<video>\n<source>\nfallback\n</video>`, `</video>`/`</button>`/`</embed>`
  standalone) — html 122 → 126 — accept closing forms under Pandoc with
  `closes_at_open_tag: true`; new `interior_starts_with_void_block_tag`
  helper in projector; single-RawBlock emit on no-match-open / fresh-block-close
  closes the previously-recursive tail-text reparse.
- 2026-05-10 — Multi-line void open-tag recognition (`<embed\n
  src="x">`) — html 117 → 122 — generalized
  `find_multiline_open_end` over tag name + simple per-line
  TEXT/NEWLINE emit; void early-exit returns `end_line_idx + 1`.
- 2026-05-10 — Incomplete open-tag projector recursion fix
  (`<embed\n` etc. with no `>`) — html 113 → 117 — new
  `pandoc_html_open_tag_closes` gate in `block_dispatcher`; CM
  type-6 stays liberal.
- 2026-05-10 — Phase 3 void-element `eitherBlockOrInline` lift
  (`<embed>`, `<area>`, `<source>`, `<track>`) — html 105 → 113 —
  new `PANDOC_VOID_BLOCK_TAGS` + `closes_at_open_tag: bool`;
  projector void-tag branch with `inline_pending` rule; split
  `flush_html_block_text` (demotes) vs `flush_html_block_tail_text`
  (preserves Para).
- 2026-05-09 — Phase 3 `eitherBlockOrInline` non-void lift (`<iframe>`,
  `<button>`, `<video>`, `<del>`, etc.) — html 94 → 105 — context-aware
  projector with `inline_pending` flag + parser-side
  `cannot_interrupt`; blocked iframe (#287) unblocked.
- 2026-05-09 — Phase 3 corpus expansion (HTML5 sectioning + grouping:
  `<header>`, `<footer>`, `<main>`, `<details>`, `<figure>`,
  `<figcaption>`, `<nav>`) — html 87 → 94 — pure corpus growth + doc
  comment update; documented `eitherBlockOrInline` gap.
- 2026-05-09 — Phase 5 audit pivoted to `<DIV>` losslessness fix —
  html 87 → 87 — `emit_div_open_tag_tokens` had literal `"<div"`
  instead of source bytes; one-line fix + uppercase paired fixture.
  Projector cleanup deferred (low value).
- 2026-05-09 — Phase 3 dialect-divergent `blockHtmlTags`
  (`<dialog>`/`<canvas>` etc.) — html 80 → 87 — split CM/Pandoc
  block-tag lists; 7 new corpus cases.
- 2026-05-09 — Phase 5 Plain/Para promotion rule for `<div>`
  recursive reparse — html 76 → 80 — projector-only;
  `close_butted = byte_at(close_start - 1) != '\n'`; demote LAST
  block only.
- 2026-05-09 — Phase 1 multi-line `<div>` open-tag
  HTML_ATTRS structural lift — html 75 → 76 — per-line
  `HTML_ATTRS` nodes (not one big spanning node); quote state threads
  across line boundaries.
- 2026-05-09 — Phase 5 cross-boundary `RefsCtx` inheritance for
  outer→inner refs/footnotes/heading-slugs — html 72 → 75 — new
  `build_refs_ctx_inherited`; `parse_pandoc_blocks` calls it with
  `Some(&outer)`; AST gains `Clone`.
- 2026-05-09 — Phase 5 inner-`RefsCtx` for `parse_pandoc_blocks`
  recursive reparse — html 62 → 72 — heading auto-ids, ref defs,
  footnote defs inside `<div>` resolve in inner ctx; outer ctx
  saved via `mem::take` and restored.
- 2026-05-08 — Phase 5 depth-aware nested `<div>` close scan
  (case 199 unblocked) — html 57 → 62 — `count_tag_balance` walks
  same-name opens/closes; new `depth_aware` field on `BlockTag`;
  CM verbatim keeps first-close.
- 2026-05-08 — Phase 5/6 projector-level `markdown_in_html_blocks`
  for non-sectioning block tags — html 47 → 57 — byte-aware
  `split_html_block_by_tags`; new `find_matching_html_close`,
  `flush_html_block_text`, `extract_html_tag_name`.
- 2026-05-08 — CommonMark type-4 lowercase declaration recognition
  — html 47 → 47 (CM-side fix; no Pandoc corpus impact) — paired
  parser fixture.
- 2026-05-08 — Phase 4 follow-up: gate type-4/type-5 HTML blocks
  off under Pandoc dialect — html 39 → 47 — `<!DOCTYPE>`/`<![CDATA>`
  fall through to paragraph parsing; `try_parse_inline_html` gained
  `dialect: Dialect` parameter.
- 2026-05-08 — Phase 4 comments + processing instructions corpus
  pin — html 27 → 39 — pure corpus growth; declaration/CDATA
  parser-shape gap noted.
- 2026-05-08 — Phase 3 sectioning + verbatim negative-space pin
  (`<section>`, `<article>`, `<aside>`, `<nav>`, `<pre>`, `<style>`,
  `<script>`, `<textarea>`) — html 17 → 27 — pure corpus growth.
- 2026-05-08 — Phase 2 `<span>` inline lift — html 9 → 17 —
  `INLINE_HTML_SPAN` retag of `BRACKETED_SPAN`; attribute region
  restructured from `SPAN_ATTRIBUTES` token to `HTML_ATTRS` node.
  `<span>` was already lifting; corrected the misleading "INLINE_HTML"
  starting-state claim from Phase 1's RECAP.
- 2026-05-08 — Phase 1 `<div>` block lift (issue #263 closed) —
  html 0 → 9 — `HTML_BLOCK_DIV` wrapper retag + `HTML_ATTRS`
  open-tag tokenization; `AttributeNode::can_cast(HTML_ATTRS)` so
  salsa walk picks up `<div id>` automatically; nested-div blocked
  as Phase 5 target.
