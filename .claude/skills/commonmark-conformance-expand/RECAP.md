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

## Latest session — 2026-04-29 (v)

**Pass count: 574 → 581 / 652 (89.1%, +7)**

All seven wins are in the Emphasis section, from a single
parser-shape fix for the underscore-emphasis closer rule. Both
Pandoc and CommonMark dialects agree on the failing cases
(verified with `pandoc -f commonmark` vs `pandoc -f markdown`),
so the fix is dialect-independent and tightens both flavors.

### Root cause: missing intraword check on `_` closer

`is_valid_ender` (used only by `try_parse_three`) correctly rejects
`_` closers followed by an alphanumeric character — the intraword
underscore rule. But the parallel closer check inside
`parse_until_closer_with_nested_two` and
`parse_until_closer_with_nested_one` (used by `try_parse_one` and
`try_parse_two`) only rejected `_` closers preceded by whitespace.
So `_foo_bar`, `_foo_bar_baz_`, `__foo__bar`, etc. all matched the
first `_` as opener and the inner `_` as closer, producing
`<em>foo</em>bar` instead of literal `_foo_bar`.

Pandoc itself rejects these too (intraword underscores extension
default). This was a latent Pandoc-flavor bug masked by the lack of
direct tests on `_foo_bar`-style inputs.

Fix: add a symmetric "for `_` closer, reject if followed by
alphanumeric" check at both closer-detection sites in
`crates/panache-parser/src/parser/inlines/core.rs`. Mirrors the
existing "preceded by whitespace" check directly above and the
rules in `is_valid_ender`. No `Dialect` gating — applies to all
dialects.

### Wins unlocked

- #372 `_(_foo)` — no emphasis
- #374 `_foo_bar` — no emphasis
- #375 `_пристаням_стремятся` — Cyrillic alnum, intraword
- #376 `_foo_bar_baz_` — outer pair emphasizes, inner `_`s literal
- #398 `__(__foo)` — strong-version of #372
- #400 `__foo__bar` — strong-version of #374
- #401 `__пристаням__стремятся` — strong-version of #375

### Files changed

- **Parser-shape gap**:
  - `crates/panache-parser/src/parser/inlines/core.rs`: in both
    `parse_until_closer_with_nested_two` and
    `parse_until_closer_with_nested_one`, add the
    "underscore closer followed by alphanumeric → not a closer"
    skip directly after the existing whitespace-preceded skip.
- **New parser fixture + snapshot**:
  - `emphasis_intraword_underscore_closer/input.md` (default
    flavor — fix applies to both dialects). Pins the literal-text
    CST for `_foo_bar`, `_foo_bar_baz_`, `__foo__bar`, `_(_foo)`.
- **Allowlist additions** (Emphasis): #372, #374, #375, #376,
  #398, #400, #401.

### Don't redo

- Don't add a paired CommonMark fixture for the underscore-closer
  fix. Pandoc-markdown and CommonMark agree on these inputs
  (verified with both pandoc front-ends). The default-flavor
  fixture is sufficient; a paired CommonMark fixture would be
  pure churn.
- Don't try to fix #373 `_(_foo_)_` with the same approach. It
  needs nested-one logic inside `parse_until_closer_with_nested_two`
  for the *same* delimiter character (currently we only try
  nested-two for the same delim). After this session's fix, it
  produces `<em>(_foo</em>)_` instead of the spec's
  `<em>(<em>foo</em>)</em>`. That's a separate, larger change.
- Don't conflate the asterisk failures (#352, #354, #366–369)
  with the underscore failures. Pandoc and CommonMark *disagree*
  on the asterisk cases (e.g., pandoc emphasizes `*$*alpha` to
  `<em>$</em>alpha`, CommonMark does not), so those need
  `Dialect::CommonMark` gating in the opener/closer flanking
  checks, not a flavor-agnostic tightening.

### Suggested next targets, ranked

1. **Asterisk flanking under CommonMark dialect (#352, #354,
   #366, #367, #368, #369)** — dialect divergence. Pandoc's
   asterisk opener/closer checks are looser than CommonMark's
   flanking rules. Gate stricter rules on
   `config.dialect == Dialect::CommonMark` in the closer-detect
   sites in `inlines/core.rs`, and at the `*` opener
   "followed-by-whitespace" check. Could unlock 6+ examples
   sharing the root cause.
2. **Underscore nested-one (#373)** — `_(_foo_)_` needs the inner
   `_foo_` to be parsed as nested emphasis when scanning for the
   outer `_` closer. Touches
   `parse_until_closer_with_nested_two` to also try nested-one
   for the same delim character (currently only nested-two is
   tried). One-example win, but groundwork for further `_`
   nesting.
3. **Empty list item closes the list when followed by blank line
   (#280)** — parser-shape gap.
4. **List with non-uniform marker indentation (#312)** —
   parser-shape gap; 4-space `- e` should be lazy continuation
   of preceding `- d`.
5. **Tabs (#2, #5, #6, #7)** — column-aware tab expansion;
   substantial.
6. **HTML block #148** — `</pre>` inside HTML block followed by
   blank-line content should be inline raw HTML in the resumed
   paragraph; renderer/parser disagreement on where the HTML
   block ends.
7. **Reference link followed by another bracket pair (#569,
   #571)** — CMark left-bracket scanner stack model. Large.
8. **Nested LINKs in link text (#518, #519, #520, #532, #533)** —
   CommonMark §6.4 forbids real nesting; outer must un-link.
   Same scanner-stack work.
9. **HTML-tag/autolink interaction with link brackets (#524,
   #526, #536, #538)** — bracket scanner must skip past raw HTML
   and autolinks.
10. **Block quotes lazy-continuation #235, #251** — last two
    blockquote failures.
11. **Fence inside blockquote inside list item (#321)**.
12. **Lazy / nested marker continuation (#298, #299)**.
13. **Multi-block content in `1.     code` items (#273, #274)**.
14. **Setext-in-list-item (#300)**.
15. **Ref-def dialect divergence #201** — `[foo]: <bar>(baz)`.
    Low priority.
