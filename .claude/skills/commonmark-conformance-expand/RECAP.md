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

## Latest session — 2026-04-29 (xxii)

**Pass count: 629 → 631 / 652 (96.8%, +2)**

CommonMark §5.2 rule #2: when a list marker is followed by ≥ 5
columns of post-marker whitespace and non-empty content, the
list item's content column is `marker + 1` (the marker plus
exactly one — possibly virtual — space). The surplus whitespace
becomes part of the content, typically forming an indented code
block on the marker line itself. Both `try_parse_list_marker`
and the renderer's `list_item_content_column` were unaware of
this rule, so `1.     indented code` (#273) buffered the wrong
content_col, and `-\t\tfoo` (#7) didn't even produce a CODE_BLOCK
inside the LIST_ITEM.

### Targets unlocked

- **#7** `-\t\tfoo` — bullet + 2 tabs. Total 7 cols of post-
  marker whitespace from col 1 → rule fires; spaces_after_bytes
  collapses to 0 (the entire tab1 stays in content) and the
  marker space is virtually absorbed from tab1.
- **#273** `1.     indented code` — ordered marker + 5 spaces.
  Rule fires; 1 space consumed as marker space, 4 spaces remain
  in content as the indented-code prefix.

### Root cause + fix

Three coordinated changes, parser-shape + renderer:

1. **Col-aware indent computation**
   (`parser/utils/container_stack.rs`):
   - `leading_indent` was hardwired to start at col 0. Added
     `leading_indent_from(line, start_col)` that seeds tab
     expansion from a given source column. `leading_indent` now
     delegates to `leading_indent_from(_, 0)`.
2. **Marker rule + propagation**
   (`parser/blocks/lists.rs`,
   `parser/block_dispatcher.rs`,
   `parser/core.rs`):
   - New `marker_spaces_after(after_marker, marker_end_col)`
     helper computes effective cols col-aware. If the post-
     marker WS is ≥ 5 cols and content is non-empty, returns
     `(spaces_after_cols=1, spaces_after_bytes, virtual=true/false)`
     — bytes is 1 if next byte is a literal space (consumed as
     marker space), 0 if it's a tab whose source-col span > 1
     (the whole tab stays in content, marker space is virtual).
   - `ListMarkerMatch` / `ListPrepared` /
     `ListItemEmissionInput` / `Container::ListItem` all gained
     a `virtual_marker_space: bool` field threaded through.
   - The bullet, ordered-decimal, fancy-list, hash, example, and
     parens-style branches in `try_parse_list_marker` all call
     `marker_spaces_after(after_marker, _indent_cols + marker_len)`
     instead of `leading_indent(after_marker)`.
   - UpperAlpha `A.` keeps its 2-space minimum gate but now uses
     `leading_indent_from` for the gate so tab-after-`A.` cases
     count cols correctly. Rule application happens after the
     gate passes.
3. **Indented-code-from-marker-line emission**
   (`parser/core.rs:`
   `maybe_open_indented_code_in_new_list_item`):
   - New helper called after every `add_list_item` /
     `add_list_item_with_nested_empty_list` /
     `start_nested_list` site (3 call sites, mirroring
     `maybe_open_fenced_code_in_new_list_item`).
   - Inspects the just-pushed `Container::ListItem`'s buffer.
     If it has exactly one text segment whose leading whitespace
     reaches `content_col + 4` source cols (col-aware from
     `content_col - virtual_marker_space`), clears the buffer
     and emits `CODE_BLOCK > CODE_CONTENT > WHITESPACE + TEXT +
     NEWLINE` directly into the LIST_ITEM. Single-line only;
     multi-line indented-code on continuation lines is handled
     by the normal block-detection path.
4. **Renderer virtual marker space**
   (`tests/commonmark/html_renderer.rs`):
   - `list_item_content_column` now tracks `chars_after_marker`
     and adds 1 col when `saw_marker && chars_after_marker == 0`,
     so a LIST_ITEM with `LIST_MARKER` followed directly by a
     CODE_BLOCK (no WHITESPACE token between them) reports the
     logical content_col instead of just the literal marker
     width. This makes `li_indent + 4` correctly equal to
     `target_strip_cols` for the indented-code walker, and the
     col-walking from start_col=0 lines up with the source-col
     tab-stops.

### Files changed

- **Parser-shape gap**:
  - `parser/utils/container_stack.rs` — `leading_indent_from`
    helper + `virtual_marker_space` field on `Container::ListItem`.
  - `parser/blocks/lists.rs` — `marker_spaces_after` helper;
    `virtual_marker_space` threaded through `ListMarkerMatch`,
    `ListItemEmissionInput`, all `Container::ListItem` push
    sites, `add_list_item`, `add_list_item_with_nested_empty_list`,
    `start_nested_list`, `finish_list_item_with_optional_nested`.
  - `parser/block_dispatcher.rs` — `virtual_marker_space` on
    `ListPrepared` + `ListParser::detect_prepared`.
  - `parser/core.rs` — `maybe_open_indented_code_in_new_list_item`
    helper + 3 call sites; `ListItemEmissionInput` constructions
    pass `virtual_marker_space` through (6 sites).
- **Renderer gap** (test-only):
  - `tests/commonmark/html_renderer.rs` —
    `list_item_content_column` virtual-marker-space awareness.
- **Parser fixture** (CommonMark only):
  - `list_item_indented_code_tabs_commonmark/{input.md,
    parser-options.toml}` pins `LIST_ITEM > LIST_MARKER + CODE_BLOCK
    > CODE_CONTENT > WHITESPACE "\t\t" + TEXT "foo"` for
    `-\t\tfoo`. Wired in `golden_parser_cases.rs`. Snapshot
    accepted via `INSTA_UPDATE=always`.
- **Allowlist additions**: #7 (Tabs), #273 (List items).

No formatter golden case for #7: pandoc agrees with the
expected output across both `commonmark` and `markdown` flavors
(`-\t\tfoo` → `BulletList [[CodeBlock "  foo"]]` in both), and
the existing top-level Pandoc fixtures already cover bullet +
indented-code idempotency.

### Don't redo

- **Don't add a paired Pandoc parser fixture for #7's CST shape.**
  Both dialects produce the same `LIST_ITEM > CODE_BLOCK` shape
  for `-\t\tfoo` after the parser fix; the existing Pandoc list
  fixtures cover the structural pattern. The fixture is
  CommonMark-flavor for symmetry with the harness, not for
  dialect-divergence.
- **Don't widen `maybe_open_indented_code_in_new_list_item` to
  multi-line buffers.** The current single-line gate
  (`iter.next().is_some() → return`) is intentional. Multi-line
  same-line-then-continuation cases (e.g.
  `1.     code1\n       code2` if such a panache code path
  arises) need the renderer to walk first lines from
  `start_col=content_col-virtual` and continuation lines from
  `start_col=0` — those are *different start_cols within the
  same CODE_CONTENT*. That's a renderer refactor, not a parser
  one. Don't try to handle it in the parser hook.
- **Don't gate `marker_spaces_after` on `Dialect::CommonMark`.**
  Verified pandoc-markdown agrees with CommonMark on rule #2 for
  `-\t\tfoo` and `1.     code` — both flavors should apply the
  rule. Adding a dialect gate would re-break the Pandoc-flavor
  cases that currently rely on the same logic.
- **Don't change `consume_leading_cols_from` start_col handling
  for list items.** The walker still uses `start_col=0` for
  list-item-only (no bq) indented-code; the virtual-marker-space
  fix lives entirely in `list_item_content_column`'s logical
  return value. Adjusting the walker's start_col on top of this
  would double-count.
- **Don't try `start_col = li_indent` for the renderer walker.**
  It happens to work for #7 (tab alignment) but breaks
  continuation-line cases like `- foo\n\n      bar` where
  start_col=0 is correct. The current logic (start_col=0 + li_indent
  inflated by virtual) is the right invariant.

### Why #274 still fails (and won't fall out of #273's fix)

`1.      indented code` (6 spaces) expects ` indented code` (1
leading space). After my parser changes, content_col=3 and the
buffer text has 5 leading spaces (1 was consumed as marker
space). The renderer's indented-code walker uses `start_col=0`
with `target=li_indent+4=7`. Walking 5 spaces from col 0 reaches
col 5, target 7 not reached, so the walker returns
`(byte_idx=5, slack=0)` and outputs `line[5..]="indented code"`
without the expected leading space. To fix this, the renderer
would need to walk the FIRST line of a CODE_CONTENT (when
CODE_BLOCK is the first non-MARKER non-WHITESPACE child of
LIST_ITEM) from `start_col=content_col`, while continuation
lines stay at `start_col=0`. That's a per-line context split
inside `code_block_content`. Concretely:

```rust
let li_indent = enclosing_list_item_content_column(node);
let first_line_is_marker_line =
    code_block_is_first_block_in_list_item(node);
for (idx, line) in raw_lines.into_iter().enumerate() {
    let start_col = if idx == 0 && first_line_is_marker_line {
        li_indent  // walk from content_col (with virtual already
                   // baked into li_indent if relevant)
    } else {
        0  // continuation line, body at col 0 of its own line
    };
    let strip_cols = bq_depth * 2 + li_indent + 4;
    // ... walk + emit
}
```

This is the **suggested next target** (and naturally handles
#274 plus any other "≥ 5 spaces, content > 4 cols of WS"
variants).

### Suggested next targets, ranked

1. **#274 first-line-vs-continuation start_col split** — see
   sketch above. Should also clean up any latent issues with
   future "≥ 5 cols" variants. Renderer-only fix; parser CST is
   already correct.
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
   markers. (Carried over.)
8. **#278 `-\n  foo\n-\n  ```\n…`** — empty marker followed by
   indented content; multiple bugs. (Carried over.)
9. **#300 setext-in-list-item** — `- # Foo\n- Bar\n  ---\n  baz`
    should treat `Bar\n  ---` as setext h2. (Carried over.)
10. **#523 `*foo [bar* baz]`** — emphasis closes inside link
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
  `consume_leading_cols_from(body, start_col, target)`. Session
  (xxii) closed the parser-side gap: list-item markers now
  apply CommonMark §5.2 rule #2 via `marker_spaces_after`, with
  a parallel `virtual_marker_space` flag mirroring the bq
  `virtual_absorbed` bookkeeping. The bq and list-item virtual
  spaces are *separate* concepts — don't try to unify their
  fields. They contribute additively in the renderer's
  `code_block_content` when both are present (rare; not yet
  exercised by a passing example).
