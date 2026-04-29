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

## Latest session — 2026-04-29 (xv)

**Pass count: 618 → 619 / 652 (94.9%, +1)**

Single Link reference definitions win: #201 (`[foo]: <bar>(baz)`)
— the last remaining LRD failure. CommonMark §4.7 requires the
title (when on the same line as the destination) to be separated
from the destination by at least one space or tab; Pandoc accepts
the title even when directly attached. Verified against pandoc
`-f commonmark` (paragraphs) vs `-f markdown` (Link). Dialect
divergence.

### Target unlocked

- **#201** → CommonMark now rejects `[foo]: <bar>(baz)` as an LRD
  candidate; the dispatcher falls back to a paragraph (matching
  the spec). Pandoc behavior unchanged (still parses as
  `[foo]: <bar>` with title `baz`).

### Root cause

`try_parse_reference_definition_with_mode` accepted any title
that `parse_title` could match, regardless of whether the title
was whitespace-separated from the destination. Under CommonMark
§4.7, a same-line title without a preceding space/tab is
malformed.

### Fix

`crates/panache-parser/src/parser/blocks/reference_links.rs`:

- Added `dialect: Dialect` parameter to the public
  `try_parse_reference_definition` /
  `try_parse_reference_definition_lax` and the internal
  `_with_mode` helper. Threads through from
  `block_dispatcher.rs` (call sites in the LRD detector and the
  setext-vs-LRD priority check).
- Inside the title-detection branch, after computing
  `crossed_newline` and `title_start`, when
  `dialect == CommonMark && !crossed_newline && title_start ==
  after_url` (no whitespace consumed by `skip_ws_one_newline`),
  return `None` — caller sees no LRD candidate and the line
  becomes a paragraph.

### Files changed

- **Dialect divergence** (parser):
  - `crates/panache-parser/src/parser/blocks/reference_links.rs`:
    new `dialect` parameter; CommonMark gate on
    same-line-attached title.
  - `crates/panache-parser/src/parser/block_dispatcher.rs`: pass
    `ctx.config.dialect` to LRD parse calls (parse_fn and the
    setext-priority check).
- **Paired parser fixtures**:
  - `reference_definition_attached_title_commonmark/{input.md,parser-options.toml}`
    (`flavor = "commonmark"`) — pins paragraph + paragraph CST.
  - `reference_definition_attached_title_pandoc/{input.md,parser-options.toml}`
    (`flavor = "pandoc"`) — pins REFERENCE_DEFINITION + PARAGRAPH
    CST.
  - Wired into `golden_parser_cases.rs` before
    `reference_definition_inside_blockquote`.
- **Formatter golden case** (CommonMark only):
  - `tests/fixtures/cases/reference_definition_attached_title_commonmark/`
    with `panache.toml` setting `flavor = "commonmark"`. Output
    is byte-identical to input (`[foo]: <bar>(baz)\n\n[foo]\n`)
    but pins idempotency under the new CommonMark block sequence
    (PARAGRAPH + PARAGRAPH instead of REFDEF + PARAGRAPH).
    Wired into `tests/golden_cases.rs` before `reference_footnotes`.
- **Allowlist addition** (Link reference definitions): #201.

### Don't redo

- **Don't widen the gate to other "title without whitespace"
  cases.** The check is specifically `title_start == after_url
  && !crossed_newline` — i.e. zero whitespace skipped on the
  same line. Multi-line titles (preceded by a newline) are
  permitted by CommonMark §4.7 even with no leading space on the
  next line, so the `crossed_newline` clause is load-bearing.
- **Don't generalize to other ref-def parser branches.** The
  only place CommonMark and Pandoc actually disagree on LRD
  parsing in the failing-spec set is the title-separator rule.
  Other deviations (multi-line label/destination, escapes, etc.)
  already match between dialects.
- **Don't try to repurpose `strict_eol` for this.** The existing
  `strict_eol: bool` controls the MMD-lax behavior (trailing
  attribute tokens on the same line as the title); it's
  orthogonal to the dialect-level title-separator rule. Keep
  them as independent parameters.

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
