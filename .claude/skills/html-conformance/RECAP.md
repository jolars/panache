# HTML conformance — running session recap

This file is the rolling, terse handoff between sessions of the
`html-conformance` skill. Read it at the start of a session for the
suggested next sub-target and known traps; rewrite the **Latest session**
entry at the end with what changed and what to look at next.

Keep entries short. Pass counts + a one-line root cause beat a narrative.
The hard-won judgment calls (why a lever was chosen, why an approach was
reverted, what trap to avoid) are the load-bearing content here.

--------------------------------------------------------------------------------

## Latest session — 2026-05-08 (Phase 2 — `<span>` inline lift)

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
