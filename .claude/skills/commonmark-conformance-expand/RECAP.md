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

## Latest session — 2026-04-29 (xi)

**Pass count: 611 → 615 / 652 (94.3%, +4)**

All wins in Links section. Bracket-side counterpart to session (x)'s
emphasis-side autolink/raw-HTML skip: link/image bracket scanner
(`find_link_close_bracket`) was matching `]` literally inside raw
HTML attribute values and (CommonMark-only) autolink URLs, so a
construct like `[foo <bar attr="](baz)">` was greedily linkified.

### Targets unlocked

- **#524** `[foo <bar attr="](baz)">` → literal `[foo ` + raw HTML
  with the `]` inside the attribute value (universal — both pandoc
  dialects agree).
- **#526** `[foo<https://example.com/?search=](uri)>` → literal
  `[foo` + autolink whose body contains `](uri)` (CommonMark only;
  Pandoc keeps the greedy link parse).
- **#536** same as #524 with reference-link `[ref]` form.
- **#538** same as #526 with reference-link `[ref]` form.

### Root cause

`find_link_close_bracket` already skips code spans (so `]` inside a
backtick run can't close the bracket — example #525). The same
opacity rule applies to:

1. **Raw HTML spans** (`<tag attr="..."` ... `>`) — universal across
   dialects per pandoc verification.
2. **Autolinks** (`<scheme://...>`) — CommonMark only. Pandoc-markdown
   does *not* treat these as opaque inside link text, so the same
   input parses differently between dialects (#526, #538).

Without the skip, a `]` inside `attr="..."` or inside the autolink
URL closed the link bracket, the link/image was emitted with
truncated text, and the trailing `"` / `>` was left as literal.

### Fix

`crates/panache-parser/src/parser/inlines/links.rs`:

1. New `LinkScanContext { skip_raw_html, skip_autolinks }` struct
   with `from_options(&ParserOptions)` constructor:
   - `skip_raw_html = extensions.raw_html` (universal).
   - `skip_autolinks = extensions.autolinks && dialect == CommonMark`.
2. `find_link_close_bracket` extended with a `b'<'` arm that tries
   `try_parse_autolink` (when `skip_autolinks`) then
   `try_parse_inline_html` (when `skip_raw_html`) and skips past
   the matched span. Order matters: autolink before raw HTML, since
   both start with `<` and autolink is the more specific shape.
3. `try_parse_inline_image`, `try_parse_inline_link`,
   `try_parse_reference_link` signatures grew a `LinkScanContext`
   parameter; production call sites in `core.rs` and
   `block_dispatcher.rs` build it from `config`; unit-test call
   sites pass `LinkScanContext::default()` (preserves prior
   behavior — no skipping in those tests).

### Files changed

- **Parser-shape / dialect-divergence gap**:
  - `crates/panache-parser/src/parser/inlines/links.rs`: new
    `LinkScanContext`; bracket scanner skips raw HTML / autolinks;
    `try_parse_*` signatures updated.
  - `crates/panache-parser/src/parser/inlines/core.rs`: pass
    `LinkScanContext::from_options(config)` at all link/image entry
    points + the emphasis-side skip-over-link calls.
  - `crates/panache-parser/src/parser/block_dispatcher.rs`: same
    for the standalone-image detector.
- **Paired parser fixtures + snapshots**:
  - `link_text_skips_raw_html_{commonmark,pandoc}/` — identical CST
    under both dialects (universal fix).
  - `link_text_skips_autolink_{commonmark,pandoc}/` — divergent CSTs:
    CommonMark recognizes the autolink, Pandoc keeps the greedy
    link parse.
  - Wired into `golden_parser_cases.rs` next to `links`.
- **Allowlist additions** (Links): #524, #526, #536, #538.
- Cleaned up an orphan
  `golden_parser_cases__parser_cst_list_nested_same_line_marker.snap.new`
  (pre-existing, unrelated; the fixture is now suffixed
  `_commonmark` / `_pandoc`).

No CommonMark formatter golden case added: only inline shape
changes; block sequence is unchanged (one paragraph per example).
Per the rule, only add a CM formatter case when block sequence
diverges between dialects.

### Don't redo

- Don't unify `skip_autolinks` and `skip_raw_html` into one flag.
  Pandoc-markdown skips raw HTML inside link text but does *not*
  skip autolinks — verified against pandoc. Conflating the two
  would regress Pandoc-mode parses (greedy link still expected for
  `[foo<https://...>...]`).
- Don't move the `b'<'` skip arm before `b'\\'` or `b'`'`. Backslash
  escape and code-span opacity are higher precedence per CommonMark
  §6, and the existing test #525 (code-span inside link text)
  depends on the order.
- Don't drop the `LinkScanContext` parameter from
  `try_parse_inline_image` even though `block_dispatcher.rs`'s
  standalone-image detector mostly hits trivially-shaped input —
  Quarto/Pandoc image attributes can in principle contain `<...>`
  in alt text, and threading the ctx keeps the bracket scanner
  consistent across all entry points.
- Don't widen the autolink skip to bare URIs (GFM
  `extensions.autolink_bare_uris`). Bare URIs don't start with `<`
  so they don't intercept the bracket scan; if one ever does cause
  link confusion, that's its own fix.

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
3. **Formatter fix for nested-only outer LIST_ITEM** — carried over.
   Unblocks removing the dialect gate on same-line nested list
   markers (#298, #299). Probably one formatter change in
   `crates/panache-formatter/src/formatter/lists.rs`.
4. **#280 empty list item closes the list** — `-\n\n  foo\n`
   should produce empty LI + separate paragraph under CommonMark.
   Pandoc keeps `foo` as the list item content. Dialect divergence.
   Parser-shape gap, gate on CommonMark. (Carried over.)
5. **#312 list with non-uniform marker indentation** — `- a\n -
   b\n  - c\n   - d\n    - e\n` should be 4 sibling items with
   `- e` as lazy paragraph continuation of `d`. Both dialects
   agree per pandoc. Parser-shape gap, universal. (Carried over.)
6. **Tabs (#2, #5, #6, #7)** — column-aware tab expansion;
   substantial. (Carried over.)
7. **HTML block #148** — `</pre>` inside HTML block followed by
   blank-line content. (Carried over.)
8. **Reference link followed by another bracket pair (#569, #571)**
   — CMark left-bracket scanner stack model. Large. (Carried over.)
9. **Nested LINKs in link text (#518, #519, #520, #532, #533)** —
   CommonMark §6.4 forbids real nesting; outer must un-link. Same
   scanner-stack work as #569/#571. (Carried over.)
10. **Block quotes lazy-continuation #235, #251** — last two
    blockquote failures. (Carried over.)
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
- The session (x) emphasis-side autolink/raw-HTML skip in
  `parse_until_closer_with_nested_*` is the load-bearing companion
  to this session's bracket-side fix. The two skip patterns must
  stay aligned: emphasis closer scan and link bracket scan both
  treat raw HTML as universally opaque, but only the link bracket
  scan gates autolink-skip on CommonMark dialect (emphasis-side
  skip is universal because Pandoc-markdown also treats autolinks
  as opaque to emphasis closers).
