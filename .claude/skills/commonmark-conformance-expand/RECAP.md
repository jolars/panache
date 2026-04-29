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

## Latest session — 2026-04-29 (ix)

**Pass count: 599 → 606 / 652 (92.9%, +7)**

All wins in Emphasis section. CommonMark-only delimiter splitting
behaviors that Pandoc-markdown explicitly does not perform.

### Targets unlocked

- **#467** `***foo***` → `<em><strong>foo</strong></em>` (em outside
  strong). CM rule 14 prefers em-outer; Pandoc wraps the other way.
- **#442** `**foo*` → `*<em>foo</em>` (split-opener: 2-char left
  flanker matches 1-char right closer with leading literal).
- **#454** `__foo_` → `_<em>foo</em>` (same, underscore).
- **#443** `*foo**` → `<em>foo</em>*` (split-closer: 1-char opener
  matches first char of trailing 2-char run, trailing literal).
- **#455** `_foo__` → `<em>foo</em>_` (same, underscore).
- **#409** `*foo *bar**` → `<em>foo <em>bar</em></em>` (free win
  from #443 mechanism — inner `*bar*` closes via split-closer at
  `**`).
- **#416** `foo***bar***baz` → `foo<em><strong>bar</strong></em>baz`
  (free win from #467 swap).

### Root cause

CommonMark §6.2 allows partial-match between delimiter runs of
different lengths: a 2-char opener can match a 1-char closer (and
vice-versa), with the leftover delimiter char emitted as literal text
adjacent to the emphasis. CMark's full process_emphasis algorithm uses
a delimiter stack; panache uses Pandoc's recursive enclosure parser
which doesn't model leftover delimiters. Pandoc-markdown explicitly
rejects these splits (`**foo*` → literal), so the splitting must be
gated on `Dialect::CommonMark`.

For `***foo***`, both dialects parse to a 3-char run on each side —
the difference is *which* of em/strong wraps the other. Symmetric
shape, opposite emit order.

### Fix

In `crates/panache-parser/src/parser/inlines/core.rs`:

1. **`try_parse_three` (around line 351)** — when a 3-char closer
   matches the 3-char opener, swap emit order under CommonMark to
   `EMPHASIS[STRONG[content]]`. Pandoc keeps the existing
   `STRONG[EMPHASIS[content]]`.
2. **`try_parse_two` fallback** — after the normal `**` closer scan
   fails, under CommonMark only, scan for a 1-char closer
   (`parse_until_closer_with_nested_two` with delim_count=1). On
   match, emit `TEXT(<delim>) + EMPHASIS[<delim>, content, <delim>]`
   and return `closer_pos + 1 - pos` consumed.
3. **`parse_until_closer_with_nested_two` nested-two-failed branch**
   — when nested two fails (Pandoc-poisons the outer one), under
   CommonMark validate the run as a 1-closer
   (`is_valid_same_delim_closer`) and return `Some(pos)` (the first
   char of `**` is the closer, leaving the second as literal trail).

### "Tail-end only" heuristic

Both split-* fallbacks are restricted to the case where the leftover
delim char would have *no further occurrences* of the same delim in
the remainder of the parse range. Without this restriction, the
fallback fires on cases like #402 `__foo__bar__baz__` and #412
`*foo**bar*` — and emits *worse* output than the prior literal-text
fallback because we don't model CMark's full delimiter-stack matching.

Implementation: `!text[(closer_pos+1).min(...)..end].contains(delim_char)`
for split-opener; `!text[(pos+2).min(...)..end].contains(delim_char)`
for split-closer.

This intentionally leaves #402, #408, #412, #426 unsolved (they
require true delimiter-stack matching). The heuristic is the smallest
guard that captures all the wins without regressing existing examples
or making the failures uglier than literal text.

### Dialect gating

All three changes are gated on `config.dialect == Dialect::CommonMark`.
Verified against pandoc:

- `pandoc -f markdown` literal for `**foo*`, `__foo_`, `*foo**`,
  `_foo__`; `Strong[Emph[foo]]` for `***foo***`. Matches existing
  Pandoc-mode behavior.
- `pandoc -f commonmark` matches the new CM-mode CST shapes.

### Files changed

- **Dialect divergence** (parser inline core):
  - `crates/panache-parser/src/parser/inlines/core.rs`:
    - `try_parse_three`: dialect-gated emit-order swap for 3-3 match.
    - `try_parse_two`: CM-only split-opener fallback.
    - `parse_until_closer_with_nested_two`: CM-only split-closer
      branch when nested two fails.
- **Paired parser fixtures + snapshots**:
  - `emphasis_split_runs_commonmark/` — pins CM CST for the 7 inputs
    above (8 paragraphs).
  - `emphasis_split_runs_pandoc/` — pins Pandoc CST (literal text for
    splits, `Strong[Emph]` for triples) — confirms no Pandoc
    regression.
  - Wired into `golden_parser_cases.rs` next to `emphasis_*`.
- **Allowlist additions** (Emphasis and strong emphasis): #409, #416,
  #442, #443, #454, #455, #467.

No CommonMark formatter golden case added: block sequence is
unchanged (one paragraph per example), only inline shape differs.
Idempotency verified manually under both flavors. Per the rule, only
add a CM formatter case when block sequence diverges.

### Don't redo

- Don't drop the "tail-end only" heuristic. Without it #402, #412,
  #408 emit *worse* output (`_<em>foo</em>_bar__baz__` for #402 vs
  the original literal). The heuristic is what makes the partial
  splitting safe in the absence of a real delimiter-stack.
- Don't drop the `Dialect::CommonMark` gate on any of the three
  branches. Pandoc-markdown explicitly does not split runs; removing
  the gate would regress every Pandoc-mode test that passes a
  `**foo*`-style literal through.
- Don't reuse this approach for #402/#408/#412/#426 by widening the
  heuristic. Those need a proper delimiter-stack rewrite (see
  "Suggested next targets" #1 below). A wider linear fallback is a
  whack-a-mole loss.
- Don't move the split-opener fallback inside
  `parse_until_closer_with_nested_one`. Doing so would cause the
  nested-one path to start emitting Emph nodes for what should be
  poison cases, and the recursive call sites lose their
  Pandoc-equivalent behavior. Keep the split logic at the
  `try_parse_two` boundary.
- The split-closer fallback uses `is_valid_same_delim_closer`, NOT
  `is_valid_ender`. The two have different rules for `_` (the former
  is what `parse_until_closer_with_nested_*` already uses for
  closer-validity; the latter is `try_parse_three`'s ender-shape
  check). Don't conflate.
- The `has_enough_delim_chars_ahead` heuristic from session (vii) is
  still load-bearing for #470/#471 — don't touch.

### Suggested next targets, ranked

1. **Proper delimiter-stack for emphasis (#402, #408, #412, #426,
   #464, #465, #466, #468)** — rewrite emphasis to use CMark's
   process_emphasis algorithm (delimiter stack with leftover
   matching). Largest single fix; would also unlock the 4+ char run
   cases (currently rejected outright) and the rule-of-3 cases.
   Substantial; gate on `Dialect::CommonMark`. Pandoc-markdown stays
   on the recursive enclosure parser.
2. **Formatter fix for nested-only outer LIST_ITEM** — carried
   over from (viii). Unblocks removing the dialect gate on
   same-line nested list markers (#298, #299). Probably one
   formatter change in
   `crates/panache-formatter/src/formatter/lists.rs`.
3. **#280 empty list item closes the list** —
   `-\n\n  foo\n` should produce empty LI + separate paragraph
   under CommonMark. Pandoc keeps `foo` as the list item content.
   Dialect divergence. Parser-shape gap, gate on CommonMark.
4. **#312 list with non-uniform marker indentation** — `- a\n -
   b\n  - c\n   - d\n    - e\n` should be 4 sibling items with
   `- e` as lazy paragraph continuation of `d`. Both dialects
   agree per pandoc. Parser-shape gap, universal.
5. **HTML-inside-emphasis (#475, #476, #477)** — `**<a href="**">`
   should NOT match the `**` inside `href="**"` as a closer. The
   inline_html scanner needs to consume the tag *before* emphasis
   sees the embedded delimiters. Renderer or parser-shape gap.
6. **#472 `*foo *bar baz*`** — CommonMark expects `*foo <em>bar
   baz</em>` (outer `*` literal because inner content has unmatched
   delim flanking). Likely needs delimiter-stack work too.
7. **Tabs (#2, #5, #6, #7)** — column-aware tab expansion;
   substantial. (Carried over.)
8. **HTML block #148** — `</pre>` inside HTML block followed by
   blank-line content. (Carried over.)
9. **Reference link followed by another bracket pair (#569, #571)**
   — CMark left-bracket scanner stack model. Large. (Carried over.)
10. **Nested LINKs in link text (#518, #519, #520, #532, #533)** —
    CommonMark §6.4 forbids real nesting; outer must un-link. Same
    scanner-stack work. (Carried over.)
11. **HTML-tag/autolink interaction with link brackets (#524, #526,
    #536, #538)** — bracket scanner must skip past raw HTML and
    autolinks. (Carried over.)
12. **Block quotes lazy-continuation #235, #251** — last two
    blockquote failures. (Carried over.)
13. **Fence inside blockquote inside list item (#321)**. (Carried
    over.)
14. **Same-line blockquote inside list item (#292, #293)** — `> 1. >
    Blockquote` needs the inner `>` to open a blockquote inside the
    list item. (Carried over.)
15. **#273, #274 multi-block content in `1.     code` items** —
    spaces ≥ 5 after marker means content_col is at marker+1 and the
    rest is indented code. (Carried over.)
16. **#278 `-\n  foo\n-\n  ```\n…`** — empty marker followed by
    indented content; multiple bugs. (Carried over.)
17. **#300 setext-in-list-item** — `- # Foo\n- Bar\n  ---\n  baz`
    should treat `Bar\n  ---` as setext h2. (Carried over.)
18. **Ref-def dialect divergence #201** — `[foo]: <bar>(baz)`. Low
    priority. (Carried over.)
