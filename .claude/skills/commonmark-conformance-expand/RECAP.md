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

## Latest session — 2026-04-29 (xiv)

**Pass count: 617 → 618 / 652 (94.8%, +1)**

Single Block-quotes win: #235 (`> - foo\n- bar\n`) — the last
remaining Block-quotes failure. CommonMark exits the blockquote
on `- bar` and starts a fresh top-level list; Pandoc keeps the
inner list going inside the blockquote (verified against pandoc
`-f commonmark` vs `-f markdown`). Dialect divergence.

### Target unlocked

- **#235** → CommonMark now produces `<blockquote><ul><li>foo
  </li></ul></blockquote><ul><li>bar</li></ul>` (two top-level
  blocks). Pandoc behavior unchanged (single blockquote
  containing the full list).

### Root cause

In `parse_line`'s `bq_depth < current_bq_depth` branch
(`core.rs:1670+`), when `bq_depth == 0` we have a "lazy list
continuation" path (around line 1726) that fires when:
- the parser is `in_blockquote_list` (a list lives inside an
  open blockquote), and
- the current line has a list marker matching an open list
  level via `find_matching_list_level`.

That path appends the new list item *inside the blockquote*,
keeping the blockquote open even though the line has no `>`.
For Pandoc that's correct; for CommonMark §5.1 it's wrong —
without a `>` prefix on a line that itself starts a block-level
construct (a list marker), the blockquote must close.

### Fix

`crates/panache-parser/src/parser/core.rs` (one-line gate):

```rust
if bq_depth == 0 && self.config.dialect != crate::options::Dialect::CommonMark {
    if lists::in_blockquote_list(&self.containers)
        && let Some(marker_match) = try_parse_list_marker(line, self.config) { ... }
}
```

Under CommonMark we fall through to the existing close-paragraph
+ `close_blockquotes_to_depth(0)` path immediately below, which
then re-enters `parse_inner_content(line, None)` for the bare
line and opens a new top-level list correctly.

### Files changed

- **Dialect divergence** (parser):
  - `crates/panache-parser/src/parser/core.rs`: gated the
    `bq_depth == 0` lazy-list-continuation block on
    `Dialect != CommonMark`.
- **Paired parser fixtures**:
  - `blockquote_list_no_marker_closes_commonmark/{input.md,parser-options.toml}`
    (`flavor = "commonmark"`)
  - `blockquote_list_no_marker_continues_pandoc/{input.md,parser-options.toml}`
    (`flavor = "pandoc"`)
  - Wired into `golden_parser_cases.rs` after
    `blockquote_list_blockquote`. Snapshots pin the divergent
    CST shapes (CommonMark: BLOCK_QUOTE@0..8 + LIST@8..14;
    Pandoc: single BLOCK_QUOTE@0..14 with a 2-item list).
- **Formatter golden case** (CommonMark only):
  - `tests/fixtures/cases/blockquote_list_no_marker_closes_commonmark/`
    with `panache.toml` setting `flavor = "commonmark"`. Output
    is `> - foo\n\n- bar\n`. Wired into
    `tests/golden_cases.rs` after
    `thematic_break_interrupts_paragraph_commonmark`. No paired
    Pandoc formatter case (existing list/blockquote goldens
    already cover Pandoc-default behavior).
- **Allowlist addition** (Block quotes): #235.

### Don't redo

- **Don't widen the gate to other lazy-continuation paths.** The
  surrounding `if matches!(..., Container::Paragraph { .. })`
  block immediately above (lines 1676-1725) handles
  *paragraph* lazy continuation at reduced blockquote depth and
  is already gated on `Dialect::CommonMark` differently (the HR
  interrupt check). The list-continuation gate added here is
  specifically about list markers at `bq_depth == 0`; don't try
  to unify the two.
- **Don't add a Pandoc formatter golden** — the existing
  blockquote/list goldens (`blockquotes`, `blockquote_list_*`)
  already exercise Pandoc-flavored idempotency. Adding a
  duplicate would just be churn per the skill rules.
- **Don't extend the gate to `bq_depth > 0` shallowing** (e.g.
  three levels of `>` reducing to one). That's a different
  state (depth still positive, just shallower) and goes through
  the same-depth or paragraph-lazy paths above; CommonMark and
  Pandoc agree there for list markers (the inner blockquote
  stays open).
- **Don't try to fold the close path into the gate.** The
  fall-through at lines 1793+ (close paragraph, close
  blockquotes to depth 0, then re-parse `line` at top level)
  Just Works under CommonMark — `parse_inner_content(line,
  None)` opens the new top-level list as a fresh container
  cycle. Adding manual list-open code in the CommonMark branch
  would duplicate that path.

### Suggested next targets, ranked

1. **Proper delimiter-stack for emphasis (#402, #408, #412, #417,
   #426, #445, #457, #464, #465, #466, #468)** — rewrite emphasis
   to use CMark's process_emphasis algorithm (delimiter stack with
   leftover matching). Largest single fix; would unlock the 4+ char
   run cases (currently rejected outright) and the rule-of-3 cases.
   Substantial; gate on `Dialect::CommonMark`. Pandoc-markdown
   stays on the recursive enclosure parser. (Carried over.)
2. **#472 `*foo *bar baz*`** — CommonMark expects `*foo <em>bar
   baz</em>` (outer `*` literal because inner content has unmatched
   delim flanking). Likely needs delimiter-stack work too. (Carried
   over.)
3. **Formatter fix for nested-only outer LIST_ITEM** — carried over.
   Unblocks removing the dialect gate on same-line nested list
   markers (#298, #299). Probably one formatter change in
   `crates/panache-formatter/src/formatter/lists.rs`.
4. **#280 empty list item closes the list** — `-\n\n  foo\n`
   should produce empty LI + separate paragraph under CommonMark.
   Pandoc keeps `foo` as the list item content. Dialect divergence.
   Parser-shape gap, gate on CommonMark. (Carried over.)
5. **Tabs (#2, #5, #6, #7)** — column-aware tab expansion;
   substantial. (Carried over.)
6. **HTML block #148** — `</pre>` inside HTML block followed by
   blank-line content. (Carried over.)
7. **Reference link followed by another bracket pair (#569, #571)**
   — CMark left-bracket scanner stack model. Large. (Carried over.)
8. **Nested LINKs in link text (#518, #519, #520, #532, #533)** —
   CommonMark §6.4 forbids real nesting; outer must un-link. Same
   scanner-stack work as #569/#571. (Carried over.)
9. **Fence inside blockquote inside list item (#321)**. (Carried
   over.)
10. **Same-line blockquote inside list item (#292, #293)** — `> 1. >
    Blockquote` needs the inner `>` to open a blockquote inside the
    list item. (Carried over.)
11. **#273, #274 multi-block content in `1.     code` items** —
    spaces ≥ 5 after marker means content_col is at marker+1 and the
    rest is indented code. (Carried over.)
12. **#278 `-\n  foo\n-\n  ```\n…`** — empty marker followed by
    indented content; multiple bugs. (Carried over.)
13. **#300 setext-in-list-item** — `- # Foo\n- Bar\n  ---\n  baz`
    should treat `Bar\n  ---` as setext h2. (Carried over.)
14. **#523 `*foo [bar* baz]`** — emphasis closes inside link bracket
    text mid-flight. Probably needs delimiter-stack work + bracket
    scanner integration. (Carried over from session x.)
15. **Ref-def dialect divergence #201** — `[foo]: <bar>(baz)`. Low
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
- Session (xii)'s lazy paragraph continuation across reduced
  blockquote depth (`bq_depth < current_bq_depth` branch) is
  orthogonal to session (xiii)'s list-marker lazy continuation and
  this session's bq_depth=0 list-continuation gate; the three paths
  share the "lazy continuation" name but operate on different state
  (blockquote depth descent vs list-item indent vs blockquote-list
  exit). Don't try to unify them.
- Session (xiii)'s `try_lazy_list_continuation` only fires for
  `BlockEffect::OpenList` with `indent_cols ≥ 4`. Other
  interrupting-block effects (HR, ATX, fence) at deep indent
  inside lists go through the existing CommonMark §5.2 close path
  at `core.rs:2447+` (`close_lists_above_indent`); don't touch
  that path on its account.
