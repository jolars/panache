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

## Latest session — 2026-04-30 (xxvii)

**Pass count: 647 → 648 / 652 (99.4%, +1)**

Took the carried-over #300 setext-in-list-item target from
session (xxvi). The List items section moved from 47 pass /
1 fail to **48 pass / 0 fail** — fully clean.

### Targets unlocked

- **#300** (List items): `- # Foo\n- Bar\n  ---\n  baz\n`
  — single-line setext h2 inside a list item, followed by a
  plain continuation.

### Root cause + fix

The list parser consumes the marker line `- Bar` and
buffers `Bar\n` in the LIST_ITEM's text buffer (no
PARAGRAPH is opened). When the dispatcher processes the
next line `  ---`, it has no way to "see" the buffered
text as a setext-text candidate, so the thematic-break
parser claims the underline and emits `<hr />` between two
plain blocks instead of folding them into a setext
heading.

The fix introduces
`Parser::try_fold_list_item_buffer_into_setext` (called
from `parse_inner_content` *before* `dispatcher_match` is
computed). It fires only when the innermost container is
`ListItem` with exactly one buffered text segment, the
current line's indent is ≥ the item's `content_col`
(CommonMark §5.2 — bare `---` at column 0 still escapes
the item), and the buffered text + current line satisfies
`try_parse_setext_heading`. On match it emits a `HEADING`
node directly via `emit_setext_heading`, clears the
buffer, advances `pos`, and returns true.

The fix is dialect-agnostic. Pandoc-markdown agrees on the
single-line case (`pandoc -f markdown` returns
`Header 2 [Str "Bar"], Plain [Str "baz"]` for the same
input); multi-line setext inside list items *is*
dialect-divergent (Pandoc treats it as continuation text)
and is intentionally out of scope here — the helper bails
when `segment_count() != 1`.

**Files touched:**

- **MOD** `crates/panache-parser/src/parser/core.rs`:
  added imports (`emit_setext_heading`,
  `try_parse_setext_heading`); new method
  `try_fold_list_item_buffer_into_setext`; one call site in
  `parse_inner_content` immediately before the initial
  `dispatcher_match` detection.
- **NEW** parser fixture
  `crates/panache-parser/tests/fixtures/cases/setext_heading_in_list_item_commonmark/`
  with `flavor = "commonmark"`, wired into
  `golden_parser_cases.rs` and a new insta snapshot.
- **MOD** `crates/panache-formatter/src/formatter/lists.rs`:
  added `is_loose_trigger_block` helper plus two new
  loose-detection conditions to the existing
  `is_loose` calculation:
  - `has_blank_within_item` — CMark §5.3 spec rule
    (item has 2+ block-level children separated by a
    blank line). Was missing; affected items like
    `- foo\n\n  bar\n- baz` whose item 1 had
    PLAIN+BLANK+PLAIN.
  - `has_structural_multi_block` — pandoc-flavored
    rule that any item with 2+ block-level children
    *where at least one is HEADING / CODE_BLOCK /
    HORIZONTAL_RULE* forces the list loose. Required
    for #300 (HEADING + PLAIN with no source blank).
    HTML_BLOCK is intentionally excluded so panache's
    own `<!-- panache-ignore-* -->` directives don't
    flip otherwise-tight lists.
- **NEW** formatter golden case
  `tests/fixtures/cases/setext_heading_in_list_item/`
  (default flavor, no `panache.toml`) wired into
  `golden_cases.rs`. The formatter now produces a
  blank-separated loose layout matching pandoc:
  `- # Foo\n\n- ## Bar\n\n  baz\n`.
- **MOD** `tests/commonmark/allowlist.txt` — appended `300`
  in sorted order under the existing List items section.

### Don't redo

- **Don't drop the indent guard**
  (`underline_indent_cols < content_col → return false`).
  Without it, `- Foo\n---\n` (#94/#99: `---` at column 0)
  and `- foo\n-\n- bar\n` (#281/#282: bare `-` sibling
  marker) flip to spurious setext headings inside the
  outgoing list item. Caught during initial implementation
  via the allowlist guard.
- **Don't lift the `segment_count() != 1` gate to handle
  multi-line setext inside list items.** Pandoc and
  CommonMark disagree on whether
  `- Foo\n  Bar\n  ---\n` is a setext heading
  (CommonMark: yes; Pandoc: paragraph with em-dash). If a
  future session wants to cover the multi-line case, it
  must dialect-gate to CommonMark and add paired parser
  fixtures.
- **Don't move the call site below `dispatcher_match`
  detection.** The thematic-break parser would otherwise
  claim `---` first; the helper is correctness-load-bearing
  to run before block detection.
- **Don't rely on the renderer's `<hr />` heuristic to
  hide regressions.** The pre-fix CST had the buffer
  flushed as a separate `PLAIN` followed by a real
  `HORIZONTAL_RULE` node — the parser, not the renderer,
  was producing the wrong shape. Confirmed by inspecting
  `cargo run -- parse --config /tmp/cm.toml`.
- **Don't include HTML_BLOCK in
  `is_loose_trigger_block`.** The `ignore_directives`
  fixture has LIST_ITEMs containing PLAIN + HTML_BLOCK
  + PLAIN + HTML_BLOCK, where the HTML blocks are
  panache-specific ignore-format directives. Pandoc
  treats raw HTML inline so the surrounding list stays
  tight; if HTML_BLOCK is counted, the formatter starts
  inserting blank lines between items (one of the
  initial broader rules I tried regressed exactly this
  fixture).
- **Don't drop the `!has_nested_lists` exclusion
  unconditionally.** Lists like `- a\n  - b\n- c` need
  to stay tight; nested LISTs are not block children
  that count toward looseness. The combined
  `(... || has_structural_multi_block) && !has_nested_lists`
  formulation keeps the established tight-with-nested
  behavior while letting HEADING/CODE/HR force loose.

### Suggested next targets, ranked

The remaining 4 failures (#523, #533, #569, #571) all
sit in **Links** and all require the link-bracket scanner
work that the prior recap labeled as the next destination
for the inline IR migration. None of the four is
tractable as a small, self-contained fix.

1. **Inline IR migration** — turn
   `delimiter_stack`'s `Vec<DelimRun>` into a real
   linked-list IR with `Text`,
   `Construct(GreenNode)`, `DelimRun`, `EmphasisGroup`,
   `BracketMarker` events. Move the byte walk in
   `parse_inline_range_impl` (CommonMark path) into an
   IR-builder pass, and emission into an IR-walker.
   Prerequisite for the bracket fixes.
2. **#523 / #533** — emphasis closing inside link
   bracket text, and the inner-link → outer-`[…][ref]`
   suppression rule.
3. **#569 / #571** — `[foo][bar][baz]` reference-link
   nesting. Needs the bracket scanner plus a
   refdef-aware resolution pass that scans
   right-to-left (or with three-pair lookahead).
4. **Pandoc dialect migration onto the unified
   algorithm.** Once the IR is in place, parameterize
   `process_emphasis`'s flanking predicates by
   `Dialect` and run Pandoc through it too.
5. **Formatter fix for nested-only outer LIST_ITEM** —
   carried prerequisite for lifting the same-line
   nested-LIST and blockquote-in-list-item dialect gates.
6. **Multi-line setext inside list items** (CommonMark
   only) — paired parser + formatter fixtures, dialect-
   gated. Strictly cosmetic; no spec example exercises
   it in the conformance harness today.

### Carry-forward from prior sessions

(Carrying forward from session xxvi unless noted.)

- The session (xxvi) emphasis carry-forward (delimiter
  stack, `EmphasisPlan`, `Vec<DelimRun>` representation,
  the four "don't redo" notes about
  `coalesce-on-Literal`, `openers_bottom`, byte-keyed
  plan, plan threading through nested recursion) is all
  still load-bearing on the CommonMark inline path.
  Don't extend that module to bracket markers without
  first migrating to the linked-list IR.
- Session (xxv)'s setext list-item indent guard and
  `in_marker_only_list_item` flag in
  `block_dispatcher.rs` are load-bearing for #278; this
  session's fold helper is *additive* (different code
  path, different state) and does not interact with
  them. Don't try to merge the two.
- The session (ix) "tail-end only" emphasis heuristic and
  Pandoc dialect gate still apply to the Pandoc inline
  path only; session (xxvi) replaced the CommonMark path
  entirely.
- Session (x)/(xi) link-scanner skip pattern (autolink /
  raw-HTML opacity for emphasis closer + link bracket
  close) is load-bearing for #524/#526/#536/#538. Don't
  unify the autolink and raw-HTML skip flags — Pandoc
  treats them differently. The bracket scanner work for
  #523/#533/#569/#571 will need to interoperate with
  these flags, not replace them.
- Session (xii)'s lazy paragraph continuation across
  reduced blockquote depth, session (xiii)'s
  `try_lazy_list_continuation` for OpenList only at
  `indent_cols ≥ 4`, session (xvii)'s HTML block #148
  fix (`</pre>` rejection in VERBATIM_TAGS), session
  (xviii)'s `disallow_inner_links` flag scope (inline
  links only — reference-link nesting #569/#571 needs a
  different pass), session (xix)/(xxi)/(xxii)'s
  column-aware indented-code logic (list-item
  `marker_spaces_after` and `virtual_marker_space`
  separate from blockquote `virtual_absorbed`), and
  session (xxiv)'s same-line blockquote-in-list-item
  branch dialect gate are all unchanged and unaffected
  by this session.


