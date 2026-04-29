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

## Latest session — 2026-04-29 (xiii)

**Pass count: 616 → 617 / 652 (94.6%, +1)**

Single Lists win: #312 (`- a\n - b\n  - c\n   - d\n    - e\n`) —
non-uniform marker indentation where the deepest line `    - e`
sits at indent 4, which is below the deepest open list item's
content column (5) and outside any matching sibling level. Both
dialects agree per pandoc (universal fix).

### Target unlocked

- **#312** → 4 sibling items `a`, `b`, `c`, `d` (leading 0..3
  spaces all match the outer base 0 list under
  `find_matching_list_level`'s shallow rule), and `    - e`
  becomes lazy continuation of `d`'s plain content rather than
  opening a spurious sibling list at column 4.

### Root cause

For `    - e`, the dispatcher detects a list marker (indent 4 ≥ 4
guard at `block_dispatcher.rs:671` only rejects outside lists),
so `handle_list_open_effect` runs. It can't find a matching list
level (bullet at indent 4 vs base 0 fails the shallow/deep mixed
rule in `find_matching_list_level`) and indent 4 < deepest
content_col 5 means the line can't open a child list either —
the existing fallback path at `core.rs:743+` then unconditionally
opens a fresh top-level list. CommonMark says a marker line at
indent ≥ 4 that fits no open level should fall through to lazy
paragraph continuation of the deepest open item.

### Fix

`crates/panache-parser/src/parser/core.rs`:

1. New helper `Parser::try_lazy_list_continuation(block_match,
   content)`: returns true iff the line is a `BlockEffect::OpenList`
   match with `prepared.indent_cols ≥ 4`, `in_list`, indent <
   `current_content_col`, and `find_matching_list_level` returns
   `None`. In that case it appends `content` to whichever
   container is open at the top of the stack — `Paragraph` →
   `paragraphs::append_paragraph_line`, `ListItem` →
   `buffer.push_text` — and returns true.
2. In the `YesCanInterrupt` branch of `parse_line` (around the
   former `self.emit_list_item_buffer_if_needed()` call), call
   `try_lazy_list_continuation` *before* the buffer flush. If it
   returns true, advance pos and return — skipping both the
   buffer flush and `handle_list_open_effect`.

Doing the check before the flush is load-bearing: if the buffer is
emitted first, the prior item's plain content becomes a sealed
`PLAIN` node and the lazy-continued line lands in a *separate*
sibling `PLAIN` inside the same `LIST_ITEM`. The pre-flush check
keeps both lines in one `PLAIN`.

### Files changed

- **Parser-shape gap (universal)**:
  - `crates/panache-parser/src/parser/core.rs`: new
    `try_lazy_list_continuation` helper + early-return call site
    in the `YesCanInterrupt`/`OpenList` branch.
- **Parser fixture + snapshot**:
  - `list_marker_indent_4_below_content_col/input.md` (single
    fixture, no `parser-options.toml` — both dialects agree).
  - Wired into `golden_parser_cases.rs` between
    `list_item_indented_code` and `list_mixed_bullets_commonmark`.
- **Allowlist addition** (Lists): #312.

No paired CommonMark/Pandoc fixture and no formatter golden case
added: pandoc agrees on the CST shape, and the formatter's
existing list goldens already cover idempotency for plain-content
items.

### Don't redo

- **Don't put the check in the dispatcher.** The dispatcher
  doesn't have container access and can't call
  `find_matching_list_level`. An indent-only check there is too
  aggressive — it breaks
  `nested_lower_roman_with_uneven_marker_width_stays_single_nested_list`
  (`a. retain.\n\n     i. short;\n    ii. short;\n   iii. short;\n`),
  where `    ii. short;` at indent 4 is a legitimate sibling of
  the deep roman list at base 5 (matched via the deep-ordered
  abs_diff ≤ 3 rule). The check must run after
  `find_matching_list_level` so deep ordered siblings still match.
- **Don't fold the check into `handle_list_open_effect`'s fallback
  path** (the line 743+ "open a new top-level list" branch). By
  that point `emit_list_item_buffer_if_needed` has already
  emitted the prior item's `PLAIN`, so a later `buffer.push_text`
  on the same `LIST_ITEM` produces a *second* `PLAIN` sibling
  inside the item rather than appending. The check must short-
  circuit *before* the buffer flush in `parse_line`.
- **Don't widen the lazy-continuation path to indent < 4.** Below
  4-space leading indent, `find_matching_list_level` returning
  `None` for a same-style marker means a genuinely new top-level
  list (e.g. a different marker style at column 0 after some other
  list closed). The indent ≥ 4 gate is what makes the line not a
  marker per CommonMark §4 in the first place.

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
   before landing. (Carried over.)
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
6. **Tabs (#2, #5, #6, #7)** — column-aware tab expansion;
   substantial. (Carried over.)
7. **HTML block #148** — `</pre>` inside HTML block followed by
   blank-line content. (Carried over.)
8. **Reference link followed by another bracket pair (#569, #571)**
   — CMark left-bracket scanner stack model. Large. (Carried over.)
9. **Nested LINKs in link text (#518, #519, #520, #532, #533)** —
   CommonMark §6.4 forbids real nesting; outer must un-link. Same
   scanner-stack work as #569/#571. (Carried over.)
10. **Fence inside blockquote inside list item (#321)**. (Carried
    over.)
11. **Same-line blockquote inside list item (#292, #293)** — `> 1. >
    Blockquote` needs the inner `>` to open a blockquote inside the
    list item. (Carried over.)
12. **#273, #274 multi-block content in `1.     code` items** —
    spaces ≥ 5 after marker means content_col is at marker+1 and the
    rest is indented code. (Carried over.)
13. **#278 `-\n  foo\n-\n  ```\n…`** — empty marker followed by
    indented content; multiple bugs. (Carried over.)
14. **#300 setext-in-list-item** — `- # Foo\n- Bar\n  ---\n  baz`
    should treat `Bar\n  ---` as setext h2. (Carried over.)
15. **#523 `*foo [bar* baz]`** — emphasis closes inside link bracket
    text mid-flight. Probably needs delimiter-stack work + bracket
    scanner integration. (Carried over from session x.)
16. **Ref-def dialect divergence #201** — `[foo]: <bar>(baz)`. Low
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
  orthogonal to this session's list-marker lazy continuation; the
  two paths share the "lazy continuation" name but operate on
  different state (blockquote depth descent vs list-item indent
  vs content_col). Don't try to unify them.
- This session's `try_lazy_list_continuation` only fires for
  `BlockEffect::OpenList` with `indent_cols ≥ 4`. Other
  interrupting-block effects (HR, ATX, fence) at deep indent
  inside lists go through the existing CommonMark §5.2 close path
  at `core.rs:2447+` (`close_lists_above_indent`); don't touch
  that path on its account.
