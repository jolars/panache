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

## Latest session — 2026-04-29 (xviii)

**Pass count: 621 → 625 / 652 (95.9%, +4)**

Inline-link nesting prohibition under CommonMark dialect (§6.4):
"Links may not contain other links, at any level of nesting."
Implemented as a parser-side rejection of the outer link/ref-link
when the candidate link text contains a valid inner *inline* link.
Image *descriptions* are not links, so they may legitimately
contain a link (#517/#531 stay green); but a LINK *inside* an
image's alt still deactivates outer LINK openers, so the helper
recurses into image alt text. Reference-link nesting (#533/#569/
#571) is **not** addressed — it requires resolving labels against
the document's refdef map, which the parser does not have.

### Targets unlocked

- **#518** `[foo [bar](/uri)](/uri)` → outer rejected; `[foo `
  text + `[bar](/uri)` link + `](/uri)` text.
- **#519** `[foo *[bar [baz](/uri)](/uri)*](/uri)` → all outer
  brackets become text, inner-most `[baz](/uri)` survives as a
  link inside emphasis (recursive emit applies the same rule at
  every level).
- **#520** `![[[foo](uri1)](uri2)](uri3)` → image survives;
  description recursively parses to literal `[` + link
  `foo→uri1` + literal `](uri2)`, alt-text plain string is
  `[foo](uri2)`.
- **#532** `[foo [bar](/uri)][ref]` (with `[ref]: /uri`) →
  outer ref-link rejected (because text contains valid inline
  link); inner inline link survives, then `[ref]` resolves as a
  shortcut.

### Root cause

`try_parse_inline_link` and `try_parse_reference_link` greedily
matched outer brackets without honoring CommonMark's bracket-
deactivation rule. The parser would accept `[foo [bar](/uri)](/uri)`
as an outer LINK whose LINK_TEXT recursively parsed back to the
inner LINK — the CST nested LINKs, which is forbidden under
CommonMark. Pandoc, by contrast, *does* allow nested LINKs (verified
via `pandoc -f markdown -t native`), so this is a structural
**dialect divergence**, not a per-feature toggle.

### Fix

- `crates/panache-parser/src/parser/inlines/links.rs`:
  - Added `disallow_inner_links: bool` field to `LinkScanContext`,
    set true under `Dialect::CommonMark` via
    `LinkScanContext::from_options`.
  - New helper `link_text_contains_inner_link(text, ctx, strict_dest)`
    scans candidate link text byte-by-byte (skipping code spans,
    backslash escapes, and dialect-gated raw HTML / autolink
    spans), and returns `true` as soon as it finds:
    - a `[` that starts a valid inline link (recursive
      `try_parse_inline_link` call, which itself applies the
      same rule), or
    - an `![` whose image's alt text contains an inner link
      (recursive `link_text_contains_inner_link` on the alt).
    Images by themselves do not count, so `[link with ![img]]`
    stays a valid link.
  - Gated the rule inside `try_parse_inline_link` and
    `try_parse_reference_link` on `ctx.disallow_inner_links`,
    after the close-bracket / dest validation steps. Image
    parsing (`try_parse_inline_image`) is **not** gated — images
    can legitimately contain links.

### Files changed

- **Dialect divergence** (CommonMark only):
  - `crates/panache-parser/src/parser/inlines/links.rs`:
    `LinkScanContext` gains `disallow_inner_links`; new
    `link_text_contains_inner_link` helper; gate added at the
    end of `try_parse_inline_link` and just after
    close-bracket extraction in `try_parse_reference_link`.
- **Parser fixtures** (paired):
  - `link_inside_link_text_commonmark/{input.md,parser-options.toml}`
    pins the outer-rejected CST (text + LINK + text).
  - `link_inside_link_text_pandoc/{input.md,parser-options.toml}`
    pins the outer-LINK-with-inner-LINK CST (Pandoc behavior
    unchanged).
  - Both wired into `golden_parser_cases.rs` before
    `link_text_skips_autolink_*`.
- **Formatter golden case** (CommonMark only — different
  formatted output than Pandoc default):
  - `tests/fixtures/cases/link_inside_link_text_commonmark/`
    with `panache.toml` setting `flavor = "commonmark"`. Pins
    `\[foo [bar](/uri)\](/uri)` — escaped brackets so the
    re-parse keeps the outer brackets literal. (Pandoc default
    keeps `[foo [bar](/uri)](/uri)` verbatim and is already
    covered by existing `links` golden.)
- **Allowlist additions** (Links section): #518, #519, #520,
  #532.

### Don't redo

- **Don't apply the rule to images.** Per CommonMark §6.5 + §6.4
  the prohibition is on nested *links*, not on links-inside-images.
  An earlier draft of `link_text_contains_inner_link` returned
  true on any inner image, which silently regressed #517 + #531.
  The fix recurses into image alt text but does not itself fail
  on the image.
- **Don't apply the rule to `try_parse_inline_image` directly.**
  Same reason: images can contain links. Only the *recursive
  parse* of an image's alt content should re-apply the rule, via
  the standard inline-parse path that already uses the same ctx.
- **Don't try to extend the rule to reference-link nesting (#533,
  #569, #571).** Those need the refdef map at parse time. The
  helper only detects inline-link shape (`[X](dest)`), not ref
  link shape (`[X][Y]`), because matching `[Y]` requires resolving
  against refs that aren't yet collected at this point in the
  pipeline.
- **Don't gate `disallow_inner_links` on an extension flag** —
  it tracks the structural CommonMark identity (per
  `.claude/rules/parser.md`), not a single named feature.
- **Don't widen the formatter case to also test Pandoc.** The
  Pandoc default output (`[foo [bar](/uri)](/uri)`) is already
  exercised by existing `links` and similar fixtures; adding a
  paired `*_pandoc` formatter case would be churn.

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
3. **Reference-link nesting (#533, #569, #571)** — CMark
   left-bracket scanner stack with refdef-aware resolution. The
   helper added this session deliberately handles only inline links;
   ref-link nesting needs a different pass that has access to the
   collected refdef map. Probably the next big link cluster.
4. **Formatter fix for nested-only outer LIST_ITEM** — carried over.
   Unblocks removing the dialect gate on same-line nested list
   markers (#298, #299). Probably one formatter change in
   `crates/panache-formatter/src/formatter/lists.rs`.
5. **Tabs (#2, #5, #6, #7)** — column-aware tab expansion;
   substantial. (Carried over.)
6. **Fence inside blockquote inside list item (#321)**. (Carried
   over.)
7. **Same-line blockquote inside list item (#292, #293)** — `> 1. >
   Blockquote` needs the inner `>` to open a blockquote inside the
   list item. (Carried over.)
8. **#273, #274 multi-block content in `1.     code` items** —
   spaces ≥ 5 after marker means content_col is at marker+1 and the
   rest is indented code. (Carried over.)
9. **#278 `-\n  foo\n-\n  ```\n…`** — empty marker followed by
   indented content; multiple bugs. (Carried over.)
10. **#300 setext-in-list-item** — `- # Foo\n- Bar\n  ---\n  baz`
    should treat `Bar\n  ---` as setext h2. (Carried over.)
11. **#523 `*foo [bar* baz]`** — emphasis closes inside link bracket
    text mid-flight. Probably needs delimiter-stack work + bracket
    scanner integration. (Carried over.)

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
- Session (xvii)'s HTML block #148 fix (`</pre>` rejection in the
  VERBATIM_TAGS branch) is orthogonal to this session's link
  nesting work — it's still active under CommonMark and remains
  Pandoc-unreachable via `extract_block_tag_name(_, false)`.
- This session's `disallow_inner_links` flag and
  `link_text_contains_inner_link` helper are deliberately scoped
  to inline links only. When extending to reference-link nesting
  (#533/#569/#571), do not retrofit the helper — that path needs
  refdef resolution, which doesn't fit the current parser-only
  scan.
