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

## Latest session — 2026-04-29 (x)

**Pass count: 606 → 611 / 652 (93.7%, +5)**

All wins in Emphasis section. Universal (both-dialect) parser-shape
fix: emphasis closer scan was not skipping past raw HTML / autolink
spans, so embedded delimiters inside attribute values or autolink
URLs were mis-matched as closers (and the inline_html / autolink
scanner then over-ran into the body, breaking losslessness).

### Targets unlocked

- **#475** `*<img src="foo" title="*"/>` → `*<img...>` (single `*`
  literal, raw HTML preserved).
- **#476** `**<a href="**">` → literal `**<a href="**">`.
- **#477** `__<a href="__">` → same with `_`.
- **#480** `**a<https://foo.bar/?q=**>` → `**a` literal + autolink.
- **#481** same with `_`.

### Root cause

`parse_until_closer_with_nested_two` and
`parse_until_closer_with_nested_one` already skip over code spans,
inline math, inline links, and reference links so their interiors
aren't scanned for emphasis closers — but they were missing the same
treatment for `<...>` autolinks and inline raw HTML. The scanner
walked byte-by-byte and matched the `*`/`**`/`__` inside an
attribute value or autolink URL as a closer. The body parser then
re-parsed the same span and the inline_html scanner extended past
the (wrong) closer, producing a CST whose total byte length exceeded
the input — i.e. a losslessness violation that was happening under
*both* dialects.

Per pandoc, both `-f commonmark` and `-f markdown` produce
`Str "*" + RawInline html` / `Str "**" + RawInline html` /
`Str "**a" + Link` for these inputs. So this is a universal
parser-shape fix, not a dialect divergence.

### Fix

In `crates/panache-parser/src/parser/inlines/core.rs`, both
`parse_until_closer_with_nested_two` and
`parse_until_closer_with_nested_one`: add two new skip blocks
between the existing reference-link skip and the
nested-emphasis fallback:

1. `<` + `extensions.autolinks` + `try_parse_autolink(...)` →
   skip past the autolink span.
2. `<` + `extensions.raw_html` + `try_parse_inline_html(...)` →
   skip past the inline HTML span.

Order matters: autolink must come before raw_html since both start
with `<` and the autolink form is more specific.

### Files changed

- **Parser-shape gap** (universal, both dialects):
  - `crates/panache-parser/src/parser/inlines/core.rs`:
    - `parse_until_closer_with_nested_two`: skip autolinks and
      inline raw HTML during closer scan.
    - `parse_until_closer_with_nested_one`: same.
- **Paired parser fixtures + snapshots**:
  - `emphasis_skips_raw_html_and_autolink_commonmark/`
  - `emphasis_skips_raw_html_and_autolink_pandoc/`
  - Both snapshot to identical CSTs (no dialect divergence here).
  - Wired into `golden_parser_cases.rs` next to `emphasis_split_runs_*`.
- **Allowlist additions** (Emphasis and strong emphasis): #475,
  #476, #477, #480, #481.

No CommonMark formatter golden case added: block sequence is
unchanged (one paragraph per example); only inline shape changes
(raw HTML / autolink now siblings of TEXT instead of nested inside
EMPHASIS / STRONG). Per the rule, only add a CM formatter case when
block sequence diverges between dialects.

### Don't redo

- Don't gate this fix on `Dialect::CommonMark`. The Pandoc-mode
  parse for these inputs was *also* losslessness-breaking before,
  and pandoc itself produces the same shape under both flavors.
  Gating would regress Pandoc-mode for the same inputs.
- Don't move the autolink/raw-HTML skip ahead of the existing
  `try_parse_inline_link` skip — the bracket-link skip is for `[`
  starts, the new skips are for `<` starts; orthogonal but adjacent
  for code locality.
- Don't skip native_spans here. CommonMark doesn't have them, and
  the Pandoc native-span syntax `<span>...</span>` doesn't have the
  same delimiter-poisoning problem (no `*` inside opening tag).
  Adding it could subtly change Pandoc parses we already cover.
- Don't skip bare URIs (GFM `extensions.autolink_bare_uris`) here.
  Bare URIs don't start with `<` so they don't intercept the same
  way; if one ever does cause emphasis confusion, that's its own
  fix.

### Suggested next targets, ranked

1. **Proper delimiter-stack for emphasis (#402, #408, #412, #417,
   #426, #445, #457, #464, #465, #466, #468)** — rewrite emphasis
   to use CMark's process_emphasis algorithm (delimiter stack with
   leftover matching). Largest single fix; would unlock the 4+ char
   run cases (currently rejected outright) and the rule-of-3 cases.
   Substantial; gate on `Dialect::CommonMark`. Pandoc-markdown
   stays on the recursive enclosure parser.
2. **#472 `*foo *bar baz*`** — CommonMark expects `*foo <em>bar
   baz</em>` (outer `*` literal because inner content has unmatched
   delim flanking). Likely needs delimiter-stack work too.
3. **Formatter fix for nested-only outer LIST_ITEM** — carried over
   from (viii). Unblocks removing the dialect gate on same-line
   nested list markers (#298, #299). Probably one formatter change
   in `crates/panache-formatter/src/formatter/lists.rs`.
4. **#280 empty list item closes the list** — `-\n\n  foo\n`
   should produce empty LI + separate paragraph under CommonMark.
   Pandoc keeps `foo` as the list item content. Dialect divergence.
   Parser-shape gap, gate on CommonMark.
5. **#312 list with non-uniform marker indentation** — `- a\n -
   b\n  - c\n   - d\n    - e\n` should be 4 sibling items with
   `- e` as lazy paragraph continuation of `d`. Both dialects
   agree per pandoc. Parser-shape gap, universal.
6. **Tabs (#2, #5, #6, #7)** — column-aware tab expansion;
   substantial. (Carried over.)
7. **HTML block #148** — `</pre>` inside HTML block followed by
   blank-line content. (Carried over.)
8. **Reference link followed by another bracket pair (#569, #571)**
   — CMark left-bracket scanner stack model. Large. (Carried over.)
9. **Nested LINKs in link text (#518, #519, #520, #532, #533)** —
   CommonMark §6.4 forbids real nesting; outer must un-link. Same
   scanner-stack work. (Carried over.)
10. **HTML-tag/autolink interaction with link brackets (#524, #526,
    #536, #538)** — bracket scanner must skip past raw HTML and
    autolinks. The matching emphasis-side fix landed in (x); the
    bracket-side fix is the same idea applied to
    `try_parse_inline_link` / `try_parse_reference_link`'s bracket
    scan. (Carried over, now slightly easier since the pattern is
    already established.)
11. **Block quotes lazy-continuation #235, #251** — last two
    blockquote failures. (Carried over.)
12. **Fence inside blockquote inside list item (#321)**. (Carried
    over.)
13. **Same-line blockquote inside list item (#292, #293)** — `> 1. >
    Blockquote` needs the inner `>` to open a blockquote inside the
    list item. (Carried over.)
14. **#273, #274 multi-block content in `1.     code` items** —
    spaces ≥ 5 after marker means content_col is at marker+1 and the
    rest is indented code. (Carried over.)
15. **#278 `-\n  foo\n-\n  ```\n…`** — empty marker followed by
    indented content; multiple bugs. (Carried over.)
16. **#300 setext-in-list-item** — `- # Foo\n- Bar\n  ---\n  baz`
    should treat `Bar\n  ---` as setext h2. (Carried over.)
17. **Ref-def dialect divergence #201** — `[foo]: <bar>(baz)`. Low
    priority. (Carried over.)

### Carry-forward from prior sessions

The "Don't redo" notes from session (ix) about emphasis split-runs
("tail-end only" heuristic, dialect gate, not widening to
#402/#408/#412/#426) all still apply. Don't touch those paths
without first reading session (ix)'s recap.
