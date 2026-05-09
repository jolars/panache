# HTML conformance — running session recap

This file is the rolling, terse handoff between sessions of the
`html-conformance` skill. Read it at the start of a session for the
suggested next sub-target and known traps; rewrite the **Latest session**
entry at the end with what changed and what to look at next.

Keep entries short. Pass counts + a one-line root cause beat a narrative.
The hard-won judgment calls (why a lever was chosen, why an approach was
reverted, what trap to avoid) are the load-bearing content here.

--------------------------------------------------------------------------------

## Latest session — 2026-05-09 (Phase 3 — dialect-divergent `blockHtmlTags` for `<dialog>`/`<canvas>`/etc.)

**html (block + inline) pass count: 80 → 87** (7 new corpus cases —
all passing).
**Workspace test count: 0 failing → 0 failing** (all green).
**Total pandoc conformance: 272/272 → 279/279 (100.0% → 100.0%)**.

### What landed

Pandoc-markdown's `htmlBlock` reader uses pandoc's own
`blockHtmlTags` set (see
`pandoc/src/Text/Pandoc/Readers/HTML/TagCategories.hs`), which
disagrees with CommonMark §4.6 type-6 in **both directions**:

- **CM-only** block tags (Pandoc treats as inline raw HTML):
  `dialog`, `legend`, `menuitem`, `optgroup`, `option`, `frame`,
  `base`, `basefont`, `link`, `param`.
- **Pandoc-only** block tags (CM does not type-6 recognize):
  `canvas`, `hgroup`, `isindex`, `meta`, `output`.

Until this session, `BLOCK_TAGS` was the single shared list used
by **both** the parser's `try_parse_html_block_start` AND the
projector's `is_html_block_tag_name` callsite in
`split_html_block_by_tags`. So `<dialog>foo</dialog>` produced
`Plain [Str "foo"]` flanked by `RawBlock`s under Pandoc, where
pandoc emits `Para [RawInline "<dialog>", Str "foo", RawInline
"</dialog>"]`.

Three changes:

1. **`PANDOC_BLOCK_TAGS` constant + `is_pandoc_block_tag_name`
   predicate** in `parser/blocks/html_blocks.rs` mirror pandoc's
   `blockHtmlTags`. The CM-list-as-`BLOCK_TAGS` stays unchanged
   (CM still recognizes `<dialog>` as type-6).
2. **`try_parse_html_block_start`** gates the membership check
   on `is_commonmark`: CM uses `BLOCK_TAGS`, Pandoc uses
   `PANDOC_BLOCK_TAGS`. The `is_commonmark` flag was already
   plumbed end-to-end (dispatcher + continuation), so no other
   wiring needed.
3. **Projector** (`pandoc_ast.rs::split_html_block_by_tags`)
   switches its `is_html_block_tag_name(name)` callsite to
   `is_pandoc_block_tag_name(name)`. The projector only runs
   under Pandoc dialect, so this is a direct substitution.

CST losslessness preserved — this is purely a recognizer-list
swap; the `HTML_BLOCK` byte ranges that *do* lift remain the
same shape, only fewer tags trigger the lift now. Tags that no
longer trigger the lift parse as paragraphs containing
`INLINE_HTML` nodes (CommonMark-spec inline HTML, dialect-shared).

All 15 divergent tags verified end-to-end (`pandoc -f markdown
-t native` matches `panache parse --to pandoc-ast` byte-for-byte).

### Files in committable diff

- `crates/panache-parser/src/parser/blocks/html_blocks.rs` —
  add `PANDOC_BLOCK_TAGS` const + `is_pandoc_block_tag_name`
  pub fn; gate the BLOCK_TAGS membership check by
  `is_commonmark`; new unit test
  `test_dialect_specific_block_tag_membership` covering all 15
  divergent tags in both directions. Net ~80 lines.
- `crates/panache-parser/src/pandoc_ast.rs` — projector callsite
  swaps `is_html_block_tag_name` → `is_pandoc_block_tag_name`
  (2-line change: import + use site).
- 7 new corpus directories under
  `crates/panache-parser/tests/fixtures/pandoc-conformance/corpus/`:
  - `0273-html-inline-dialog-plain` —
    `<dialog>foo</dialog>` (Para+RawInlines).
  - `0274-html-inline-option-plain` —
    `<option>foo</option>` (Para+RawInlines).
  - `0275-html-inline-legend-with-text` — legend in
    surrounding paragraph text.
  - `0276-html-block-canvas-plain` —
    `<canvas>foo</canvas>` (RawBlock+Plain+RawBlock).
  - `0277-html-block-meta-plain`
  - `0278-html-block-hgroup-plain`
  - `0279-html-block-output-plain`
- 2 new paired parser fixtures:
  - `crates/panache-parser/tests/fixtures/cases/html_block_dialog_inline_pandoc/`
    + snapshot pinning the Paragraph + INLINE_HTML CST shape.
  - `crates/panache-parser/tests/fixtures/cases/html_block_dialog_inline_commonmark/`
    + snapshot pinning the HTML_BLOCK shape (CM-side reference).
- `crates/panache-parser/tests/golden_parser_cases.rs`
  registers both new cases.
- `crates/panache-parser/tests/pandoc/allowlist.txt`
  — new section `# html-block / html-inline (Pandoc-specific
  blockHtmlTags …)` with ids 273..279.
- `crates/panache-parser/tests/pandoc/report.txt` +
  `docs/development/pandoc-report.json` (regenerated; 279/279
  passing, 100%; html 80 → 87).

No salsa, formatter, linter, or other host-side changes.

### Why parser- and projector-side together

The projector swap alone would fix `parse --to pandoc-ast`
output but would leave the structural CST wrong: under Pandoc,
`<dialog>foo</dialog>` would still parse as `HTML_BLOCK`, which
is incorrect under pandoc-markdown semantics — pandoc treats
the line as a paragraph with raw inline HTML. The parser change
makes the CST match what pandoc-native semantics dictate.
Salsa, the formatter, and any LSP consumer all walk the
structural CST, not the projection.

### What's still NOT covered

- **HTML5 tags pandoc doesn't list at all** — e.g. `<dialog>`
  is in HTML5 but pandoc's `blockHtmlTags` predates HTML5; if
  pandoc later adds it, the lists will need re-sync. Not a bug
  today, just a "this list is frozen against pandoc 3.9.0.2".
- **Form-element nested lists** — `<form><option>X</option>...`
  combinations may produce different shapes than tested here;
  no corpus case forces them.
- **DocBook / EPUB block tag overlap** — pandoc's full
  `blockTags` is `unions [blockHtmlTags, blockDocBookTags,
  epubTags]`. Panache only mirrors `blockHtmlTags`. If a real
  document needs DocBook lift (e.g. `<itemizedlist>`), add
  separate sets. Pandoc's `htmlBlock` reader uses the union; we
  use only the HTML subset. Edge case; defer until evidence.

### Suggested next sub-targets, ranked

1. **Phase 3 corpus expansion — sectioning / verbatim
   negative-space.** Pin existing pandoc-native parity for the
   modern HTML5 tags not yet in the corpus: `<header>`,
   `<footer>`, `<main>`, `<details>`, `<figure>`,
   `<figcaption>`, `<form>`, `<table>`, `<iframe>`. These
   currently match (verified in this session's probes), but
   without corpus cases a future regression won't be caught.
   Pure low-risk corpus growth; ~6-10 cases.
2. **Phase 4 — Comments, processing instructions, declarations,
   CDATA projection.** Already mostly correct (verified earlier
   for `<?xml>` and `<!DOCTYPE>`); just seed corpus to lock
   behavior. Pure negative-space pinning.
3. **Phase 5 — projector cleanup.** Audit
   `try_div_html_block`'s remaining byte-aware close lookahead.
   Some paths may simplify now that parser-side multi-line +
   nested + balanced opens all lift structurally. Low risk,
   low immediate value.
4. **Outer-wins-on-conflict for inherited refs/footnotes**
   (still deferred — no corpus exercises it).

### Don't redo / known traps (new this session)

- **CM and Pandoc have different `blockHtmlTags` lists, in
  both directions.** Panache previously shared one list for
  both purposes (parser type-6 detection + projector tag
  splitting). Don't undo the split — the lists differ by 15
  tags total. The parser's `is_commonmark` flag is the gate;
  the projector only runs under Pandoc and should use
  `is_pandoc_block_tag_name` directly.
- **Pandoc 3.9.0.2's list is taken from
  `pandoc/src/Text/Pandoc/Readers/HTML/TagCategories.hs`.**
  When bumping the pinned pandoc version, re-check this list
  for additions/removals. The vendored read-only pandoc
  checkout in the workspace root is the ground truth.
- **`<dialog>` matches CM type-7 (complete tag on a line) even
  when removed from CM's BLOCK_TAGS.** The unit test for
  dialect-specific membership must assert "is NOT a `BlockTag`
  variant under CM" — not "is `None` under CM" — because
  type-7 is a separate, valid CM HTML-block recognizer.
- **The CM-side dialog snapshot shows `HTML_BLOCK` (type-6),
  not `Type7`.** Why? Because `<dialog>` IS in CM's BLOCK_TAGS
  list, so type-6 fires before type-7's fallback path. The
  fixture intentionally captures this CM behavior alongside
  the Pandoc one.
- **Pandoc's `blockHtmlTags` notably includes `pre`,
  `script`, `style`, `textarea`** even though those are
  panache's `VERBATIM_TAGS`. Membership in
  `PANDOC_BLOCK_TAGS` for these tags is harmless (they hit
  the BlockTag arm first; verbatim handling kicks in
  separately via `VERBATIM_TAGS.contains`). Don't try to
  remove them from `PANDOC_BLOCK_TAGS`.

--------------------------------------------------------------------------------

## Earlier session — 2026-05-09 (Phase 5 — Plain/Para promotion rule for `<div>` recursive reparse)

**html (block + inline) pass count: 76 → 80** (4 new corpus cases —
all passing).
**Workspace test count: 0 failing → 0 failing** (all green).
**Total pandoc conformance: 268/268 → 272/272 (100.0% → 100.0%)**.

### What landed

Projector-only fix in `try_div_html_block` (`pandoc_ast.rs`). The old
rule keyed on `multiline = byte_after_open_gt == '\n'` and only
demoted when `blocks.len() == 1`. Both halves were wrong:

- The signal is whether the *close* `</div>` sits on its own
  column-0 line, not whether content starts on a fresh line after
  the open `>`.
- Demotion applies to the LAST block regardless of how many blocks
  precede it (probe11: `<div>\nfoo\n\nbar</div>` → `[Para foo,
  Plain bar]` in pandoc; old logic kept both as Para).

New rule: `close_butted = byte_at(close_start - 1) != '\n'`. When
`close_butted`, demote the trailing `Para` (if present) to `Plain`;
otherwise leave blocks as-is. Verified across 16 ad-hoc probes and
4 new corpus cases:

- `<div>X</div>` (one-liner): Plain ✓
- `<div>\nX</div>` (close butted to last line): Plain ✓
- `<div>X\n</div>` (close on own line): Para ✓
- `<div>X\n   </div>` (close on indented line): Plain ✓
- `<div>\nfoo\n\nbar</div>` (multi-block, butted close): Para+Plain ✓
- `<div>trailing\nbody\n</div>` (issue #263 trailing-after-`>` form): Para ✓

### Files in committable diff

- `crates/panache-parser/src/pandoc_ast.rs` — `try_div_html_block`:
  drop `multiline`, compute `close_butted` from byte before
  `</div>`, demote trailing `Para` regardless of block count. Net
  ~10 lines changed.
- 4 new corpus directories under
  `crates/panache-parser/tests/fixtures/pandoc-conformance/corpus/`:
  - `0269-html-block-div-trailing-on-open-with-close-on-own-line`
    — `<div id="x">trailing\nbody\n</div>` (Para). Pins issue #263
    "trailing after `>` + close on own line" shape.
  - `0270-html-block-div-single-line-content-close-on-own-line` —
    `<div id="x">foo\n</div>` (Para). Simplest "Para preserved by
    own-line close" case.
  - `0271-html-block-div-multi-block-with-butted-close` —
    `<div id="x">\nfoo\n\nbar</div>` (Para+Plain). Demonstrates
    demotion applies to LAST block only, not all blocks.
  - `0272-html-block-div-close-on-indented-line` —
    `<div id="x">a\n   </div>` (Plain). Pins the whitespace-aware
    edge: indent before close → Plain.
- `crates/panache-parser/tests/pandoc/allowlist.txt` — new section
  `# html-block (div Plain/Para promotion ...)` with ids 269..272.
- `crates/panache-parser/tests/pandoc/report.txt` +
  `docs/development/pandoc-report.json` (regenerated; 272/272
  passing, 100%).

No parser, salsa, or formatter logic changes — pure projector +
corpus.

### Why projector-only

CST shape is unchanged for these inputs — the parser already
produces `HTML_BLOCK_DIV` with raw `HTML_BLOCK_CONTENT` between the
open/close `HTML_BLOCK_TAG`s. The bug was purely in how the
projector reads the boundary between content and close tag to
decide Plain vs Para. No structural change needed.

### What's still NOT covered

- **`flush_html_block_text` (non-div HTML blocks with intermixed
  tags)** has its own `trailing_blank = trailing_newlines >= 2`
  rule that demotes the last Para → Plain when the chunk butts up
  against the next tag. Different context (inter-tag fragment
  inside a non-div HTML block, not balanced div content), and the
  current corpus doesn't exercise the cases where this rule could
  diverge from pandoc. Defer until evidence.
- **`<span>` Phase 2** still pending — the inline-side analog of
  Phase 1. Coordinates with `pandoc-ir-migrate` which already
  emits a `ConstructKind::PandocOpaque` event for `<span>`; the
  lift just retags `INLINE_HTML` → `INLINE_HTML_SPAN` for balanced
  spans under Pandoc. No conformance case forces this yet.
- **Cross-boundary cite numbering** + **outer-wins-on-conflict for
  inherited refs/footnotes** — both still deferred (no corpus case
  exercises them).

### Suggested next sub-targets, ranked

1. **Phase 2 — `<span>` lift.** The inline-side Phase 1 analog. The
   IR machinery in `inline_ir.rs` already opaque-scans `<span>`;
   add an `INLINE_HTML_SPAN` retag in
   `parser/inlines/inline_html.rs` (or wherever the wrapper is
   emitted) gated on `Dialect::Pandoc`, and reuse `parse_html_attrs`
   in the projector. Should unlock `<span id="x">…</span>` anchor
   indexing for the linter just like `<div id>`.
2. **Phase 5 — projector cleanup.** Now that the Plain/Para rule
   is correct, audit `try_div_html_block`'s remaining
   byte-aware close lookahead. Some paths may simplify now that
   parser-side multi-line + nested + balanced opens all lift
   structurally. Low risk, low immediate value.
3. **Phase 5 — `flush_html_block_text` Plain/Para audit.** Same
   shape of bug may live in the inter-tag fragment path. Add a
   probe: `<p>foo</p>X<p>bar</p>` etc. and see what pandoc returns
   vs. panache.
4. **Outer-wins-on-conflict for inherited refs/footnotes** (still
   deferred — no corpus exercises it).

### Don't redo / known traps (new this session)

- **The Plain/Para signal is `</div>`-side, not `<div>`-side.**
  Any logic that keys on whether content starts with `\n` after
  the open `>` is reading the wrong end. The rule is about how
  the close tag terminates the recursive parse: column-0 fresh
  line → Para; butted or indented → Plain.
- **Demotion applies to the LAST block only.** Earlier blocks in
  a multi-block div keep their Para shape. The old logic gated on
  `blocks.len() == 1` and silently no-op'd for multi-block cases,
  hiding the bug.
- **`expected.native` whitespace differs from naive panache
  output, but the harness `normalize_native` collapses it** — when
  comparing structurally-equivalent outputs side-by-side via
  `tr -d '\n'`, you'll see noise from "Str \"x\" , Y" vs "Str
  \"x\", Y" formatting that doesn't reflect a real divergence. Use
  the harness (or look at the report) for ground truth, not raw
  string comparison.
- **`html_block`, `html_div_block`, and `emit_html_block` all
  strip trailing newlines from `content` before calling
  `try_div_html_block`** (`while content.ends_with('\n') { pop()
  }`). That means inside `try_div_html_block`, `</div>` is at the
  very end of `content`. The byte at `close_start - 1` is the
  byte immediately preceding `</div>` — which is the signal we
  need. Don't add your own trailing-newline strip on top.

--------------------------------------------------------------------------------

## Earlier session — 2026-05-09 (Phase 1 — multi-line `<div>` open-tag structural HTML_ATTRS lift)

**html (block + inline) pass count: 75 → 76** (1 new corpus case —
passing).
**Workspace test count: 0 failing → 0 failing** (all green).
**Total pandoc conformance: 267/267 → 268/268 (100.0% → 100.0%)**.

### What landed

Parser-side fix to expose the attribute region of a multi-line
`<div\n  attrs\n>` open tag as a structural `HTML_ATTRS` node (one
per attribute line). Until now, the parser's
`emit_div_open_tag_tokens` only handled single-line open tags; if
the open `>` wasn't on the first line the entire post-`<div`
content fell through into raw `HTML_BLOCK_CONTENT` TEXT, so the
salsa anchor walk (which keys on `AttributeNode::cast` over
`HTML_ATTRS`) missed the `id` and `undefined-anchor` lint fired
even when the id existed.

Three changes in `crates/panache-parser/src/parser/blocks/html_blocks.rs`:

1. **`find_multiline_div_open_end`** — scans `lines[start_pos..]`
   for the first unquoted `>` past the `<div` literal, threading
   quote state across newlines. Returns `None` for single-line
   opens (existing path keeps owning them) or when `>` is missing
   entirely.
2. **`emit_multiline_div_open_tag`** — emits per-line tokens:
   - Line 0: `WHITESPACE?` (indent) + `TEXT("<div")` + (`WHITESPACE`
     + `HTML_ATTRS{TEXT}`)? + `NEWLINE`.
   - Lines 1..N-1: `WHITESPACE?` (indent) + `HTML_ATTRS{TEXT}` +
     `NEWLINE`.
   - Line N (last): `WHITESPACE?` + `(HTML_ATTRS{TEXT} +
     WHITESPACE?)?` + `TEXT(">")` + `TEXT(trailing)?` + `NEWLINE`.
   Result: each attribute line gets its own structural `HTML_ATTRS`
   so the existing `AttributeNode` descendants walk picks up the
   `id` from whichever line declares it.
3. **`parse_html_block_with_wrapper`** — calls
   `find_multiline_div_open_end` for `HTML_BLOCK_DIV` wrappers
   (`bq_depth == 0`) and routes to `emit_multiline_div_open_tag`
   when a multi-line open is detected. Depth-aware close tracking
   now sums `count_tag_balance` across *all* open-tag lines (was
   line 0 only) so the depth counter starts correct for multi-line
   opens. `same_line_closed` is gated to single-line opens since
   the `>` of a multi-line open is by definition not on line 0.
   `current_pos` advances past the consumed open-tag lines.

CST losslessness verified by per-test byte-equality assertion.
Source bytes unchanged — only structural granularity within the
open `HTML_BLOCK_TAG` is finer.

### Files in committable diff

- `crates/panache-parser/src/parser/blocks/html_blocks.rs`
  — multi-line detection + emission + depth tracking; 2 new unit
  tests (`test_parse_div_block_multiline_open_close_separate_line_pandoc`,
  `test_parse_div_block_multiline_open_close_inline_pandoc`).
- 1 new corpus case under
  `crates/panache-parser/tests/fixtures/pandoc-conformance/corpus/`:
  `0268-html-block-div-multiline-open-tag-gt-on-own-line` —
  `<div\n  id=...\n  class=...\n>` (close `>` on its own line).
  Case 0262 already covered the inline-`>` form; 0268 pins the
  separate-line form. Both now expose attrs structurally as
  `HTML_ATTRS`.
- 1 new parser fixture:
  `crates/panache-parser/tests/fixtures/cases/html_block_div_multiline_open_pandoc/`
  with snapshot
  `golden_parser_cases__parser_cst_html_block_div_multiline_open_pandoc.snap`
  pinning the new per-line `HTML_ATTRS` shape.
- `crates/panache-parser/tests/golden_parser_cases.rs` (1 new case
  registration).
- `crates/panache-parser/tests/pandoc/allowlist.txt`
  — new section `# html-block (multi-line div open tag — ...)` with
  id 268.
- `crates/panache-parser/tests/pandoc/report.txt` +
  `docs/development/pandoc-report.json` (regenerated; 268/268
  passing, 100%).

No projector, salsa, or formatter logic changes — pure parser
shape fix. The salsa indexer's existing `AttributeNode::cast` walk
picks up the new `HTML_ATTRS` nodes for free.

### Why parser-side (not projector-side)

The projector already handled multi-line opens correctly via its
recursive byte reparse — `parse --to pandoc-ast` was returning the
right `Div ("x", ["y"], [])` shape even before this fix. But the
salsa anchor index walks the structural CST, not the projection,
so `<div\n  id="anchor">...` produced false-positive
`undefined-anchor` lint diagnostics. Fixing it in the projector
wouldn't help salsa; the structural CST was the real gap.

Verified end-to-end:

```
printf '<div\n  id="anchor-x"\n  class="y">\n\nC.\n\n</div>\n\nSee [link](#anchor-x).\n' \
  > /tmp/t.md
panache lint /tmp/t.md
# before fix: warning: [undefined-anchor] Anchor '#anchor-x' not found
# after fix:  No issues found in 1 file(s)
```

### What's still NOT covered

- **Multi-line open with content trailing the `>`** — e.g.
  `<div\n  id="x">trailing\n</div>`. The trailing-content path
  works projection-wise, but pandoc emits `Para` while panache
  emits `Plain` for the `trailing\n` chunk. Out of scope here;
  this is a Para/Plain promotion gap in the existing recursive
  reparse (`flush_html_block_text` heuristics in `pandoc_ast.rs`).
  No corpus case yet.
- **Multi-line open inside a blockquote** (`bq_depth > 0`). The
  multi-line detection is gated on `bq_depth == 0` because
  `find_multiline_div_open_end` doesn't strip blockquote markers
  per-line; falling back to single-line emission keeps existing
  blockquote behavior unchanged. No corpus case.
- **`<span>` multi-line open** — same gap on the inline side. Phase
  2 handles `<span>`; the same logic (cross-line attribute lift)
  applies but inline-html-tags can't span newlines today (only
  single-line `<span ...>` is recognized in
  `inline_html.rs::try_parse_inline_html`). Edge case; defer until
  evidence.

### Suggested next sub-targets, ranked

1. **Phase 5 — Para/Plain promotion for trailing content after the
   open `>` of a multi-line div.** When `<div ...>trailing\nbody\n
   </div>` lands the inner reparse, pandoc emits `Para` but panache
   emits `Plain`. Affects single-line opens too when the content
   isn't blank-separated. Look at `flush_html_block_text` / the
   recursive reparse promotion logic in `pandoc_ast.rs`.
2. **Cross-boundary cite numbering** (still deferred — no corpus
   exercises it). Pass outer's terminal cite counter as the
   starting offset to the inner pre-pass.
3. **Outer-wins-on-conflict for inherited refs/footnotes** (still
   deferred — no corpus exercises it).
4. **Projector cleanup.** Audit whether the byte-aware close
   lookahead in `try_div_html_block` is still load-bearing now
   that parser-side multi-line + nested + balanced opens all lift
   structurally. Likely some paths can simplify.

### Don't redo / known traps (new this session)

- **`input.lines()` strips newlines; the parser uses
  `split_lines_inclusive`.** When writing parser unit tests that
  assert byte-equal losslessness, use
  `crate::parser::utils::helpers::split_lines_inclusive` to build
  the `lines: Vec<&str>` input — `input.lines()` returns lines
  with trailing newlines stripped, which silently breaks
  losslessness checks. The existing `test_parse_div_block_*` tests
  used `input.lines()` and got away with it because they only
  asserted `new_pos`, not byte-equality.
- **Quote state must thread across line boundaries.** The
  `find_multiline_div_open_end` scanner explicitly preserves
  `quote: Option<u8>` across the line transition so `<div\n
  data-x="multi\nline"\n>` doesn't terminate at the inner `>` on
  the second line. Don't reset quote state per line.
- **`emit_div_open_tag_tokens` (single-line) and
  `emit_multiline_div_open_tag` (multi-line) are both load-bearing.**
  The dispatch happens in `parse_html_block_with_wrapper` based on
  `find_multiline_div_open_end`'s return value. Don't try to
  unify them — single-line has trailing-after-`>` content (e.g.
  `<div>foo</div>`) that's structurally part of the same `HTML_BLOCK_TAG`,
  and the depth-aware `same_line_closed` check fires for it.
  Multi-line never has the close on line 0 by definition.
- **Salsa is downstream of CST, not projection.** A projection-only
  fix that produces correct pandoc-native output is invisible to
  the linter. If the linter (or LSP, or any salsa consumer) shows
  stale diagnostics after a CST change, check for the
  `~/.cache/panache/` disk cache — the CLI keys on a tool
  fingerprint that doesn't invalidate on code changes. `rm -rf
  ~/.cache/panache/` before CLI verification.
- **Multi-line `HTML_ATTRS` per attribute line is the right
  shape.** Not one big `HTML_ATTRS` spanning all attribute lines —
  newlines/indentation aren't attribute bytes. The
  `AttributeNode::cast` walk visits each `HTML_ATTRS` separately;
  `parse_html_attribute_list` parses each line's bytes; whichever
  line declares `id` registers it. This avoids the awkward case of
  a synthesized `HTML_ATTRS` containing structural newlines.

--------------------------------------------------------------------------------

## Earlier session — 2026-05-09 (Phase 5 — cross-boundary RefsCtx inheritance for outer→inner refs/footnotes/heading-slugs)

**html (block + inline) pass count: 72 → 75** (3 new corpus cases —
all passing).
**Workspace test count: 0 failing → 0 failing** (all green).
**Total pandoc conformance: 264/264 → 267/267 (100.0% → 100.0%)**.

### What landed

Projector-only fix in `pandoc_ast.rs` so the recursive `<div>` reparse
inherits outer-document refs/footnotes/heading-slug history into the
inner `RefsCtx`. Previous session (264 cases) gave the inner reparse
its own ctx so inner-defined refs/footnotes/auto-ids worked, but
*cross-boundary* uses still failed: a `[label]: url` def outside a
`<div>` couldn't be referenced inside, and an outer `# Section` +
inner `# Section` produced two `("section", …)` instead of pandoc's
`("section", …)` + `("section-1", …)`.

Three changes:

1. **`build_refs_ctx_inherited(tree, parent: Option<&RefsCtx>)`** —
   new variant. Seeds `seen_ids` from `parent.heading_ids` (reverse-
   engineering counts from final ids: `base` → count >= 1, `base-N` →
   count >= N+1, max per base). After the inner pre-pass, folds
   parent `refs` / `footnotes` / `heading_ids` into the inner ctx via
   `or_insert` (inner-defined keys win on conflict; matches scoping
   semantics, not pandoc's true document-order rule, but no current
   corpus exercises an outer-loses-to-inner-on-shared-key case).
2. **`parse_pandoc_blocks`** — calls `build_refs_ctx_inherited(&doc,
   Some(&outer))` instead of plain `build_refs_ctx(&doc)`. The `outer`
   `RefsCtx` was already saved via `mem::take` and restored at end,
   so inheritance just means reading from `outer` while it's parked.
3. **`render_unresolved_reference_inline`** — added a `lookup_ref`
   step before the unresolved fallback. Required because the inner
   parser produces `UNRESOLVED_REFERENCE` (no def visible in the
   inner CST), and the inherited `REFS_CTX.refs` only helps if the
   projector actually queries it. Symmetric for image-shape
   (`![label]` produces `Image`, link-shape produces `Link`).

`Block`, `Inline`, `TableData`, `GridCell`, `Citation` gained
`#[derive(Clone)]` so footnote bodies (`Vec<Block>`) can be inherited
by value across the boundary. No existing call site relied on these
not being `Clone`.

### Files in committable diff

- `crates/panache-parser/src/pandoc_ast.rs`
  — `build_refs_ctx` becomes a thin wrapper over new
  `build_refs_ctx_inherited`; `parse_pandoc_blocks` calls the latter
  with `Some(&outer)`; `render_unresolved_reference_inline` adds a
  `lookup_ref` resolution step; AST types gain `Clone`. Net ~50
  lines added.
- 3 new corpus directories under
  `crates/panache-parser/tests/fixtures/pandoc-conformance/corpus/`:
  - `0265-html-block-div-inherits-outer-ref-link` — outer
    `[example]: url` def used inside `<div>` resolves to Link.
  - `0266-html-block-div-inherits-outer-footnote` — outer
    `[^x]: ...` def referenced inside `<div>` produces Note.
  - `0267-html-block-div-heading-slug-disambiguation` — outer
    `# Section` + inner `# Section` slugs to `section` /
    `section-1`.
- `crates/panache-parser/tests/pandoc/allowlist.txt`
  — new section `# html-block (div recursive parse — outer
  ref-link defs, footnote defs, and heading-slug history inherited
  …)` with ids 265..267.
- `crates/panache-parser/tests/pandoc/report.txt` +
  `docs/development/pandoc-report.json` (regenerated; 267/267
  passing, 100%).

No parser, salsa, or formatter logic changes — pure projector +
corpus.

### Why projector-only (still)

Same reasoning as the previous session — CST shape is unchanged,
inner content remains raw TEXT, and the bug is in *how* the
projector reparses + resolves. Parser-side restructuring (parsing
inner content into structural blocks at parse time) remains a Phase
5 target but isn't needed for these gaps; inheritance threading is
strictly a `RefsCtx`-construction + projection-resolution issue.

### What's still NOT covered

- **Outer-wins-over-inner ref-conflict.** Pandoc's actual rule is
  "first def in document order wins". With inner-wins-on-conflict we
  diverge if both halves define the same label. Not exercised by
  current corpus. Fix requires offset-aware merging when inheriting.
- **Multi-line open tags** (`<div\n  id="x">\n…</div>` where the
  open tag's `>` is on a separate line). Still falls back to opaque
  `HTML_BLOCK`. Edge case.
- **Cross-boundary cite numbering.** `cite_note_num_by_offset` is
  built per-CST and not inherited — an inline `Cite` group inside a
  `<div>` would get `noteNum=1` regardless of how many cites/notes
  preceded it outside. Fixable by also folding outer's
  `cite_note_num_by_offset` snapshot, but the offset spaces are
  disjoint between outer/inner CSTs so the inherited entries never
  match anyway. Real fix would re-number inner cites starting from
  outer's terminal counter; corpus doesn't exercise this.

### Suggested next sub-targets, ranked

1. **Bring outer-wins-on-conflict to ref/footnote inheritance.**
   Currently inner-defined keys win. To match pandoc fully, track
   the byte-offset of each def relative to the document and prefer
   the earlier one. Add 1-2 corpus cases (outer-then-inner same
   label vs inner-then-outer) and tighten the merge in
   `build_refs_ctx_inherited`. Low ROI unless a real document
   actually does this — defer until evidence.
2. **Cross-boundary cite numbering.** Pass outer's terminal cite
   counter as a starting offset to the inner pre-pass so cites
   inside `<div>` continue rather than restart. Will need the
   `collect_cite_note_nums` signature to accept a starting counter
   (currently hardcoded to 0 at line 164).
3. **Multi-line open tags.** Still falls back to opaque
   `HTML_BLOCK` when `<div\n  attrs>` spans real lines without the
   closing `>` on line 1. The `try_parse_html_block_start` only
   inspects line 1; teach it to continue scanning until the open
   tag closes. Edge case; probably low ROI until a real document
   hits it.
4. **Projector cleanup.** Now that recursive reparse correctly
   inherits the outer ctx, the legacy `try_div_html_block` byte-
   level re-tokenizer's role overlaps with parser-side
   `HTML_BLOCK_DIV` lift. Audit whether the byte-aware close
   lookahead in mid-block scenarios is still needed.

### Don't redo / known traps (new this session)

- **`UNRESOLVED_REFERENCE` is what the inner parser emits** —
  not `LINK_REFERENCE`. The parser resolves refs *during parsing*
  by checking the same CST's reference defs; when there are none in
  the same CST (because they're in the outer), it emits the
  unresolved variant. So inheritance has TWO sides: (a) the inner
  ctx must hold the outer's refs (handled in
  `build_refs_ctx_inherited`), AND (b) the projector must
  re-resolve at projection time when it sees
  `UNRESOLVED_REFERENCE` (handled in
  `render_unresolved_reference_inline`). Forgetting (b) means the
  inherited refs are dead weight — the projector falls back to
  emitting raw `[label]` bytes.
- **`Block`, `Inline`, etc. were `Debug`-only by design** until
  this session. They're projection-only types that previously never
  needed cloning. Adding `Clone` to all of them was straightforward
  (no non-cloneable fields), but if you add a future variant with
  a non-`Clone` payload, you'll need to keep the `Clone` cascade
  intact or switch to `Rc<...>` for the shared field.
- **Heading-id reverse-engineering from `heading_ids` set is
  best-effort.** It can't distinguish "outer has only `section-1`
  (an explicit id from `# Section {#section-1}`)" from "outer had
  two `# Section`s and disambiguated to `section-1`". In the first
  case the inner `# Section` should slug to `section` (count starts
  at 0 for `section`); in the second it should slug to `section-2`.
  Current logic conflates them, picking `section-2`. Affects only
  pathological mixes of explicit + auto ids; no corpus case
  exercises it.
- **`lookup_ref` is keyed on `normalize_ref_label`** (case-fold +
  whitespace-collapse). The unresolved-resolution branch passes the
  raw `label` text (which is what `text_node.text()` returns), and
  `lookup_ref` re-normalizes internally. Don't pre-normalize the
  label at the call site — would double-normalize.

--------------------------------------------------------------------------------

## Earlier session — 2026-05-09 (Phase 5 — inner-`RefsCtx` for `parse_pandoc_blocks` recursive reparse)

**html (block + inline) pass count: 62 → 72** (10 new corpus cases —
all passing).
**Workspace test count: 0 failing → 0 failing** (all green).
**Total pandoc conformance: 254/254 → 264/264 (100.0% → 100.0%)**.

### What landed

Projector-only fix in `parse_pandoc_blocks`
(`crates/panache-parser/src/pandoc_ast.rs:1365`): the recursive
reparse of `<div>...</div>` inner content now builds a fresh
`RefsCtx` from the inner CST (via existing `build_refs_ctx`), swaps
it in for the duration of the inner projection, and restores the
outer `REFS_CTX` after. Three failure modes collapse to one root
cause and unlock together:

- **Heading auto-ids** inside `<div>` were empty
  (`Header 1 ("", [], [])` instead of `("heading", [], [])`)
  because `heading_id_by_offset` is keyed on the *outer* CST's text
  ranges; the recursive parse produces fresh zero-based offsets that
  never match.
- **Reference-link defs** inside `<div>` were swallowed by the
  inner CST but never resolved (`See [example].` stayed as raw
  text), because `REFS_CTX.refs` was the outer's empty map.
- **Footnote references** inside `<div>` (e.g. `text.[^x]`) failed
  the same way — `REFS_CTX.footnotes` lookup missed.

Cross-boundary id disambiguation (outer `# heading` + inner
`# heading` should slug to `heading-1`) is an acceptable gap: pandoc
parses `<div>...</div>` natively in one pass with a document-wide
seen-ids map, but our recursive boundary is isolated. No corpus case
exercises this; flagging it for a future session if a real document
hits it.

### Files in committable diff

- `crates/panache-parser/src/pandoc_ast.rs`
  — `parse_pandoc_blocks` swaps in inner `RefsCtx`. Net ~10 lines
  added (mem::take + build_refs_ctx + restore).
- 10 new corpus directories under
  `crates/panache-parser/tests/fixtures/pandoc-conformance/corpus/`:
  - `0255-html-block-div-multi-paragraph` — `Para` then `Para`
    inside `<div>`
  - `0256-html-block-div-with-heading` — `# Heading` and
    `## Subheading` get auto-ids inside `<div>`
  - `0257-html-block-div-with-ref-link` — `[example]: url` def
    + `[example]` use both inside `<div>` resolves
  - `0258-html-block-div-with-footnote` — `[^x]` ref + `[^x]: ...`
    def both inside `<div>` produces `Note`
  - `0259-html-block-div-with-blockquote-and-code` — `<div>` with
    paragraph, blockquote, fenced code block
  - `0260-html-block-div-with-bullet-and-ordered-list` — both
    list types inside `<div>`
  - `0261-html-block-div-with-pipe-table` — `Table` inside `<div>`
  - `0262-html-block-div-multiline-open-tag` —
    `<div\n  id=\"x\"\n  class=\"y\">` first-line-only scan still
    matches when bytes following the close `>` start with `\n`
  - `0263-html-block-div-with-link-and-image` — Link + auto-figure
    inside `<div>`
  - `0264-html-block-div-with-attrs-keyval` — `<div data-key="value"
    id="x" class="a b">` projects `( "x" , [ "a", "b" ] , [ ( "data-key" , "value" ) ] )`
- `crates/panache-parser/tests/pandoc/allowlist.txt`
  — new section `# html-block (div recursive parse — inner heading
  auto-ids, ref-link defs, footnote defs resolve in inner RefsCtx
  instead of leaking outer)` with ids 255..264.
- `crates/panache-parser/tests/pandoc/report.txt` +
  `docs/development/pandoc-report.json` (regenerated; 264/264
  passing, 100%).

No parser, salsa, or formatter logic changes — pure projector +
corpus.

### Why projector-only (not parser-side)

CST shape is unchanged: `<div>` content is still raw TEXT inside
`HTML_BLOCK_CONTENT`. The fix is purely in how the projector
*reparses* that text. Parser-side restructuring (parsing inner
content into structural blocks at parse time) remains a Phase 5
target but wasn't needed for these gaps — the recursive reparse
strategy already produces the right shape; it just needed the right
ref/heading context.

### What's still NOT covered

- **Cross-boundary id disambiguation** — outer + inner heading
  with identical slug both project as `("heading", [], [])` instead
  of pandoc's `("heading", ...)` + `("heading-1", ...)`. To match
  pandoc fully we'd need to thread the outer `seen_ids` into the
  inner `RefsCtx` build, *and* feed the outer `heading_ids` set so
  inner duplicates pick up `-N` suffixes. Fixable in
  `build_refs_ctx` accepting an optional inherited seen-ids map.
  Not exercised by current corpus.
- **Cross-boundary ref-link inheritance** — an outer
  `[outer]: http://...` defined before the `<div>` is not visible
  to a `[outer]` use inside the `<div>` (inner RefsCtx wipes outer
  refs). Pandoc's one-pass parse sees both. Same fix shape as
  above; same lack of corpus coverage. Probably worth a corpus
  case + targeted refs-merge in a future session.
- **Cross-boundary footnote-def inheritance** — same story.
  Pandoc lets a `<div>` body use a footnote defined outside.

### Suggested next sub-targets, ranked

1. **Cross-boundary inheritance for refs/footnotes/seen-ids.**
   Thread the outer `RefsCtx`'s `refs`, `footnotes`, and
   `heading_ids`/`seen_ids` into the inner `build_refs_ctx` so
   recursive reparse matches pandoc on:
   - Outer `[label]: url` used inside `<div>`.
   - Outer `[^x]: ...` referenced inside `<div>`.
   - Outer + inner heading slug collisions disambiguating to
     `-1`/`-2`.
   Add 3 corpus cases (one per axis) and tighten `build_refs_ctx`
   to accept an inherited-context arg.
2. **Multi-line open tags.** Still falls back to opaque
   `HTML_BLOCK` when `<div\n  attrs>` spans real lines without the
   closing `>` on line 1 (case 0262 works because the close `>` is
   on the same line as the last attribute). The
   `try_parse_html_block_start` only inspects line 1; teach it to
   continue scanning until the open tag closes. Edge case;
   probably low ROI until a real document hits it.
3. **Projector cleanup.** With case 199 lifting structurally and
   inner-ctx now correctly seeded, `try_div_html_block`'s legacy
   byte-aware close lookahead may be safe to simplify. Audit
   carefully — Phase 5/6 added it for mid-block bare `<div>`
   handling.
4. **`<!ENTITY x "y">` Quoted projection gap** still present
   (smart-quote / Quoted feature; out-of-scope for this skill).

### Don't redo / known traps (new this session)

- **`parse_pandoc_blocks` is called from at least two sites** —
  `flush_html_block_text` (mid-stream non-tag content within a
  block-tag splitter) and `try_div_html_block` (whole-`<div>`
  recursive lift). Both want the inner-ctx semantics, so the swap
  belongs in `parse_pandoc_blocks` itself, not at the call sites.
- **`build_refs_ctx` mutates `REFS_CTX` mid-build** (it stages
  cite-num/example-num maps into the thread-local before the heading
  pre-pass so footnote bodies can see them — see comment at line
  139). When swapping, save the outer FIRST (`mem::take`), THEN
  call `build_refs_ctx`, THEN install the result. Skipping the
  `mem::take` and just running `build_refs_ctx` would let the inner
  pre-pass leak into the outer ctx mid-projection.
- **`fenced_div` does NOT use `parse_pandoc_blocks`.** It walks the
  existing structural CST via `collect_block`, so fenced-div
  headings/refs already resolve through the *outer* ctx. The fix
  here only affects `<div>...</div>` HTML blocks (whose inner is
  raw TEXT) and any other future caller that reparses arbitrary
  text. Don't accidentally generalize to fenced divs — would
  double-build the ctx for no gain.
- **`heading_id_by_offset` is offset-keyed, NOT slug-keyed.** The
  outer-ctx-leak symptom looks like "auto-id missing" but the
  root cause is that the inner CST's offsets are zero-based and
  don't intersect with the outer's offset space. Tempting wrong
  fix: copy the outer's `heading_ids` into the inner. That doesn't
  resolve auto-ids — the inner heading needs its OWN slug computed
  and registered. The right fix is to *build* an inner ctx, not to
  share the outer's lookup tables.
- **Conformance comparison is whitespace-insensitive** —
  `normalize_native` collapses pandoc's pretty-printed multi-line
  block output to single-line. So differences between
  `[ Para [Str "x"] , Para [Str "y"] ]` (panache one-line) and
  pandoc's vertically-stacked output are not real divergences.
  Don't be misled by visual diff when probing.

--------------------------------------------------------------------------------

## Earlier session — 2026-05-08 (Phase 5 — depth-aware nested `<div>` close scan)

**html (block + inline) pass count: 57 → 62** (5 new corpus cases —
1 unblocked + 4 new — all passing).
**Workspace test count: 0 failing → 0 failing** (all green).
**Total pandoc conformance: 249/250 → 254/254 (99.6% → 100.0%)**.
**`blocked.txt`: 1 → 0 entries** (case 199 unblocked).

### What landed

Parser-side depth-aware close scan for non-verbatim HTML block tags
under `Dialect::Pandoc`. Mirrors pandoc's `htmlInBalanced`: balanced
opens/closes of the same tag name, block ends only when depth
returns to 0. CommonMark verbatim path (CM §4.6 type-1) keeps its
"first close wins" semantics.

`HtmlBlockType::BlockTag` gained a `depth_aware: bool` field, set
to `!is_commonmark` at construction time in
`try_parse_html_block_start`. New helper
`count_tag_balance(line, tag_name) -> (opens, closes)` walks
`<...>` brackets, respects quoted attribute values, and skips
self-closing forms. `parse_html_block_with_wrapper` consults the
field: when set, threads a depth counter across lines and closes
when `depth <= 0`; otherwise falls back to the existing
`is_closing_marker` substring path.

End-to-end: case 199 `<div id="outer">…<div id="inner">…</div>…</div>`
now projects to nested `Div("outer", …) [Div("inner", …) [Para …]]`
matching pandoc-native exactly. Inner `<div>...</div>` stays as raw
TEXT inside `HTML_BLOCK_CONTENT`; the projector's existing
`try_div_html_block` recursive reparse via `parse_pandoc_blocks`
handles the inner div correctly because the parser fix fires for
each level.

### What's covered + what's NOT

Covered (with corpus cases):
- 199 (already in corpus, was blocked) — 2-deep nested
- 251 — 3-deep nested with attributes
- 252 — sibling inner divs (`<div>a</div>` then `<div>b</div>` inside outer)
- 253 — `<div>` containing a `<table>` (verifies only `<div>` counts toward depth)
- 254 — outer with id, inner with class

Not covered (still gaps):
- **Multi-line open tags.** `<div\n  id="x">` still falls back to
  opaque `HTML_BLOCK` because `try_parse_html_block_start` only
  inspects the first line. Edge case.
- **Pandoc-dialect verbatim tags with internal `<script>` strings.**
  Now depth-aware too (since `depth_aware: !is_commonmark` applies
  uniformly). For pathological cases like
  `<script>let x = '<script>';</script><script>...</script>` this
  may over-extend. Pandoc itself uses `htmlInBalanced` so behavior
  matches. Not exercised by corpus.
- **Mismatched depth (`</div>` without matching open).** When a
  content line has more closes than opens, depth goes negative and
  we close on that line. Acceptable — the orphan close text stays
  in the closing `HTML_BLOCK_TAG`. No corpus case yet.

### Why parser-side (not projector-side)

Earlier projector-only sessions added byte-aware splitters that
already handled depth correctly inside `try_div_html_block` /
`find_matching_html_close`. But that only fires when the parser
produces a single `HTML_BLOCK[_DIV]` containing the whole construct.
With nested divs the parser was *closing too early*, so the inner
`</div>` and outer `</div>` ended up in different CST nodes — the
projector never saw the whole construct as one byte range, and the
trailing `</div>` got reparsed as a paragraph with `RawInline`.

Fix had to be in the parser. Once the parser keeps the whole
balanced range as one `HTML_BLOCK_DIV`, the projector's existing
recursive reparse (`parse_pandoc_blocks`) correctly handles each
level.

### Files in committable diff

- `crates/panache-parser/src/parser/blocks/html_blocks.rs`
  — `HtmlBlockType::BlockTag.depth_aware` field; new
  `count_tag_balance` helper; depth-aware threading in
  `parse_html_block_with_wrapper`; 3 new unit tests
  (`test_parse_div_block_nested_pandoc`,
  `test_parse_div_block_same_line_pandoc`,
  `test_commonmark_verbatim_first_close`); existing tests updated
  to include the new field.
- `crates/panache-parser/tests/pandoc/blocked.txt`
  — emptied (was: 199 only).
- `crates/panache-parser/tests/pandoc/allowlist.txt`
  — 199 added under `# html-block`; new section
  `# html-block (nested div — depth-aware close scan, mirrors pandoc's htmlInBalanced)`
  with ids 251..254.
- 4 new corpus directories under
  `crates/panache-parser/tests/fixtures/pandoc-conformance/corpus/`:
  - `0251-html-block-div-three-deep`
  - `0252-html-block-div-sibling-inner`
  - `0253-html-block-div-with-table`
  - `0254-html-block-div-inner-class`
- 2 paired parser fixtures + snapshots:
  - `crates/panache-parser/tests/fixtures/cases/html_block_div_nested_pandoc/`
  - `crates/panache-parser/tests/fixtures/cases/html_block_div_nested_commonmark/`
  (the CommonMark fixture pins the unchanged blank-line-terminated
  type-6 behavior — separate `HTML_BLOCK`s for each line.)
- 1 formatter golden:
  `tests/fixtures/cases/html_block_div_nested_idempotent/`
  pinning `<div>` round-trip (parsed → formatted → parsed →
  formatted is byte-identical).
- `crates/panache-parser/tests/golden_parser_cases.rs` (2 new
  case registrations).
- `tests/golden_cases.rs` (1 new case registration).
- `crates/panache-parser/tests/pandoc/report.txt` +
  `docs/development/pandoc-report.json` (regenerated;
  254/254 passing, 100%).

No projector, salsa, or formatter logic changes — pure parser
correctness fix + corpus.

### Suggested next sub-targets, ranked

1. **Bump the corpus into "harder" markdown_in_html_blocks
   territory.** Now that conformance is 100% on the curated corpus,
   the corpus itself is the limiter. Cases worth adding:
   - Multi-paragraph content inside `<div>` where blank-line
     handling matters (`<div>\n\nfoo\n\nbar\n\n</div>`) — verify
     Para vs Plain promotion across multiple inner blocks.
   - `<div>` with content that itself contains a `<table>` /
     `<dl>` / `<ul>` (markdown_in_html_blocks for non-sectioning
     tags inside a div).
   - Mixed inline + block raw HTML inside a div (e.g. `<em>foo</em>`
     adjacent to `<table>...</table>`).
   - Multi-line open tags (`<div\n  id="x">\n...</div>`) — still
     falls back to opaque `HTML_BLOCK`; would need
     `try_parse_html_block_start` to span lines.
2. **`<!ENTITY x "y">` Quoted projection gap** still present
   (smart-quote / Quoted feature; out-of-scope for html-conformance).
3. **Projector cleanup.** With case 199 now lifting structurally,
   the projector's `find_matching_html_close` and its byte-aware
   `<div>` lookahead could potentially be simplified — but verify
   first: the Phase 5/6 session that added them targeted bare
   `<div>` opens in mid-block (not at the start). Removing them
   could regress those paths. Audit before pulling.

### Don't redo / known traps (new this session)

- **The block dispatcher decides the wrapper kind** in
  `block_dispatcher.rs::parse_prepared` based on tag_name being
  "div" + Pandoc dialect + `native_divs`. The parser's
  `parse_html_block_with_wrapper` doesn't see the wrapper choice
  until it's passed in; depth-aware tracking lives in the parser
  fn and uses the **block_type's tag_name**, not the wrapper kind.
  Keep them in sync: don't add a new tag-name branch in the
  dispatcher without also confirming `count_tag_balance` is
  invoked for it (today it is — depth_aware fires for all
  BlockTag variants under Pandoc).
- **CommonMark verbatim must keep first-close semantics.** CM §4.6
  type-1 says the block ends with the line containing the
  corresponding end tag — not depth-aware. Setting `depth_aware:
  !is_commonmark` honors this; verbatim under Pandoc DOES get
  depth tracking, which is what pandoc itself does
  (`htmlInBalanced`). New unit test
  `test_commonmark_verbatim_first_close` pins the non-depth-aware
  behavior.
- **Self-closing `<tag/>` doesn't bump depth.** The
  `count_tag_balance` helper checks `bytes[j-1] == b'/'` at the
  closing `>` to detect `/>`. If you tweak the helper, keep this
  check — without it `<div/>` would erroneously be counted as a
  net-zero open+close, breaking depth bookkeeping.
- **Quoted attribute values can hide `<` and `>`.** The helper
  tracks quote state inside tag brackets so `<div attr="<inner>">`
  doesn't count `<inner>` as a tag. The probe scans `"`/`'` and
  matches the same quote to close. Don't loosen this — pandoc's
  parser does the same.
- **`HtmlBlockType::BlockTag` is `Box<dyn Any>`-roundtripped via
  the block dispatcher.** The dispatcher stores the detected type
  in a `Box<dyn Any>` payload then downcasts in `parse_prepared`.
  Adding a new field to `BlockTag` requires the existing
  `Clone`/`PartialEq`/`Eq`/`Debug` derives to extend automatically;
  no manual Any/clone work needed. Just remember to update **all**
  test sites that build a `BlockTag` literal — `cargo check` will
  point them out via E0063.
- **Don't be confused by the `199` blocked.txt comment** — it said
  "needs depth-tracking pre-scan in
  `parser/blocks/html_blocks.rs`". The fix here is more
  surgical than a full pre-scan: depth tracking is interleaved
  with the existing line-walking close-marker check, not a
  separate pre-pass. Net code change is ~70 lines added.

--------------------------------------------------------------------------------

## Earlier session — 2026-05-08 (Phase 5/6 — projector-level `markdown_in_html_blocks` for non-sectioning block tags)

**html (block + inline) pass count: 47 → 57** (10 new corpus cases,
all passing).
**Workspace test count: 0 failing → 0 failing** (all green).
**Total pandoc conformance: 239/240 → 249/250** (99.6% → 99.6%).

### What landed

Projector-only fix to make the public `panache_parser::to_pandoc_ast`
API split HTML blocks the way pandoc-markdown does under
`markdown_in_html_blocks` (default-on). No parser changes — the CST
keeps a single opaque `HTML_BLOCK` per balanced tag pair; the
projector now walks the bytes and emits per-tag `RawBlock`s plus
markdown-parsed `Plain`/`Para` for non-tag content between them.

`emit_html_block` (`crates/panache-parser/src/pandoc_ast.rs`)
replaces its line-based splitter with byte-aware tag scanning:
- Walks `<` positions, tries `parse_open_tag` / `parse_close_tag`,
  filters via the new `html_blocks::is_html_block_tag_name` so
  inline-only tags (`<em>`, `<a>`, `<input>`, `<br>`, …) pass
  through into the surrounding `Plain` content as RawInline.
- Each block-level open/close tag emits its own `RawBlock`.
- A `<div>...</div>` encountered in mid-stream gets a depth-aware
  lookahead via the new `find_matching_html_close`. When balanced,
  the chunk goes through the existing `try_div_html_block` lift
  → `Block::Div` (matches pandoc's `native_divs` recursing inside
  e.g. `<aside>` / `<table>` content).
- Inter-tag text is reparsed via the existing `parse_pandoc_blocks`
  helper. The new `flush_html_block_text` promotes a trailing
  `Para` to `Plain` when no blank line separates it from the next
  tag — matches pandoc's pattern of `Para` for blank-terminated
  chunks vs `Plain` butted up against a closing tag.
- Verbatim openers (`<!--`, `<?`, `<![CDATA[`, `<!`, raw-text
  elements) keep the early-return as a single `RawBlock` — no
  splitting inside.

`is_complete_html_tag_line` (the old line-based splitter helper)
is gone; nothing else uses it.

`is_html_block_tag_name(name)` is a new `pub fn` on
`crates/panache-parser/src/parser/blocks/html_blocks.rs`. Exposes
the module's existing `BLOCK_TAGS` list to the projector.

### Why projector-only (and not parser-level structural change)

The skill RECAP suggested a parser-level fix that would split
HTML-block scanning so each balanced tag pair emits a separate
`HTML_BLOCK`. That's much more invasive: it changes the CST shape
(many existing snapshots), the LSP folding ranges, and the
formatter's HTML_BLOCK walking.

Projector-only stays byte-equal in the CST (single `HTML_BLOCK`
per balanced outer tag pair) and just reads the bytes more
carefully on the way to pandoc-native. The conformance harness is
the consumer that cares; LSP/formatter/salsa keep working unchanged.
A future parser-level lift could come later if there's a concrete
LSP/formatter need — but this session deliberately scoped to the
smallest fix that unlocks ~10 conformance cases.

### What this fix does NOT do

- **Nested `<div>` (case 199) is still failing.** Root cause is
  parser-side: the HTML-block scanner closes the outer `<div>` at
  the FIRST `</div>` it sees, regardless of depth. The projector's
  new `find_matching_html_close` is depth-aware but it can't fix
  what the CST already mis-shaped. Phase 5 still pending; needs
  depth-aware pre-scan in `parser/blocks/html_blocks.rs`.
- **Multi-paragraph inter-tag content with mixed blank-line
  patterns is partial.** The `Para → Plain` promotion fires only
  on the LAST block in a chunk. So `<td>foo\n\nbar</td>` →
  `[Para foo, Plain bar, </td>]` (correct). But edge cases with
  more complex blank-line patterns may still drift; covered cases
  in 0250 plus the ones above are passing.
- **Top-level `<table>` opener with NO closing `</table>` in the
  document.** The parser keeps it as an unclosed HTML block
  (`found_closing = false` path); the projector sees the bytes and
  splits them, which is fine. Not a regression, but no corpus
  coverage yet.

### Files in committable diff

- `crates/panache-parser/src/parser/blocks/html_blocks.rs`
  — new `pub fn is_html_block_tag_name`.
- `crates/panache-parser/src/pandoc_ast.rs`
  — new `split_html_block_by_tags`, `flush_html_block_text`,
  `extract_html_tag_name`, `find_matching_html_close`,
  `trailing_newlines`. Removed `is_complete_html_tag_line`.
  Rewrote `emit_html_block` documentation. Net +~140 lines.
- `crates/panache-parser/tests/pandoc/allowlist.txt` — 10 new ids
  under new `# html-block (markdown_in_html_blocks — non-sectioning
  block tags …)` section header.
- 10 new corpus directories under
  `crates/panache-parser/tests/fixtures/pandoc-conformance/corpus/`:
  - `0241-html-block-table-cell-emphasis` — `<table>/<tr>/<td>*one*</td>` minimal
  - `0242-html-block-table-two-rows` — 2-row, 4-cell table
  - `0243-html-block-dl-dt-dd` — `<dl>` with one `<dt>` and one `<dd>` containing `*def*`
  - `0244-html-block-ul-li` — `<ul>` with two `<li>`s, one with emphasis
  - `0245-html-block-p-inline` — single-line `<p>some text</p>`
  - `0246-html-block-p-multiline` — multi-line `<p>foo\nbar</p>` with SoftBreak
  - `0247-html-block-aside-with-p` — `<aside>` containing `<p>*foo*</p>`
  - `0248-html-block-form-input-button` — `<form>` with `<input>` (inline-only) and `<button>` (also inline-only — pandoc treats `<button>` as inline tag)
  - `0249-html-block-aside-with-nested-div` — `<aside>` containing a balanced `<div id="x">*foo*</div>` (verifies depth-aware Div lift inside non-div outer)
  - `0250-html-block-table-with-paragraph` — multi-paragraph content inside `<td>` (Para+Para, then close tag)
- `crates/panache-parser/tests/pandoc/report.txt` +
  `docs/development/pandoc-report.json` (regenerated; pass rate
  239/240 → 249/250).

No parser, formatter, or salsa changes — pure projector + corpus.

### Suggested next sub-targets, ranked

1. **Phase 5 (nested `<div>`, blocked.txt id 199)** —
   parser-side depth-aware HTML-block pre-scan in
   `parser/blocks/html_blocks.rs::parse_html_block_with_wrapper`.
   The current scanner uses
   `is_closing_marker(line, &block_type)` which matches the FIRST
   `</tag>` regardless of depth. Fix: track `<tag>...` depth as
   we walk lines (counting opens/closes of the SAME tag name) and
   close only when depth returns to 0. This unlocks case 199 and
   makes panache safe for documents with nested `<div>` content.
   Will require updating the existing `try_parse_html_block_start`
   call sites and possibly the `HtmlBlockType::BlockTag` payload.
2. **Add corpus coverage for "outer block tag without
   markdown content" edge cases.** E.g. `<table>` with no
   content, mixed inline+block tags inside a table cell, etc.
   Most should pass with the new splitter; goal is corpus
   coverage so future regressions are caught.
3. **`<!ENTITY x "y">` Quoted projection gap** noted in earlier
   sessions: pandoc emits `Quoted DoubleQuote [Str "y"]` for the
   `"y"` part inside a declaration; panache emits `Str "\"y\">"`.
   Probably out-of-scope for html-conformance — it's a smart-quote
   / Quoted feature gap.
4. **Multi-paragraph inter-tag content edge cases.** The current
   `Para→Plain` promotion fires only on the last block. Some
   patterns (e.g. trailing blank-then-tag, content with embedded
   block constructs like lists/code) may need refinement. Probe
   before committing to this; might already be covered.

### Don't redo / known traps (new this session)

- **`find_matching_close` was already taken** in `pandoc_ast.rs`
  — it's the smart-quote bracket scanner that takes
  `(&[Inline], usize, char, &[bool])`. The new HTML helper is
  named `find_matching_html_close` to avoid collision. Don't try
  to "merge" the two; they operate on completely different inputs
  (Inline slice vs raw byte string). Stale rust-analyzer
  diagnostics may keep referencing the old name after the rename
  — `cargo check` is the truth.
- **`<button>` is an inline tag, not a block tag.** Even though
  semantically it forms a block in HTML, pandoc-markdown treats it
  as inline (it's not in pandoc's block-tags list, which we mirror
  via `BLOCK_TAGS`). So `<form><input><button>Submit</button></form>`
  emits the `<button>` as a `RawInline` inside the surrounding
  `Plain`, not as a separate `RawBlock`. Don't be tempted to add
  it to `BLOCK_TAGS` — pandoc disagrees.
- **Multi-paragraph trailing-`Plain` rule is "no trailing blank
  line before next tag → last Para becomes Plain".** I picked the
  threshold of "≥2 trailing newlines = blank line" via
  `trailing_newlines(text) >= 2`. Single newline = no blank, last
  Para → Plain. This matches pandoc for the cases I tested, but
  pandoc's actual rule is fuzzier (it depends on the parsing
  state); if a future case fails on this, look at
  `flush_html_block_text` first.
- **`try_div_html_block` requires the WHOLE content to be a single
  `<div>...</div>`** with optional surrounding whitespace. Calling
  it on a sub-range works ONLY if you pass exactly the
  `<div>...</div>` slice (including the open and close tags), no
  surrounding bytes. The new splitter slices `&content[i..div_end]`
  before calling.
- **`parse_pandoc_blocks` recursion is safe.** When the inter-tag
  text contains another HTML construct (e.g. a `<div>...</div>`),
  the recursive parse produces an `HTML_BLOCK_DIV` which projects
  via `html_div_block`. No infinite loop because each level
  strips at least the outer tag bytes before recursing.
- **The line-based path emitted `RawBlock` for single-line
  `<p>foo</p>`.** That was wrong even before this session — pandoc
  splits it. The new byte-aware splitter fixes this. Existing
  passing cases survived because the corpus didn't have any
  single-line `<p>foo</p>`-style cases — they were all multi-line
  or div-shaped.

--------------------------------------------------------------------------------

## Earlier session — 2026-05-08 (CommonMark type-4 lowercase declaration recognition)

**html (block + inline) pass count: 47 → 47** (no corpus change — fix
is CommonMark-side, conformance corpus is Pandoc-dialect only).
**Workspace test count: 0 failing → 0 failing** (all green).
**Parser-crate golden cases: 283 → 285** (2 new paired fixtures).

### What landed

One-line fix: `is_ascii_uppercase()` → `is_ascii_alphabetic()` at
`crates/panache-parser/src/parser/blocks/html_blocks.rs:147` in
`try_parse_html_block_start`'s Declaration arm. CommonMark §4.6 type-4
spec says "line begins with the string `<!` followed by an ASCII
letter" — uppercase-only was a pre-existing CommonMark gap. The
existing `# CommonMark spec.txt v0.31.2` test suite (652/652) only
exercises uppercase `<!DOCTYPE`, so this didn't show up there.

End-to-end behavior:
- `<!doctype html>` under CommonMark dialect now emits `HTML_BLOCK`
  (and `RawBlock (Format "html") "<!doctype html>\n"` from the
  projector), matching `pandoc -f commonmark -t native`.
- Pandoc dialect unchanged — still falls through to `Para [Str
  "<!doctype", Space, Str "html>"]`, matching `pandoc -f markdown`.
- Inline path was already correct (`parse_declaration` uses
  `is_ascii_alphabetic`).

### Files in committable diff

- `crates/panache-parser/src/parser/blocks/html_blocks.rs` (1-line
  fix + 2 new assertions in `test_try_parse_declaration` covering
  lowercase under both dialects)
- `crates/panache-parser/tests/golden_parser_cases.rs` (2 new case
  registrations)
- `crates/panache-parser/tests/fixtures/cases/html_block_doctype_lowercase_commonmark/`
  + `_pandoc/` paired parser fixtures (+ snapshots)

No corpus, no projector, no formatter, no salsa changes — pure
CommonMark recognizer fix.

### Suggested next sub-targets, ranked

1. **Phase 5 / 6 — `markdown_in_html_blocks` for non-sectioning
   block tags.** Highest-impact remaining gap. Pandoc default
   parses markdown inside *most* HTML block tags except the four
   verbatim ones; panache currently silently drops content inside
   `<table>/<tr>/<td>/<dl>/<dt>/<dd>/<ul>/<ol>/<li>/<form>`. Fix
   in `parser/blocks/html_blocks.rs` — split HTML-block scanning
   so each balanced tag pair emits a separate `HTML_BLOCK` and
   intermediate content is fed back to the block dispatcher. Add
   ~6-10 corpus cases.
2. **Phase 5 (nested div, blocked.txt id 199)** — depth-aware
   pre-scan. Same machinery needed for #1; could ride along.
3. **`<!ENTITY x "y">` Quoted projection gap.** Noted in earlier
   session: pandoc emits `Quoted DoubleQuote [Str "y"]` for the
   `"y"` part inside a declaration; panache emits `Str "\"y\">"`.
   Smart_punctuation / Quoted feature gap, not html-conformance
   per se. Possibly out-of-scope for this skill.

### Don't redo / known traps (new this session)

- **The `panache.toml` flavor key is top-level, not under
  `[format]`.** Probe configs that use `[format]\nflavor = …`
  silently pick up the default Pandoc flavor — the parse output
  will look like nothing changed. Use `flavor = "common-mark"` at
  the top of the file. (See `docs/guide/configuration.qmd:31`.)
- **The CLI binary (`target/debug/panache`) is a separate build
  artifact from `cargo test`.** After a parser change, `cargo
  build --bin panache` is needed before manual probes; otherwise
  the binary still has the old behavior even though tests pass.
- **CommonMark spec.txt corpus only tests uppercase `<!DOCTYPE`.**
  The CM HTML-blocks suite (44/44) doesn't exercise lowercase
  declarations, so this gap was undetected by the spec-conformance
  harness. When tightening parser recognizers, paired fixtures
  under `tests/fixtures/cases/` are the right place to pin the
  behavior.

--------------------------------------------------------------------------------

## Earlier session — 2026-05-08 (Phase 4 follow-up — gate type-4/type-5 HTML blocks under Pandoc dialect)

**html (block + inline) pass count: 39 → 47** (8 new corpus cases,
all passing).
**Workspace test count: 0 failing → 0 failing** (all green).

### What landed

Phase 4 follow-up gates HTML-block **types 4 (declaration) and 5
(CDATA)** off under `Dialect::Pandoc`, both at the block-level
recognizer and the inline raw-HTML recognizer. CommonMark dialect
keeps current behavior: bare `<!DOCTYPE html>` and `<![CDATA[…]]>`
still emit `RawBlock`. Under Pandoc, the bytes fall through to
paragraph parsing, matching `pandoc -f markdown -t native`:

- `<!DOCTYPE html>` → `Para [Str "<!DOCTYPE", Space, Str "html>"]`
- `<![CDATA[hello <not> world]]>` →
  `Para [Str "<![CDATA[hello", Space, RawInline (Format "html") "<not>", Space, Str "world]]>"]`

Two recognizer changes:

1. **Block-level**
   (`crates/panache-parser/src/parser/blocks/html_blocks.rs::try_parse_html_block_start`):
   `Declaration` and `CData` arms now gated on `is_commonmark`. They
   no longer match under Pandoc, so the block dispatcher falls
   through to paragraph parsing.
2. **Inline-level**
   (`crates/panache-parser/src/parser/inlines/inline_html.rs::try_parse_inline_html`):
   added a `dialect: Dialect` parameter. Internally, `parse_cdata`
   and `parse_declaration` are skipped under Pandoc; `parse_html_comment`,
   `parse_processing_instruction`, `parse_close_tag`, and
   `parse_open_tag` always run. Comments + PIs continue to project
   as RawInline under Pandoc, matching pandoc-native.

`LinkScanContext` (in `parser/inlines/links.rs`) gained a `dialect`
field so its bracket-skip path uses the right recognizer when a
link's text contains `<!DOCTYPE …>`-like bytes (extremely rare in
real input, but consistent now).

Three call sites updated to pass the dialect:
- `parser/inlines/core.rs:1208` (main inline dispatcher, IR-fallback path)
- `parser/inlines/links.rs:120` and `:252` (link text scanning, bracket close)
- `parser/inlines/inline_ir.rs:536` (inline IR builder)

### What Phase 4 follow-up still does NOT do

- **CommonMark type-4 lowercase recognition.** CommonMark spec says
  type-4 starts with `<!` followed by **any** ASCII letter; panache's
  block recognizer still requires uppercase. Pandoc-CommonMark agrees
  with the spec (`pandoc -f commonmark` recognizes `<!doctype html>`
  as RawBlock). This is a pre-existing CommonMark gap not exercised
  by the new corpus and out of Phase 4 scope. Note: the inline path
  is fine — both dialects of CommonMark handle the lowercase form
  via `parse_declaration` (which uses `is_ascii_alphabetic`).
- **Phase 5 / 6 work** for `markdown_in_html_blocks` on non-sectioning
  block tags (`<table>`, `<tr>`, `<td>`, `<dl>`, `<ul>` etc.) — same
  gap as noted in the previous Phase 3/4 sessions. Highest-impact
  remaining target.

### Files in committable diff

- `crates/panache-parser/src/parser/blocks/html_blocks.rs` (block
  gate + 2 unit tests updated to pass dialect explicitly)
- `crates/panache-parser/src/parser/inlines/inline_html.rs` (added
  `dialect: Dialect` parameter, gated `parse_cdata` /
  `parse_declaration`, expanded internal tests with `matches_cm` /
  `no_match_pandoc` helpers)
- `crates/panache-parser/src/parser/inlines/core.rs` (1 call site)
- `crates/panache-parser/src/parser/inlines/links.rs`
  (`LinkScanContext` gained `dialect`; 2 call sites)
- `crates/panache-parser/src/parser/inlines/inline_ir.rs` (1 call site)
- `crates/panache-parser/tests/fixtures/cases/html_block_doctype_pandoc/`
  + `_commonmark/` paired parser fixtures (+ snapshots)
- `crates/panache-parser/tests/golden_parser_cases.rs` (2 new case
  registrations)
- 8 new conformance corpus directories under
  `crates/panache-parser/tests/fixtures/pandoc-conformance/corpus/`:
  - `0233-html-block-doctype-plain` — `<!DOCTYPE html>`
  - `0234-html-block-doctype-lowercase` — `<!doctype html>`
  - `0235-html-block-doctype-between-paras` — DOCTYPE between paras
  - `0236-html-block-cdata-plain` — `<![CDATA[content]]>`
  - `0237-html-block-cdata-with-html` — CDATA containing `<not>`
    raw HTML (RawInline lifted inside the Para)
  - `0238-html-block-cdata-multiline` — CDATA spanning soft breaks
  - `0239-html-inline-doctype-mid-para` — DOCTYPE inside a Para
  - `0240-html-inline-cdata-mid-para` — CDATA inside a Para
- `crates/panache-parser/tests/pandoc/allowlist.txt` (8 new ids
  under two new section headers)
- `crates/panache-parser/tests/pandoc/report.txt` +
  `docs/development/pandoc-report.json` (regenerated; pass rate
  231/232 → 239/240).

### Suggested next sub-targets, ranked

1. **Phase 5 / 6 — `markdown_in_html_blocks` for non-sectioning
   block tags.** Highest-impact remaining gap. Pandoc default
   parses markdown inside *most* HTML block tags except the four
   verbatim ones; panache currently silently drops content inside
   `<table>/<tr>/<td>/<dl>/<dt>/<dd>/<ul>/<ol>/<li>/<form>` (see
   probe in earlier RECAP entry). Fix in
   `parser/blocks/html_blocks.rs` — split HTML-block scanning so
   each balanced tag pair emits a separate `HTML_BLOCK` and
   intermediate content is fed back to the block dispatcher. Add
   ~6-10 corpus cases.
2. **Phase 5 (nested div, blocked.txt id 199)** — depth-aware
   pre-scan. Same machinery needed for #1; could ride along.
3. **CommonMark type-4 lowercase gap.** Tighten the upper-case-only
   gate in `try_parse_html_block_start` to `is_ascii_alphabetic` so
   CommonMark dialect matches the spec (`<!doctype html>`). Probably
   a 5-line change; verify with a paired fixture and the existing
   commonmark corpus.

### Don't redo / known traps (new this session)

- **`try_parse_inline_html` now takes a `dialect: Dialect` param.**
  Any new inline-recognizer call site must pass dialect from its
  closest config/options scope. The compile errors guide you:
  `core.rs` has `config.dialect`; `inline_ir.rs` has `config.dialect`
  (don't fall for `is_commonmark`-only); `links.rs` uses
  `LinkScanContext.dialect` (already populated by
  `LinkScanContext::from_options`).
- **`LinkScanContext::Default` had to grow a `dialect`.** I picked
  `Dialect::Pandoc` as the default since that's what `for_flavor`
  returns for the default `Flavor::Pandoc`. If anyone constructs
  `LinkScanContext::default()` and then tries to use it for raw-HTML
  scanning under CommonMark, they'll silently get the Pandoc-only
  recognizer. Always derive via `from_options(config)` in real code.
- **The Pandoc CST for `<![CDATA[hello <not> world]]>` contains an
  `UNRESOLVED_REFERENCE` shape** (the `[hello <not> world]` segment
  matches the bracket grammar; lookup fails; flattens to Str via the
  projector). This is not a bug — the projector correctly emits the
  matching pandoc-native shape. If you find yourself trying to
  "fix" the CST to avoid UNRESOLVED_REFERENCE here, don't — the
  bracket-shape is genuinely ambiguous in this byte stream and the
  resolver gets the right answer.
- **`<!ENTITY x "y">`-style declarations would diverge** from
  pandoc-native because pandoc emits `Quoted DoubleQuote [Str "y"]`
  for `"y"` while panache emits `Str "\"y\">"`. This is a separate
  smart_punctuation / Quoted feature gap, not part of the
  type-4/type-5 work. Don't add `<!ENTITY x "y">` as a corpus case.

--------------------------------------------------------------------------------

## Earlier session — 2026-05-08 (Phase 4 — comments + processing instructions corpus pin)

**html (block + inline) pass count: 27 → 39** (12 new corpus cases,
all passing, no code change required).
**Workspace test count: 0 failing → 0 failing** (all green).

### What landed

Phase 4 is **partial corpus expansion** — comments and processing
instructions match pandoc-native exactly under `Flavor::Pandoc`,
both block and inline. Declarations (`<!DOCTYPE>`) and CDATA were
*not* pinned: panache currently emits `RawBlock` for them, but
pandoc-markdown emits `Para [Str ...]` (treats them as plain
text). That divergence is a parser-shape gap deferred to a later
session.

Added 12 corpus directories under
`crates/panache-parser/tests/fixtures/pandoc-conformance/corpus/`:

- `0221-html-block-comment-plain` — `<!-- comment -->`
- `0222-html-block-comment-multiline` — `<!--\nMulti\nline\n-->`
- `0223-html-block-comment-between-paras` — comment between two
  Paras
- `0224-html-block-comment-adjacent` — two consecutive comments
  → two RawBlocks
- `0225-html-block-comment-with-tags` — comment containing
  `<span>nested</span>` (still one opaque RawBlock — pandoc does
  not recurse)
- `0226-html-inline-comment-plain` — `Text with <!-- c --> in it.`
- `0227-html-inline-comment-no-spaces` — `Text with <!--c--> in it.`
- `0228-html-inline-comment-multiline` — inline comment spanning
  a hard-break boundary
- `0229-html-inline-comment-multiple` — multiple inline comments
  in one Para
- `0230-html-block-pi-plain` — `<?php echo "hi"; ?>`
- `0231-html-block-pi-xml` — `<?xml version="1.0"?>`
- `0232-html-inline-pi-plain` — `See <?php ... ?> output.`

All cases match `pandoc -f markdown -t native` byte-equivalent
(after `normalize_native`). No parser, projector, formatter, or
salsa changes — Phase 4 is pure corpus pinning.

### What Phase 4 still does NOT do

- **`<!DOCTYPE html>` and other declarations.** Pandoc-markdown
  emits `Para [Str "<!DOCTYPE", Space, Str "html>"]` (literal
  text); panache emits `RawBlock (Format "html") "<!DOCTYPE html>"`.
  Probe:
  ```
  printf '<!DOCTYPE html>\n' | pandoc -f markdown -t native
  printf '<!DOCTYPE html>\n' | cargo run -- parse --to pandoc-ast
  ```
  Pandoc reads HTML blocks via `htmlTag isBlockTag`
  (`pandoc/src/Text/Pandoc/Readers/Markdown.hs:1117`) which
  matches `<!DOCTYPE>` but rejects the bare declaration form in
  default markdown. Fix would gate panache's HTML-block type-4
  recognition under `Flavor::Pandoc` (or only accept it under
  CommonMark dialect).

- **`<![CDATA[...]]>`.** Same pattern: pandoc-markdown emits a
  Para with literal text and finds inline raw HTML inside (e.g.
  `<not>`); panache emits a single opaque RawBlock. CommonMark
  type-5 recognition needs to be gated off in `Flavor::Pandoc`.

- **The bigger `markdown_in_html_blocks` story** for non-sectioning
  block tags (e.g. `<table>`, `<tr>`, `<td>`, `<dl>`) — same Phase
  5-class parser-shape gap noted in the previous Phase 3 session.

### Files in committable diff

- 12 new corpus directories under
  `crates/panache-parser/tests/fixtures/pandoc-conformance/corpus/`
  (each with `input.md` + `expected.native` generated via
  `pandoc 3.9.0.2 -f markdown -t native`).
- `crates/panache-parser/tests/pandoc/allowlist.txt` (12 new ids
  221–232 under two new section headers
  `# html-block (comments + processing instructions)` and
  `# html-inline (comments + processing instructions)`).
- `crates/panache-parser/tests/pandoc/report.txt` +
  `docs/development/pandoc-report.json` (regenerated; pass rate
  219/220 → 231/232).

No parser, projector, formatter, or salsa changes — Phase 4
matches Phase 3's pure-negative-space shape.

### Suggested next sub-targets, ranked

1. **Phase 4 follow-up — gate HTML-block type-4 / type-5
   recognition under Pandoc dialect.** Today panache emits
   `RawBlock` for `<!DOCTYPE html>` and `<![CDATA[...]]>`; pandoc
   emits Para with literal text. The fix likely lives in
   `crates/panache-parser/src/parser/blocks/html_blocks.rs`'s
   `try_parse_html_block_start` — under `Dialect::Pandoc`, types
   4 and 5 should *not* match (fall back to paragraph parsing).
   CommonMark dialect must keep them. Add 4-6 corpus cases (all
   declaration/CDATA variants) once the gate works. Paired
   parser fixture required.
2. **Phase 5 / 6 — `markdown_in_html_blocks` for non-sectioning
   block tags.** Highest-impact remaining gap. Pandoc default
   parses markdown inside *most* HTML block tags except the four
   verbatim ones; panache currently silently drops content inside
   `<table>/<tr>/<td>/<dl>/<dt>/<dd>/<ul>/<ol>/<li>/<form>`. Fix
   in `parser/blocks/html_blocks.rs` — split HTML-block scanning
   so each balanced tag pair emits a separate `HTML_BLOCK` and
   intermediate content is fed back to the block dispatcher. Add
   ~6-10 corpus cases once it works.
3. **Phase 5 (nested div, blocked.txt id 199)** — depth-aware
   pre-scan. Same machinery needed for #2 above; could ride
   along.

### Don't redo / known traps (new this session)

- **Pandoc-markdown does NOT recognize HTML type-4 (declarations)
  or type-5 (CDATA) as raw HTML.** This is markdown-flavor
  specific — `pandoc -f commonmark` keeps RawBlock for
  `<!DOCTYPE html>`. The divergence is in
  `pandoc/src/Text/Pandoc/Readers/Markdown.hs:1117` where
  `htmlBlock` uses `htmlTag isBlockTag` (only matches block-level
  TAGS), not the broader CommonMark "starts with `<!`-letter"
  rule. Don't try to pin DOCTYPE/CDATA cases without fixing this
  first — the current parser-shape will project to the wrong
  pandoc-native shape.
- **Phase 4 is partial.** Skill description says Phase 4 covers
  "comments, processing instructions, declarations, CDATA". The
  first two are easy negative-space pins (what this session did);
  the latter two require a parser-shape fix (next session). Don't
  re-read the skill RECAP and assume Phase 4 is "done" — check
  whether declaration/CDATA gate has shipped before declaring it
  complete.
- **Comment recognition recurses correctly inline.** Even multi-line
  inline comments (`<!-- TODO\nspans -->` inside a Para) parse
  as a single RawInline that spans the soft-break. Don't try to
  split them at line boundaries — pandoc keeps them whole.

--------------------------------------------------------------------------------

## Earlier session — 2026-05-08 (Phase 3 — sectioning + verbatim negative-space pin)

**html (block + inline) pass count: 17 → 27** (10 new corpus cases,
all passing, no code change required).
**Workspace test count: 0 failing → 0 failing** (all green).

### What landed

Phase 3 is **pure corpus expansion** — every panache CST shape and
projector arm needed for these cases already existed. The 10 cases
just pin the behavior so future regressions are caught.

Added 10 corpus directories under
`crates/panache-parser/tests/fixtures/pandoc-conformance/corpus/`:

- `0211-html-block-section-plain` — `<section>...</section>`
- `0212-html-block-article-plain` — `<article>...</article>`
- `0213-html-block-aside-plain` — `<aside>...</aside>`
- `0214-html-block-nav-plain` — `<nav>...</nav>`
- `0215-html-block-section-with-attrs` —
  `<section class="intro">...</section>`
- `0216-html-block-pre-blocks-markdown` — `<pre>` with
  markdown-looking body (`# Not a heading`, `*not emph*`); pandoc
  emits one opaque RawBlock, **explicit spec exception** to
  `markdown_in_html_blocks` (see `assets/pandoc-spec/raw-html.md:55-56`).
- `0217-html-block-style-plain` — `<style>...</style>`
- `0218-html-block-script-plain` — `<script>...</script>`
- `0219-html-block-textarea-plain` — `<textarea>...</textarea>`
- `0220-html-block-script-with-attrs` —
  `<script type="text/javascript">...</script>`

Sectioning tags emit a 3-block sequence
(`RawBlock "<section>"`, `Plain [...]`, `RawBlock "</section>"`),
matching pandoc-native — the open/close tags are NOT lifted into a
wrapper; inner body **is** parsed as markdown. This is type-6
HTML-block behavior and panache already gets it right.

Verbatim tags (`<pre>/<style>/<script>/<textarea>`) emit a single
opaque RawBlock containing the full open+body+close — no markdown
parsing inside, matching pandoc-native and the spec exception.

### What Phase 3 still does NOT do

- **The bigger `markdown_in_html_blocks` story** for non-sectioning,
  non-verbatim block tags (e.g. `<table>`, `<tr>`, `<td>`, `<dl>`).
  Pandoc-native breaks each tag into its own `RawBlock "html"` and
  parses surrounding markdown; panache currently groups the whole
  construct into one opaque `HTML_BLOCK` and **drops the inner
  per-tag content**. Probe:
  ```
  printf '<table>\n<tr>\n<td>*one*</td>\n</tr>\n</table>\n' \
    | cargo run -- parse --to pandoc-ast
  ```
  emits only the wrapping `<table>`/`<tr>`/`</tr>`/`</table>` —
  the `<td>*one*</td>` line is lost. This is a bigger Phase 5-class
  parser-shape gap (split HTML-block scanner so each balanced pair
  emits a separate `HTML_BLOCK` and content between gets fed back
  to block parsing). Not addressed here.

- **Plain vs Para promotion divergence**. With blank lines around
  the inner body (`<section>\n\nfoo\n\n</section>`), pandoc emits
  Para; panache emits Plain. Same root cause as the
  `<table>` case — pandoc's recursive block reparse handles
  blank-line spacing differently. Out of Phase 3 scope.

- **Trailing-close-tag-as-RawBlock**. With nested `<section>` closes
  followed by a paragraph, pandoc emits the trailing `</section>` as
  a top-level `RawBlock`; panache wraps it in `Para [ RawInline
  "</section>" ]`. Same family of issues.

### Files in committable diff

- 10 new corpus directories under
  `crates/panache-parser/tests/fixtures/pandoc-conformance/corpus/`
  (each with `input.md` + `expected.native` generated via
  `pandoc 3.9.0.2 -f markdown -t native`).
- `crates/panache-parser/tests/pandoc/allowlist.txt` (10 new ids
  211–220 under new `# html-block (sectioning + verbatim — no
  markdown inside verbatim, simple cases)` section header).
- `crates/panache-parser/tests/pandoc/report.txt` +
  `docs/development/pandoc-report.json` (regenerated; pass rate
  209/210 → 219/220).

No parser, projector, formatter, or salsa changes — Phase 3 is pure
negative-space / corpus coverage.

### Suggested next sub-targets, ranked

1. **Phase 5 (or new Phase 6) — `markdown_in_html_blocks` for
   non-sectioning block tags.** Highest-impact remaining gap. Pandoc
   default behavior (per `assets/pandoc-spec/raw-html.md:25-61`)
   parses markdown inside *most* HTML block tags except the four
   verbatim ones; panache currently silently drops content inside
   `<table>/<tr>/<td>/<dl>/<dt>/<dd>/<ul>/<ol>/<li>/<form>` etc.
   when used as raw HTML blocks. The fix likely lives in
   `parser/blocks/html_blocks.rs` — split HTML-block scanning so
   each balanced tag pair emits a separate `HTML_BLOCK` and
   intermediate content is fed back to the block dispatcher. Add
   ~6-10 corpus cases (`<table>` + cells, `<dl>` + items,
   `<ul>` + list items, balanced inline-children-of-block).
2. **Phase 4 — Comments / processing instructions / declarations /
   CDATA projection.** Pin `RawBlock "html"` / `RawInline "html"`
   for each. CST is already correct; this is corpus + projector
   verification, possibly all-passing today.
3. **Phase 5 (nested div, blocked.txt id 199)** — depth-aware
   pre-scan in `parser/blocks/html_blocks.rs`. Same machinery
   needed for #1 above; could ride along.

### Don't redo / known traps (new this session)

- **Plain-vs-Para divergence on blank-line-surrounded sectioning
  bodies** is a real gap but NOT a Phase 3 case — don't try to
  shoehorn a corpus case for `<section>\n\nfoo\n\n</section>`
  that emits Plain on panache and Para on pandoc; it will fail.
  Save the input pattern for the bigger
  `markdown_in_html_blocks` work.
- **Sectioning tags work without code change because pandoc's
  HTML-block-type-6 already includes them.** The recap for Phase 1
  / Phase 2 hinted that Phase 3 might "need code" — it does not.
  All 10 cases passed on the first conformance run. The lift
  metaphor doesn't apply here: the open/close tags stay raw, only
  the inner body gets markdown parsing (which it already does).
- **Verbatim tags' carve-out is spec-explicit**
  (`assets/pandoc-spec/raw-html.md:55-56`). When the
  `markdown_in_html_blocks` work in #1 above lands, the tag-name
  recognizer must NOT recurse into `<script>/<style>/<pre>/
  <textarea>` bodies. This is type-1 HTML-block behavior in pandoc.
- **`<table>` + `<td>` content drop is silent.** Panache emits a
  4-RawBlock sequence (`<table>`, `<tr>`, `</tr>`, `</table>`) and
  drops the `<td>*one*</td>` lines entirely. No diagnostic. When
  doing #1 above, write a probe test FIRST that exercises this so
  the fix has a clear before/after.

--------------------------------------------------------------------------------

## Earlier session — 2026-05-08 (Phase 2 — `<span>` inline lift)

**html (block + inline) pass count: 9 → 17** (8 new corpus cases for
`html-inline-span`, all passing).
**Workspace test count: 0 failing → 0 failing** (all green).

### What landed

Phase 2 mirrors Phase 1 on the inline side. Two structural CST
changes for `<span>...</span>` under `Dialect::Pandoc`, both
byte-lossless:

1. **Wrapper retag**: the existing `BRACKETED_SPAN` shape used by
   `emit_native_span` is replaced with `INLINE_HTML_SPAN` for the
   HTML form. The bracketed `[content]{attrs}` form keeps using
   `BRACKETED_SPAN`. CommonMark dialect (with `native_spans`
   extension explicitly enabled) keeps emitting `BRACKETED_SPAN`
   for the legacy path.
2. **Open-tag tokenization**: inside the open tag, the bytes
   `<span ATTRS>` are split into
   `TEXT("<span") + WHITESPACE + HTML_ATTRS{TEXT(attrs)}
   + (WHITESPACE)? + TEXT(">")`. Mirrors `emit_div_open_tag_tokens`
   with one improvement: the new `emit_span_open_tag_tokens`
   preserves multi-whitespace (the legacy `BRACKETED_SPAN`
   emission collapsed multi-whitespace attribute regions to a
   single space — a pre-existing minor losslessness divergence
   that the new path no longer has).

`AttributeNode::can_cast` already accepts `HTML_ATTRS`, so the
salsa indexer's existing `for attr in
tree.descendants().filter_map(AttributeNode::cast)` walk picks up
`<span id>` automatically. **No parallel salsa walk** — the
existing `SPAN_ATTRIBUTES` walk continues to handle the bracketed
`[content]{attrs}` form (which uses `SPAN_ATTRIBUTES` as a NODE
wrapping `{attrs}`); the HTML form no longer emits
`SPAN_ATTRIBUTES` under Pandoc.

`emit_native_span` signature changed: now takes `(builder, raw,
content, config)` where `raw` is the full `<span...>content</span>`
slice. Open-tag length is computed as
`raw.len() - content.len() - "</span>".len()`. Both callers
(`parser/inlines/core.rs::parse_inline_text` IR-driven branch and
the legacy CommonMark+native_spans dispatcher) pass
`&text[pos..pos+len]`.

Projector got an `INLINE_HTML_SPAN` match arm in `pandoc_ast.rs`
(`inline_html_span_inline`) that reads `HTML_ATTRS` directly via
`parse_html_attrs` and walks `SPAN_CONTENT` via the standard
inline projection path. The legacy `bracketed_span_inline` arm is
unchanged.

Formatter accepts `INLINE_HTML_SPAN` with a dedicated arm in
`crates/panache-formatter/src/formatter/inline.rs`. The arm walks
children verbatim for tokens and the `HTML_ATTRS` node, recurses
through `SPAN_CONTENT` for nested inline content. No smart-quote
or escape transformation in the open/close-tag region.

### What Phase 2 still does NOT do

- **Multi-line `<span>` open tags.** `<span\n  id="x">` works (the
  recognizer accepts whitespace including newlines), but the
  open-tag tokenization treats internal newlines as whitespace —
  no special wrapping. Edge case; corpus doesn't exercise it yet.
- **Tag-name case sensitivity.** `try_parse_native_span` matches
  only literal `<span` — uppercase `<SPAN>` falls through to opaque
  `INLINE_HTML`. Pandoc-native is also case-sensitive on this in
  default markdown, so this matches.
- **Inside Pandoc bracket-text suppression**. The IR scanner gates
  span recognition on `!in_pandoc_bracket`, so `[**foo
  <span>bar</span>**]` inside link text stays opaque. This was
  already the case before Phase 2 — confirmed it didn't regress.

### Files in committable diff

- `crates/panache-parser/src/syntax/kind.rs` (new
  `INLINE_HTML_SPAN` variant)
- `crates/panache-parser/src/parser/inlines/native_spans.rs`
  (new `emit_span_open_tag_tokens`; `emit_native_span` signature
  change + dialect-aware wrapper)
- `crates/panache-parser/src/parser/inlines/core.rs` (2 callers
  pass `&text[pos..pos+len]` instead of attributes string)
- `crates/panache-parser/src/pandoc_ast.rs` (new
  `inline_html_span_inline` + match arm)
- `crates/panache-formatter/src/formatter/inline.rs`
  (`INLINE_HTML_SPAN` formatter arm)
- `src/linter/rules/undefined_anchor.rs` (2 new tests:
  `resolves_explicit_id_on_html_inline_span`,
  `resolves_explicit_id_on_html_inline_span_inside_paragraph`)
- `crates/panache-parser/tests/pandoc/allowlist.txt` (8 new ids
  under new `# html-inline` section header)
- `crates/panache-parser/tests/fixtures/pandoc-conformance/corpus/`
  — 8 new `0203..0210-html-inline-span-*/` directories
- `crates/panache-parser/tests/fixtures/cases/html_inline_span_with_id_pandoc/`
  + `_commonmark/` paired parser fixtures (+ snapshots)
- Updated existing snapshot:
  `parser_cst_issue_175_native_span_unicode_panic.snap`
  (BRACKETED_SPAN → INLINE_HTML_SPAN retag, byte-identical CST).
- `tests/fixtures/cases/html_inline_span_idempotent/`
  formatter golden (round-trip pinning).
- `crates/panache-parser/tests/pandoc/report.txt` +
  `docs/development/pandoc-report.json` (regenerated).

### Issue #263 sibling status

`<span id="anchor-c">marker</span>\n\nSee [link](#anchor-c).\n`
no longer raises `undefined-anchor`. Verified via 2 new unit tests
in `src/linter/rules/undefined_anchor.rs` and corpus case
`0208-html-inline-span-issue-263` (passes against pandoc-native).

### Suggested next sub-targets, ranked

1. **Phase 3 — Negative-space pin.** Add ~6-10 corpus cases for
   `<section>`, `<article>`, `<aside>`, `<nav>` (RawBlock) and
   verbatim tags `<pre>`/`<style>`/`<script>`/`<textarea>` (no
   markdown inside). Most should pass without code change; corpus
   coverage is the goal so future regressions are caught. Mostly
   block-level (verbatim tags inside paragraphs need separate
   inline-level cases).
2. **Phase 4 — Comments / processing instructions / declarations
   / CDATA projection.** Pin `RawBlock "html"` / `RawInline "html"`
   for each. CST is already correct; this is corpus + projector
   verification.
3. **Phase 5 (nested div, blocked.txt id 199)** — needs
   depth-aware pre-scan in `parser/blocks/html_blocks.rs`. Higher
   complexity than Phase 3/4; defer until those land.

### Don't redo / known traps (new this session)

- **`<span>` was ALREADY lifting under Pandoc before Phase 2.**
  Phase 1's RECAP guidance to "retag `INLINE_HTML` to
  `INLINE_HTML_SPAN`" was misleading — the actual starting state
  was `BRACKETED_SPAN` with a `SPAN_ATTRIBUTES` token (from
  `emit_native_span`), not `INLINE_HTML`. The IR's
  `ConstructKind::NativeSpan` event already routed Pandoc-dialect
  spans through `BRACKETED_SPAN`. Phase 2 retagged
  `BRACKETED_SPAN` → `INLINE_HTML_SPAN` and restructured the open
  tag's attribute region from `SPAN_ATTRIBUTES` token to
  `HTML_ATTRS` node. If you find yourself re-reading the skill's
  RECAP for Phase 3+ guidance, **verify against the live code**
  before acting on any "current state" claim.
- **The legacy `BRACKETED_SPAN` HTML-form path collapsed
  multi-whitespace attribute regions** (e.g. `<span  id="x">`
  emitted `<span id="x">` in the CST → losslessness divergence).
  This was a pre-existing bug not exercised by any fixture. Phase
  2's new `INLINE_HTML_SPAN` path is byte-exact. The legacy
  CommonMark+native_spans path still has the bug, but that path is
  effectively unreachable since `native_spans` defaults off in CM.
- **`SPAN_ATTRIBUTES` is asymmetric**: a TOKEN under HTML form
  (legacy CommonMark path), a NODE under bracketed-span form. The
  salsa indexer's `for span_attrs in
  tree.descendants().filter(...)` walk only sees the NODE form.
  After Phase 2, the HTML form under Pandoc no longer emits
  `SPAN_ATTRIBUTES` at all — it uses `HTML_ATTRS` node, picked up
  by `AttributeNode::cast`. Don't try to "unify" the salsa walks
  unless you also unify the emission shapes; the asymmetry is
  intentional for the bracketed form.
- **Section header in the conformance corpus is the FIRST `-`
  segment**: `0203-html-inline-span-plain` → section="html",
  slug="inline-span-plain". Both `html-block-*` and
  `html-inline-*` cases land in section "html" in the report
  (`html: 17 pass / 1 fail`). The `# html-inline` allowlist
  section header is purely for human organization; the runner
  doesn't inspect it.

--------------------------------------------------------------------------------

## Earlier session — 2026-05-08 (Phase 1 — `<div>` block lift)

**html-block pass count: 0 → 9** (10 corpus cases seeded; 9 passing,
1 blocked as nested-div Phase 5 target).
**Workspace test count: 0 failing → 0 failing** (all green).

### What landed

Phase 1 ships **two** structural CST changes for `<div>` HTML
blocks under `Dialect::Pandoc`, both byte-lossless:

1. **Wrapper retag**: `HTML_BLOCK` → `HTML_BLOCK_DIV` for matched
   div blocks. Gated on `Dialect::Pandoc && extensions.native_divs
   && tag_name == "div"`.
2. **Open-tag tokenization**: inside the open `HTML_BLOCK_TAG`,
   the bytes `<div ATTRS>` are split into
   `TEXT("<div") + WHITESPACE + HTML_ATTRS{TEXT(attrs)} + TEXT(">")`.
   `HTML_ATTRS` is a new `SyntaxKind`. Source bytes unchanged —
   just finer granularity.

`AttributeNode::can_cast` accepts `HTML_ATTRS`. The existing
salsa indexer's `for attr in
tree.descendants().filter_map(AttributeNode::cast)` walk picks up
`<div id>` automatically, the same way it handles fenced-div
`DIV_INFO` and heading `ATTRIBUTE`. **No parallel salsa walk** —
my earlier sketch had one; it was deleted as redundant.

`AttributeNode::id()` and `id_value_range()` route by
`SyntaxKind`: `HTML_ATTRS` uses `parse_html_attribute_list`
(public sibling helper extracted from
`parse_html_tag_attributes`); other kinds use the existing
`try_parse_trailing_attributes` for `{...}` pandoc syntax.

Block dispatcher decides the wrapper kind in
`parser/block_dispatcher.rs::parse_prepared`; the actual
emission lives in new `parse_html_block_with_wrapper` in
`parser/blocks/html_blocks.rs`. The open-tag tokenization helper
`emit_div_open_tag_tokens` handles quoted attribute values
correctly (a same-line `<div id="x">Content</div>` doesn't get
its open-tag `>` confused with the close tag's `>`).

Projector got an `HTML_BLOCK_DIV` match arm in `pandoc_ast.rs`
that delegates to the existing `try_div_html_block` byte-level
reparser. **The projector did NOT simplify** — it gained a
parallel arm that produces the same `Block::Div` output as
before. Future structural recursion (Phase 5) will replace
`try_div_html_block` with a CST walk.

Formatter accepts `HTML_BLOCK_DIV` wherever it accepts
`HTML_BLOCK` (text emission is identical because the wrapper
walk goes through `descendants_with_tokens` and emits all
tokens verbatim regardless of structure).

### What Phase 1 still does NOT do

- **Recursive content parsing.** Bytes inside the div (between
  open and close tags) are still raw TEXT in
  `HTML_BLOCK_CONTENT`, not block-parsed at parse time. The
  pandoc-native projector reparses them on demand. A real
  structural lift would have `PARAGRAPH`, `LIST`, etc. as direct
  children of `HTML_BLOCK_DIV`.
- **Multi-line open tags.** `<div\n  id="x">` falls back to opaque
  `HTML_BLOCK` because `try_parse_html_block_start` only inspects
  the first line. Edge case.
- **Nested divs (corpus id 199).** The HTML-block scanner is
  depth-unaware; outer div closes at the first inner `</div>`.
  Phase 5 target.

### Files in committable diff

- `crates/panache-parser/src/syntax/kind.rs` (new variant)
- `crates/panache-parser/src/parser/blocks/html_blocks.rs`
- `crates/panache-parser/src/parser/block_dispatcher.rs`
- `crates/panache-parser/src/parser/utils/attributes.rs`
- `crates/panache-parser/src/pandoc_ast.rs`
- `crates/panache-formatter/src/formatter/core.rs`
- `crates/panache-formatter/src/utils.rs`
- `src/salsa.rs`
- `src/linter/rules/undefined_anchor.rs` (2 new tests)
- `crates/panache-parser/tests/pandoc/allowlist.txt`
  (9 new ids under `# html-block`)
- `crates/panache-parser/tests/pandoc/blocked.txt` (199 nested div)
- `crates/panache-parser/tests/fixtures/pandoc-conformance/corpus/`
  — 10 new `<NNNN>-html-block-<slug>/` directories
- `crates/panache-parser/tests/fixtures/cases/html_block_div_with_id_pandoc/`
  + `_commonmark/` paired parser fixtures (+ snapshots)
- Updated existing snapshots: `parser_cst_html_block.snap`,
  `parser_cst_html_block_commonmark_type6_type7_pandoc.snap` (pure
  HTML_BLOCK → HTML_BLOCK_DIV retag, byte-identical CST).
- `tests/fixtures/cases/html_block_div_idempotent/` formatter
  golden (round-trip pinning).
- `docs/reference/linter-rules.qmd` (removed `<div id>` limitation
  note; kept `<a id>` / `<a name>`).
- `crates/panache-parser/tests/pandoc/report.txt` +
  `docs/development/pandoc-report.json` (regenerated).
- `.claude/skills/html-conformance/SKILL.md` + `RECAP.md` (new).

### Issue #263 status

**Closed.** `<div id="anchor-c">Content.</div>\n\nSee
[link](#anchor-c).\n` no longer raises `undefined-anchor`. Verified
via:
- 2 new unit tests in
  `src/linter/rules/undefined_anchor.rs`.
- Manual CLI repro: `panache lint /tmp/263.md` → "No issues found".
- Corpus case `0201-html-block-div-issue-263` passes against
  pandoc-native.

### Suggested next sub-targets, ranked

1. **Phase 2 — Inline `<span>` lift.** Mirror Phase 1 minimally:
   add `INLINE_HTML_SPAN` SyntaxKind, retag the existing
   `INLINE_HTML` wrapper when a balanced `<span>...</span>` is
   recognized under Pandoc. Coordinate with `pandoc-ir-migrate`
   Phase 1 — IR's opaque scan stays; the parser-side retag is
   complementary. Probe `*foo <span>bar</span> baz*` to confirm
   emphasis doesn't pair into the span.
2. **Phase 3 — Negative-space pin.** Add ~5-8 corpus cases for
   `<section>`, `<article>`, `<aside>`, `<nav>` (stay as
   `RawBlock`) and verbatim tags `<pre>`/`<style>`/`<script>`/
   `<textarea>` (no markdown inside). Most should pass without
   any code change; goal is corpus coverage so future regressions
   are caught.
3. **Phase 5 (nested div, blocked.txt id 199)** — needs depth-aware
   pre-scan in `parser/blocks/html_blocks.rs`. Higher complexity
   than Phase 2/3; defer until Phase 2 lands.

### Don't redo / known traps (new this session)

- **Disk lint cache at `~/.cache/panache/` serves stale
  `undefined-anchor` results.** This bit me hard during salsa
  development: `cargo build` succeeds, unit tests pass, but
  `panache lint` keeps emitting the OLD diagnostic. The CLI reads
  cached lint output keyed on a tool-fingerprint that did NOT
  invalidate when I changed the lint rule. Fix: `rm -rf
  ~/.cache/panache/` between debugging runs, OR set
  `cache.enabled = false` in `panache.toml`. Always validate the
  rule via unit tests first; CLI is downstream. (Also documented
  in top-level `AGENTS.md`.)
- **`<div id="x">Content</div>` on one line is ONE
  `HTML_BLOCK_TAG`, not two.** The parser's `is_closing_marker`
  match fires on the same line as the open. The open-tag
  tokenization helper `emit_div_open_tag_tokens` therefore must
  scan to the first **unquoted** `>` — both the helper and
  `parse_html_tag_attributes` get this right; `strip_suffix('>')`
  would grab the close tag's `>` and break things.
- **HTML_ATTRS is the structural pattern; do NOT add synthetic
  tokens.** The right way to expose attributes structurally is
  finer-grained tokenization of the EXISTING source bytes (split
  one TEXT into `TEXT + WHITESPACE + HTML_ATTRS{TEXT} + TEXT`).
  This preserves losslessness because no new bytes are emitted.
  Adding synthetic ATTRIBUTE tokens — like the rejected initial
  draft did — would duplicate bytes and break the
  tree-text-equals-input invariant.
- **An earlier draft of Phase 1 had a parallel salsa walk for
  `HTML_BLOCK_DIV`.** It was redundant once `HTML_ATTRS` got
  added to `AttributeNode::can_cast`. The parallel walk was
  deleted. If you find yourself adding a new walk for a kind
  that "looks like an attribute region", check whether you can
  add it to `AttributeNode::can_cast` instead — that's the
  established pattern (see `DIV_INFO`, `ATTRIBUTE`,
  `SPAN_ATTRIBUTES` are all SPAN_ATTRIBUTES).
- **The legacy `try_div_html_block` byte-level reparser in
  `pandoc_ast.rs` STAYS.** It's still how the projector renders
  the div's inner content, since the CST keeps the inner bytes
  as raw TEXT. Don't delete until Phase 5 produces structural
  inner blocks at parse time.
- **Existing parser snapshots that contain `<div>` under Pandoc
  WILL change** when this lands. Three fixtures hit this in
  Phase 1; all diffs are pure tokenization-granularity changes
  (same bytes, more nodes). Don't blanket-accept — review each
  to confirm bytes are unchanged.
