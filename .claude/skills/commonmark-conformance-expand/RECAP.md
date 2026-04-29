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

## Latest session — 2026-04-29 (xii)

**Pass count: 615 → 616 / 652 (94.5%, +1)**

Single Block-quotes win: #251 (`>>> foo\n> bar\n>>baz\n`) — lazy
paragraph continuation at *reduced* blockquote depth. Both dialects
agree (universal fix), but only this one example in the spec
exercises the construct.

### Target unlocked

- **#251** `>>> foo\n> bar\n>>baz\n` → single nested-3 paragraph
  with all three lines as content. The `>` markers on lines 2/3
  are buffered inside the deep paragraph for losslessness; the
  HTML renderer ignores them, the formatter re-emits prefixes
  from container nesting.

### Root cause

`else if bq_depth < current_bq_depth` branch of `parse_line` only
checked for lazy paragraph continuation when `bq_depth == 0`.
CommonMark §5.1 lazy continuation works at *any* reduced bq_depth
— a paragraph started at depth 3 with `>>> foo` survives a
follow-on `> bar` (1 marker) or `>>baz` (2 markers) as long as the
content doesn't itself open an interrupting block.

### Fix

`crates/panache-parser/src/parser/core.rs` (the `bq_depth <
current_bq_depth` branch):

1. Hoist the paragraph-lazy-continuation check out of the
   `bq_depth == 0` block so it fires whenever the deepest
   container is a `Paragraph`.
2. When `bq_depth > 0`, buffer the explicit `>` markers into the
   paragraph via `paragraphs::append_paragraph_marker` (same
   pattern used by the same-depth and `bq_depth==0`-with-list
   continuation paths) and append `inner_content` (markers
   stripped) as paragraph text. When `bq_depth == 0`, append the
   raw `line` as before.
3. Existing HR-interrupt check on `line` is preserved — `>` lines
   never look like HR, so the check stays permissive in the new
   path.

The `bq_depth == 0` lazy-list-continuation arm is unchanged; it
still fires only when `bq_depth == 0` because the list path needs
column-anchored marker matching that doesn't generalize to nested
blockquotes.

### Files changed

- **Parser-shape gap (universal)**:
  - `crates/panache-parser/src/parser/core.rs`: lazy paragraph
    continuation generalized to `bq_depth < current_bq_depth`,
    not just `bq_depth == 0`.
- **Parser fixture + snapshot**:
  - `blockquote_lazy_continuation_reduced_markers/input.md`
    (single fixture, no `parser-options.toml` — both dialects
    agree on this CST shape).
  - Wired into `golden_parser_cases.rs` next to
    `blockquote_depth_change`.
- **Allowlist addition** (Block quotes): #251.

No paired CommonMark/Pandoc fixture and no formatter golden case
added: the fix is universal (both dialects produce the same
paragraph-with-buffered-markers CST) and the formatter idempotency
already holds via `panache debug format --checks all` on the
example.

### Don't redo

- Don't try to extend lazy *list* continuation to `bq_depth > 0`
  using the same generalization. Lists need column-anchored marker
  matching that depends on `current_bq_depth == 0` for the inner
  content's leading-indent calculation; the existing `bq_depth ==
  0` arm is structurally different from paragraph lazy.
- Don't widen the HR-interrupt check to other interrupting blocks
  (list markers, ATX, fence) without verifying against pandoc per
  example. The existing HR-only check matches established behavior
  for the `bq_depth == 0` path; broadening it would also affect
  long-standing #235-style cases that have separate dialect rules
  (Pandoc keeps lazy, CommonMark doesn't).
- Don't drop the marker buffering on the new path. The `>` tokens
  must end up *inside* the paragraph (not before it as siblings)
  for the formatter to skip them when re-emitting blockquote
  prefixes from container nesting. Emitting them outside the
  paragraph would split the paragraph or produce a malformed CST.

### Suggested next targets, ranked

1. **Proper delimiter-stack for emphasis (#402, #408, #412, #417,
   #426, #445, #457, #464, #465, #466, #468)** — rewrite emphasis
   to use CMark's process_emphasis algorithm (delimiter stack with
   leftover matching). Largest single fix; would unlock the 4+ char
   run cases (currently rejected outright) and the rule-of-3 cases.
   Substantial; gate on `Dialect::CommonMark`. Pandoc-markdown
   stays on the recursive enclosure parser. (Carried over.)
2. **#235 `> - foo\n- bar`** — last remaining Block-quotes failure.
   CommonMark exits the blockquote on `- bar` and starts a fresh
   list; Pandoc continues the inner list inside the blockquote.
   Dialect divergence. Likely needs the existing `bq_depth == 0`
   lazy-paragraph path to also check "would this line be a list
   marker that interrupts under CommonMark" before appending —
   parallel to the HR-interrupt check. Verify against pandoc
   before landing.
3. **#472 `*foo *bar baz*`** — CommonMark expects `*foo <em>bar
   baz</em>` (outer `*` literal because inner content has unmatched
   delim flanking). Likely needs delimiter-stack work too. (Carried
   over.)
4. **Formatter fix for nested-only outer LIST_ITEM** — carried over.
   Unblocks removing the dialect gate on same-line nested list
   markers (#298, #299). Probably one formatter change in
   `crates/panache-formatter/src/formatter/lists.rs`.
5. **#280 empty list item closes the list** — `-\n\n  foo\n`
   should produce empty LI + separate paragraph under CommonMark.
   Pandoc keeps `foo` as the list item content. Dialect divergence.
   Parser-shape gap, gate on CommonMark. (Carried over.)
6. **#312 list with non-uniform marker indentation** — `- a\n -
   b\n  - c\n   - d\n    - e\n` should be 4 sibling items with
   `- e` as lazy paragraph continuation of `d`. Both dialects
   agree per pandoc. Parser-shape gap, universal. (Carried over.)
7. **Tabs (#2, #5, #6, #7)** — column-aware tab expansion;
   substantial. (Carried over.)
8. **HTML block #148** — `</pre>` inside HTML block followed by
   blank-line content. (Carried over.)
9. **Reference link followed by another bracket pair (#569, #571)**
   — CMark left-bracket scanner stack model. Large. (Carried over.)
10. **Nested LINKs in link text (#518, #519, #520, #532, #533)** —
    CommonMark §6.4 forbids real nesting; outer must un-link. Same
    scanner-stack work as #569/#571. (Carried over.)
11. **Fence inside blockquote inside list item (#321)**. (Carried
    over.)
12. **Same-line blockquote inside list item (#292, #293)** — `> 1. >
    Blockquote` needs the inner `>` to open a blockquote inside the
    list item. (Carried over.)
13. **#273, #274 multi-block content in `1.     code` items** —
    spaces ≥ 5 after marker means content_col is at marker+1 and the
    rest is indented code. (Carried over.)
14. **#278 `-\n  foo\n-\n  ```\n…`** — empty marker followed by
    indented content; multiple bugs. (Carried over.)
15. **#300 setext-in-list-item** — `- # Foo\n- Bar\n  ---\n  baz`
    should treat `Bar\n  ---` as setext h2. (Carried over.)
16. **#523 `*foo [bar* baz]`** — emphasis closes inside link bracket
    text mid-flight. Probably needs delimiter-stack work + bracket
    scanner integration. (Carried over from session x.)
17. **Ref-def dialect divergence #201** — `[foo]: <bar>(baz)`. Low
    priority. (Carried over.)

### Carry-forward from prior sessions

- The "Don't redo" notes from session (ix) about emphasis split-runs
  ("tail-end only" heuristic, dialect gate, not widening to
  #402/#408/#412/#426) still apply. Don't touch those paths without
  first reading session (ix)'s recap.
- The session (x) / (xi) link-scanner skip pattern (autolink/raw-HTML
  opacity for emphasis closer + link bracket close) is load-bearing
  for #524/#526/#536/#538. Don't unify the autolink and raw-HTML
  skip flags — Pandoc treats them differently.
- This session's lazy-continuation generalization is independent of
  the link-scanner work, but the next target (#235) sits on the
  same `else if bq_depth < current_bq_depth` branch and will need
  to interact carefully: #235 is `bq_depth == 0` with paragraph
  open inside a blockquote-list, where lazy continuation currently
  fires (Pandoc-correct) but CommonMark wants list-marker interrupt.
  Plan to add a CommonMark-only list-marker interrupt check
  alongside the HR check, similar in shape.
