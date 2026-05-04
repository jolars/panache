# Pandoc IR migration — running session recap

This file is the rolling, terse handoff between sessions of the
`pandoc-ir-migrate` skill. Read it at the start of a session for suggested next
sub-targets and known traps; rewrite the **Latest session** entry at the end
with what changed and what to look at next.

Keep entries short. Test counts + a one-line root cause beat a narrative. The
hard-won judgment calls (why a lever was chosen, why an approach was reverted,
what trap to avoid) are the load-bearing content here.

--------------------------------------------------------------------------------

## Latest session — 2026-05-04 (xvi)

**Workspace test count: 0 failing → 0 failing.** **Bugs #1/#2 resolved
out-of-band by the parser-as-source-of-truth split.** A separate,
multi-session effort (plan at
`/home/jola/.claude/plans/alright-let-s-commit-to-vivid-corbato.md`)
introduced a new `UNRESOLVED_REFERENCE` SyntaxKind so the Pandoc
parser stops emitting shape-only `LINK` nodes for unresolved
bracket-shape patterns. Bug #1 (`[foo]` no refdef → malformed LINK)
is **fully fixed**; bug #2 (`*foo [bar* baz]` emphasis crosses
brackets) is **partially fixed** — the parse no longer produces a
malformed partial LINK, but the cross-bracket emphasis pairing isn't
reconstructed (deferred to stage 6 of that plan).

This work crossed parser/linter/LSP/formatter/projector boundaries,
exactly as the SKILL.md scope-boundary note warned ("Bugs #1/#2 are
NOT migration-blockers ... cross-cut beyond the inline-IR
migration's scope"). It is not part of this skill's pandoc-IR
migration work and shouldn't be revisited here. If a Pandoc inline
test now references `UNRESOLVED_REFERENCE` instead of `LINK` shape-
only, the new behavior is correct — verify against
`pandoc -f markdown -t native` per `.claude/rules/parser.md`.

### Files in committable diff

- `.claude/skills/pandoc-ir-migrate/RECAP.md` (this entry).

### Suggested next sub-targets, ranked

1. **(Optional) Sweep `assets/`, `docs/`, and `.claude/rules/` for
   stale IR-migration phase references.** Spot-check only.
2. **(Optional) Stage 6 of the source-of-truth plan**: bug #2's
   emphasis-crossing fix via a 4-pass restructure of
   `build_full_plans`. Out of scope here; tracked under that plan.

### Don't redo / known traps (new this session)

- **NEW: bugs #1/#2 are no longer "deferred" — they're resolved.**
  Recaps (xii)/(xiii)/(xiv)/(xv) all listed these as the top-ranked
  next sub-target. They've now landed via the source-of-truth
  migration and should not appear in future recap "next sub-targets"
  lists. The SKILL.md scope-boundary note correctly carved them out
  as a parser-linter-LSP cross-cut.

--------------------------------------------------------------------------------

## Earlier session — 2026-04-30 (xv)

**Workspace test count: 0 failing → 0 failing.** **Polish: residual
Phase-N marker sweep.** Sub-targets #2/#3 from recap-(xiv). Greppped
the parser source for "Phase [0-9]" and removed the IR-migration
markers that recap-(xiii) missed: one in `parser.rs`'s
`populate_refdef_labels` doc and five "Phase N" parentheticals in the
opaque-construct scan branches in `inline_ir.rs::build_ir`. The
"Phase 7.1" hits in `formatter/tables.rs` and the YAML/CommonMark-spec
phases are unrelated and stay. clippy + fmt + full workspace test
suite green.

### Files in committable diff

- `crates/panache-parser/src/parser.rs` (1 docstring tweak)
- `crates/panache-parser/src/parser/inlines/inline_ir.rs` (5
  parenthetical phase tags dropped from comments at the
  NativeSpan / FootnoteReference / BracketedCitation / BareCitation /
  BracketedSpan opaque-construct scan branches in `build_ir`)
- `.claude/skills/pandoc-ir-migrate/RECAP.md` (this entry).

### Verification done

- `cargo check --workspace`: clean.
- `cargo test --workspace --no-fail-fast`: all green.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  clean.
- `cargo fmt -- --check`: clean.

### Suggested next sub-targets, ranked

1. **Bugs #1/#2: parser-as-source-of-truth path.** Out of scope for
   this skill; multi-session parser-linter-LSP cross-cut. Carried
   from recap-(xiv).
2. **(Optional) Sweep `assets/`, `docs/`, and `.claude/rules/` for
   stale IR-migration phase references.** Spot-check only — these are
   the unsearched corners. Likely empty: `.claude/rules/parser.md`
   already references migration *concepts* (TEXT-coalescence rule,
   pandoc-native-as-reference) without phase numbers, which is the
   intended end-state.

### Don't redo / known traps

All traps from recap-(i) through recap-(xiv) still apply. No new
traps this session — the sweep was mechanical comment cleanup.

--------------------------------------------------------------------------------

## Earlier session — 2026-04-30 (xiv)

**Workspace test count: 0 failing → 0 failing.** **Audit only — no
code change.** Investigated sub-target #1 from recap-(xiii): "drop
`LinkScanContext.skip_autolinks`?" Conclusion: **NOT redundant — must
stay.** The recap-(xii)/(xiii) suggestion conflated two mechanisms that
operate in different code paths and different dialects. No diff this
session beyond updating RECAP.md.

### What was investigated

`pandoc_bracket_extent` (Pandoc-only, `build_ir`) and
`LinkScanContext.skip_autolinks` (CM-only, `find_link_close_bracket`)
look superficially overlapping but do different work:

- **`pandoc_bracket_extent`** suppresses *autolink Construct emission*
  in `build_ir`'s scan while inside a Pandoc bracket-shape link/image's
  text. Effect: the IR's `process_brackets` and emphasis pass don't see
  spurious autolink Constructs inside Pandoc link text.
- **`skip_autolinks`** controls *bracket-counting opacity* in
  `find_link_close_bracket` (and `link_text_contains_inner_link`) —
  the dispatcher's helper that walks forward to find the matching `]`.
  Under CM it skips past `<...>` autolinks as opaque so a `]` inside
  the URL doesn't terminate the bracket; under Pandoc it does NOT skip,
  so an inner `]` legitimately terminates.

The two mechanisms are complementary, not redundant. The Pandoc
lookahead in `build_ir` (`try_pandoc_bracket_link_extent`) calls
`try_parse_inline_link` etc. with `ctx.skip_autolinks = false`, and
that's how the lookahead arrives at the correct (Pandoc-native-matching)
extent. Removing `skip_autolinks` would break this.

### Pandoc-native verification

Anchor case from recap-(xi): `[foo<https://example.com/?search=](uri)>`.

```
$ printf '%s' '[foo<https://example.com/?search=](uri)>' \
    | pandoc -f markdown -t native
[ Para [ Link ("","",[]) [Str "foo<https://example.com/?search="] ("uri","")
       , Str ">" ] ]

$ printf '%s' '[foo<https://example.com/?search=](uri)>' \
    | pandoc -f commonmark -t native
[ Para [ Str "[foo"
       , Link ("","",[]) [Str "https://example.com/?search=](uri)"]
              ("https://example.com/?search=](uri)","") ] ]
```

Pandoc-native results materially diverge: under markdown the bracket
closes at the inner `]`; under commonmark the autolink swallows that
`]` and the outer `[` is unresolved. `skip_autolinks` is the lever that
encodes this divergence in the dispatcher's bracket-scanner.

### Files changed

- `.claude/skills/pandoc-ir-migrate/RECAP.md` (this entry).

### Suggested next sub-targets, ranked

1. **Bugs #1/#2: parser-as-source-of-truth path.** Out of scope for
   this skill; multi-session parser-linter-LSP cross-cut.
2. **Optional Phase-N grep across the skill's surrounding code** —
   rules / fixtures / docs may still carry phase markers that help
   readers situate code. Skim and tighten where they aren't load-
   bearing as historical reference.
3. **Run an unprompted polish pass on `inline_ir.rs`'s long inline
   comments around `compute_flanking` / `process_emphasis_in_range_filtered`
   to drop "Phase N" framings if any remain.** Spot-check only —
   recap-(xiii) walked the docstrings, not the inline algorithm
   comments.

### Don't redo / known traps (new this session)

- **NEW: `LinkScanContext.skip_autolinks` is load-bearing — DON'T
  drop it.** Recap-(xii) and recap-(xiii) listed dropping it as a
  cleanup candidate based on overlap with `pandoc_bracket_extent`.
  The two mechanisms operate in different code paths (build-time IR
  scan vs dispatcher's bracket-counting helper) and serve different
  dialects. Removing it would change the Pandoc parse of
  `[foo<https://...>](uri)>`-style inputs to match CM semantics
  (verified above against pandoc-native). Field stays.

--------------------------------------------------------------------------------

## Earlier session — 2026-04-30 (xiii)

**Workspace test count: 0 failing → 0 failing.** **Polish: docstring cleanup
pass.** Sub-target #1 from recap-(xii). Updated module-level and type-level
docstrings in `inline_ir.rs` and `core.rs` that still framed the IR as
`Dialect::CommonMark`-only or claimed brackets are dispatcher-driven for Pandoc
--- both stale post-Phase-8-D. Also dropped "Phase 2/4/5/8" in-line phase tags
from comments at dispatcher branches and the IR-driven dispatch arms; the
migration is complete-in-spirit so phase numbers no longer help readers situate
the code. Net diff: documentation only --- `inline_ir.rs`
(~6 docstring blocks rewritten) and
`core.rs` (~7 comment blocks tightened). 0 →
0 tests; clippy + fmt + CommonMark conformance allowlist all green.

### What landed this session

1. **`inline_ir.rs`module docstring**: rewrote the 3-pass pipeline summary. Old:
   "Inline IR for the CommonMark dialect" + "The IR is
   `Dialect::CommonMark`-only. The Pandoc dialect retains its existing
   recursive-descent inline parser; both paths coexist behind the `dialect`
   switch in `super::core::parse_inline_text_recursive`." New: "Inline IR for
   both CommonMark and Pandoc dialects" + 3-pass summary now (1) covers Pandoc
   opaque scan additions in `build_ir`,
   (2) reorders to brackets-then-emphasis to match the actual pass order in
       `build_full_plans`, (3) describes the dialect-specific bracket resolution
       semantics (CM refdef-aware + §6.3 deactivation; Pandoc shape-only +
       outer-wins via `suppress_inner_links`), and
   (4) names the dispatcher's role as "called to *parse* a matched range, not to
       *resolve* it" --- the migration-complete invariant.
2. **`ConstructDispo`docstring**: dropped the "(Phase 2)" phase tag.
3. **`ConstructPlan`docstring**: rewrote "currently inline footnotes and native
   spans. Phase 2 of the Pandoc IR migration: ..." to enumerate all six
   dispatched constructs (inline footnotes, native spans, footnote references,
   bracketed citations, bare citations, bracketed spans) and clarify that the
   legacy dispatcher branches are CM-gated and only fire when the relevant
   extension is enabled.
4. **`build_full_plans`docstring**: rewrote "return both the `BracketPlan` and
   the byte-keyed `EmphasisPlan` --- packaged together so the CommonMark inline
   emission path can consume them in one go." New: returns the bundled
   `InlinePlans` (emphasis, brackets, and constructs) for either dialect.
5. **`ConstructKind`variant docstrings** (`InlineFootnote`, `NativeSpan`,
   `FootnoteReference`, `BracketedCitation`, `BareCitation`, `BracketedSpan`):
   dropped the "(Phase 2)", "(Phase 3)", "(Phase 4)", "(Phase 5)" phase tags.
   The variants' purpose, recognition site, and dispatcher-fallback gating
   remain accurately documented.
6. **`core.rs`module docstring**: rewrote "Emphasis pair selection is entirely
   IR-driven; brackets are IR-driven for `Dialect::CommonMark` and
   dispatcher-driven for `Dialect::Pandoc`." New: resolution decisions for
   emphasis, brackets, and standalone Pandoc constructs are entirely IR-driven
   for both dialects; the dispatcher's `try_parse_*` recognizers are still
   called to *parse* matched byte ranges into CST subtrees but no longer
   participate in resolution.
7. **`parse_inline_text_recursive`docstring**: rewrote the legacy "recursive
   emphasis algorithm" framing (greedy left-to-right, first-match-wins, "When we
   see `*` or `_`, try to parse emphasis recursively") that described the
   deleted `try_parse_emphasis` path. Now describes the IR-plan-driven walk.
8. **In-line "(Phase N)" tags removed** from comments at:
   - The `ConstructPlan`-lookup IR-driven dispatch arm at the top of
     `parse_inline_range_impl` ("Phase 2" → just "IR-driven dispatch:").
   - The `BracketPlan`-lookup IR-driven dispatch arm ("(Phase 8)").
   - The CM-gated dispatcher branches for inline footnotes ("Phase 2" tag
     dropped), native spans ("Phase 2"), footnote refs / bracketed citations
     ("Phases 3-4"), bracketed spans ("Phase 5"), bare citations ("Phase 4"),
     suppress-author citations ("Phase 4"). Wording trimmed to "Under Pandoc
     dialect this is consumed via the IR's `ConstructPlan`..." in each case.

### Verification done

- `cargo check --workspace`: clean.
- `cargo test --workspace --no-fail-fast`: 0 failing (same buckets as pre-edit).
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: clean.
- `cargo fmt -- --check`: clean.
- `cargo test -p panache-parser --test commonmark commonmark_allowlist`: green.

### Files in committable diff

- `crates/panache-parser/src/parser/inlines/inline_ir.rs`
- `crates/panache-parser/src/parser/inlines/core.rs`
- `.claude/skills/pandoc-ir-migrate/RECAP.md` (this entry).

### Suggested next sub-targets, ranked

1. **Drop `LinkScanContext.skip_autolinks`?** Carried forward from recap-(xii).
   Since `build_ir`'s `pandoc_bracket_extent` now suppresses autolink Constructs
   while inside a Pandoc bracket, the dispatcher's per-call `skip_autolinks`
   flag may be redundant. Audit call sites and simplify if so. Minor cleanup.
2. **Bugs #1/#2: parser-as-source-of-truth path.** Out of scope for the IR
   migration; multi-session parser-linter-LSP cross-cut.
3. **Optionally**: search for any remaining Phase-N references in the skill's
   surrounding code (rules, fixtures) --- though the SKILL.md itself
   intentionally retains the phased plan as historical reference.

### Don't redo / known traps

All Phase 1--8 traps still apply. No new traps discovered this session; diff is
documentation-only.
