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

## Latest session — 2026-04-29 (vii)

**Pass count: 592 → 597 / 652 (91.6%, +5)**

All five wins are in the Emphasis section, from one same-delim
nested-emphasis fix that applies under both dialects.

### Root cause: panache's recursive scanner missed nested same-delim openers

When scanning the inside of `*X*` for its closer,
`parse_until_closer_with_nested_two` only attempted nested `**`
(strong) — it never attempted another `*X*`. Symmetric gap in
`parse_until_closer_with_nested_one` for `__X__`. So inputs like
`*(*foo*)*` and `__foo, __bar__, baz__` resolved to a single flat
emphasis at the first available closer, dropping the nested span
panache should produce. Pandoc-markdown agrees with CommonMark
here for `_(_foo_)_` and the `__` cases, so the fix is universal.
For the `*` cases (#369, #407) the dialects diverge but the
existing CommonMark right-flanking gate already prevents the
outer's premature close; the new nested attempt rounds out the
result.

Fix: when an isolated delim run of exactly `delim_count` fails
the closer check (right-flanking for `*` under CommonMark, or
the underscore intraword/right-flanking gates under both
dialects), retry it as a *nested same-delim opener* via
`try_parse_emphasis` with a throwaway builder. On success, skip
the consumed bytes and keep scanning for the outer closer; on
failure, advance one char without poisoning the outer.

Critical heuristic to avoid regressing #470: only attempt the
nested when there are at least `2 * delim_count` `delim_char`s
remaining ahead. Without this, `*foo __bar *baz bim__ bam*` (#470)
greedily binds the `*` at "bar *baz" to the trailing `*` and
leaves the outer `*` with no closer. The "≥ 2*delim_count
remaining" check ensures both the nested span and the outer
emphasis can plausibly close. CommonMark's stack algorithm
naturally handles this via opener removal during inner closer
matching; we approximate with the count check.

Also factored the closer-validity check (underscore intraword,
underscore right-flanking, asterisk CommonMark right-flanking)
out of the inline `continue` paths into a single
`is_valid_same_delim_closer` helper to make the new
"closer-failed-at-isolated-run" branching readable.

### Wins unlocked

- #369 `*(*foo*)*` → `<em>(<em>foo</em>)</em>` (CM only)
- #373 `_(_foo_)_` → `<em>(<em>foo</em>)</em>` (both dialects)
- #389 `__foo, __bar__, baz__` → nested STRONG (both dialects)
- #407 `*foo *bar* baz*` → nested EMPH (CM only)
- #425 `__foo __bar__ baz__` → nested STRONG (both dialects)

### Files changed

- **Parser-shape (universal nested-same-delim) + dialect
  approximation of CommonMark's stack semantics**:
  - `crates/panache-parser/src/parser/inlines/core.rs`:
    - New `is_valid_same_delim_closer` helper consolidating the
      `_`/`*` closer rules.
    - New `has_enough_delim_chars_ahead` heuristic guarding
      nested attempts (≥ 2 * delim_count).
    - In both `parse_until_closer_with_nested_two` (delim_count=1)
      and `parse_until_closer_with_nested_one` (delim_count=2):
      rewired the closer-check branch to set a
      `closer_failed_at_isolated_run` flag and, when set, invoke
      `try_parse_emphasis` for nested same-delim with the
      heuristic gate. On success skip; on failure advance.
- **New paired parser fixtures + snapshots**:
  - `emphasis_same_delim_nested_commonmark/` — CST snapshot
    pinning nested EMPH/STRONG for all five inputs.
  - `emphasis_same_delim_nested_pandoc/` — same inputs, pins
    Pandoc-markdown CST: `*(*foo*)*` is two adjacent EMPH and
    `*foo *bar* baz*` is partial-EMPH-with-literal-tail
    (matches `pandoc -f markdown -t native`); the underscore
    cases match CommonMark.
  - Wired into `golden_parser_cases.rs` next to existing
    `emphasis_*` cases.
- **Allowlist additions** (Emphasis): #369, #373, #389, #407,
  #425.

No CommonMark formatter golden case added: while the formatted
output of `*foo *bar* baz*` differs between CM
(`*foo *bar* baz*`) and Pandoc (`*foo* bar\* baz\*`), the block
sequence is unchanged (still PARAGRAPH-only). Verified
idempotency manually under CommonMark (`format(format(x)) ==
format(x)` for all five inputs).

### Don't redo

- Don't drop the `has_enough_delim_chars_ahead` heuristic. It
  isn't strictly correct CommonMark semantics (the real
  algorithm uses delimiter-stack opener-removal when nested
  other-delim emphasis closes), but it preserves #470 and #471
  which are currently in the allowlist. Removing it regresses
  #470 immediately.
- Don't try to apply this approach to #408 / #409 / #426 /
  #402. Those need *partial-match* logic (a length-2 opener
  matching against a length-1 closer, leaving residual
  delimiters) — that's the next step toward a real delimiter
  stack and is a substantially larger refactor.
- Don't try to apply this to #416 / #417 (rule-of-3). Different
  story: closer rejection when the open+close delimiter run
  lengths sum to a non-trivial multiple of 3.
- Don't pass the inner content via the temp_builder. We
  intentionally discard it; the outer emphasis's content gets
  re-parsed via `parse_inline_range_nested` once the outer
  closer is found, which correctly emits the nested span.
- Don't tighten `try_parse_emphasis_nested` here either —
  that's still its own follow-up and not load-bearing for these
  five wins.

### Suggested next targets, ranked

1. **Partial-match emphasis (#402, #408, #426)** — the
   delimiter-stack idea: a `__` opener can match a single `_`
   closer (consuming one of two `_`s), leaving residual openers
   for later closers. Currently `try_parse_two` looks for a
   length-2 closer only. Largest tractable fix; needs care to
   avoid regressing existing `__` behavior and to keep Pandoc
   parity. Likely unlocks 3+ examples.
2. **Rule of 3 (#412, #416, #417)** — CommonMark §6.2:
   "If one of the delimiters can both open and close (strong)
   emphasis, then the sum of the lengths of the delimiter runs
   containing the open and close delimiters must not be a
   multiple of 3 unless both lengths are multiples of 3."
   Cross-dialect divergence; gate on `Dialect::CommonMark`.
3. **#409 `*foo *bar**`** — needs partial-match for `**` closer
   to be split between inner `*` and outer `*`. Companion to
   (1).
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
