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

## Latest session — 2026-04-29 (xvii)

**Pass count: 620 → 621 / 652 (95.2%, +1)**

Single HTML blocks win: #148 (`<table>...<pre>\n...\n\n_world_.\n</pre>\n</td></tr></table>`).
CommonMark §4.6 type-1 HTML blocks start only with **opening**
tags from the verbatim set (`<pre`, `<script`, `<style`,
`<textarea`); closing forms like `</pre>` do not start any HTML
block (they're not in type-6's tag list, and type-7 explicitly
excludes verbatim names). Verified against pandoc `-f
commonmark` — `</pre>` stays inside the paragraph there too.
Parser-shape gap (CommonMark only).

### Target unlocked

- **#148** → `</pre>` no longer starts a verbatim HTML block
  under CommonMark; the paragraph absorbs it as inline raw
  HTML. The next line `</td></tr></table>` still interrupts as
  a type-6 block (because `td` is in the type-6 tag list).
  Pandoc behavior unchanged (closing forms were already
  rejected by `extract_block_tag_name` when `accept_closing =
  false`).

### Root cause

`try_parse_html_block_start` accepted *any* form (opening or
closing) of a verbatim tag as starting a type-1 block, as long
as `extract_block_tag_name` returned the name. Under CommonMark
that function accepts closing tags (for type-6 detection), so
`</pre>` slipped into the verbatim branch and wrongly opened a
new type-1 block — interrupting the paragraph.

### Fix

- `crates/panache-parser/src/parser/blocks/html_blocks.rs`:
  inside `try_parse_html_block_start`, after extracting the tag
  name, compute `is_closing = trimmed.starts_with("</")` and
  gate the VERBATIM_TAGS branch on `!is_closing`. The BLOCK_TAGS
  branch is unchanged (type-6 legitimately accepts closing
  forms). With the gate in place, `</pre>` falls through to the
  type-7 check, which already rejects verbatim names — so it
  returns `None` (not an HTML block start) and the line stays
  inside the paragraph as inline raw HTML.

### Files changed

- **Parser-shape** (CommonMark only):
  - `crates/panache-parser/src/parser/blocks/html_blocks.rs`:
    new `is_closing` check in the VERBATIM_TAGS branch of
    `try_parse_html_block_start`.
- **Parser fixture** (CommonMark only — Pandoc path unchanged):
  - `html_block_pre_close_tag_inline_commonmark/{input.md,parser-options.toml}`
    (`flavor = "commonmark"`) — pins HTML_BLOCK +
    BLANK_LINE + PARAGRAPH(...INLINE_HTML(`</pre>`)...) +
    HTML_BLOCK CST. Wired into `golden_parser_cases.rs` after
    `html_block_commonmark_type6_type7_pandoc`.
- **Formatter golden case** (CommonMark only):
  - `tests/fixtures/cases/html_block_pre_close_tag_inline_commonmark/`
    with `panache.toml` setting `flavor = "commonmark"`.
    Pins idempotent reformat: `_world_.` → `*world*.` and
    `</pre>` joins the paragraph wrap as `*world*. </pre>`.
    Wired into `tests/golden_cases.rs` after
    `html_block_commonmark_type6_type7`.
- **Allowlist addition** (HTML blocks): #148.

### Don't redo

- **Don't widen the gate to also reject opening verbatim
  tags.** `<pre>`, `<script>`, etc. on a line *do* legitimately
  start a type-1 block under CommonMark. The bug is closing-only.
- **Don't add `pre`/`script`/`style`/`textarea` to BLOCK_TAGS.**
  CommonMark type 6 explicitly excludes them so the type-1
  semantics (close on matching `</tag>`) take precedence over
  type-6's blank-line rule. Adding them to BLOCK_TAGS would
  collapse the two types and break the matching-close-tag rule.
- **Don't try to fix this in the type-7 branch.** Type 7 is
  reachable here only after the BLOCK_TAGS / VERBATIM_TAGS
  branches both miss; the closing-verbatim case was being
  *captured* by VERBATIM_TAGS before type 7 could see it.
  Fixing it required moving the rejection earlier, not later.
- **Don't extend the gate to non-CommonMark dialects.** Under
  Pandoc, `extract_block_tag_name(_, false)` already rejects
  closing tags entirely, so the VERBATIM_TAGS branch is
  unreachable for `</pre>` anyway. Adding `is_closing` checks
  there would be dead code.

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
