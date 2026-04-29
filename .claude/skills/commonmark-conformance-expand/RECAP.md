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

## Latest session — 2026-04-29 (xvi)

**Pass count: 619 → 620 / 652 (95.1%, +1)**

Single List items win: #280 (`-\n\n  foo\n`) — empty list-item
marker followed by a blank line followed by indented content.
CommonMark §5.2 says a list item can begin with at most one
blank line; once the marker line is empty and the next line is
blank, the item is closed. Subsequent indented content is a
separate paragraph. Pandoc keeps the indented content inside
the same item. Verified against pandoc `-f commonmark`
(`BulletList [[]] , Para [Str "foo"]`) vs `-f markdown`
(`BulletList [[Plain [Str "foo"]]]`). Dialect divergence.

### Target unlocked

- **#280** → CommonMark now closes the empty list item at the
  first blank line, lets the parent List close (since the next
  line has no list marker), and parses `  foo` as a top-level
  paragraph. Pandoc behavior unchanged.

### Root cause

`ContinuationPolicy::compute_levels_to_keep` kept any open
ListItem alive when the next non-blank line's indent met the
item's `content_col`. For an empty marker line (`-` with no
content) followed by a blank, that allowed indented content to
re-attach as continuation — wrong under CommonMark §5.2. Pandoc
allows this attachment, so the existing behavior was correct
for `Dialect::Pandoc`.

### Fix

- `crates/panache-parser/src/parser/utils/container_stack.rs`:
  added `marker_only: bool` field to `Container::ListItem`. True
  iff the item has so far seen only its marker line (no text,
  blockquote marker, or nested-list content).
- `crates/panache-parser/src/parser/blocks/lists.rs`:
  initializes `marker_only` per construction site:
  - `finish_list_item_with_optional_nested` final fallthrough:
    `text_to_buffer.trim().is_empty()` (true for `-\n`, `- \n`).
  - Recursion case (same-line nested marker): `false` (nested
    LIST counts as content).
  - `add_list_item_with_nested_empty_list`: `false` (nested LIST
    counts as content).
- `crates/panache-parser/src/parser/core.rs`: when content is
  pushed to the buffer (parse_inner_content, list-item buffering
  branch, blockquote-marker emission), flips `marker_only` to
  `false` if the pushed text is non-blank or a structural marker.
- `crates/panache-parser/src/parser/utils/continuation.rs`:
  inside the `ListItem` arm, when `marker_only && dialect ==
  CommonMark`, `continue` (don't keep the level). If the next
  non-blank line *also* has no list marker AND the parent List
  was the most recently kept level, walk `keep_level` back by 1
  so the List closes too — otherwise the List would absorb the
  blank and following paragraph as continuation content.

### Files changed

- **Dialect divergence** (parser):
  - `crates/panache-parser/src/parser/utils/container_stack.rs`:
    new `marker_only` field on `Container::ListItem`.
  - `crates/panache-parser/src/parser/blocks/lists.rs`:
    initialize `marker_only` at all 4 ListItem construction
    sites.
  - `crates/panache-parser/src/parser/core.rs`: flip
    `marker_only` to false when buffering text (line ~617),
    when buffering continuation lines inside ListItem (line
    ~2685), and when buffering blockquote markers (line ~1266).
    Also added `..` to one destructuring pattern that didn't
    need the new field.
  - `crates/panache-parser/src/parser/utils/continuation.rs`:
    new CommonMark gate inside the `ListItem` match arm; also
    walks `keep_level` back when the parent List has nothing to
    continue with.
- **Paired parser fixtures**:
  - `empty_list_marker_blank_then_content_commonmark/{input.md,parser-options.toml}`
    (`flavor = "commonmark"`) — pins LIST + BLANK_LINE +
    PARAGRAPH CST (list closes after empty item).
  - `empty_list_marker_blank_then_content_pandoc/{input.md,parser-options.toml}`
    (`flavor = "pandoc"`) — pins LIST(LIST_ITEM(BLANK_LINE+PLAIN))
    CST (item absorbs the indented content).
  - Wired into `golden_parser_cases.rs` after
    `emphasis_skips_shortcut_reference_link`.
- **Formatter golden case** (CommonMark only):
  - `tests/fixtures/cases/empty_list_marker_blank_then_content_commonmark/`
    with `panache.toml` setting `flavor = "commonmark"`. Input
    `-\n\n  foo\n`, expected `- \n\nfoo\n` (formatter renders
    bare bullet as `- ` and lifts the now-non-list paragraph out
    of any indent). Pins idempotency under the new structural
    shape (LIST + PARAGRAPH instead of LIST(item with content)).
    Wired into `tests/golden_cases.rs` after
    `blockquote_list_no_marker_closes_commonmark`.
- **Allowlist addition** (List items): #280.

### Don't redo

- **Don't try to detect "marker line was empty" by inspecting
  the buffer at blank-line time.** The buffer is flushed during
  blank-line handling, so by the time the second blank arrives
  it's empty regardless of whether the marker line had content.
  `marker_only` is the durable signal — it persists across
  buffer flushes and is updated on push, not on flush.
- **Don't gate the List branch directly on `marker_only`.** The
  ListItem arm runs *after* the List arm in the same loop; by
  then the List has already optimistically claimed the next
  line as continuation. The correct fix is to walk
  `keep_level` *back* in the ListItem arm when the next line
  has no marker, not to make the List arm aware of its child's
  state (which would couple the two arms unnecessarily).
- **Don't drop the `next_marker.is_none()` clause.** When the
  next line *is* a list marker (e.g. `-\n\n- a\n`), the parent
  List should stay open so the new marker re-uses it — both
  CommonMark and Pandoc agree on that case (just differ on tight
  vs loose). Walking `keep_level` back unconditionally would
  collapse two-item lists into separate single-item lists.
- **Don't extend `marker_only` to flow into close-time logic.**
  The empty-list-item close path
  (`close_containers_to`) already handles items with empty
  buffers correctly; the new flag is only consulted by
  `ContinuationPolicy`. Adding it to other places would risk
  rendering mismatches (e.g. an item whose buffer was just
  flushed mid-stream would be misread as marker-only).

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
4. **Tabs (#2, #5, #6, #7)** — column-aware tab expansion;
   substantial. (Carried over.)
5. **HTML block #148** — `</pre>` inside HTML block followed by
   blank-line content. (Carried over.)
6. **Reference link followed by another bracket pair (#569, #571)**
   — CMark left-bracket scanner stack model. Large. (Carried over.)
7. **Nested LINKs in link text (#518, #519, #520, #532, #533)** —
   CommonMark §6.4 forbids real nesting; outer must un-link. Same
   scanner-stack work as #569/#571. (Carried over.)
8. **Fence inside blockquote inside list item (#321)**. (Carried
   over.)
9. **Same-line blockquote inside list item (#292, #293)** — `> 1. >
    Blockquote` needs the inner `>` to open a blockquote inside the
    list item. (Carried over.)
10. **#273, #274 multi-block content in `1.     code` items** —
    spaces ≥ 5 after marker means content_col is at marker+1 and the
    rest is indented code. (Carried over.)
11. **#278 `-\n  foo\n-\n  ```\n…`** — empty marker followed by
    indented content; multiple bugs. (Carried over.) Note:
    `marker_only` from this session is unrelated — #278 has content
    on the line *immediately following* the empty marker (no blank
    in between), so the new gate doesn't fire.
12. **#300 setext-in-list-item** — `- # Foo\n- Bar\n  ---\n  baz`
    should treat `Bar\n  ---` as setext h2. (Carried over.)
13. **#523 `*foo [bar* baz]`** — emphasis closes inside link bracket
    text mid-flight. Probably needs delimiter-stack work + bracket
    scanner integration. (Carried over from session x.)

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
