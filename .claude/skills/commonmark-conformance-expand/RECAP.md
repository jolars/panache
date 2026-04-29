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

## Latest session — 2026-04-29 (xxiii)

**Pass count: 631 → 632 / 652 (96.9%, +1)**

Implemented the renderer-only fix sketched at the end of
session (xxii)'s recap: split `code_block_content`'s per-line
walk so the *first* line of a CODE_BLOCK that opens on the
list-item marker line walks from `start_col = li_indent`,
while continuation lines stay at `start_col = 0`. The CST
already had the right shape from session (xxii)'s parser work;
only the renderer was producing the wrong leading whitespace
for "≥ 5 spaces with leftover" variants.

### Targets unlocked

- **#274** `1.      indented code\n\n   paragraph\n\n       more code`
  — ordered marker + 6 spaces. Rule #2 fires; 1 space consumed
  as marker space (literal WHITESPACE token between MARKER and
  CODE_BLOCK), 5 spaces remain in CODE_CONTENT's leading
  WHITESPACE. li_indent=3, target_strip=7. With start_col=3 for
  line 0, walker advances 4 cols (consuming 4 of the 5 spaces),
  reaches target, emits `" indented code"` with the expected 1
  leading space. The continuation paragraph and the second
  CODE_BLOCK (`more code`, line at col 0 with 7 leading spaces)
  are unaffected because they are not the first block in the
  LIST_ITEM (paragraph in between) — `code_block_is_first_block_in_list_item`
  returns false for the second CODE_BLOCK.

### Root cause + fix

Pure renderer change in `tests/commonmark/html_renderer.rs`:

1. **`code_block_is_first_block_in_list_item(node)`** helper:
   walks `node.parent()`'s `children_with_tokens()` skipping
   LIST_MARKER and WHITESPACE tokens; returns true iff the first
   non-trivia child node equals `node`. This identifies the
   marker-line indented-code case produced by the parser's
   `maybe_open_indented_code_in_new_list_item` hook.
2. **Per-line `start_col` split** in the indented-code branch of
   `code_block_content`: switched the per-line loop to enumerate
   `raw_lines`, computed `li_first_line_shift = li_indent` when
   `idx == 0 && first_line_is_marker_line` else 0, and passed
   `start_col = bq_start_col + li_first_line_shift` into
   `consume_leading_cols_from`. `strip_cols` is unchanged
   (`bq_depth * 2 + li_indent + 4`).

Verified `code_block_is_first_block_in_list_item` requires the
PRESENCE of WHITESPACE between MARKER and CODE_BLOCK in the #274
case (which the parser does emit because `1.` was followed by a
literal space marker-space). For the #7 case (`-\t\tfoo`, no
literal space — entire tab stays in content), there is no
WHITESPACE token between MARKER and CODE_BLOCK, but the helper
still returns true (it skips both LIST_MARKER and WHITESPACE).
The existing #7 path was passing pre-session via the renderer's
`list_item_content_column` virtual-marker-space addition; this
session's change preserves that — for #7 li_indent=2,
target=6, body has `\t\tfoo` from col 0+li_indent=2; first tab
takes col 2→4, second tab col 4→8 (next_stop=8 > target=6,
slack=2, byte_idx=2)... wait that would give 2 spaces of slack,
prepending "  foo". The expected output is `  foo` (2 leading
spaces, since the spec strips 4 cols of indent from a tab that
expands to 8 cols). Let me re-verify against the test... (the
allowlist still includes #7 and the suite passes, so the math
works out — for #7 the previous start_col=0 + li_indent=2 (with
virtual) lined up such that walker reached target with slack=2;
with this session's change, start_col becomes
bq_start_col + li_indent = 0 + 2 = 2, then the first tab
expands col 2→4, second tab col 4→8 (next_stop > target=6,
slack=2). Same slack, same output. Compatible.)

### Files changed

- **Renderer gap** (test-only):
  - `tests/commonmark/html_renderer.rs` — added
    `code_block_is_first_block_in_list_item` helper and
    `li_first_line_shift` per-line logic in `code_block_content`.
- **Parser fixture** (CommonMark only):
  - `list_item_indented_code_marker_line_partial_overflow/{input.md,
    parser-options.toml}` pins the multi-block LIST_ITEM CST
    shape for #274 (CODE_BLOCK marker-line + PLAIN paragraph +
    CODE_BLOCK continuation at deeper indent). Wired in
    `golden_parser_cases.rs`. Snapshot accepted via
    `INSTA_UPDATE=always`.
- **Allowlist additions**: #274 (List items).

No formatter golden case: this is a renderer-only fix consuming
a CST shape the parser was already producing correctly. No new
structural shape was introduced.

### Don't redo

- **Don't move the `li_first_line_shift` logic into
  `consume_leading_cols_from`.** The walker is generic and
  reused for the `bq_depth > 0` and continuation-line paths.
  Keep the marker-line context (which depends on the parent
  LIST_ITEM's child layout) in the caller.
- **Don't extend `code_block_is_first_block_in_list_item` to
  recurse through wrapper nodes.** It looks at `node.parent()`
  only (not ancestors). A CODE_BLOCK nested inside a
  blockquote-inside-list-item is not the marker-line case — the
  blockquote breaks the chain. The current direct-parent check
  is the right invariant.
- **Don't try to detect "marker line" by inspecting CODE_CONTENT
  text.** The parser's `maybe_open_indented_code_in_new_list_item`
  hook is what produces the special CST layout (CODE_BLOCK as
  first block in LIST_ITEM with leading WHITESPACE in
  CODE_CONTENT). The structural test on the CST is the
  load-bearing signal, not the bytes.
- **Don't try to fix #278 by extending the marker-line hook to
  the empty-marker case (`-\n  foo`).** That's a different
  parser code path (marker followed by blank then indented
  content); see §"Why #278 still fails" notes carried from
  prior sessions. The marker-line indented-code hook is for
  same-line content only.

### Suggested next targets, ranked

1. **Proper delimiter-stack for emphasis (#402, #408, #412, #417,
   #426, #445, #457, #464, #465, #466, #468)** — rewrite emphasis
   to use CMark's process_emphasis algorithm. Largest single fix;
   substantial; gate on `Dialect::CommonMark`. (Carried.)
2. **#472 `*foo *bar baz*`** — CommonMark expects `*foo <em>bar
   baz</em>`. Likely needs delimiter-stack work. (Carried.)
3. **Reference-link nesting (#569, #571)** — `[foo][bar][baz]`
   with only `[baz]` defined should parse as `[foo]` + ref-link
   `[bar][baz]`. CMark left-bracket scanner stack with
   refdef-aware resolution. Probably the next big link cluster.
   (Carried; #533 stays in this group.)
4. **#533** — inline link with emphasis closing inside the
   bracket text plus a trailing reference link. (Carried;
   companion to #569/#571.)
5. **Formatter fix for nested-only outer LIST_ITEM** — carried
   over. Unblocks removing the dialect gate on same-line nested
   list markers (#298, #299).
6. **Same-line blockquote inside list item (#292, #293)** — `> 1.
   > Blockquote` needs the inner `>` to open a blockquote inside
   the list item. Both pandoc dialects agree (verified — not a
   dialect divergence) — fix in `lists.rs`
   `finish_list_item_with_optional_nested` to also handle `>`
   markers. (Carried over.)
7. **#278 `-\n  foo\n-\n  ```\n…`** — empty marker followed by
   indented content; multiple bugs. (Carried over.)
8. **#300 setext-in-list-item** — `- # Foo\n- Bar\n  ---\n  baz`
    should treat `Bar\n  ---` as setext h2. (Carried over.)
9. **#523 `*foo [bar* baz]`** — emphasis closes inside link
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
