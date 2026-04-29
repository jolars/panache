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

## Latest session — 2026-04-29 (viii)

**Pass count: 597 → 599 / 652 (91.9%, +2)**

Both wins are in List items: #298 and #299 — same-line nested list
markers (`- - foo`, `1. - 2. foo`).

### Root cause: same-line nested list markers were never recursed

When a list item's first-line content begins with another list marker
followed by content, CommonMark requires emitting a nested LIST inside
the outer LIST_ITEM. Panache previously buffered the entire content
(including any inner marker) as inline text, so `- - foo` rendered as
`<li>- foo</li>` instead of `<li><ul><li>foo</li></ul></li>`.

The existing `is_content_nested_bullet_marker` /
`add_list_item_with_nested_empty_list` path only handled the bare-inner
case (`- *`, `- +`, `- -`) where the inner marker has no content after
it. The non-bare case fell through to plain text.

### Fix

1. New `finish_list_item_with_optional_nested` helper in
   `crates/panache-parser/src/parser/blocks/lists.rs`. After
   `emit_list_item` produces the outer LIST_ITEM markers and a buffered
   text, the helper:
   - Tries `try_parse_list_marker` on the buffered text.
   - If it matches AND there's content after the inner marker AND the
     buffered text isn't itself a thematic break (`- * * *` exception),
     recursively opens a nested LIST + LIST_ITEM, emits the inner item
     via the same helper (so 3+ levels work), and returns.
   - Otherwise, buffers the text and pushes the outer ListItem
     container — original behavior.

2. Both `add_list_item` and `start_nested_list` now route through this
   helper. They now take a `&ParserOptions` so the recursive
   `try_parse_list_marker` has the dialect/extensions context.

3. **Container-stack unwind fix** in
   `crates/panache-parser/src/parser/core.rs`
   `handle_list_open_effect` (third branch, no-matching-level case):
   the previous code popped while `ListItem` then while `List`, which
   doesn't fully unwind interleaved `[..., LIST, LIST_ITEM, LIST,
   LIST_ITEM]` stacks the recursion produces. Combined into a single
   loop popping while last is either `ListItem` or `List`. Without
   this, a new top-level list arriving after a nested-recursion-line
   ended up nested inside the previous outer LIST_ITEM. (See "Don't
   redo" below: this affected only the deep interleaved case; existing
   single-level nested behavior was already correct because that path
   matched on `find_matching_list_level` and used
   `close_containers_to(level + 1)` instead of the unwind loops.)

### Dialect gating

The recursion is gated to `Dialect::CommonMark`:

```
let dialect_allows_nested = config.dialect == crate::Dialect::CommonMark;
```

Pandoc-markdown also nests in this position (verified with `pandoc -f
markdown -t native`: `- b. foo` is a bullet wrapping an alpha-ordered
list), but the **formatter** does not yet support emitting an outer
LIST_ITEM whose only child is a nested LIST — when triggered, it drops
the outer marker and emits just the inner list, breaking idempotency
(specifically, the `escaped_double_underscore_in_list_item_stays_idempotent`
test). So under Pandoc dialect we keep the existing flat behavior and
the formatter test stays green; under CommonMark we land the nested
shape and unlock #298, #299. **Removing the dialect gate is a
follow-up that requires a formatter fix first** — see "Suggested next
targets" #N below.

### Files changed

- **Parser-shape (universal)**:
  - `crates/panache-parser/src/parser/blocks/lists.rs`:
    - New `finish_list_item_with_optional_nested` helper.
    - `add_list_item` and `start_nested_list` take `&ParserOptions` and
      route through it.
    - Thematic-break short-circuit so `- * * *` keeps producing a
      thematic-break inside the list item, not a deep bullet chain.
  - `crates/panache-parser/src/parser/core.rs`:
    - All 6 `add_list_item` call sites pass `self.config`.
    - `start_nested_list` call site passes `self.config`.
    - Combined `while ListItem` + `while List` unwind loops in
      `handle_list_open_effect` third branch.
- **Paired parser fixtures + snapshots**:
  - `list_nested_same_line_marker_commonmark/` — pins nested CST for
    `- - foo` and `1. - 2. foo` (3 levels).
  - `list_nested_same_line_marker_pandoc/` — pins flat CST (current
    Pandoc-mode behavior; will change when the formatter supports
    nested-only outer items).
  - Wired into `golden_parser_cases.rs` next to existing `list_*`
    cases.
- **Allowlist additions** (List items): #298, #299.

No CommonMark formatter golden case added: the new structural shape
under CommonMark is `LIST > LIST_ITEM > LIST > LIST_ITEM`, but the
**Pandoc** existing formatter behavior already round-trips the *same
input* differently (flat single-level list with `- foo` text), and the
formatter golden suite covers Pandoc defaults. Adding a CommonMark
formatter case would only confirm the same flat-output formatter
behavior is reused under CommonMark too, which isn't worth the churn.
The parser fixture is sufficient.

### Don't redo

- Don't drop the `Dialect::CommonMark` gate. It's load-bearing for the
  Pandoc `escaped_double_underscore_in_list_item_stays_idempotent`
  test. Removing it requires fixing the formatter to handle a
  LIST_ITEM whose only child is a non-empty nested LIST (probably:
  emit outer marker + space + recurse inline, like pandoc's `- b.
  WHERE...` round-trip).
- Don't drop the thematic-break short-circuit. Without it `- * * *`
  (#61) regresses immediately — the `* * *` content gets recursed as
  three nested bullet markers.
- Don't drop the unwind-loop merge in `handle_list_open_effect` third
  branch. Without it, a non-matching new list at indent 0 after a
  nested same-line line ends up inside the previous outer LIST_ITEM
  instead of at document level.
- Don't try to handle the **bare inner marker** case (`- *`, `- -`,
  `- +`) in the new helper. The existing
  `add_list_item_with_nested_empty_list` path is still load-bearing
  for that — it closes the inner LIST_ITEM (empty) rather than leaving
  it open. Keep them separate.
- Don't extend recursion to handle `> 1. > Blockquote` (#292, #293) by
  reusing this helper. Those need same-line *blockquote markers*
  inside list items, which is a different mechanism (open
  `Container::BlockQuote` mid-item, not open another `Container::List`).
- The `has_enough_delim_chars_ahead` heuristic from session (vii) is
  still load-bearing for emphasis #470/#471 — don't touch.

### Suggested next targets, ranked

1. **Formatter fix for nested-only outer LIST_ITEM** — the unblocker
   for removing the Pandoc dialect gate and gaining whatever pandoc
   parity Pandoc-markdown gets out of the recursion. Probably 1
   formatter file change in
   `crates/panache-formatter/src/formatter/lists.rs`
   (`format_list_item`'s lines.is_empty() / has_only_empty_nested_list
   path needs a new case: when the LIST_ITEM has a non-empty nested
   LIST as its first content and no PLAIN/PARAGRAPH, emit outer marker
   + space inline and let the nested list recursion continue on the
   same line). Verify against pandoc round-trip.
2. **Partial-match emphasis (#402, #408, #426)** — the
   delimiter-stack idea: a `__` opener can match a single `_`
   closer (consuming one of two `_`s), leaving residual openers
   for later closers. Currently `try_parse_two` looks for a
   length-2 closer only. Largest tractable fix; needs care to
   avoid regressing existing `__` behavior and to keep Pandoc
   parity. Likely unlocks 3+ examples. (Carried over from vii.)
3. **Rule of 3 (#412, #416, #417)** — CommonMark §6.2:
   "If one of the delimiters can both open and close (strong)
   emphasis, then the sum of the lengths of the delimiter runs
   containing the open and close delimiters must not be a
   multiple of 3 unless both lengths are multiples of 3."
   Cross-dialect divergence; gate on `Dialect::CommonMark`.
   (Carried over.)
4. **#409 `*foo *bar**`** — needs partial-match for `**` closer
   to be split between inner `*` and outer `*`. Companion to (2).
5. **#280 empty list item closes the list** —
   `-\n\n  foo\n` should produce empty LI + separate paragraph
   under CommonMark. Pandoc keeps `foo` as the list item content.
   Dialect divergence. Parser-shape gap, gate on CommonMark.
6. **#312 list with non-uniform marker indentation** — `- a\n -
   b\n  - c\n   - d\n    - e\n` should be 4 sibling items with
   `- e` as lazy paragraph continuation of `d`. Both dialects
   agree per pandoc. Parser-shape gap, universal.
7. **Tabs (#2, #5, #6, #7)** — column-aware tab expansion;
   substantial. (Carried over.)
8. **HTML block #148** — `</pre>` inside HTML block followed by
   blank-line content should be inline raw HTML in the resumed
   paragraph; renderer/parser disagreement on where the HTML
   block ends. (Carried over.)
9. **Reference link followed by another bracket pair (#569,
   #571)** — CMark left-bracket scanner stack model. Large.
   (Carried over.)
10. **Nested LINKs in link text (#518, #519, #520, #532, #533)** —
    CommonMark §6.4 forbids real nesting; outer must un-link.
    Same scanner-stack work. (Carried over.)
11. **HTML-tag/autolink interaction with link brackets (#524,
    #526, #536, #538)** — bracket scanner must skip past raw HTML
    and autolinks. (Carried over.)
12. **Block quotes lazy-continuation #235, #251** — last two
    blockquote failures. #235 is a dialect divergence (CommonMark
    closes the BQ-list pair, Pandoc lazy-continues). #251 is the
    `>>>` / `> bar` / `>>baz` lazy-continuation pattern.
13. **Fence inside blockquote inside list item (#321)**.
    (Carried over.)
14. **Same-line blockquote inside list item (#292, #293)** —
    `> 1. > Blockquote` needs the inner `>` to open a blockquote
    inside the list item. Conceptually similar to (1)'s same-line
    nesting, but different mechanism (BlockQuote container, not
    List container). Lazy continuation `continued here.` should
    fold into the inner blockquote's paragraph.
15. **#273, #274 multi-block content in `1.     code` items** —
    spaces ≥ 5 after marker means content_col is at marker+1 and
    the rest is indented code. Currently we accept all spaces as
    spaces_after.
16. **#278 `-\n  foo\n-\n  ```\n…`** — empty marker followed by
    indented content. We currently misinterpret `-\n  foo` as
    setext heading; multiple bugs likely.
17. **#300 setext-in-list-item** — `- # Foo\n- Bar\n  ---\n  baz`
    should treat `Bar\n  ---` as setext h2.
18. **Ref-def dialect divergence #201** — `[foo]: <bar>(baz)`.
    Low priority. (Carried over.)
