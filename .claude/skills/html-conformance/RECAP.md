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
  reset per-line. `count_tag_balance`, `find_multiline_div_open_end`
  do this right.
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
- **Closing forms must be excluded from the block-start
  recognizer.** `<button>` opens a block; `</button>` does not (it
  goes inline). Mirrors pandoc's `htmlTag isBlockTag` which only
  matches open tags.
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
| 5 | `markdown_in_html_blocks` interaction edge cases | **Mostly complete** — depth-aware nested div, Plain/Para promotion, refs inheritance, projector-level splitter all landed; outer-matched-pair-with-inner-split-conflict still gappy |

Multi-line `<div>` open-tag structural HTML_ATTRS lift landed
(2026-05-09). Multi-line void open-tag still falls back to opaque
HTML_BLOCK.

--------------------------------------------------------------------------------

## Latest session — 2026-05-10 (Phase 3 — void-element `eitherBlockOrInline` lift)

**html (block + inline) pass count: 105 → 113** (+8 new corpus
cases). **Workspace: 0 → 0 failing.** **Total pandoc conformance:
297/297 → 305/305.** **New parser fixtures: 2** (paired `<embed>`
Pandoc / CommonMark).

### What landed

Closes the previous session's "Suggested next sub-target #1": void
`eitherBlockOrInline` tags (`<embed>`, `<area>`, `<source>`,
`<track>`) now lift to a single RawBlock at fresh-block positions,
stay inline mid-paragraph, and split inside an existing strict-block
parent (`<video>\n<source>\n</video>` is now 3 RawBlocks instead of
RawBlock+Plain[RawInline]+RawBlock — closes the previous "next #2"
as a free byproduct).

1. New `PANDOC_VOID_BLOCK_TAGS` const + `is_pandoc_void_block_tag_name`
   pub fn in `parser/blocks/html_blocks.rs`.
2. New `closes_at_open_tag: bool` field on `BlockTag`. Distinct from
   `depth_aware` (would walk to EOF for void) and
   `closed_by_blank_line`. When true, the block always ends on the
   open-tag line.
3. `try_parse_html_block_start` adds a void-tag branch (Pandoc +
   non-closing form + membership).
4. `block_dispatcher.rs::cannot_interrupt` extended to include void
   tags.
5. `pandoc_ast.rs::split_html_block_by_tags` adds a void-tag branch:
   emits a single RawBlock per instance via the `inline_pending`
   rule. No matched-pair lookup.
6. **Bonus**: split `flush_html_block_text` into inter-tag
   (demotes) vs `flush_html_block_tail_text` (preserves Para). The
   pre-existing uniform demotion silently broke `<form>\nfoo\n`
   and would have broken `<embed src="x"> trailing text`.

8 new corpus cases (0298–0305) under a new
`# html-block (eitherBlockOrInline void elements …)` allowlist
section. 2 new paired parser fixtures
(`html_block_embed_void_{pandoc,commonmark}`) pin the
dialect-divergent CST shape (Pandoc closes on void-tag line;
CommonMark continues to blank line). New unit test
`test_pandoc_void_block_tag_membership`.

### Files in committable diff

- Parser-shape: `parser/blocks/html_blocks.rs` (~85 lines net),
  `parser/block_dispatcher.rs` (~3 lines).
- Projector: `pandoc_ast.rs` (~30 lines).
- Corpus: 8 new dirs under `corpus/0298..0305-…/`.
- Allowlist + report regenerated.
- 2 new parser fixtures + snapshots, registered in
  `golden_parser_cases.rs`.

No salsa, formatter, linter, LSP, or other host-side changes.

### Suggested next sub-targets, ranked

1. **`<video>\n<source>\nfallback\n</video>` outer-wins-on-conflict.**
   Pandoc emits 2 RawBlocks (`<video>`, `<source>`) then a single
   Para containing `fallback\n</video>` as Para+RawInline (the
   matched-pair lift was abandoned once `<source>` split out).
   Today panache emits 4 blocks (RawBlock + RawBlock +
   Plain[fallback] + RawBlock(`</video>`)). The fix needs the
   projector to recognize that once a strict-block tag has emitted
   *inside* the matched-pair scan, the outer tag's closing should
   downgrade to inline. Likely a state flag in
   `find_matching_html_close_with_start` or a post-process pass.
   No corpus case yet — add a deliberately-blocked entry first.
2. **Multi-line void open tags** (`<embed\n  src="x">`). Today
   `try_parse_html_block_start` only inspects the first line, so
   the line falls through to inline raw HTML. Generalize the
   multi-line open-tag path that already exists for `<div>`.
3. **CommonMark type-4 lowercase recognition gap**. Tighten the
   uppercase-only gate in `try_parse_html_block_start` to
   `is_ascii_alphabetic` so CM dialect matches the spec
   (`<!doctype html>`). ~5-line change.
4. **Audit `parse_html_attrs` and `find_matching_html_close` for
   literal-byte hazards** (still on the list from earlier sessions).
5. **Outer-wins-on-conflict for inherited refs/footnotes** (still
   deferred — no corpus exercises it).

### New trap (folded into Persistent traps)

- `closes_at_open_tag` is the right model for void tags
  (`depth_aware` walks to EOF; `closed_by_blank_line` swallows the
  next line under CommonMark semantics).
- Tail text in a split HTML block must NOT demote Para→Plain (split
  `flush_html_block_text` into inter-tag vs tail).
- Void tags fall through to CM type 7 when not in BLOCK_TAGS;
  `<embed>` and `<area>` aren't in CM BLOCK_TAGS but ARE complete
  tags on a line by themselves → Type7. Tests should assert Type7,
  not None.

--------------------------------------------------------------------------------

## Earlier sessions (compact log)

Newest first. One line per session: date — phase/sub-target — pass
count delta — root cause / lever.

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
