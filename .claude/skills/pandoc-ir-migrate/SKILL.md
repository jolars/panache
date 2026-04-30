---
name: pandoc-ir-migrate
description: Incrementally migrate Panache's Pandoc-dialect inline parsing
  onto the unified inline IR (currently CommonMark-only) one bounded
  sub-task at a time, verifying every CST divergence against pandoc-native
  before fixing or deferring.
---

Use this skill when asked to advance the Pandoc-dialect inline IR
migration, fix a specific Pandoc emphasis / inline regression introduced
by an IR-migration step, or pick "the next best phase" to work on.

## Scope boundaries

- Target is the inline IR pipeline at
  `crates/panache-parser/src/parser/inlines/inline_ir.rs` and its
  consumers in `crates/panache-parser/src/parser/inlines/core.rs`. The
  goal is to retire `crates/panache-parser/src/parser/inlines/delimiter_stack.rs`
  and the legacy recursive-descent emphasis path
  (`try_parse_emphasis`, `try_parse_one/two/three`,
  `parse_until_closer_with_nested_*`) once both dialects share the IR.
- This is a **long-horizon effort** (8 phases — see "Phased plan"
  below). Each session moves one phase or sub-task forward; do not try
  to land sweeping rewrites in one go.
- Inline IR changes for the Pandoc dialect must not regress CommonMark
  conformance. The conformance harness at
  `crates/panache-parser/tests/commonmark/allowlist.txt` is the
  load-bearing guard — re-run it every session.
- Pandoc-native is the **behavioral reference** for the migration. The
  legacy `try_parse_emphasis` recursive-descent path may itself diverge
  from pandoc on edge cases; do NOT preserve a legacy fixture's output
  when it conflicts with `pandoc -f markdown -t native`. See "Pandoc
  vs legacy fixture trap" below.
- Do not commit a session whose diff regresses any test that was green
  at the start. Defer the unfinished work to a follow-up session and
  document in `RECAP.md` instead. The user has explicitly asked for
  this discipline; follow it strictly.

## Related rules to read first

These project rules apply directly to this skill's work; read them
before starting if you haven't already loaded them this session:

- `.claude/rules/parser.md` — `Dialect` vs `Extensions` split, dialect
  pandoc-verification requirement, CST losslessness, the
  TEXT-coalescence-vs-structural-diff distinction (added for this
  skill), and the pandoc-native-is-the-reference rule.
- `.claude/rules/integration-tests.md` — formatter golden cases live
  under top-level `tests/fixtures/cases/` and are wired in
  `tests/golden_cases.rs`; parser-only cases live under
  `crates/panache-parser/tests/fixtures/cases/` and wire into
  `crates/panache-parser/tests/golden_parser_cases.rs`. Do not mix them.
- `.claude/rules/commonmark.md` — Pandoc-IR changes that affect the
  shared `inline_ir.rs` algorithm must keep `Dialect::CommonMark`
  byte-identical; the conformance allowlist is the regression guard.

## Harness noise to ignore inside this skill

The runtime occasionally injects a `system-reminder` nudging you to use
`TaskCreate` / `TaskUpdate` for tracking. **For most sub-tasks the
workflow below is linear (probe → classify → fix → fixture → recap), so
task tools add overhead without value.** Skip them for migration
sessions unless the user explicitly asks for a task list. The exception
is multi-phase sessions (rare) — there a task list per phase helps.

## Phased plan

The migration is bounded by 9 phases. Pick one (or part of one) per
session. The latest phase status lives in `RECAP.md`.

**Phase 0** — Thread `Dialect` through `inline_ir`'s `compute_flanking`,
`process_emphasis*`. Stub branch points; no behavior change.

**Phase 1** (keystone) — Pandoc emphasis on the IR. `build_full_plans`
skips `process_brackets` under Pandoc; `build_ir` recognises Pandoc
opaque constructs (math `$...$`, inline links, ref links, footnote
refs `[^id]`, citations `[@cite]`, bracketed spans `[text]{attrs}`,
inline footnotes `^[note]`, native spans `<span>...</span>`) as
`ConstructKind::PandocOpaque` events so emphasis can't pair across
them. Activate dialect gates in flanking + matching + cascade. Drop
the dispatcher fork in `parse_inline_text_recursive` /
`parse_inline_text`. Widen `populate_refdef_labels` to both dialects.

**Phase 2** — Move `^[note]` and `<span>...</span>` recognition out
of the dispatcher's ordered-try chain in `parse_inline_range_impl`
into `build_ir` as `Construct` events. Pure additive; no algorithm
change.

**Phase 3** — `[^id]` footnote refs into IR scan as
`Construct::FootnoteReference`.

**Phase 4** — `[@cite]` bracketed citations + bare `@cite` into IR
scan as `Construct::BracketedCitation` / `Construct::BareCitation`.

**Phase 5** — Bracketed spans `[text]{attrs}` into IR `process_brackets`
(first phase that *changes* `process_brackets`). Adds a new
`BracketResolutionKind::BracketedSpan` variant and gates
`bracket_says_resolved` on kind.

**Phase 6** — Pandoc links/images dispatched IR-first. Originally
planned as full BracketPlan-driven dispatch with
`deactivate_earlier_link_openers` gated on CommonMark only; recap-(viii)
verified against pandoc-native that the gating mechanism would have
been wrong (pandoc-native does NOT allow nested links — outer wins,
inner literal — so the rule isn't "don't deactivate," it's "deactivate
inner instead of outer"). Phase 6 instead followed the Phase 2-5
`ConstructPlan` pattern: a new `ConstructKind::PandocLinkOrImage`
that drives IR-first dispatch through the dispatcher's existing
`try_parse_*` chain, with the legacy `[`/`![` branches in
`parse_inline_range_impl` gated on `Dialect::CommonMark`. Pure
additive; the three pre-existing pandoc-native divergences below
are preserved and addressed in Phase 8.

**Phase 7** — Delete the legacy emphasis path: `try_parse_emphasis*`,
`try_parse_one/two/three`, `parse_until_closer_with_nested_*`,
`parse_inline_range`, the recursive-descent emphasis branch in
`parse_inline_range_impl`. Delete the `delimiter_stack` module;
relocate `EmphasisPlan` / `DelimChar` / `EmphasisKind` into
`inline_ir.rs`.

**Phase 8** (final) — Pandoc bracket emission on the IR's
`BracketPlan`. Enable `process_brackets` under `Dialect::Pandoc`
(currently CommonMark-only in `build_full_plans`) with two
Pandoc-aware semantic differences:
1. **Outer-wins link-in-link suppression.** When a Pandoc bracket
   resolves, mark all bracket events inside its range as inactive
   so they emit literal. This is the OPPOSITE of CommonMark §6.3's
   `deactivate_earlier_link_openers` (inner-wins). NOT a toggle of
   that flag — a different rule.
2. **Refdef-aware shortcut resolution under Pandoc.** Replace
   `is_commonmark` short-circuit in `process_brackets`'s shortcut
   path with refdef-map consultation for both dialects. Pandoc's
   shape-only `reference_resolves = true` is wrong; it produces
   malformed LINK nodes for unresolved shortcuts and steals inner
   `*`/`_` chars from the emphasis scanner.

This fixes the three pre-existing pandoc-native divergences noted
in recap-(viii):
- `[foo]` with no refdef parses as malformed LINK → should be
  literal (`Str "[foo]"`).
- `*foo [bar* baz]` produces TEXT + partial LINK → should be all
  literal (refdef miss frees the `*` for emphasis pairing).
- `[link [inner](u2)](u1)` allows nested LINK → should be
  outer-wins with inner literal.

Cleanup once Phase 8 lands: `ConstructKind::PandocLinkOrImage` and
its `ConstructDispo` variant become unused (folded into
`BracketDispo::Open`); the CM-gated `[`/`![` legacy dispatcher
branches in `parse_inline_range_impl` either stay (as the CM
emission path) or get unified with the IR-driven path. Pandoc-
specific golden fixtures for the three bug cases land under
`crates/panache-parser/tests/fixtures/cases/`, verified against
`pandoc -f markdown -t native`.

**Migration-complete invariant**: after Phase 8, no Pandoc inline
construct depends on the dispatcher's `try_parse_*` chain for
*resolution* decisions. The dispatcher recognizers are still called
to *parse* a matched range into a CST subtree, but "what is this
byte range?" is answered exclusively by the IR. That is the
end-state of this migration.

The full design rationale lives in `/home/jola/.claude/plans/let-s-create-a-plan-noble-barto.md`
(initial migration plan; treat as historical reference once the recap
has captured each phase's outcome).

## Key files

- `crates/panache-parser/src/parser/inlines/inline_ir.rs` — IR scan,
  bracket resolution, emphasis pass; primary site for dialect
  parameterization. Where most edits land.
- `crates/panache-parser/src/parser/inlines/core.rs` — dispatcher,
  emission walk; ~1500 lines disappear in Phase 7.
- `crates/panache-parser/src/parser/inlines/delimiter_stack.rs` —
  legacy emphasis plan builder. Exposes `EmphasisPlan` /
  `DelimChar` / `EmphasisKind` types still used by the IR's
  `build_emphasis_plan`. Deleted in Phase 7.
- `crates/panache-parser/src/parser/inlines/{citations,bracketed_spans,inline_footnotes,native_spans,math,links}.rs`
  — `try_parse_*` recognizers reused by `build_ir`'s opaque-construct
  scan. Don't duplicate logic; call into them.
- `crates/panache-parser/src/parser.rs` — `populate_refdef_labels`
  (widened to both dialects in Phase 1).
- `crates/panache-parser/src/options.rs` — `Dialect`, `Extensions`,
  `ParserOptions`. No signature changes; consult for flag defaults.
- `crates/panache-parser/tests/commonmark/allowlist.txt` —
  CommonMark conformance regression guard. Must stay green.
- `crates/panache-parser/tests/fixtures/cases/` — parser golden
  scenarios (paired CommonMark/Pandoc fixtures live here).
- `tests/fixtures/cases/` — formatter golden scenarios (only add a
  CommonMark-flavor case here when the parser change produces a
  different *block sequence* than the Pandoc path).

## Failure buckets

Every Pandoc-dialect IR migration regression falls into one of these.
Classify before editing.

- **TEXT-coalescence diff** — old fixture had multiple adjacent TEXT
  spans (e.g. `TEXT@0..5 "**foo" + TEXT@5..6 "*"`); new IR produces
  a single coalesced TEXT (`TEXT@0..6 "**foo*"`). Same bytes, same
  structure, no STRONG/EMPHASIS/etc node difference. **Benign;
  snapshot update is safe** after confirming the structural shape
  matches pandoc-native (see "Pandoc vs legacy fixture trap"). Don't
  invent a fix to preserve the split — the IR's coalescence is an
  improvement, not a bug.
- **Missing dialect gate** — Pandoc-specific rule (intraword
  underscore hard-rule, mod-3 disable, asymmetric (1,2)/(2,1)
  rejection, opener count >= 4 rejection, triple-emph nesting flip,
  Pandoc closer-flanking-free) isn't applied. Fix: add or correct the
  branch in `compute_flanking` / `process_emphasis_in_range_filtered`.
- **Missing opaque construct** — IR's `build_ir` doesn't recognise a
  Pandoc inline construct, so emphasis pairs across its content (or
  worse, the dispatcher's later parse re-claims bytes already
  consumed by emphasis, breaking losslessness). Fix: add the
  recognizer to `build_ir` under `!is_commonmark`, emitting
  `ConstructKind::PandocOpaque` (or the phase-specific kind).
  Losslessness is the canary — if the parser-crate suite shows
  "tree text does not match input", losslessness has broken and a
  missing opaque construct is the most likely cause.
- **Cascade-amenable algorithmic divergence** — IR matches an
  emphasis pair that Pandoc rejects because of an unmatched
  same-character run between opener and closer (`*foo**bar*`,
  `**foo *bar** baz*`). Fix: ensure the run is flanking-eligible
  (both `can_open && can_close`) so `pandoc_cascade_invalidate`
  catches it. If the cascade rule itself needs widening, do so
  carefully — the trap below documents one over-broad version.
- **Scoped-pass-needed algorithmic divergence** — nested
  strong-of-emph or emph-of-strong cases where the IR's left-to-right
  closer walk matches the wrong pair (`**foo *bar* baz**` should be
  STRONG[foo, EM[bar], baz]; `*foo **bar* baz**` should be `*foo` +
  STRONG[bar* baz]; `***foo **bar** baz***` should be
  EM[STRONG[foo], "bar", STRONG[baz]]). Fix needs strong-first pass
  preference + scoped emphasis recursion on each strong-matched
  inner range. **Don't try the "two-pass + un-remove-between"
  shortcut** — it breaks the pair-crossing invariant (see trap
  below). The structurally clean approach is recursive scoped
  passes in `build_full_plans` analogous to the bracket scoped pass
  for CommonMark.
- **Genuine algorithmic divergence beyond IR expressivity** — pandoc
  recursive-descent semantics that the delim-stack can't express
  even with scoped passes. Defer; document in `RECAP.md` and (if
  meaningful) in `crates/panache-parser/tests/commonmark/blocked.txt`
  or a new pandoc-equivalent file.

### Pandoc vs legacy fixture trap

The legacy `try_parse_emphasis` path is **not** the migration's
reference. It approximates pandoc-native but has its own bugs and
quirks. When the new IR output differs from a legacy fixture, do
NOT default to "preserve the fixture". Instead:

```
printf '<input>' | pandoc -f markdown -t native > /tmp/pd.txt
printf '<input>' | pandoc -f commonmark -t native > /tmp/cm.txt
```

- New IR output matches `/tmp/pd.txt` (Pandoc native) → fixture is
  wrong; update the fixture (and snapshot) to the IR output. Note
  the prior fixture as a legacy-parser bug in `RECAP.md`.
- New IR output differs from `/tmp/pd.txt` AND old fixture matched
  `/tmp/pd.txt` → regression; fix the IR.
- New IR output differs from `/tmp/pd.txt` AND old fixture also
  differed (both wrong, in different ways) → fix toward
  pandoc-native; don't preserve either old behavior.
- TEXT-coalescence diffs are below this verification's resolution
  (pandoc-native doesn't pin TEXT-token granularity). For these,
  confirm structural elements (`Strong`, `Emph`, `Link`, etc.)
  match in count and nesting; if so, update the snapshot and move
  on.

### Trap: two-pass + un-remove-between breaks pair crossing

A natural-looking but wrong attempt at "strong-first preference":
run a first pass over `count >= 2` closers, mark `removed[]` on
between-events as usual, then un-remove them between passes so a
second pass can match nested `count == 1` emphasis. This breaks
the pair-crossing invariant: in pass 2, an emphasis can pair with
an opener whose strong partner is INSIDE the emphasis's range,
producing an EM whose markers cross a STRONG's markers. The
emission walk then produces invalid CST.

**Correct approach**: scoped emphasis passes. After a strong match
in pass 1, run `process_emphasis_in_range` on the inner event range
(open_idx + 1, close_idx) WITH its own state (separate `count`,
`source_start`, `removed` arrays). Mirrors the bracket scoped pass
for CommonMark in `build_full_plans` (lines 1247-1306 of
`inline_ir.rs`). The state separation is the crucial bit; reusing
the outer pass's state is what causes the crossing.

### Trap: cascade-rule over-invalidation

The cascade rule (`pandoc_cascade_invalidate`) checks for
"unmatched same-ch run between matched pair". The flanking-eligibility
filter on those runs MUST be `can_open && can_close` (both true), not
`can_open || can_close`. The `||` version invalidates legitimate
matches when intraword `_` (e.g. `_foo_bar_baz_`) sits between an
opener and closer; pandoc-native does pair the outer `_`s and treats
the inner `_`s as literal text inside emphasis content. The
recursive-descent path's "if inner attempt fails, outer fails"
semantics only fires when the inner construct could actually have
opened/closed — i.e. both flanking sides are eligible.

## Algorithmic toolbox

These are the dialect-aware levers in `inline_ir.rs`. When fixing a
regression, identify which lever applies before editing.

- **`compute_flanking`** branches on `Dialect`:
  - **CommonMark**: §6.2 left/right-flanking exact rules.
  - **Pandoc**: `can_open = !followed_by_ws`; `can_close = true`
    (no flanking gate — pandoc-markdown's `ender` is count-only).
    Underscore intraword hard-rule applies on top: `_` adjacent to
    alphanumeric on either side cannot open/close on that side.
- **`pandoc_reject` opener-finder gate**: rejects (1,2), (2,1), and
  count_o >= 4. Verified against pandoc-native: (1,3), (3,1), (2,3),
  (3,2), and any (≤3, 4+) DO match.
- **Mod-3 rejection** (CommonMark §6.2 rule 9): gated on
  `is_commonmark`; disabled under Pandoc.
- **Consume rule for triple-emph nesting**: when `count[o] >= 3 &&
  count[c] >= 3` AND `!is_commonmark`, consume = 1 first (emph
  innermost) instead of consume = 2 (CommonMark default, strong
  outermost). This produces STRONG(EM(...)) for `***x***` under
  Pandoc.
- **`pandoc_cascade_invalidate`**: post-pass that walks resolved
  matches and invalidates any pair containing an unmatched same-ch
  run with both `can_open && can_close`. Iterates to fixed point.
- **Pandoc opaque construct scan in `build_ir`**: under
  `!is_commonmark`, recognises link/ref-link/image/footnote-ref/
  citation/bracketed-span/inline-footnote/native-span/math forms as
  `ConstructKind::PandocOpaque`. This is what keeps emphasis from
  pairing across these constructs while emission stays on the
  legacy `try_parse_*` chain.
- **(PLANNED — Phase 1 follow-up)** Scoped emphasis passes on
  strong-matched inner ranges, in `build_full_plans` for
  `Dialect::Pandoc`. The structural shape mirrors the existing
  CommonMark bracket scoped pass (lines 1247-1306 of `inline_ir.rs`).

## Session recap (`RECAP.md`)

This skill keeps a rolling recap at
`.claude/skills/pandoc-ir-migrate/RECAP.md`. It is the handoff
between sessions — short, judgment-call-only, not a duplicate of the
test report.

- **At the start of a session**: read `RECAP.md` first. The
  "Suggested next sub-targets" section is the recommended starting
  point, and the "Don't redo / known traps" list keeps you from
  reinventing fixes that already landed (or already failed). If the
  user named a specific phase or test cluster, prefer that, but
  still skim the recap so you don't redo prior work.
- **At the end of a session**: rewrite the **Latest session** entry
  in `RECAP.md` with: phase + sub-target tackled, test count
  before → after (full workspace), files changed, what *not* to
  redo, ranked next sub-targets, and any new traps discovered.
  Keep it terse — a fresh session should pick up from this entry
  without scrolling the prior conversation.
- **If the session ends with regressions** (uncommitted diff is
  partial), the recap MUST say so explicitly: list the failing
  tests, classify each into a bucket, and rank the next sub-target
  by likely shared root cause. Do NOT mark the session "done"
  if the suite has new red.

## Workflow

1. **Read `RECAP.md`** for current phase, deferred sub-targets, and
   trap list. If the user named a sub-target, prefer it; otherwise
   pick the top-ranked next sub-target from the recap.

2. **Establish the test baseline** before editing:
   ```
   cargo test --workspace --no-fail-fast 2>&1 | grep -E "^test " | grep "FAILED" | sort -u
   ```
   Save the failing-test set; this is what "no regression" means
   for this session.

3. **Probe the failing inputs.** For each input the sub-target
   touches, capture both the IR's current CST and pandoc-native:
   ```
   pandoc <input>.md -f markdown -t native     # primary reference
   pandoc <input>.md -f commonmark -t native   # only if dialect-divergent
   ```
   For triage across several inputs, drop a throwaway
   `#[ignore]`-d probe test into
   `crates/panache-parser/tests/probe_phase1.rs` (or similar
   per-phase name). Template:

   ```rust
   use panache_parser::parse;
   #[test]
   #[ignore = "probe specific inputs"]
   fn probe_targets() {
       for input in [/* failing inputs */] {
           let tree = parse(input, None);
           eprintln!("=== {:?} ===\n{:#?}\n", input, tree);
       }
   }
   ```

   Run with
   `cargo test -p panache-parser --test probe_phase1 -- --ignored --nocapture`.
   **Delete the probe file before finishing the session.**

4. **Classify each divergence into a failure bucket** (see
   "Failure buckets" above). Apply the pandoc-vs-legacy-fixture
   trap protocol for any structural diff. TEXT-coalescence diffs
   are benign — confirm structure, then snapshot-update.

5. **Pick the smallest dialect-aware lever** that addresses the
   bucket. Resist adding a new code path when an existing dialect
   gate already covers the case (just turn the gate on).

6. **Apply the change. Validate immediately**:
   ```
   cargo test -p panache-parser --no-fail-fast
   cargo test --workspace --no-fail-fast
   cargo test -p panache-parser --test commonmark commonmark_allowlist
   ```
   The conformance allowlist must stay green; CommonMark behavior
   must not change.

7. **Verify net change**: same baseline command from step 2.
   - Failures should be a subset of the baseline.
   - If a NEW failure appears (i.e., a previously-green test went
     red), the change introduced a regression. Revert and try a
     different lever, OR shrink the change scope. Don't proceed
     with regressions in the diff.

8. **For TEXT-coalescence snapshot updates**: regenerate snapshots
   (`find ... -name "*.snap.new" -delete` and re-run tests to get
   fresh `.new` files), then `cargo insta review` (or compare
   `.snap` vs `.snap.new` manually) and accept only diffs that are
   purely TEXT-coalescence. For each: write a one-line note in the
   recap.

9. **Update `RECAP.md`** with the session outcome. If the session
   ends mid-sub-task with regressions, capture the partial state
   and rank what's left.

10. **Commit** only if the diff is clean:
    - All baseline failures gone OR a strict subset.
    - No new failures.
    - clippy and fmt clean.
    - Commit message names the phase + sub-target (e.g.
      "fix(parser): Phase 1 — add Pandoc opaque math scan").
    Otherwise leave the diff uncommitted in the working tree and
    document the deferral.

## Dos and don'ts

- **Do** read `RECAP.md` before picking a sub-target.
- **Do** verify against pandoc-native, not the legacy parser, when a
  fixture and the new IR disagree.
- **Do** keep the CommonMark conformance allowlist green every
  session. If a Pandoc-side change requires touching `inline_ir.rs`,
  re-run the allowlist test before declaring done.
- **Do** record every new trap encountered in `RECAP.md` so future
  sessions don't re-walk the same wrong path.
- **Don't** preserve a legacy-fixture output when it conflicts with
  pandoc-native. The fixture is the bug.
- **Don't** treat TEXT-coalescence diffs as regressions. They're
  improvements; update the snapshot.
- **Don't** commit a session whose diff has *any* new red test.
  Defer instead.
- **Don't** try the "two-pass + un-remove-between" shortcut for
  scoped emphasis (see trap above). The state-separation
  requirement is non-negotiable.
- **Don't** widen the cascade rule to `can_open || can_close` (see
  trap above). It must be `&&`.
- **Don't** delete the legacy `try_parse_emphasis` path before
  Phase 7. The dispatcher's `parse_inline_range_impl` still
  consumes Pandoc bracket constructs through it during Phases 1-6.
- **Don't** edit conformance `report.txt` /
  `docs/development/commonmark-report.json` by hand; they're
  derived. Re-run `commonmark_full_report` to refresh.

## Report-back format

When done, report:

1. Phase + sub-target tackled (e.g. "Phase 1 — scoped emphasis
   passes for strong-of-emph nesting").
2. Workspace test count before → after (e.g. "13 failing → 4
   failing").
3. Files changed, classified by failure bucket.
4. Snapshots updated and the rationale (TEXT-coalescence /
   structural-change-verified-vs-pandoc-native).
5. Suggested next sub-target ranked by likely shared root cause.
6. Any new trap discovered, captured in `RECAP.md`.

If the session ends without committing, also report:

7. The exact remaining red tests, classified into buckets.
8. Why they were deferred (which structural change they need).
