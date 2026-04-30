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

## Latest session — 2026-04-30 (xxix)

**Pass count: 648 → 651 / 652 (99.8%, +3)**

User asked for "implement a full IR" rather than the
minimal #523 fix. Landed the IR scaffolding *and* a
refdef-aware bracket gate, which together unlocked
**#523, #569, #571** (three of the four remaining link
failures). Only **#533** remains failing.

### Targets unlocked

- **#523** — `*foo [bar* baz]` now correctly emits
  `<em>foo [bar</em> baz]`. Inner `*` is exposed to the
  emphasis scanner because the shortcut bracket
  `[bar* baz]` no longer opaque-skips when its label
  isn't in the document refdef map.
- **#569** — `[foo][bar][baz]` with `[baz]: /url`. The
  middle `[bar][baz]` resolves; refdef-aware emission
  now refuses to emit the trailing `[foo][bar]` as a
  reference link (no refdef), so `[foo]` falls through
  to literal text.
- **#571** — same as #569 but with `[foo]` also defined
  as a refdef. Even though `foo` resolves, the
  left-to-right `[foo][bar]` form fails (bar not in
  refdefs), so `[foo]` stays literal — matches CMark's
  scan order.

### Architecture: what landed

Two new modules in `crates/panache-parser/src/parser/inlines/`:

1. **`refdef_map.rs`** — document-level refdef label
   pre-pass. `collect_refdef_labels(input, dialect)`
   walks the input lines once and collects normalised
   labels into an `Arc<HashSet<String>>`. The scanner
   handles refdefs at line-start *and* after a stripped
   blockquote prefix (`> [foo]: /url` — spec example
   #218). Normalisation matches the test renderer's
   `normalize_label` (lowercase + collapse whitespace +
   `ß` → `ss` for the CMark §6.4 sharp-S case fold,
   spec example #540).

2. **`inline_ir.rs`** — full IR scaffolding for the
   CommonMark inline pipeline (build_ir,
   process_emphasis, process_brackets,
   build_full_plans, BracketPlan, EmphasisPlan adapter
   `build_emphasis_plan`). Tested in isolation (9 unit
   tests pass). **NOT yet wired into production
   emission** — see *Don't redo* below for why and what
   the next session should do with it.

Plumbing change: `ParserOptions` gained
`refdef_labels: Option<Arc<HashSet<String>>>`
(serde-skip). Top-level `parse()` and
`parse_incremental_suffix()` populate it via
`populate_refdef_labels()` when `dialect ==
CommonMark`. The field is consulted by
`delimiter_stack::reference_resolves` and the matching
helper in `core.rs`.

The actual #523/#569/#571 fix:
`delimiter_stack::scan_delim_runs` and the four bracket
branches in `core::parse_inline_range_impl` now gate
their reference-link/image opaque-skip on
`reference_resolves(...)`, which checks the
(normalised) label against the refdef map. When the
label doesn't resolve, the bracket pair stays
transparent to the emphasis scanner *and* unrecognised
by the emission walk — both halves needed in sync,
otherwise the emphasis pair forms but emission still
emits a (mis-spanning) LINK node.

Snapshot updates (intentional; previous CST emitted
LINK nodes for unresolved shortcut references that the
renderer would later down-convert to text):

- `tests/snapshots/golden_parser_cases__parser_cst_inline_link_dest_strict_commonmark.snap`
- `tests/snapshots/golden_parser_cases__parser_cst_reference_definition_attached_title_commonmark.snap`
- `tests/fixtures/cases/inline_link_dest_strict_commonmark/expected.md`
  (formatter now escapes `\[link\](/my uri)` as
  literal text — idempotent under the new parse)
- `tests/fixtures/cases/reference_definition_attached_title_commonmark/expected.md`
  (same — `\[foo\]: <bar>(baz)` and `\[foo\]`)

### Files touched

- `crates/panache-parser/src/options.rs` — added
  `refdef_labels` field.
- `crates/panache-parser/src/parser.rs` — top-level
  refdef pre-pass plumbing.
- `crates/panache-parser/src/parser/inlines.rs` —
  module declarations.
- `crates/panache-parser/src/parser/inlines/refdef_map.rs`
  — NEW module.
- `crates/panache-parser/src/parser/inlines/inline_ir.rs`
  — NEW module (foundation only, not yet emission-wired).
- `crates/panache-parser/src/parser/inlines/delimiter_stack.rs`
  — `reference_resolves` helper, refdef-aware skip in
  `scan_delim_runs`, public `EmphasisPlan::from_dispositions`.
- `crates/panache-parser/src/parser/inlines/core.rs` —
  `reference_resolves` wrapper, refdef-aware gate on the
  four bracket branches in `parse_inline_range_impl`,
  `BracketPlan` parameter threaded (currently always
  `None` — reserved for the IR migration).
- `crates/panache-parser/tests/commonmark/allowlist.txt`
  — added 523, 569, 571 under `# Links`.
- `crates/panache-parser/tests/commonmark/blocked.txt`
  — removed the 523 entry (no longer passing-by-accident).
- `crates/panache-formatter/src/config.rs`,
  `src/config/types.rs` — fill in the new
  `refdef_labels: None` field.
- 4 snapshot/fixture files for intentional CST changes.

### Don't redo

- **Don't try to wire `inline_ir::build_full_plans`
  into emission as-is.** It works in isolation but the
  emphasis/bracket pass *order* is wrong for cases
  where they overlap (e.g. spec #473 `*[bar*](/url)`).
  My first attempt produced 15 regressions because the
  IR's `process_emphasis` runs before
  `process_brackets` — emphasis pairs can incorrectly
  form across what would be a link's bracket boundary,
  then bracket resolution succeeds, leaving emission
  with two overlapping resolutions and no way to
  reconcile them.

  The CMark reference impl interleaves the two passes:
  bracket resolution at each `]` is *immediately*
  followed by emphasis processing on that link's inner
  range, with the link wrapped around the result.
  Replicating this on the IR means either:
  (a) running `process_brackets` first and then
      `process_emphasis` *scoped* to each resolved
      bracket pair's inner event range, or
  (b) walking the events left-to-right and resolving
      brackets and emphasis in interleaved order.

  Either way, top-level emphasis (between resolved
  bracket pairs) is processed last, on whatever events
  are left.

- **Don't try to use the `BracketPlan` for emission
  without solving the boundary problem.** When a
  resolved link's `suffix_end` falls past a recursion
  boundary (which happens whenever an outer emphasis
  range wraps over a link), the plan can't naively
  emit the link inside the emphasis's recursive call.
  My first attempt hit this and emitted both the link
  AND the emphasis closer, double-counting bytes and
  breaking losslessness.

- **Don't merge `inline_ir`'s emphasis plan with
  `delimiter_stack`'s.** The current
  `delimiter_stack::scan_delim_runs` + `process_emphasis`
  is the production path; the IR's parallel
  implementation is for *future* migration. Keeping
  them separate avoids accidental coupling. The
  `EmphasisPlan::from_dispositions` constructor I
  added is the migration seam — it lets the IR build
  an `EmphasisPlan` from the same byte-keyed shape, so
  a future session can swap the two without changing
  the emission walk.

- **The `bracket_plan: Option<&BracketPlan>` parameter
  in `parse_inline_range_impl` is intentionally always
  `None` right now.** Don't remove it — it's the
  hook the next session can wire when the
  pass-ordering problem is solved. (Kept as a `let _ =
  bracket_plan;` placeholder.)

### Suggested next targets, ranked

1. **#533** — `[foo *bar [baz][ref]*][ref]`. Last
   remaining link failure. Needs the §6.3 link-in-link
   suppression rule for *reference* links: when the
   inner `[baz][ref]` resolves, the outer
   `[foo ...][ref]` must NOT also resolve as a
   reference link (it should fall through to literal).
   The current refdef-aware gate doesn't address this
   because both labels resolve — the rule is about
   *nesting*, not resolution. Likely needs the IR's
   `deactivate_earlier_link_openers` semantics applied
   to the existing `try_parse_reference_link` path.
   Could be solved either by:
   - Extending `disallow_inner_links` (the existing
     CommonMark gate for inline link nesting in
     `links::link_text_contains_inner_link`) to also
     reject reference-link inner content. Today it
     only checks for inline links/images.
   - Or: porting the IR's bracket scanner to drive
     emission for reference links specifically.

2. **Inline IR migration (the real one).** Now that
   the IR module exists and is unit-tested, a future
   session can wire it into production by solving the
   pass-ordering problem (see *Don't redo* above).
   When that lands, #533 falls out for free (the IR
   already has the link-in-link suppression in
   `process_brackets::deactivate_earlier_link_openers`).

3. **Pandoc dialect migration onto the unified
   algorithm.** Once the IR drives CommonMark
   emission, parameterize the flanking predicates by
   `Dialect` and run Pandoc through the same pipeline.

4. **Formatter fix for nested-only outer LIST_ITEM** —
   carried prerequisite for lifting the same-line
   nested-LIST and blockquote-in-list-item dialect
   gates.

5. **Multi-line setext inside list items** (CommonMark
   only) — paired parser + formatter fixtures, dialect-
   gated. Strictly cosmetic; no spec example exercises
   it in the conformance harness today.

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
