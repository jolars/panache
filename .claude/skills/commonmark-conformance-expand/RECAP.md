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

## Latest session — 2026-04-29 (xx)

**Pass count: 627 → 628 / 652 (96.3%, +1)**

Fenced code openers must interrupt a lazy-continuing blockquote
paragraph under CommonMark, in the same way HR already does.
Without that, a fence that appears below `> b` (with no `>` on
the fence line) stayed inside the blockquote's paragraph and got
parsed as an inline code span, rather than closing the blockquote
and opening a `CODE_BLOCK` at the outer level.

### Targets unlocked

- **#321** `- a\n  > b\n  ```\n  c\n  ```\n- d` — list item with
  child blockquote `b`, sibling fenced code block `c`, then
  second list item `d`.

### Root cause

In `crates/panache-parser/src/parser/core.rs`, the
`bq_depth < current_bq_depth` branch (≈ line 1685) handles a
line with fewer `>` markers than the open blockquote stack. When
the last container is a paragraph, it tries lazy continuation,
but only bails out to "close paragraph + close blockquote, then
parse this line as a fresh block" when the line itself is a
paragraph-interrupter. The interrupter check was HR-only:
```rust
let interrupts_via_hr = is_commonmark
    && try_parse_horizontal_rule(line).is_some();
```
A `\`\`\`` line is also a paragraph-interrupter under CommonMark,
but it wasn't in the predicate, so it got lazily appended.

Verified with pandoc: both `-f commonmark` and `-f markdown`
agree that a fence terminates the blockquote paragraph for the
standalone `> b\n\`\`\`...\n\`\`\`` case. Inside the list-item-
containing case (#321), pandoc-markdown additionally never opens
the inner blockquote (lazy paragraph absorbs `> b`), so the bug
only surfaces under CommonMark dialect. The fix is gated on
`Dialect::CommonMark` to match the existing HR pattern (HR
genuinely diverges between dialects — `> b\n---` is setext h2
under markdown, blockquote + HR under commonmark).

### Fix

- `crates/panache-parser/src/parser/core.rs`: extend the
  interrupter predicate in the lazy-continuation branch to also
  detect a fence opener (uses `code_blocks::try_parse_fence_open`,
  which already handles up-to-3 leading spaces). Renamed the
  bail-out condition from `!interrupts_via_hr` to
  `!interrupts_via_hr && !interrupts_via_fence`.

### Files changed

- **Dialect divergence** (parser):
  - `crates/panache-parser/src/parser/core.rs` — fence opener now
    also breaks lazy paragraph continuation across reduced
    blockquote depth, gated on `Dialect::CommonMark`.
- **Parser fixture** (CommonMark only):
  - `fence_interrupts_blockquote_paragraph_commonmark/{input.md,
    parser-options.toml}` pins the `BLOCK_QUOTE > PARAGRAPH "b"`
    + sibling `CODE_BLOCK` shape. Wired into
    `golden_parser_cases.rs`. Snapshot accepted via
    `INSTA_UPDATE=always`.
- **Allowlist additions** (Lists section): #321.

No formatter golden case added: the new CommonMark-only block
sequence (BLOCK_QUOTE then CODE_BLOCK siblings) is structurally
identical to the existing Pandoc shape for `> b\n\`\`\`...\`\`\``
once you give it a blank line before the fence (which Pandoc
already requires); existing top-level fixtures cover the
formatted-output and idempotency checks. No paired Pandoc parser
fixture either: under Pandoc the input doesn't even open the
inner blockquote (paragraph absorbs `> b`), so a paired fixture
would just be the absorbed-paragraph shape — not a meaningful
contrast.

### Don't redo

- **Don't widen the interrupter predicate to ATX, list, or
  setext underlines without dialect gating and paired pandoc
  verification.** Each interrupts paragraphs under CommonMark
  but not under Pandoc-markdown:
  - ATX: `> b\n# h` — pandoc-markdown lazily appends `# h` to
    the bq paragraph as text; commonmark closes bq, opens `<h1>`.
  - List: `> b\n- d` — pandoc-markdown lazily appends; commonmark
    closes bq, opens list.
  - HR: already handled (gated).
  Any expansion needs `Dialect::CommonMark` gating + a paired
  parser fixture per construct, and a pandoc-native verify.
- **Don't move `try_parse_fence_open` out of `code_blocks.rs`.**
  It's a `pub(crate)` block-detection helper; the import in
  `core.rs` is via the existing `super::blocks::code_blocks`
  alias. No need to lift it to `utils/`.
- **Don't generalize `try_parse_fence_open` to handle indents
  ≥ 4 spaces just because list items have content_col.** The
  existing logic uses raw `line` for HR/fence checks — fine for
  list items with content_col ≤ 3 (the line's leading-space count
  matches the relative indent). For content_col ≥ 4 the same
  pre-existing limitation already applies to HR; that's a wider
  refactor (column-aware interrupter checks) and not unlocked
  by any current failing example.

### Suggested next targets, ranked

1. **#6 (`>\t\tfoo`) and #7 (`-\t\tfoo`)** — column-aware
   marker-utility refactors. (Carried; see session xix's
   "Don't redo" note for scope.)
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
   the list item. (Carried over.)
8. **#273, #274 multi-block content in `1.     code` items** —
   spaces ≥ 5 after marker means content_col is at marker+1 and
   the rest is indented code. (Carried over; likely shares helper
   with the #6/#7 column-aware refactor.)
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
  `consume_leading_cols` in the renderer) does not yet extend to
  blockquote-marker post-space consumption or list-item
  content-col derivation. #6/#7 still need that wider refactor.
