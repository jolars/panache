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

## Latest session — 2026-04-29 (xix)

**Pass count: 625 → 627 / 652 (96.2%, +2)**

Mixed tab/space indented code: a line whose leading whitespace
expands (column-wise, tabstop = 4) to ≥ 4 cols is an indented code
block, even if the column-4 boundary lands on a tab — e.g. `  \tfoo`
(2 spaces + tab) is indented code, not paragraph. The renderer's
indent-stripping was also byte-level; column-aware stripping is
required so that a tab partially consumed by the strip emits its
remaining cols as virtual spaces in the rendered content.

### Targets unlocked

- **#2** `  \tfoo\tbaz\t\tbim` → indented code with content
  `foo\tbaz\t\tbim` (interior tabs preserved verbatim).
- **#5** `- foo\n\n\t\tbar` → loose list item with `<p>foo</p>`
  + indented code containing `  bar` (li_indent=2 + 4-col strip
  = 6 cols total; second tab contributes 2 cols of slack as
  virtual spaces).

### Root cause

Two byte-level checks broke column-aware indent semantics:

1. `is_indented_code_line` in
   `crates/panache-parser/src/parser/blocks/indented_code.rs`
   counted only pure-space prefixes (or a leading `\t`), so a line
   like `  \t…` (2 sp + tab = 4 cols) returned false and the
   dispatcher fell through to paragraph. With column-aware
   accounting it returns true, parsing it as `CODE_BLOCK`.
2. The conformance harness's renderer at
   `crates/panache-parser/tests/commonmark/html_renderer.rs`
   stripped up to 4 leading **bytes** (or one leading tab) per
   line of indented-code content. For the list-item case
   (`li_indent=2` + 4-col indent marker = 6 cols total), this
   missed both the partial-tab consumption and the slack-as-
   virtual-spaces emission CMark requires.

Both flavors agree on the construct (verified with
`pandoc -f commonmark -t native` and `-f markdown -t native`),
so the parser change is dialect-neutral — no `Dialect::CommonMark`
gate, no paired Pandoc fixture.

### Fix

- `crates/panache-parser/src/parser/blocks/indented_code.rs`:
  rewrote `is_indented_code_line` to call `leading_indent` and
  return `cols >= 4` (column-aware via the existing helper in
  `parser/utils/container_stack.rs`).
- `crates/panache-parser/tests/commonmark/html_renderer.rs`:
  - New `consume_leading_cols(body, target_cols)` helper that
    advances byte-by-byte through ` ` / `\t`, advancing `col`
    against tabstop = 4. If a tab would push past `target_cols`,
    it stops and returns `(byte_idx + 1, slack, true)` where
    `slack = next_stop - target_cols`.
  - Replaced the indented-code branch's separate `li_indent`
    + 4-col passes with a single `strip_cols = li_indent + 4`
    pass using the helper. The slack (if any) is prepended to
    the line as literal spaces before the byte tail. Whitespace-
    only lines that don't reach the strip target collapse to
    just the newline (preserving the prior blank-line rule).
  - Old `strip_leading_spaces_per_line` retained — still used
    by the fenced-code branch and HTML blocks, where it's
    correct (those paths only see spaces).

### Files changed

- **Parser-shape gap** (dialect-neutral):
  - `crates/panache-parser/src/parser/blocks/indented_code.rs`
    (`is_indented_code_line` made column-aware).
- **Renderer gap** (test code only):
  - `crates/panache-parser/tests/commonmark/html_renderer.rs`
    (new `consume_leading_cols` helper, indented-code branch
    rewritten to single column-aware pass).
- **Parser fixture** (single, dialect-neutral):
  - `indented_code_mixed_tab_space/input.md` pins the new
    `CODE_BLOCK` shape with `WHITESPACE@0..3 "  \t"` +
    `TEXT@3..15 "foo\tbaz\t\tbim"`. Wired into
    `golden_parser_cases.rs` next to `indented_code`.
- **Allowlist additions** (Tabs section): #2, #5.

No formatter golden case added: the parser already lossless-roundtrips
the input; existing `tab_handling` and `tab_preserve` formatter
fixtures cover the surrounding behavior, and the new shape isn't
something the formatter rewrites differently from the prior
paragraph shape (the input isn't reachable via panache's standard
formatted output of any other input).

### Don't redo

- **Don't try to fix #6 (`>\t\tfoo`) or #7 (`-\t\tfoo`) by
  retouching `is_indented_code_line` alone.** The parser doesn't
  even reach the indented-code dispatcher for those: blockquote
  marker stripping (`try_parse_blockquote_marker`) only consumes
  one literal *space* after `>`, not a partial tab; list-item
  parsing's content_col / 5-col-rule logic likewise treats post-
  marker whitespace byte-wise. Both need column-aware refactors
  of the marker utilities (`parser/utils/marker_utils.rs`,
  `parser/blocks/blockquotes.rs`, list-item content-col
  derivation in `parser/blocks/list_items*` and the dispatcher).
  Substantial; not a one-line fix.
- **Don't widen the new `consume_leading_cols` helper to the
  fenced-code or HTML-block paths.** Fenced-code stripping is
  driven by the opener's literal space count (CMark §4.5) — its
  rule is already implemented in `code_block_content`'s `if
  is_fenced` branch using a byte-level walk that's correct for
  spaces-only opener indent, and tabs in the opener are vanishingly
  rare. HTML blocks don't strip indent. Touching either is
  yak-shaving.
- **Don't add a paired Pandoc parser fixture for
  `indented_code_mixed_tab_space`.** Both flavors emit the same
  CodeBlock per pandoc; the existing dialect-neutral fixture is
  enough.
- **Don't replace `strip_leading_spaces_per_line` outright.** It's
  still load-bearing for the fenced-code stripping path (where
  the input genuinely is space-only) and the blockquote-prefix
  stripping (which deals only with `> ` style markers). Only the
  indented-code branch needed the column-aware behavior.

### Suggested next targets, ranked

1. **#6 (`>\t\tfoo`) and #7 (`-\t\tfoo`)** — both need column-aware
   marker-utility refactors. #6 wants the optional space-after-`>`
   rule to consume 1 *col* of a tab (with slack staying as content).
   #7 wants the list-item content-col derivation to apply the
   "5+ cols of post-marker whitespace ⇒ content_col = marker+1
   + indented-code" rule with column-aware tab expansion. Likely
   share helper code; tackle together. Both are dialect-neutral
   per pandoc.
2. **Proper delimiter-stack for emphasis (#402, #408, #412, #417,
   #426, #445, #457, #464, #465, #466, #468)** — rewrite emphasis
   to use CMark's process_emphasis algorithm. Largest single fix;
   would unlock the 4+ char run cases and the rule-of-3 cases.
   Substantial; gate on `Dialect::CommonMark`. Pandoc-markdown
   stays on the recursive enclosure parser. (Carried over.)
3. **#472 `*foo *bar baz*`** — CommonMark expects `*foo <em>bar
   baz</em>`. Likely needs delimiter-stack work too. (Carried
   over.)
4. **Reference-link nesting (#533, #569, #571)** — CMark
   left-bracket scanner stack with refdef-aware resolution.
   Probably the next big link cluster. (Carried over.)
5. **Formatter fix for nested-only outer LIST_ITEM** — carried
   over. Unblocks removing the dialect gate on same-line nested
   list markers (#298, #299).
6. **Fence inside blockquote inside list item (#321)**. (Carried
   over.)
7. **Same-line blockquote inside list item (#292, #293)** — `> 1.
   > Blockquote` needs the inner `>` to open a blockquote inside
   the list item. (Carried over.)
8. **#273, #274 multi-block content in `1.     code` items** —
   spaces ≥ 5 after marker means content_col is at marker+1 and
   the rest is indented code. (Carried over; likely shares helper
   with #7 above.)
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
  blockquote depth (`bq_depth < current_bq_depth` branch) is
  orthogonal to session (xiii)'s list-marker lazy continuation and
  the bq_depth=0 list-continuation gate; the three paths share the
  "lazy continuation" name but operate on different state. Don't
  try to unify them.
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
  links only. Reference-link nesting (#533/#569/#571) needs a
  different pass with refdef resolution; do not retrofit the
  helper.
