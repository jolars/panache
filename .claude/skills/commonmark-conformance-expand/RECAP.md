# CommonMark conformance — running session recap

This file is the rolling, terse handoff between sessions of the
`commonmark-conformance-expand` skill. Read it at the start of a session for
suggested next targets and known follow-ups; rewrite the **Latest session**
entry at the end with what changed and what to look at next. Remove and replace
the "Latest session" entry with a new one at the end of each session, but 
check if there is something from the prior session that should be
carried forward.

Keep entries short. The full triage data lives in
`crates/panache-parser/tests/commonmark/report.txt` and
`docs/development/commonmark-report.json`; this file is for the *judgment calls*
a fresh session can't reconstruct from those artifacts (why a target was picked,
what was deliberately skipped, which fix unlocked which group).

--------------------------------------------------------------------------------

## Latest session — 2026-04-29 (xxi)

**Pass count: 628 → 629 / 652 (96.5%, +1)**

Tab-expansion in indented code blocks inside a blockquote was
not honoring source-column tab-stops. The HTML renderer in
`tests/commonmark/html_renderer.rs` stripped the `>` byte and
then ran `consume_leading_cols` from `col=0`, which treats
each tab as a fresh 4-col expansion. Under CommonMark, a `>`
followed by a tab consumes 2 source cols (the marker plus a
virtual space); leading body tabs need to honor tab-stops as
if they started at col `2*bq_depth`, not col 0.

### Targets unlocked

- **#6** `>\t\tfoo` — blockquote with indented code block
  whose content should be `  foo` (2 leading spaces, not a
  tab) because the second tab spans cols 4–7 and is bisected
  by the indent boundary at col 6.

### Root cause

In `code_block_content` (renderer), the indented-code path:
```rust
let strip_cols = li_indent + 4;
let (...) = consume_leading_cols(body, strip_cols);
```
counted tab expansions in *body-relative* coords (start col 0)
even when `bq_depth > 0`. cmark counts in source-relative coords:
- `>\t\tfoo` → tab1 at col 1 → tabstop 4 (3 cols), tab2 at col 4
  → tabstop 8 (4 cols). Bq consumes col 0 (`>`) + virtual col 1
  (so 2 source cols total, half of tab1). Indent boundary at
  col 6 bisects tab2 (cols 4–7), leaving 2 cols of slack.
- `> >\t\tfoo` → outer strips `> ` (2 bytes, no virtual
  absorption); inner strips `>` only (1 byte) and the inner bq's
  virtual space absorbs *all* of tab1 (which only spans col 3).
  So tab2 in the body has its full 4-col expansion, fully
  absorbed by the indent threshold of 8.

The body byte for tab1 in both cases is identical, but its
effective remaining cols differ. The fix is to start the col
walker at `2*bq_depth - virtual_absorbed`, where
`virtual_absorbed` = number of bq levels per line whose strip
ate the `>` byte without a literal trailing space (and thus
absorbed 1 virtual col from the next char).

### Fix (renderer-only)

- `crates/panache-parser/tests/commonmark/html_renderer.rs`:
  - New `strip_blockquote_prefix_per_line_with_offsets` returns
    per-line `(stripped_line, virtual_absorbed)` tuples.
  - New `strip_blockquote_prefix_with_offset` is the inner
    helper; it mirrors `strip_blockquote_prefix` but also
    reports `virtual_abs += 1` for each level that didn't
    consume a trailing space byte.
  - `consume_leading_cols` was rewritten as a thin wrapper over
    a new `consume_leading_cols_from(body, start_col, target)`
    that lets callers seed the col tracker at a non-zero
    starting column. Then dropped — only the `_from` variant is
    actually used.
  - `code_block_content` (indented branch) now computes
    `bq_start_col = bq_depth * 2 - virtual_absorbed` and
    `strip_cols = bq_depth * 2 + li_indent + 4`, passing both
    to `consume_leading_cols_from`. For `bq_depth=0`,
    `virtual_absorbed` is 0 and behavior is unchanged.

No parser change. Parser already produces the right
`BLOCK_QUOTE > CODE_BLOCK > CODE_CONTENT > WHITESPACE "\t\t" +
TEXT "foo"` shape for `>\t\tfoo` under CommonMark.

### Files changed

- **Renderer gap** (test-only):
  - `crates/panache-parser/tests/commonmark/html_renderer.rs`
- **Parser fixture** (CommonMark only — pinning the CST shape
  the renderer fix depends on):
  - `blockquote_indented_code_tabs_commonmark/{input.md,
    parser-options.toml}` pins `BLOCK_QUOTE > CODE_BLOCK >
    CODE_CONTENT > WHITESPACE "\t\t" + TEXT "foo"`. Wired into
    `golden_parser_cases.rs`. Snapshot accepted via
    `INSTA_UPDATE=always`.
- **Allowlist additions** (Tabs section): #6.

No formatter golden case: the parser CST shape is identical
under both dialects (verified — pandoc agrees on both
commonmark and markdown), and existing top-level Pandoc
fixtures already cover blockquote + indented code formatting
and idempotency.

### Don't redo

- **Don't push the `start_col` shift into `consume_leading_cols`
  unconditionally.** It only makes sense when the body has been
  pre-stripped of bytes whose source-cols still influence
  tab-stop alignment. Outside of bq prefixes, callers should
  pass `start_col=0` (which the indent-only path implicitly does
  via the dropped wrapper, now inlined as `start_col=0`).
- **Don't apply the same fix mechanically to fenced code blocks.**
  Fenced code uses `opener_indent` stripping, not
  `consume_leading_cols`. The bq depth is already handled by
  `strip_blockquote_prefix_per_line` (byte-strip is enough
  there because fenced content lines preserve the content_col
  literally, not via tab-stop math).
- **Don't try to reuse the renderer's `virtual_absorbed`
  bookkeeping for #7 (`-\t\tfoo`).** #7 is a *parser-shape*
  gap: the parser doesn't even produce a `CODE_BLOCK` for
  list-item-marker + tabs (CST has `LIST_ITEM > PLAIN > TEXT
  "foo"`). The fix lives in `parser/blocks/lists.rs` /
  marker_utils column-aware logic, not in the renderer.

### Suggested next targets, ranked

1. **#7 `-\t\tfoo`** — parser-shape gap. List-item marker `-`
   followed by 2 tabs should produce a `CODE_BLOCK` inside the
   `LIST_ITEM`, but currently produces a plain text item. Both
   pandoc dialects agree on the expected shape (verified). Fix
   needs column-aware list-item content_col derivation in
   `marker_utils`/`lists.rs`. Possibly shares structure with
   the still-deferred #6/#7 column-aware refactor mentioned in
   prior recaps for blockquote markers (now partially addressed
   for the renderer, still open for the parser).
2. **Proper delimiter-stack for emphasis (#402, #408, #412, #417,
   #426, #445, #457, #464, #465, #466, #468)** — rewrite emphasis
   to use CMark's process_emphasis algorithm. Largest single fix;
   substantial; gate on `Dialect::CommonMark`. (Carried.)
3. **#472 `*foo *bar baz*`** — CommonMark expects `*foo <em>bar
   baz</em>`. Likely needs delimiter-stack work. (Carried.)
4. **Reference-link nesting (#569, #571)** — `[foo][bar][baz]`
   with only `[baz]` defined should parse as `[foo]` + ref-link
   `[bar][baz]`. CMark left-bracket scanner stack with
   refdef-aware resolution. Probably the next big link cluster.
   (Carried; #533 stays in this group.)
5. **#533** — inline link with emphasis closing inside the
   bracket text plus a trailing reference link. (Carried;
   companion to #569/#571.)
6. **Formatter fix for nested-only outer LIST_ITEM** — carried
   over. Unblocks removing the dialect gate on same-line nested
   list markers (#298, #299).
7. **Same-line blockquote inside list item (#292, #293)** — `> 1.
   > Blockquote` needs the inner `>` to open a blockquote inside
   the list item. Both pandoc dialects agree (verified — not a
   dialect divergence) — fix in `lists.rs`
   `finish_list_item_with_optional_nested` to also handle `>`
   markers, but the inner blockquote needs a real container push
   (not just a buffer rewrite), and the formatter likely needs
   work to roundtrip `LIST_ITEM > BLOCK_QUOTE` (no PLAIN sibling)
   without breaking idempotency. (Carried over, scope expanded.)
8. **#273, #274 multi-block content in `1.     code` items** —
   spaces ≥ 5 after marker means content_col is at marker+1 and
   the rest is indented code. (Carried over; likely shares helper
   with the #7 column-aware list-item parser refactor.)
9. **#278 `-\n  foo\n-\n  ```\n…`** — empty marker followed by
   indented content; multiple bugs. (Carried over.)
10. **#300 setext-in-list-item** — `- # Foo\n- Bar\n  ---\n  baz`
    should treat `Bar\n  ---` as setext h2. (Carried over.)
11. **#523 `*foo [bar* baz]`** — emphasis closes inside link
    bracket text mid-flight. Likely needs delimiter-stack work +
    bracket scanner integration. (Carried over.)

### Carry-forward from prior sessions

- The "Don't redo" notes from session (ix) about emphasis split-runs
  ("tail-end only" heuristic, dialect gate, not widening to
  #402/#408/#412/#426) still apply. Don't touch those paths without
  first reading session (ix)'s recap.
- The session (x) / (xi) link-scanner skip pattern (autolink/raw-HTML
  opacity for emphasis closer + link bracket close) is load-bearing
  for #524/#526/#536/#538. Don't unify the autolink and raw-HTML
  skip flags — Pandoc treats them differently.
- Session (xii)'s lazy paragraph continuation across reduced
  blockquote depth (`bq_depth < current_bq_depth` branch) is the
  same site session (xx) extended for fence interrupts. Don't
  conflate it with session (xiii)'s list-marker lazy continuation
  and the bq_depth=0 list-continuation gate — the three paths
  share the "lazy continuation" name but operate on different
  state. Don't try to unify them.
- Session (xiii)'s `try_lazy_list_continuation` only fires for
  `BlockEffect::OpenList` with `indent_cols ≥ 4`. Other
  interrupting-block effects (HR, ATX, fence) at deep indent
  inside lists go through the existing CommonMark §5.2 close path
  at `core.rs:2447+` (`close_lists_above_indent`); don't touch
  that path on its account.
- Session (xvii)'s HTML block #148 fix (`</pre>` rejection in the
  VERBATIM_TAGS branch) remains active under CommonMark and is
  Pandoc-unreachable via `extract_block_tag_name(_, false)`.
- Session (xviii)'s `disallow_inner_links` flag and
  `link_text_contains_inner_link` helper are scoped to inline
  links only. Reference-link nesting (#569/#571 + #533) needs a
  different pass with refdef resolution; do not retrofit the
  helper.
- Session (xix)'s column-aware indented-code logic
  (`is_indented_code_line` via `leading_indent`,
  `consume_leading_cols` in the renderer) was extended in (xxi)
  to handle blockquote tab-expansion via
  `consume_leading_cols_from(body, start_col, target)`. The
  parser-side counterpart for list-item content-col derivation
  (#7) is still open — don't reuse the renderer's
  `virtual_absorbed` bookkeeping there; the parser fix needs
  column-aware marker handling, not a post-strip renderer
  adjustment.
