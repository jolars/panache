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

## Latest session — 2026-04-30 (xxx)

**Pass count: 651 → 652 / 652 (100.0%, +1)** 🎉

User asked to focus on the inline IR migration from
session (xxix)'s #2 next target. Solved the
pass-ordering problem the prior session got stuck on,
wired the IR into production emission, unlocked the
last remaining failure **#533**.

### Targets unlocked

- **#533** — `[foo *bar [baz][ref]*][ref]` with
  `[ref]: /uri`. The IR's `process_brackets` resolves
  the inner `[baz][ref]` first; its
  `deactivate_earlier_link_openers` then marks the
  outer `[foo ...][ref]` opener inactive. With the IR
  driving emission, the outer brackets now correctly
  fall through to literal text. §6.3 link-in-link
  suppression for *reference* links — what session
  (xxix) couldn't reach with the refdef-aware gate
  alone.

### Architecture: what landed

The "real" inline IR migration:

1. **Pass-ordering fix in
   `inline_ir::build_full_plans`** —  reordered to
   `process_brackets` first, then per-resolved-bracket
   scoped `process_emphasis_in_range` (innermost
   first), then a top-level emphasis pass with an
   exclusion bitmap that hides events inside resolved
   bracket pairs. This implements strategy (a) from
   the prior session's plan and matches the CMark
   reference impl's interleaving.

2. **`process_emphasis_in_range_filtered`** — internal
   range-scoped variant with an optional event-index
   exclusion bitmap. The top-level pass uses it to
   skip delim runs inside resolved bracket pairs (so
   emphasis can't pair across a link's bracket
   boundary — spec #473's regression case from the
   prior failed wiring attempt).

3. **Wiring in `core::parse_inline_text_recursive` /
   `parse_inline_text`** — switched from
   `delimiter_stack::build_plan` to
   `inline_ir::build_full_plans`. The IR now produces
   both the `EmphasisPlan` (via
   `build_emphasis_plan` → `from_dispositions`) and
   the `BracketPlan` consumed by
   `parse_inline_range_impl`.

4. **`bracket_says_resolved` gate in
   `parse_inline_range_impl`** — added to the
   inline-image / reference-image / inline-link /
   reference-link branches. When the IR's bracket
   plan exists and says a position is NOT a resolved
   `Open`, those branches are skipped so the bracket
   bytes coalesce into TEXT. This is what makes #533
   work: the outer `[foo ...][ref]` is plan-Literal
   even though its label resolves, because the IR
   ran link-in-link suppression. The legacy
   `try_parse_reference_link` would still match
   shape-only without this gate. Note: footnote /
   citation / bracketed-span / native-span branches
   are NOT gated, since those extensions are off
   under the CommonMark flavor and the IR doesn't
   model them.

5. **Bug fix in `process_brackets`** — removed the
   `is_followed_by_inline_or_full_ref_or_collapsed`
   check on the shortcut-form fallback. Per
   CommonMark §6.3, when inline `(dest)` and full
   `[label]` / collapsed `[]` forms have already been
   tried and failed, shortcut form must match
   regardless of what byte follows the `]`. Spec
   example #568 (`[foo](not a link)` →
   `<a>foo</a>(not a link)`) requires this: the
   inline form fails on the invalid URL, then
   shortcut resolves `[foo]` even though `(` follows.

### Files touched

- `crates/panache-parser/src/parser/inlines/inline_ir.rs`
  — split `process_emphasis` into `process_emphasis`
  + `process_emphasis_in_range` +
  `process_emphasis_in_range_filtered`; reordered
  `build_full_plans` to bracket-first +
  scoped-emphasis + top-level-with-exclusion;
  removed `is_followed_by_inline_or_full_ref_or_collapsed`;
  added 2 regression tests (#473 boundary, #533
  link-in-link).
- `crates/panache-parser/src/parser/inlines/core.rs`
  — switched both CommonMark dispatchers to
  `build_full_plans`; added `bracket_says_resolved`
  closure and applied it to the four link/image
  branches.
- `crates/panache-parser/tests/commonmark/allowlist.txt`
  — added `533` under `# Links`.
- `crates/panache-parser/tests/fixtures/cases/link_in_link_reference_commonmark/`
  — NEW parser fixture (`flavor = "commonmark"`)
  pinning the §6.3 link-in-link CST shape.
- `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_link_in_link_reference_commonmark.snap`
  — NEW snapshot.
- `crates/panache-parser/tests/golden_parser_cases.rs`
  — registered the new case.

### Don't redo

- **Don't add the `is_followed_by_inline_or_full_ref_or_collapsed`
  guard back to the shortcut branch.** That bug masked
  #568 and similar cases. The shortcut form is the
  last-resort fallback; whether `(` or `[` follows is
  irrelevant once the earlier forms have been tried.
- **Don't unify `inline_ir`'s emphasis plan with
  `delimiter_stack`'s.** They share the
  `EmphasisPlan::from_dispositions` migration seam
  but the production path now reads from the IR.
  `delimiter_stack::scan_delim_runs` +
  `process_emphasis` is dead code under
  `Dialect::CommonMark` — leave it for the Pandoc
  dialect migration to reuse, don't delete in this
  session.
- **Don't gate footnote / citation / bracketed-span /
  native-span branches on `bracket_says_resolved`.**
  The IR only models CommonMark links/images. Those
  Pandoc constructs aren't in the bracket plan and
  should not be suppressed when bracket_plan is
  `Some`. Today they're off under the CommonMark
  flavor anyway, but a user who manually enables
  them shouldn't have them silently dropped.
- **The exclusion bitmap in `build_full_plans` is
  load-bearing.** Without it, the top-level emphasis
  pass pairs across a resolved link's brackets (spec
  #473 — `*[bar*](/url)` regresses to outer-emphasis
  forming over the link). The unit test
  `full_plans_emphasis_does_not_cross_resolved_link_boundary`
  guards this.

### Suggested next targets, ranked

1. **Pandoc dialect migration onto the unified
   algorithm.** The CommonMark IR path is now in
   production. To migrate Pandoc:
   - Parameterize `compute_flanking` and
     `is_left_flanking` / `is_right_flanking` in
     `inline_ir` by `Dialect` (Pandoc's intraword
     underscore rule + tail-end emphasis heuristic
     differ).
   - Build a Pandoc-specific bracket resolver
     (footnotes / citations / bracketed spans /
     native spans become bracket constructs in the
     IR).
   - Wire under `Dialect::Pandoc` in
     `parse_inline_text_recursive`.
   - Once both dialects use the IR,
     `delimiter_stack` and the recursive-descent
     emphasis path (`try_parse_emphasis`,
     `try_parse_one/two/three`) can be deleted.

2. **Decommission `delimiter_stack::scan_delim_runs`
   + `process_emphasis`.** Dead code under
   `Dialect::CommonMark` post-this-session. Remove
   only after Pandoc migration (#1) lands.

3. **Formatter fix for nested-only outer LIST_ITEM** —
   carried prerequisite for lifting the same-line
   nested-LIST and blockquote-in-list-item dialect
   gates.

4. **Multi-line setext inside list items** (CommonMark
   only) — paired parser + formatter fixtures,
   dialect-gated. Strictly cosmetic; no spec example
   exercises it in the conformance harness today.

5. **Conformance follow-ups in `blocked.txt`** —
   reference-definition leniency (#209/#213),
   code-span vs link precedence (#342/#525),
   inline-link strict syntax (#488/#490/#493/#508/#546),
   autolink validator strictness (#606/#609). Each
   needs its own focused parser fix; the current
   `blocked.txt` documents what each one exposes.

### Carry-forward from prior sessions

(Carrying forward from session xxviii unless noted.)

- Session (xxvii)'s
  `Parser::try_fold_list_item_buffer_into_setext` (in
  `parser/core.rs`) is load-bearing for #300; the
  helper *must* run before `dispatcher_match` is
  computed in `parse_inner_content`, and it must bail
  on `segment_count() != 1` (multi-line setext inside
  list items is dialect-divergent and out of scope).
  The indent guard
  `underline_indent_cols < content_col → return false`
  is also load-bearing — without it, #94/#99/#281/#282
  flip to spurious setext headings.
- Session (xxvii)'s formatter list-loose detection
  (`crates/panache-formatter/src/formatter/lists.rs`)
  uses the combined predicate
  `(has_blank_within_item || has_structural_multi_block)
  && !has_nested_lists`. **Don't include `HTML_BLOCK`
  in `is_loose_trigger_block`** — the
  `ignore_directives` fixture relies on
  PLAIN+HTML_BLOCK+PLAIN+HTML_BLOCK staying tight
  (those HTML blocks are panache's
  `<!-- panache-ignore-* -->` directives).
- Session (xxvi)'s emphasis carry-forward (delimiter
  stack, `EmphasisPlan`, `Vec<DelimRun>` representation,
  the four "don't redo" notes about
  `coalesce-on-Literal`, `openers_bottom`, byte-keyed
  plan, plan threading through nested recursion) is all
  still load-bearing on the CommonMark inline path.
- Session (xxv)'s setext list-item indent guard and
  `in_marker_only_list_item` flag in
  `block_dispatcher.rs` are load-bearing for #278.
- Session (ix)'s "tail-end only" emphasis heuristic and
  Pandoc dialect gate still apply to the Pandoc inline
  path only.
- Session (x)/(xi)'s link-scanner skip pattern (autolink
  / raw-HTML opacity for emphasis closer + link bracket
  close) is load-bearing for #524/#526/#536/#538. Don't
  unify the autolink and raw-HTML skip flags — Pandoc
  treats them differently.
- Session (xii)'s lazy paragraph continuation across
  reduced blockquote depth, session (xiii)'s
  `try_lazy_list_continuation` for OpenList only at
  `indent_cols ≥ 4`, session (xvii)'s HTML block #148
  fix (`</pre>` rejection in VERBATIM_TAGS), session
  (xviii)'s `disallow_inner_links` flag scope (inline
  links only — reference-link nesting #533 still needs
  a different pass), session (xix)/(xxi)/(xxii)'s
  column-aware indented-code logic, and session
  (xxiv)'s same-line blockquote-in-list-item branch
  dialect gate are all unchanged and unaffected by
  this session.
