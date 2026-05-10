---
name: html-conformance
description: Incrementally drive Panache's `Flavor::Pandoc` HTML-block /
  raw-HTML parsing toward `pandoc -f markdown -t native` parity by
  expanding the `html-block` / `html-inline` slice of the existing
  pandoc-conformance corpus. Lift HTML structure into the CST in the
  parser by tokenizing existing source bytes at finer granularity
  (e.g. `HTML_ATTRS` inside `HTML_BLOCK_TAG`), so the existing
  `AttributeNode` walk finds ids without a parallel byte-parser.
  Allowlist cases as parity grows.
---

Use this skill when asked to advance Panache's HTML conformance,
unblock a regression that involves raw HTML attributes (issue #263 and
its descendants), or pick "the next best phase" of the HTML lift.

## Scope boundaries

- Target is HTML-block + raw-HTML parsing under `Flavor::Pandoc`.
  Block-level: `crates/panache-parser/src/parser/blocks/html_blocks.rs`
  + `block_dispatcher.rs`. Inline-level:
  `crates/panache-parser/src/parser/inlines/inline_html.rs`. Projection:
  `crates/panache-parser/src/pandoc_ast.rs`. Salsa indexer:
  `src/salsa.rs`.
- `Flavor::Pandoc` only. CommonMark dialect must stay byte-identical
  in CST and pandoc-native projection. `Dialect::CommonMark` keeps
  the opaque `HTML_BLOCK` shape; lifts are gated on
  `Dialect::Pandoc`.
- Pandoc-native (`pandoc -f markdown -t native`) is the **behavioral
  reference**. Existing parser fixtures and projector output are not
  the reference — when they disagree with pandoc-native, fix toward
  pandoc-native.
- This is a **long-horizon effort** (5 phases — see "Phased plan"
  below). Each session moves at most one phase forward; no sweeping
  rewrites in a single go.
- Reuses the existing **pandoc-conformance harness** verbatim. New
  cases live in
  `crates/panache-parser/tests/fixtures/pandoc-conformance/corpus/<NNNN>-<section>-<slug>/`
  with section prefix `html-block` (block-level) or `html-inline`
  (inline-level). The pandoc allowlist
  (`crates/panache-parser/tests/pandoc/allowlist.txt`) gets new
  section header comments `# html-block` / `# html-inline`.
- Out-of-scope and deferred to `tests/pandoc/blocked.txt`:
  `markdown="1"` / `markdown="0"` (Ext_markdown_attribute, default
  off in pandoc-flavored markdown); `<a id>` / `<a name>` legacy
  anchor lift (pandoc does NOT lift these in default `markdown` —
  treat as opaque RawInline); malformed/unbalanced tags (fall back
  to opaque `HTML_BLOCK` / `INLINE_HTML`).

## Related rules to read first

- `.claude/rules/pandoc-conformance.md` — general workflow for the
  shared pandoc-conformance harness. This skill is a focused subset.
- `.claude/rules/parser.md` — `Dialect` vs `Extensions` split, CST
  losslessness, the pandoc-native-as-reference rule, the
  TEXT-coalescence-vs-structural-diff distinction.
- `.claude/rules/formatter.md` — idempotency divergences are often
  parser-shape bugs; verify against pandoc-native first.

## Phased plan

The HTML lift is bounded by 5 phases. Pick one (or part of one) per
session. Latest phase status lives in `RECAP.md`.

**Phase 1** — Block-level `<div>` lift (Pandoc dialect). Parser emits
`HTML_BLOCK_DIV` for matched `<div ...>...</div>`; projector consumes
it and emits `Block::Div(attrs, blocks)`; salsa indexer extracts
`id` from the open tag and registers it in
`crossref_declarations`. Unblocks issue #263.

**Phase 2** — Inline `<span>` lift (Pandoc dialect). Mirrors Phase 1
on the inline side. Coordinate with `pandoc-ir-migrate` Phase 1 —
`<span>` is already a `ConstructKind::PandocOpaque` event in
`inline_ir.rs`, so the lift must not double-handle the byte range.

**Phase 3** — Sectioning (`<section>`, `<article>`, `<aside>`,
`<nav>`) and verbatim (`<pre>`, `<style>`, `<script>`, `<textarea>`)
parity. **Negative-space work**: confirm pandoc-native keeps these
as `RawBlock "html"` with no markdown parsing inside, and pin that
behavior in the corpus. No CST shape change expected; verify the
projector emits exactly what pandoc does.

**Phase 4** — Comments, processing instructions, declarations, CDATA
projection. Pin `RawBlock "html"` / `RawInline "html"` for each
case. CST already correct — this is mostly seeding the corpus and
verifying projection output.

**Phase 5** — `markdown_in_html_blocks` interaction edge cases:
nested div-in-div; div-around-list; div-with-blank-lines (Plain vs
Para promotion); block-tag-on-same-line shapes
(`<div>foo</div>`); div-as-list-item-content. The hardest phase —
expect parser-side fixes to support pandoc's recursive parse.

## Key files

### Parser

- `crates/panache-parser/src/parser/blocks/html_blocks.rs` — block
  HTML state machine. `try_parse_html_block_start` recognizes the 7
  CommonMark types; `parse_html_block` walks lines and emits the
  CST. Phase 1 adds a `use_div_kind: bool` parameter that retags
  the wrapper from `HTML_BLOCK` to `HTML_BLOCK_DIV` when a
  matched `<div>...</div>` is recognized under Pandoc.
- `crates/panache-parser/src/parser/block_dispatcher.rs` — calls
  `try_parse_html_block_start` and `parse_html_block`. Phase 1 adds
  the matched-`</div>` pre-scan and passes the result down.
- `crates/panache-parser/src/parser/inlines/inline_html.rs` —
  inline raw HTML. Phase 2's `<span>` lift adds an
  `INLINE_HTML_SPAN` shape gated on Pandoc dialect.
- `crates/panache-parser/src/syntax/kind.rs` — new `SyntaxKind`s.
  Phase 1 adds `HTML_BLOCK_DIV`. Phase 2 adds `INLINE_HTML_SPAN`.

### Projector

- `crates/panache-parser/src/pandoc_ast.rs` — projector. Phase 1
  adds an `HTML_BLOCK_DIV` match arm in `block_from` /
  `collect_block` that walks the structural CST instead of
  re-tokenizing bytes. The legacy `try_div_html_block` byte-level
  re-tokenizer can stay as a fallback for `HTML_BLOCK` (when the
  parser hasn't lifted, e.g. malformed input under Pandoc), or be
  deleted once all matched divs are lifted at parse time.
- `parse_html_attrs` (already in `pandoc_ast.rs`) — reused to extract
  attributes from the open tag's verbatim text.

### Salsa / linter consumers

- `src/salsa.rs` — anchor index. Phase 1 adds a walk for
  `HTML_BLOCK_DIV` that reads the first `HTML_BLOCK_TAG` child,
  extracts attributes via `parse_html_attrs`, and registers the
  `id` in `crossref_declarations`. Phase 2 adds the same for
  `INLINE_HTML_SPAN`.
- `src/linter/rules/undefined_anchor.rs` — consumer of the index.
  No code change expected, just integration tests confirming that
  `<div id>` no longer produces false positives.

### Formatter

- `crates/panache-formatter/src/formatter/core.rs` — has multiple
  match arms keyed on `SyntaxKind::HTML_BLOCK`. Phase 1 needs to
  also accept `HTML_BLOCK_DIV` at each site (text emission is
  identical). Likewise `crates/panache-formatter/src/utils.rs`,
  `formatter/lists.rs`, and `directives.rs` for any HTML_BLOCK
  match.

### Tests / fixtures

- `crates/panache-parser/tests/fixtures/pandoc-conformance/corpus/`
  — corpus, section prefix `html-block` / `html-inline`.
- `crates/panache-parser/tests/pandoc/allowlist.txt` — new
  `# html-block` / `# html-inline` section comments.
- `crates/panache-parser/tests/pandoc/blocked.txt` — deferrals.
- `crates/panache-parser/tests/fixtures/cases/` — paired parser
  golden fixtures (CommonMark vs Pandoc) when CST shape diverges
  by dialect.
- `tests/fixtures/cases/` — formatter golden fixtures when Phase N
  produces a new round-trip behavior. **Required** when CST shape
  changes — `<div>` must format back as `<div>`, not `:::`.

## Losslessness invariant + Phase 1 precedent

The CST must be byte-equal to the input. The right way to expose
HTML attributes structurally is to **tokenize the existing source
bytes at finer granularity**, not to add synthetic tokens. Phase 1
demonstrates the pattern for `<div>`:

The open-tag bytes `<div id="x" class="y">` previously lived in a
single TEXT token. Phase 1 splits them into
`TEXT("<div") + WHITESPACE + HTML_ATTRS{TEXT("id=\"x\" class=\"y\"")}
+ TEXT(">")`. Source bytes are unchanged — the structural node
`HTML_ATTRS` just *groups* the existing attribute bytes so
`AttributeNode::cast` can recognize them. This mirrors how fenced
divs already work (`DIV_INFO` groups the `{#id .class}` bytes after
`:::`).

For Phase 2 (`<span>`), do the same thing inline:
`<span id="x">...</span>` should tokenize the open tag's attribute
region into an `HTML_ATTRS` node so the same `AttributeNode` walk
finds it automatically.

### Phase 1 reality — what landed for `<div>`

Phase 1 ships **two** structural changes, both byte-lossless:

1. **Wrapper retag.** When the parser recognizes `<div ...>`
   opening an HTML block under `Dialect::Pandoc`, the wrapper
   node's `SyntaxKind` is `HTML_BLOCK_DIV` instead of
   `HTML_BLOCK`. One `u16` differs at the wrapper level.
2. **Open-tag tokenization.** Inside the open
   `HTML_BLOCK_TAG`, the bytes `<div ATTRS>` are tokenized at
   finer granularity:
   ```
   HTML_BLOCK_DIV
     HTML_BLOCK_TAG (open)
       TEXT "<div"
       WHITESPACE " "
       HTML_ATTRS                ← structural attribute region
         TEXT "id=\"x\""
       TEXT ">"
       (TEXT trailing-content)?  ← for same-line <div>foo</div>
       NEWLINE
     HTML_BLOCK_CONTENT?         ← middle lines, raw TEXT (NOT
                                    block-parsed at parse time)
     HTML_BLOCK_TAG (close)
       TEXT "</div>"
       NEWLINE
   ```
   The bytes are byte-identical to source — the open-tag TEXT
   is just split at finer granularity.

`AttributeNode::can_cast(HTML_ATTRS)` returns true. The salsa
indexer's existing `for attr in
tree.descendants().filter_map(AttributeNode::cast)` walk picks
up `<div id>` ids automatically — the same walk that handles
fenced-div `DIV_INFO` and heading `ATTRIBUTE`. **No parallel
salsa walk.** No new helper threading; the standard
`AttributeNode::id()` machinery routes by kind to
`parse_html_attribute_list` for HTML attrs vs.
`try_parse_trailing_attributes` for `{...}` syntax.

What Phase 1 still does NOT do:

- **Recursive content parsing.** The bytes inside the div
  (between open and close tags) are still raw TEXT tokens at
  parse time. The pandoc-native projector still calls
  `try_div_html_block` to byte-reparse them as markdown. A real
  structural lift would have the parser produce `PARAGRAPH`,
  `LIST`, etc. as direct children of `HTML_BLOCK_DIV`. That is
  Phase 5 work, alongside depth-aware open/close pairing for
  nested divs.
- **Multi-line open tags.** `<div\n  id="x">` (open tag spanning
  multiple lines) isn't recognized as `HTML_BLOCK_DIV` — the
  parser's existing `try_parse_html_block_start` only inspects
  the first line. Currently falls back to opaque `HTML_BLOCK`.
  Edge case; revisit when corpus exercises it.

So Phase 1 fully answers "where do `<div>` attributes live in
the CST?" — the answer is **a real `HTML_ATTRS` structural
node**, not a parallel byte-parser. It does NOT yet answer
"where do the inner blocks live structurally?" — that's still
opaque TEXT and reparsed on demand by the projector.

Mirror this shape for Phase 2 (`<span>`): retag the existing
`INLINE_HTML` to `INLINE_HTML_SPAN` when a matched `<span>...</span>`
is recognized; do NOT introduce a new structural shape with child
attribute nodes. Same pattern, different kind.

## Failure buckets

Every failing HTML conformance case is one of:

- **Projector gap** — parser produces structural HTML_BLOCK_DIV
  (or HTML_BLOCK for non-divs) but the projector emits the wrong
  pandoc-native shape (missing attribute, wrong block sequencing,
  wrong RawBlock vs Plain split). Fix in `pandoc_ast.rs`.
- **Parser-shape gap** — parser produces opaque `HTML_BLOCK` for a
  construct that should lift, or vice versa. Fix in
  `html_blocks.rs` / `inline_html.rs`. Add a paired parser fixture
  pinning the CST shape under both dialects when behavior
  diverges.
- **Salsa-index gap** — projector and parser are correct but the
  anchor index doesn't see the id. Fix in `src/salsa.rs`.
- **Flavor / extension gap** — pandoc-native enables/disables the
  lift based on an extension (`Ext_native_divs`,
  `Ext_native_spans`, `Ext_markdown_in_html_blocks`); panache's
  default disagrees. Tighten in
  `crates/panache-parser/src/options.rs::pandoc_defaults()`.
- **Genuine missing feature** — pandoc construct not modeled (e.g.
  `<aside>` with `markdown="1"`). Add to `blocked.txt` with reason
  unless the construct is a clear next-phase target.

## Workflow (per session)

1. **Read `RECAP.md`** for current phase, deferred targets, traps.
   If the user named a target, prefer it.

2. **Establish the test baseline**:
   ```
   cargo test -p panache-parser --test pandoc pandoc_allowlist
   cargo test -p panache-parser --test commonmark commonmark_allowlist
   cargo test --workspace --no-fail-fast 2>&1 | grep -E "^test " | grep "FAILED" | sort -u
   ```
   Save the failing-test set; "no regression" means a strict
   subset.

3. **Regenerate the conformance report** (skip if last entry is
   stale by less than 24h):
   ```
   cargo test -p panache-parser --test pandoc pandoc_full_report \
     -- --ignored --nocapture
   ```
   Look at `crates/panache-parser/tests/pandoc/report.txt` for the
   `# html-block` and `# html-inline` slices.

4. **Pick a target**:
   - A handful of failing html-* cases sharing a likely root cause
     (e.g. all div-in-list cases failing → one parser-shape fix
     unlocks several).
   - A small corpus expansion to cover an unmodeled construct, if
     phase warrants.

5. **Probe the case(s)**:
   ```
   pandoc <case>/input.md -f markdown -t native
   pandoc <case>/input.md -f commonmark -t native   # if dialect-divergent
   ```
   For batch triage, drop a throwaway probe test in
   `crates/panache-parser/tests/pandoc.rs` (the same template as
   `pandoc-conformance.md` describes) and **delete it before
   finishing the session**.

6. **Classify into a failure bucket**, then apply the smallest
   fix. Verify the change is parser-side rather than projector-side
   only when downstream consumers (salsa, LSP, formatter) need the
   structure — projector-only fixes are fine when the structure is
   already in the CST.

7. **Add fixtures** before allowlisting:
   - **Parser fixture** (paired CommonMark/Pandoc when behavior
     diverges) under `crates/panache-parser/tests/fixtures/cases/`.
     Required when adding a new CST shape.
   - **Formatter fixture** under `tests/fixtures/cases/` when the
     change produces a new block sequence or different
     idempotency behavior. Pandoc is the default flavor — no
     `panache.toml` needed.

8. **Verify each new corpus case appears in the regenerated
   `report.txt`** before adding to the allowlist:
   ```
   grep -E '^(N1|N2|N3)$' \
     crates/panache-parser/tests/pandoc/report.txt
   ```
   Each id must show up in the passing list. Add under the
   `# html-block` / `# html-inline` section header.

9. **Run guardrails**:
   ```
   cargo test -p panache-parser --test pandoc pandoc_allowlist
   cargo test -p panache-parser --test commonmark commonmark_allowlist
   cargo test -p panache-parser
   cargo test --workspace
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   cargo fmt -- --check
   cargo test -p panache-parser --test pandoc pandoc_full_report \
     -- --ignored --nocapture
   ```
   The CommonMark allowlist must stay green; CST losslessness
   (parser-crate's tree-text-equals-input check) must stay green.

10. **Update `RECAP.md`** with the session outcome: phase touched,
    pass-rate before/after for the html-block / html-inline
    slices, files changed, new traps, ranked next targets.

## Quick reproduce: issue #263 baseline

```
printf '<div id="anchor-c">Content.</div>\n\nSee [link](#anchor-c).\n' \
  | cargo run -- lint /dev/stdin
```

Should report zero `undefined-anchor` diagnostics after Phase 1.

```
printf '<div id="x">**Hi**</div>\n' | cargo run -- parse --to pandoc-ast
```

Should emit `Div ("x", [], []) [Plain [Strong [Str "Hi"]]]` —
byte-identical to `pandoc -f markdown -t native`.

## Dos and don'ts

- **Do** verify with both `pandoc -f markdown -t native` and
  `pandoc -f commonmark -t native` before changing parser
  behavior.
- **Do** treat the existing `try_div_html_block` projector path as
  legacy — Phase 1 makes it redundant for matched divs; later
  phases may delete it once parser-side lifting covers the cases.
- **Do** keep CST losslessness intact. The new `HTML_BLOCK_DIV`
  wrapper contains only the original tag bytes (HTML_BLOCK_TAG
  open, content, HTML_BLOCK_TAG close) — no synthetic attribute
  tokens.
- **Do** add a formatter golden when CST shape changes, to lock
  idempotency. `<div>` parsed → `<div>` formatted. Never `:::`.
- **Don't** broaden the lift to non-`<div>`/`<span>` tags without
  pandoc-native verification. Sectioning and verbatim tags stay
  raw.
- **Don't** edit `expected.native` files by hand. Generate via
  `pandoc -f markdown -t native input.md > expected.native` or
  `scripts/update-pandoc-conformance-corpus.sh`.
- **Don't** add a case to the allowlist without verifying it in
  the freshly regenerated `report.txt`.
- **Don't** treat formatter idempotency divergence as a formatter
  bug without first checking the CST against pandoc-native (see
  `.claude/rules/formatter.md`).
- **Don't** silence a regression by removing an allowlist entry —
  fix the underlying cause.

## Coordination with other long-horizon efforts

- `pandoc-ir-migrate` (Phase 1) treats `<span>...</span>` as a
  `ConstructKind::PandocOpaque` event in
  `crates/panache-parser/src/parser/inlines/inline_ir.rs`. The IR's
  role is purely to keep emphasis from pairing across the span's
  bytes — it does not emit a CST node. Phase 2 of THIS skill
  (`<span>` lift) emits a structural `INLINE_HTML_SPAN` for the
  same byte range; the two are complementary, not conflicting.
  Concretely:
  - The IR's opaque scan stays as-is (don't remove the span
    recognizer).
  - The parser-side lift adds the `INLINE_HTML_SPAN` retag when
    `try_parse_inline_html` matches a balanced span under Pandoc
    dialect — the dispatcher in
    `crates/panache-parser/src/parser/inlines/core.rs` already
    consumes the byte range and emits an `INLINE_HTML` wrapper;
    Phase 2 just retags the wrapper.
  - Confirm before landing: run a paired probe test that
    constructs an emphasis-around-span case (`*foo <span>bar</span>
    baz*`) and check the CST matches pandoc-native — emphasis must
    NOT pair into the span content.

## Known traps (read before debugging)

- **The disk lint cache at `~/.cache/panache/` serves stale
  results.** When you change `src/salsa.rs`, `src/linter/`, or any
  rule output, the CLI may keep emitting the OLD diagnostic from
  cache even after `cargo build`. Symptoms: unit tests pass,
  `panache lint` still flags a fixed case, `eprintln!` from your
  changed code never fires. Fix: `rm -rf ~/.cache/panache/` and
  retry. Always run unit tests for the rule first; only chase
  CLI behavior once those pass.
- **`<div id="x">Content</div>` on one line puts everything
  (including the close tag) in a single `HTML_BLOCK_TAG` token.**
  Naive `strip_suffix('>')` on the open-tag text captures the
  wrong `>` (the close tag's). The
  `parse_html_tag_attributes` helper handles this by scanning to
  the FIRST unquoted `>`. If you write a sibling helper that does
  attribute parsing, follow the same pattern.
- **The HTML-block scanner is depth-unaware.** Nested `<div>`s
  (case `0199-html-block-div-nested`) close the outer block at
  the first inner `</div>` rather than tracking tag depth. This
  is in `tests/pandoc/blocked.txt` and is a Phase 5 target. Don't
  chase it as part of Phase 2/3/4 — it requires changes in
  `parser/blocks/html_blocks.rs` to do a depth-aware pre-scan.
- **The salsa-tracked `symbol_usage_index` query is LRU-cached
  per process** (`#[salsa::tracked(returns(ref), lru = 64)]`). A
  CLI invocation that hits the same cache key reuses the result
  without re-running `symbol_usage_index_from_tree`. Fresh
  process → fresh cache, so CLI calls are usually fine; just
  don't rely on a single test execution to exercise both code
  paths.

## Session recap (`RECAP.md`)

This skill keeps a rolling recap at
`.claude/skills/html-conformance/RECAP.md`. Layout (top → bottom):

1. **Persistent traps & invariants** — cross-session knowledge.
   Read first.
2. **Phase progress** — terse status table. Read second.
3. **Latest session** — detailed entry for the most recent session.
4. **Earlier sessions (compact log)** — one-line entries, newest
   first.

### At session start

Read in this order: Persistent traps → Phase progress → Latest
session's "Suggested next sub-targets". Skim the Earlier sessions
log only if you need to find the session that introduced a specific
behavior; the persistent traps section already holds the still-
relevant knowledge.

### At session end (compaction discipline)

The recap is a **rolling**, not append-only, document. Each session:

1. **Demote the previous "Latest session" entry** to a one-line
   summary at the TOP of the Earlier sessions log: `date —
   phase/sub-target — pass count delta — root cause / lever`.
   Discard the section's prose, file lists, and trap lists.
2. **Fold any still-relevant trap from the demoted session into
   Persistent traps**, deduplicated. A trap is still-relevant if you
   would warn a future session about it; if it was specific to
   that session's pivot or has been superseded, drop it. Persistent
   traps must stay tight — favor merging into existing bullets over
   adding new ones; aim for ≤ ~20 bullets total. Group by sub-heading
   (Disk + tooling, Parser shape & losslessness, Pandoc tag
   categorization, Projector tag splitting, Refs / footnotes /
   heading-id resolution, Out of scope).
3. **Update Phase progress** if a phase status changed.
4. **Write the new Latest session entry** in the format used by the
   current Latest session: pass count line, "What landed" (≤ ~10
   bullets, no full file lists), "Files in committable diff" (≤ ~6
   bullets at directory granularity, not per-file), "Suggested
   next sub-targets" (ranked, ≤ 5), "New trap" (note that they're
   folded into Persistent traps). Keep it terse — judgment calls
   only, not a rerun of the test report.

### Length budget

Target: **RECAP.md ≤ 400 lines.** If your session-end edit pushes
it past 400, take another compaction pass on the Earlier sessions
log (collapse adjacent entries, drop dates older than the current
phase if all session-specific traps have been folded into
Persistent). Don't compact the current session's entry — that's
for the next session to demote.

### If the session ends with regressions

(uncommitted partial diff): the recap MUST say so explicitly and
rank the next sub-target. Do NOT mark the session done if any test
that was green at start is red at end.

## Report-back format

When done, report:

1. Phase + sub-target (e.g. "Phase 1 — `<div>` lift; issue #263
   unblock").
2. html-block / html-inline pass count: before → after.
3. Workspace test count: before → after.
4. Files changed, classified by failure bucket.
5. New corpus cases added (count + section).
6. New parser/formatter fixtures.
7. Suggested next sub-target.
8. Any new trap discovered, captured in `RECAP.md`.

If the session ends without committing: list the remaining red
tests and why.
