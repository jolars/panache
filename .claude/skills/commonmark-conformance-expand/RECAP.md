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

## Latest session — 2026-04-29 (vi)

**Pass count: 581 → 592 / 652 (90.8%, +11)**

All eleven wins are in the Emphasis section, from a single
dialect-divergence fix: CommonMark's strict left/right-flanking
rules for `*` opener/closer detection. Pandoc and CommonMark
disagree (verified with `pandoc -f commonmark` vs
`pandoc -f markdown`), so the fix is gated on
`config.dialect == Dialect::CommonMark`.

### Root cause: looser asterisk flanking than CommonMark §6.2 requires

Pandoc-markdown's `ender` for `*` has no flanking rule beyond
"not preceded by whitespace" (and even that is `_`-only in
panache today). CommonMark §6.2 is strict:

- A `*` can only **open** if it is part of a *left-flanking*
  delimiter run.
- A `*` can only **close** if it is part of a *right-flanking*
  delimiter run.

The flanking rules use both adjacent characters (whitespace and
punctuation/symbol from Unicode P+S categories), so cases like
`a*"foo"*` (alnum-then-punct opener) and `*$*alpha.`
(punct-then-alnum closer) are literal under CommonMark but
emphasis under Pandoc.

Fix: add `is_left_flanking` / `is_right_flanking` helpers plus a
permissive `is_unicode_punct_or_symbol` (ASCII punctuation, or
non-ASCII non-alnum non-whitespace — wider than strict P+S but
matches every relevant spec case without a Unicode-categories
crate). Gate on CommonMark dialect at three sites in
`crates/panache-parser/src/parser/inlines/core.rs`:

1. `try_parse_emphasis` (entry-level opener): reject `*` opener
   that is not left-flanking.
2. `parse_until_closer_with_nested_two` (used while scanning
   `*...*` for its closer): reject `*` closer that is not
   right-flanking.
3. `parse_until_closer_with_nested_one` (used while scanning
   `**...**` for its closer): same right-flanking rejection.

Pandoc behavior is unchanged (the gate is dialect-specific).

### Wins unlocked

- #352 `a*"foo"*` — alnum-before opener rejected
- #354 `*$*alpha.` (incl. `£`/`€`) — punct→alnum closer rejected
- #366 `*foo bar *` — whitespace-before closer rejected
- #367 `*foo bar\n*` — same with `\n`
- #368 `*(*foo)` — punct→alnum closer rejected
- #380 `a**"foo"**` — strong-version of #352
- #391 `**foo bar **` — strong-version of #366
- #392 `**(**foo)` — strong-version of #368
- #427 `**foo **bar****` — bonus: rejected mid-run `**` closer
  resolves the nested-strong shape
- #470 `*foo __bar *baz bim__ bam*` — bonus: outer `*…*` no
  longer matches the inner `*` (right-flanking by punct/alnum
  context fails), so the underscore strong runs cleanly
- #471 `**foo **bar baz**` — bonus: same logic for `**`

### Files changed

- **Dialect divergence**:
  - `crates/panache-parser/src/parser/inlines/core.rs`: new
    flanking helpers + three CommonMark-gated rejections in
    `try_parse_emphasis`, `parse_until_closer_with_nested_two`,
    and `parse_until_closer_with_nested_one`.
- **New paired parser fixtures + snapshots**:
  - `emphasis_asterisk_flanking_commonmark/` — pins literal-text
    CST under CommonMark for the six representative cases
    (#352, #354, #366, #368, #391, #392).
  - `emphasis_asterisk_flanking_pandoc/` — same inputs, pins
    EMPHASIS/STRONG CST under Pandoc-markdown.
  - Wired into `golden_parser_cases.rs` next to existing
    `emphasis_*` cases.
- **Allowlist additions** (Emphasis): #352, #354, #366, #367,
  #368, #380, #391, #392, #427, #470, #471.

No CommonMark formatter golden case added: the new CommonMark
output is structurally identical (PARAGRAPH only) to the Pandoc
path — the rule explicitly says skip the formatter case unless
block sequence differs. Verified idempotency manually
(`format(format(x)) == format(x)` for the CommonMark output, which
is just escaped `\*` in literal text).

### Don't redo

- Don't apply the flanking gate to `try_parse_emphasis_nested`.
  That helper deliberately bypasses the "followed by whitespace"
  opener check for nested contexts; tightening it under
  CommonMark is plausibly correct but would need its own probe
  and may regress nested cases. Out of scope for this session.
- Don't apply the flanking gate inside `is_valid_ender` (used by
  `try_parse_three`). The triple-asterisk path has its own rules
  and the `***` failures (#416, #417) are the rule-of-3 nesting
  story, not flanking.
- Don't add a paired *Pandoc* formatter golden case — existing
  formatter goldens already cover Pandoc emphasis behavior.
- Don't expect #369 `*(*foo*)*` to fall out of this fix. With
  flanking, the outer `*` closer is now correctly placed at the
  trailing `*`, but we still need same-delim nested-emphasis logic
  inside `parse_until_closer_with_nested_two` to recognize the
  inner `*foo*`. Currently produces `<em>(*foo</em>)*` — wrong.
  Same structural change blocks #369, #389, #407–409, #425–426.
- Don't approximate CommonMark punctuation with strict
  ASCII-only. The spec includes Unicode P+S categories — `£`/`€`
  in #354 must register as punctuation. The current
  `!alnum && !whitespace` heuristic covers them.

### Suggested next targets, ranked

1. **Same-delimiter nested emphasis (#369, #389, #407, #408,
   #409, #425, #426, #427)** — `parse_until_closer_with_nested_two`
   currently only attempts nested `**` inside `*...*`. CommonMark
   needs nested same-delim too: when scanning `*...*` and we hit
   another `*` that fails as a closer (because not right-flanking)
   but would be a valid opener, try parsing `*X*` as nested EMPH.
   Symmetric in `parse_until_closer_with_nested_one`. Could unlock
   6–7 examples. Largest fix in this group.
2. **Rule of 3 (#402, #412, #416, #417)** — CommonMark §6.2:
   "If one of the delimiters can both open and close (strong)
   emphasis, then the sum of the lengths of the delimiter runs
   containing the open and close delimiters must not be a
   multiple of 3 unless both lengths are multiples of 3." Touches
   the closer-detection logic when both flanking conditions are
   satisfied. Cross-dialect divergence; gate on
   `Dialect::CommonMark`.
3. **Underscore nested-one (#373)** — companion to the
   same-delim nested-emphasis work in (1) but for `_`. After
   landing (1), this should be a small extension.
4. **Empty list item closes the list when followed by blank line
   (#280)** — parser-shape gap.
5. **List with non-uniform marker indentation (#312)** —
   parser-shape gap; 4-space `- e` should be lazy continuation
   of preceding `- d`.
6. **Tabs (#2, #5, #6, #7)** — column-aware tab expansion;
   substantial.
7. **HTML block #148** — `</pre>` inside HTML block followed by
   blank-line content should be inline raw HTML in the resumed
   paragraph; renderer/parser disagreement on where the HTML
   block ends.
8. **Reference link followed by another bracket pair (#569,
   #571)** — CMark left-bracket scanner stack model. Large.
9. **Nested LINKs in link text (#518, #519, #520, #532, #533)** —
   CommonMark §6.4 forbids real nesting; outer must un-link.
   Same scanner-stack work.
10. **HTML-tag/autolink interaction with link brackets (#524,
    #526, #536, #538)** — bracket scanner must skip past raw HTML
    and autolinks.
11. **Block quotes lazy-continuation #235, #251** — last two
    blockquote failures.
12. **Fence inside blockquote inside list item (#321)**.
13. **Lazy / nested marker continuation (#298, #299)**.
14. **Multi-block content in `1.     code` items (#273, #274)**.
15. **Setext-in-list-item (#300)**.
16. **Ref-def dialect divergence #201** — `[foo]: <bar>(baz)`.
    Low priority.
