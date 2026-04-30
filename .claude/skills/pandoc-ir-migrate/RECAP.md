# Pandoc IR migration — running session recap

This file is the rolling, terse handoff between sessions of the
`pandoc-ir-migrate` skill. Read it at the start of a session for
suggested next sub-targets and known traps; rewrite the **Latest
session** entry at the end with what changed and what to look at next.

Keep entries short. Test counts + a one-line root cause beat a
narrative. The hard-won judgment calls (why a lever was chosen, why an
approach was reverted, what trap to avoid) are the load-bearing
content here.

--------------------------------------------------------------------------------

## Latest session — 2026-04-30 (x)

**Workspace test count: 0 failing → 0 failing.** **Phase 8 partially
landed (sub-step C only).** Bug #3 (link-in-link nesting under Pandoc:
`[link [inner](u2)](u1)` produced a nested `LINK` instead of outer
`LINK` with literal `[inner](u2)` inside) is fixed. Bugs #1 and #2 are
DEFERRED — see "Why bugs #1/#2 deferred" below. Diff is committable.

### What landed this session

1. **`parse_inline_text` 4th arg repurposed** from unused
   `_allow_reference_links` to **load-bearing** `suppress_inner_links:
   bool`. When `true`, the recursion suppresses inner LINK / REFERENCE
   LINK recognition (images, emphasis, code, etc. still recognised).
2. **`parse_inline_range_impl` plumbed** with new
   `suppress_inner_links: bool` parameter. Threaded through the
   emphasis-recursion call site.
3. **IR-driven `ConstructDispo::PandocLinkOrImage` arm gated** on
   `!(suppress_inner_links && !is_image)`. The `if is_image { … }
   else { … }` branches were restructured to:
   ```
   if suppress_inner_links && !is_image {
       // fall through — inner link suppressed, bytes go to TEXT
   } else if is_image {
       // image dispatch
   } else {
       // link dispatch
   }
   ```
4. **`emit_inline_link` and `emit_reference_link` now pass `true`**
   for `suppress_inner_links`. All other callers (image alt,
   bracketed span, native span, inline footnote, mark, strikeout,
   subscript, superscript) keep `false` — pandoc-native verifies
   nested LINK is allowed in those contexts.
5. **Existing fixture `link_inside_link_text_pandoc` snapshot
   updated.** The old snapshot encoded the legacy nested-LINK bug
   (outer `LINK` containing inner `LINK`); the new snapshot matches
   pandoc-native (outer `LINK` with `LINK_TEXT` containing
   `TEXT@1..16 "foo [bar](/uri)"` literal).
6. **Two new Pandoc fixtures added:**
   - `image_inside_link_text_pandoc` — locks down that
     `[outer ![inner](u2)](u1)` keeps the inner `IMAGE_LINK` inside
     `LINK_TEXT` (image-in-link is allowed). Verified against
     `pandoc -f markdown -t native`.
   - `link_inside_reference_link_text_pandoc` —
     `[outer [inner](u)][bar]` with `[bar]: /barurl` produces an
     outer reference `LINK` whose text contains `TEXT "outer [inner](u)"`
     (inner LINK suppressed). Verified against pandoc-native.

### Why bugs #1 / #2 deferred

The recap-(ix) plan called for making `reference_resolves` dialect-
neutral (always consult `refdef_labels`) to fix:
- Bug #1: `[foo]` (no refdef) → malformed `LINK` → should be `Str
  "[foo]"`.
- Bug #2: `*foo [bar* baz]` → `TEXT` + partial `LINK` → should be
  all literal.

**Attempted in this session, then reverted.** Removing the
`Dialect != CommonMark → return true` short-circuit in
`reference_resolves` (core.rs:32) cleanly fixed bugs #1 and #2 at
the parser level, BUT broke 7+ downstream tests in linter / LSP /
Salsa:
- `linter::rules::undefined_references` (`reports_missing_reference_labels`,
  `implicit_heading_references_require_auto_identifiers`) — the
  linter walks `LINK` nodes to flag unresolved refs. With the fix,
  unresolved bracket patterns become `TEXT`, so the linter has
  nothing to flag.
- `lsp::handlers::heading_link_conversion` (3 tests) — code action
  to convert implicit heading links walks `LINK` nodes.
- `lsp::test_diagnostics::test_code_action_convert_implicit_heading_link_to_explicit`,
  `lsp::test_goto_definition` (4 tests),
  `lsp::test_incremental_edits::test_incremental_edit_updates_dependents`,
  `lsp::test_rename::test_rename_heading_reference_updates_shortcut_and_hash_links` —
  all walk `LINK` nodes for unresolved refs.
- `salsa::tests::symbol_usage_index_collects_heading_ranges_for_links_and_ids` —
  same root cause.
- Multi-file project tests
  (`test_unused_definitions_resolved_across_project_files`,
  `test_missing_reference_targets`) — same.
- Format golden cases (`reference_links`, `reference_images`,
  `mmd_link_attributes`, `citations`, `mmd_link_attributes_disabled`)
  — formatter walks `LINK` nodes.

The Panache parser INTENTIONALLY emits `LINK` nodes for unresolved
shortcut refs under Pandoc dialect (shape-only) so downstream
features (linter, LSP, formatter) can operate on the bracket-shaped
patterns regardless of refdef resolution. This is a deliberate
architectural choice — the fix-bug-#1-and-#2 work crosses parser /
linter / LSP boundaries and needs a coordinated change beyond the
inline-IR migration's scope.

**Bugs #1/#2 are NOT migration-blockers.** They are pre-existing
parser-vs-pandoc-native divergences. The IR migration's stated goal
is "the IR is the single source of truth for what is this byte
range?" — bugs #1/#2 are about *what the byte range MEANS* (a literal
`Str` vs a `Link` node), which is downstream of the migration's
scope.

### Verification done

- Workspace tests: 0 → 0 failing (256 → 258 in golden_parser_cases
  bucket; +2 new fixtures).
- CommonMark conformance allowlist (`commonmark_allowlist`): green.
- clippy `--all-targets --all-features -- -D warnings`: clean.
- `cargo fmt -- --check`: clean.
- Manual probes against `pandoc -f markdown -t native`:
  - `[foo [bar](/uri)](/uri)` → outer `Link` with `Str "foo", Space,
    Str "[bar](/uri)"` ✓
  - `[outer ![inner](u2)](u1)` → outer `Link` with `Str "outer",
    Space, Image [Str "inner"] ("u2","")` ✓
  - `[outer [inner](u)][bar]` (with refdef) → outer reference `Link`
    with `Str "outer", Space, Str "[inner](u)"` ✓
  - `[*emph*](u)` → `Link` with `Emph [Str "emph"]` ✓
  - `` [`code`](u) `` → `Link` with `Code "code"` ✓

### Files in committable diff

- `crates/panache-parser/src/parser/inlines/core.rs`
  (parameter additions; gate; docstring rewrites)
- `crates/panache-parser/src/parser/inlines/links.rs`
  (`emit_inline_link` and `emit_reference_link` pass `true`)
- `crates/panache-parser/tests/golden_parser_cases.rs`
  (+2 case names)
- `crates/panache-parser/tests/fixtures/cases/image_inside_link_text_pandoc/`
  (new directory: input.md, parser-options.toml)
- `crates/panache-parser/tests/fixtures/cases/link_inside_reference_link_text_pandoc/`
  (new directory: input.md, parser-options.toml)
- `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_link_inside_link_text_pandoc.snap`
  (legacy buggy fixture corrected to pandoc-native)
- `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_image_inside_link_text_pandoc.snap`
  (new)
- `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_link_inside_reference_link_text_pandoc.snap`
  (new)
- `.claude/skills/pandoc-ir-migrate/RECAP.md` (this entry)

### Suggested next sub-targets, ranked

1. **Phase 8 sub-step D — fold `ConstructDispo::PandocLinkOrImage`
   into `BracketDispo::Open` and enable `process_brackets` under
   Pandoc.** This is the architectural unification the SKILL calls
   for. After this lands, no Pandoc inline construct depends on the
   dispatcher's `try_parse_*` chain for resolution decisions —
   migration "complete" per the SKILL gate. Implementation outline:
   - Drop `try_pandoc_bracket_opaque` from `build_ir`'s scan
     (lines 586–599). Bracket bytes flow through `OpenBracket` /
     `CloseBracket` events under both dialects.
   - Drop `ConstructKind::PandocLinkOrImage` and
     `ConstructDispo::PandocLinkOrImage` variants and their
     handlers. The CM-gated legacy `[`/`![` dispatcher branches
     ungate (fire for both dialects, consuming `BracketDispo::Open`).
   - Enable `process_brackets` under Pandoc in `build_full_plans`
     (line 2252).
   - Make `process_brackets` dialect-aware: under Pandoc, skip
     `deactivate_earlier_link_openers`; instead, after committing
     a resolution, walk events between `(open_idx+1, close_idx-1)`
     and INVALIDATE inner resolutions (set `OpenBracket.resolution
     = None`, `OpenBracket.active = false`, `CloseBracket.matched
     = false`). This implements Pandoc's outer-wins via post-
     resolution invalidation.
   - Drop the dialect ternary in `parse_inline_text_recursive` /
     `parse_inline_text` so `bracket_plan = Some(&plans.brackets)`
     for both dialects.
   - Important: keep the `suppress_inner_links` flag and its
     gate on the legacy `[` link branches too (under Pandoc the
     legacy branches will now fire). Otherwise nested-link
     suppression in link text breaks again.

   **Caveat about emphasis interaction**: removing
   `try_pandoc_bracket_opaque` opens the `*` chars inside
   bracket-shaped runs to the emphasis pass. For
   `*foo [bar* baz]*` the IR currently has `[bar* baz]` opaque, so
   only `*` at pos 0 and pos 15 are DelimRuns and they pair as
   outer Emph. WITHOUT the opaque, pos 9's `*` becomes a DelimRun
   too, and the IR's left-to-right closer walk would pair (0, 9)
   as inner Emph — DIFFERENT from pandoc-native, which pairs
   (0, 15). Pandoc's recursive-descent has bracket-skip semantics
   in its `ender` walk that the IR doesn't replicate. To keep
   pandoc-native parity here, sub-step D needs ALSO to keep
   bracket-shape opacity for emphasis (but not for emission).
   One mechanism: after `process_brackets` runs under Pandoc,
   build a "bracket opacity" range list (UNRESOLVED bracket pairs
   too), and feed it into the emphasis pass exclusion bitmap
   alongside the resolved-pair exclusion. The "even unresolved
   bracket pairs are emphasis-opaque" rule is Pandoc-only.

2. **Bugs #1/#2: parser-as-source-of-truth path.** Requires
   changing the linter / LSP / formatter to walk source text (or a
   side-channel) for unresolved bracket-shaped patterns, instead of
   walking `LINK` nodes. This is its own multi-session project and
   shouldn't gate the IR migration. Consider deferring to a
   dedicated "refdef-aware downstream" workstream.

3. **Comment / docstring cleanup pass.** Carried forward from
   recap-(ix). Audit comments referencing "the legacy `try_parse_*`
   dispatcher chain" once Phase 8 sub-step D lands. Low-priority.

4. **Optional simplification**: tighten `parse_inline_range_impl`'s
   signature — `plan` and `construct_plan` are always `Some` in
   production callers. After Phase 8 sub-step D enables
   `bracket_plan` for both dialects, that one too. Mild ergonomic
   win.

### Don't redo / known traps (carried forward + new)

All Phase 1–7 traps still apply. Plus:

- **NEW (this session): refdef-aware `reference_resolves` is a
  parser-linter-LSP cross-cut, not a parser-only change.** Bugs
  #1/#2 LOOK like simple parser fixes (drop the
  `Dialect != CommonMark → return true` short-circuit) but break
  7+ downstream tests because the linter / LSP / formatter walk
  `LINK` nodes for refdef resolution. The Panache parser
  INTENTIONALLY emits `LINK` for unresolved Pandoc-shape brackets
  so downstream features have something to operate on. Don't try
  to fix bugs #1/#2 in the parser alone; coordinate with downstream
  consumers first.
- **NEW (this session): `suppress_inner_links` is the new lever.**
  Re-purposed `_allow_reference_links` parameter. Set `true` ONLY
  from `emit_inline_link` (links.rs:786) and `emit_reference_link`
  (links.rs:971). All other recursion entry points keep `false` —
  pandoc-native confirms images / spans / footnotes / emphasis
  ALLOW nested LINK. Don't over-apply: blanket-suppressing in any
  nested context would regress those.
- **NEW (this session): the IR-driven dispatch arm structure**
  for `PandocLinkOrImage` now is:
  ```
  if suppress_inner_links && !is_image { /* fall through */ }
  else if is_image { /* image dispatch */ }
  else { /* link dispatch */ }
  ```
  The empty-body first branch with a comment is the cleanest way
  to fall through under suppress for non-image. Don't refactor it
  into a `&&` guard on the link branch alone — that would lose the
  early-exit shape and make the suppression less obvious to a
  future reader.
- **NEW (this session): emphasis-opacity vs emission-opacity are
  distinct concerns under Pandoc.** Currently both are coupled in
  `try_pandoc_bracket_opaque` (one Construct event handles both).
  Phase 8 sub-step D will need to disentangle: emphasis still
  needs bracket-shape opacity (to match pandoc-native's
  `*foo [bar* baz]*` outer-pair behaviour), but emission uses
  `BracketDispo::Open` for resolved cases and falls through to
  literal TEXT for unresolved cases. See "caveat about emphasis
  interaction" in sub-step D outline above.

--------------------------------------------------------------------------------

## Earlier session — 2026-04-30 (ix)

**Workspace test count: 0 failing → 0 failing.** **Phase 7 LANDED.**
The legacy recursive-descent emphasis path is gone. The
`delimiter_stack` module is deleted; its remaining types
(`EmphasisKind`, `DelimChar`, `EmphasisPlan`) now live in
`inline_ir.rs`. Net diff: -2151 lines (724 from `delimiter_stack.rs`
deletion, ~1300 from `try_parse_emphasis*` family in `core.rs`, plus
test consolidation). CommonMark conformance allowlist preserved;
clippy + fmt clean. Diff is committable.

### What landed this session

1. **Types relocated** from `delimiter_stack` to `inline_ir`:
   `EmphasisKind`, `DelimChar`, `EmphasisPlan` (with `lookup`,
   `is_empty`, `from_dispositions`). The IR's `build_emphasis_plan`
   consumes them locally; downstream `core::parse_inline_range_impl`
   imports them from `inline_ir` instead of `delimiter_stack`.
2. **`reference_resolves` body inlined** in `core.rs`. Previously a
   thin wrapper around `delimiter_stack::reference_resolves`; now a
   self-contained helper that consults `config.refdef_labels` directly.
3. **Legacy emphasis chain deleted** (~1300 lines from `core.rs`):
   `try_parse_emphasis`, `try_parse_emphasis_nested`, `try_parse_one`,
   `try_parse_two`, `try_parse_three`,
   `parse_until_closer_with_nested_one`,
   `parse_until_closer_with_nested_two`, plus their helpers
   `find_first_potential_ender`, `is_unicode_punct_or_symbol`,
   `is_left_flanking`, `is_right_flanking`, `is_valid_ender`,
   `is_valid_same_delim_closer`. The doc-comment `try_parse_emphasis`
   chain referenced in module docstring is also gone.
4. **Private `parse_inline_range` / `parse_inline_range_nested`
   wrappers deleted.** They were only called by the legacy chain
   plus a handful of in-file tests; the tests were converted (or
   deleted as duplicates).
5. **Legacy emphasis branch in `parse_inline_range_impl` deleted.**
   The `// Pandoc dialect: existing recursive-descent emphasis path.`
   block is gone. The `if let Some(plan_ref) = plan { ... }` block
   stays — it covers both dialects since both production callers
   pass `Some(&plans.emphasis)`.
6. **`nested_emphasis: bool` parameter dropped** from
   `parse_inline_range_impl`. It only fed the deleted legacy branch.
7. **`delimiter_stack` module unregistered** in
   `crates/panache-parser/src/parser/inlines.rs`; the file is
   deleted from disk.
8. **Module docstring on `core.rs` rewritten** to describe the
   IR-driven emission walk instead of the recursive-descent algorithm.
9. **Tests consolidated**: 2 duplicates of `test_recursive_*`
   deleted (`test_parse_simple_emphasis`,
   `test_parse_nested_emphasis_strong`); 10 tests calling
   `try_parse_emphasis` / `parse_inline_range` directly converted
   to wrap with PARAGRAPH/DOCUMENT and call
   `parse_inline_text_recursive`. Byte-count assertions
   (`Some((6, 1))`-style) dropped — the IR doesn't expose that
   shape and the structural assertions still verify behaviour.
10. **`delimiter_stack`'s own test module** (10 tests:
    `simple_emph_pair`, `simple_strong_pair`,
    `rule_9_multiple_of_3_rejects`, `run_split_4_open_1_close`,
    `lazy_opener_preference`, `nested_strong_inside_emph_triple_run`,
    `intraword_underscore_rejected`, `empty_input`,
    `no_delimiters`, `escape_blocks_delim`) removed with the file.
    These tests targeted `delimiter_stack::build_plan` which no
    longer exists. The IR's own test module in `inline_ir.rs`
    covers equivalent scenarios.

### Verification done

- Workspace tests: 0 → 0 failing (one bucket: 1005 → 993, accounted
  for by 12 deleted/inlined tests above).
- CommonMark conformance allowlist (`commonmark_allowlist`): green.
- clippy `--all-targets --all-features -- -D warnings`: clean.
- `cargo fmt -- --check`: clean.

### Files in committed-ready diff

- `crates/panache-parser/src/parser/inlines.rs`
  (-2 lines: unregister `delimiter_stack` mod)
- `crates/panache-parser/src/parser/inlines/core.rs`
  (-1864 lines net: legacy chain + tests + dead branches)
- `crates/panache-parser/src/parser/inlines/delimiter_stack.rs`
  (-724 lines: file deleted)
- `crates/panache-parser/src/parser/inlines/inline_ir.rs`
  (~+50/-20: types moved in, super::delimiter_stack:: refs removed)
- `.claude/skills/pandoc-ir-migrate/RECAP.md`
  (this entry)

### Suggested next sub-targets, ranked

Phases 0–7 of the SKILL.md plan are landed, but the **migration is
not complete** until Phase 8 lands. The original 8-phase plan
treated Pandoc bracket emission as a pure dispatcher-chain
passthrough (Phase 6 was implemented as `ConstructPlan`-driven
dispatch over the existing `try_parse_*` recognizers). That keeps
the dispatcher's three pre-existing pandoc-native divergences — and
leaves Pandoc bracket emission as the ONLY path that doesn't
consume the IR's resolution decisions. Closing that gap is the
final phase.

1. **Phase 8 — Pandoc bracket emission on the IR's `BracketPlan`
   (REQUIRED to finish the migration).** Enable `process_brackets`
   under `Dialect::Pandoc` in `inline_ir::build_full_plans` (today
   gated on `Dialect::CommonMark`) and switch the Pandoc
   bracket-emission branch in `parse_inline_range_impl` to consume
   `BracketDispo` instead of re-running the dispatcher's
   `try_parse_inline_link` / `try_parse_reference_link` chain. The
   `ConstructDispo::PandocLinkOrImage` variant folds into
   `BracketDispo::Open`. The IR's bracket pass needs Pandoc-aware
   semantics for two divergences from CommonMark §6.3:
   - **No nested links.** Pandoc-native is outer-wins with inner
     literal: `[link [inner](u2)](u1)` →
     `Link [Str "link", ..., Str "[inner](u2)"] ("u1", "")`.
     CommonMark §6.3's `deactivate_earlier_link_openers` does the
     OPPOSITE (inner-wins, deactivate outer). The Pandoc rule is
     greedy outer-first matching — when an opener resolves, mark
     all bracket events inside its range as inactive so they emit
     literal. NOT just toggling `deactivate_earlier_link_openers`
     off (which is what SKILL.md originally proposed and recap-(viii)
     correctly flagged as wrong).
   - **Refdef-aware shortcut resolution.** Pandoc's
     shape-only-resolves rule (`reference_resolves` returns `true`
     unconditionally under `Dialect::Pandoc`) is wrong; it produces
     malformed LINK nodes for unresolved shortcut refs. Under
     Pandoc, `[foo]` with no refdef should parse as literal text
     (matching `Str "[foo]"`); `*foo [bar* baz]` should be all
     literal because `[bar*` is not a valid reference. The fix:
     have the IR's `process_brackets` consult `refdef_labels` for
     Pandoc too (currently it falls through to `true` because
     `is_commonmark` is false). With shortcut resolution refdef-
     gated, the inner `*` chars become available to the emphasis
     scanner, fixing both the malformed-LINK and partial-LINK bugs.

   Side effects to expect:
   - The legacy dispatcher branches for `[`/`![` link / image /
     reference-link / reference-image (currently CM-gated by
     Phase 6) become reachable from CommonMark only and can be
     deleted once the IR-driven Pandoc path replaces them. Or
     keep them for the CommonMark branch — same shape — and just
     gate the IR-driven path on dialect.
   - `ConstructKind::PandocLinkOrImage` and its `ConstructDispo`
     variant become unused once `BracketDispo::Open` carries
     image/link discrimination. Remove.
   - Pandoc-specific golden fixtures for the three bug cases
     should be added under
     `crates/panache-parser/tests/fixtures/cases/` and verified
     against `pandoc -f markdown -t native`.

   Verification gate: after Phase 8, no Pandoc inline construct
   should depend on the dispatcher's `try_parse_*` chain for
   resolution decisions — only for *parsing* the matched range
   into a CST subtree. The IR is the single source of truth for
   "what is this byte range?". That is what "migration complete"
   means.

2. **Comment / docstring cleanup pass.** With the legacy emphasis
   chain gone (Phase 7) and the bracket dispatcher chain gone or
   demoted (Phase 8), comments referencing "the legacy
   `try_parse_*` dispatcher chain" need to be audited and
   re-worded. Examples: the `bracket_plan` Some/None branches in
   `parse_inline_text_recursive` and `parse_inline_text`; the
   "legacy branches still run" comment in `parse_inline_range_impl`.
   Low-priority polish; do AFTER Phase 8 when the dust settles.

3. **Optional simplification**: tighten `parse_inline_range_impl`'s
   signature — `plan` and `construct_plan` are always `Some` in
   production callers, so `Option<&...>` could become `&...`. After
   Phase 8 enables `bracket_plan` for both dialects, that one too.
   Mild ergonomic win, not load-bearing.

### Don't redo / known traps (carried forward + new)

All Phase 1-6 traps still apply. Plus:

- **NEW (Phase 7): `EmphasisKind` / `DelimChar` / `EmphasisPlan`
  live in `inline_ir.rs`** — never recreate the
  `super::delimiter_stack::` import; that module is gone. The IR's
  own test module imports these via `use super::*;` (path is
  `crate::parser::inlines::inline_ir::DelimChar`).
- **NEW (Phase 7): `parse_inline_range_impl`'s legacy fallback is
  gone.** The `if byte == b'*' || byte == b'_' { ... continue; }`
  block now contains ONLY the IR-plan-driven match. If a future
  change wants to short-circuit the IR for some new dialect, it
  must add its branch INSIDE the `if let Some(plan_ref) = plan`
  block (or before it) — the bare `continue;` after the if-let is
  a no-advance fallback that depends on `plan` always being
  `Some` in production callers. Keep it that way.
- **NEW (Phase 7): the production callers
  `parse_inline_text_recursive` and `parse_inline_text` ALWAYS
  pass `Some(&plans.emphasis)` and `Some(&plans.constructs)` and
  pass `Some(&plans.brackets)` only for `Dialect::CommonMark`.**
  If a future caller is added, it must do the same — the
  emphasis-pass invariant assumes the plan exists.

### Files in current diff (committable)

- `crates/panache-parser/src/parser/inlines.rs`
- `crates/panache-parser/src/parser/inlines/core.rs`
- `crates/panache-parser/src/parser/inlines/delimiter_stack.rs` (deleted)
- `crates/panache-parser/src/parser/inlines/inline_ir.rs`
- `.claude/skills/pandoc-ir-migrate/RECAP.md`

--------------------------------------------------------------------------------

## Earlier session — 2026-04-30 (viii)

**Workspace test count: 0 failing → 0 failing.** **Phase 6 LANDED
(simple-pattern variant — same as Phases 2-5).** Pandoc bracket-shaped
links and images (`[text](dest)`, `[text][label]`, `[text][]`,
`[text]`, `![alt](dest)`, `![alt][label]`, etc.) now dispatched
IR-first under `Dialect::Pandoc`; legacy dispatcher branches for
`![` (inline image / reference image) and `[...](...)` /
`[...][...]` / `[...]` (inline link / reference link) gated to
`Dialect::CommonMark`. Conformance allowlist preserved; clippy +
fmt clean. Diff is committable.

### Important: Phase 6 deviated from the SKILL.md plan (same as Phase 5)

The SKILL.md description of Phase 6 calls for enabling
`process_brackets` under Pandoc and BracketPlan-driven dispatch,
with `deactivate_earlier_link_openers` gated on CommonMark only
(SKILL.md notes "Pandoc allows link-in-link"). This session
deliberately followed the simpler **Phase 2-5 pattern**
(`ConstructPlan`-driven dispatch) instead, because:

1. Continuing the Phase 2-5 pattern keeps the migration consistent
   and zero-blast-radius. `ConstructPlan`-driven dispatch is pure
   additive — same recognition algorithm, same emission helpers,
   IR just gates *when* they fire. No behavior change.
2. Verification against `pandoc -f markdown -t native` actually
   showed the SKILL.md note about "Pandoc allows link-in-link" is
   incorrect: pandoc-native DOES NOT nest links — input
   `[link [inner](u2)](u1)` produces `Link [Str "link", Space,
   Str "[inner](u2)"] (u1, "")` (outer wins, inner literal). So
   the SKILL.md's specific mechanism of gating
   `deactivate_earlier_link_openers` would have to be re-thought
   anyway; deferring this to a future bug-fix session is cleaner.
3. The legacy `try_parse_emphasis` recursive-descent path (the
   subject of Phase 7) is already dead code in the production flow:
   `parse_inline_text_recursive` and `parse_inline_text` always
   pass `Some(&plans.emphasis)` to `parse_inline_range_impl`, so
   the legacy emphasis branch at `core.rs:2549+` (gated on
   `plan.is_none()`) never fires in production. Phase 7 can
   proceed independently of full BracketPlan integration. The
   `process_brackets` under Pandoc work can be its own
   bug-fix-driven phase later (or never, if not needed).

### What landed this session

1. **New `ConstructKind::PandocLinkOrImage`** in `inline_ir.rs`. The
   existing `try_pandoc_bracket_opaque` recognition for inline
   link / reference link / inline image / reference image (which
   was previously emitted as the catch-all `PandocOpaque`) now
   emits this dedicated kind. `PandocOpaque` retains its meaning
   as the catch-all for opacity-only constructs that don't have
   a dedicated kind yet (currently: math spans via
   `try_pandoc_math_opaque`).

2. **New `ConstructDispo::PandocLinkOrImage { end }`** alongside
   the Phase 2-5 variants. `build_construct_plan` extended with
   one more match arm.

3. **IR-driven dispatch branch** in `parse_inline_range_impl`.
   When `construct_plan.lookup(pos)` returns
   `PandocLinkOrImage { end }`, the branch tries the dispatcher's
   precedence chain in order:
   - Image (`bytes[pos] == b'!'`):
     - `try_parse_inline_image` (gated on `extensions.inline_images`).
     - `try_parse_reference_image` (gated on
       `extensions.reference_links` AND `reference_resolves`).
   - Link (`bytes[pos] == b'['`):
     - `try_parse_inline_link` (gated on `extensions.inline_links`,
       called with `is_commonmark = false` since this branch only
       fires under Pandoc).
     - `try_parse_reference_link` (gated on
       `extensions.reference_links` AND `reference_resolves`).
   Same order as the dispatcher's legacy branches; the
   `pos + len == dispo_end` sanity check ensures the IR's
   recorded range matches the dispatcher's recomputed range.

4. **Three legacy dispatcher branches** gated on
   `config.dialect == Dialect::CommonMark`:
   - The `b'!['` image branch (covers inline image + reference
     image inside the same `if`).
   - The `b'['` inline link branch.
   - The `b'['` reference link branch.
   The legacy footnote-ref / bracketed-citation / bracketed-span
   branches inside the `b'['` block were already CM-gated by
   Phases 3-5; Phase 6 just adds the link/reference-link gates.

5. **Simplified `try_parse_inline_link` argument** in the
   CM-gated legacy branch. Was `config.dialect == Dialect::CommonMark`,
   now hard-coded `true` since the branch only fires under
   CommonMark anyway.

### Why this is "pure additive; no algorithm change"

Same as Phases 2-5: `try_parse_inline_link`, `try_parse_inline_image`,
`try_parse_reference_link`, `try_parse_reference_image`, and the
emission helpers (`emit_inline_link`, `emit_inline_image`,
`emit_reference_link`, `emit_reference_image`) are all unchanged.
`build_ir` and the emission walk in `parse_inline_range_impl` are
augmented with a new kind/dispo pair, but iteration logic for
everything else is untouched. The IR's recorded range and the
dispatcher's recomputed range must agree (both call the same
`try_parse_*` helpers in the same order).

### Files in committed-ready diff

- `crates/panache-parser/src/parser/inlines/inline_ir.rs`:
  +1 `ConstructKind` variant (`PandocLinkOrImage`), +1
  `ConstructDispo` variant (`PandocLinkOrImage { end }`), +3
  lines in `build_construct_plan`. Updated the `build_ir` call
  site that emits the construct event from `PandocOpaque` to
  `PandocLinkOrImage`. Updated `PandocOpaque`'s docstring to
  reflect its narrower remaining scope (math only).
- `crates/panache-parser/src/parser/inlines/core.rs`:
  +118 lines for the `PandocLinkOrImage` dispatch arm in the
  IR-driven branch (mirrors the legacy chain for the four
  link/image forms). +12 lines for the dialect gates on the
  three legacy branches. -1 line simplifying the
  `try_parse_inline_link` argument now that the branch is
  CM-only.

### Verification done

- Workspace tests: 0 → 0 failing.
- CommonMark conformance allowlist: green.
- clippy + fmt: clean.
- Manual probes (all match pre-Phase-6 baseline byte-for-byte):
  - `*[foo](http://x)*` → outer EMPHASIS containing LINK.
  - `[foo]` (no refdef) → partial LINK with TEXT "]" trailing
    (pre-existing legacy behavior; Pandoc dialect's
    `reference_resolves = true` makes the legacy parser emit
    a malformed LINK for shape-only matches; pandoc-native would
    parse this as literal `[foo]`. PRE-EXISTING BUG, OUT OF SCOPE
    for Phase 6 — see "Don't redo / known traps" below).
  - `*foo [bar* baz]` → TEXT "*foo " + partial LINK (similar
    pre-existing bug).
  - `[*foo*](http://x)` → LINK with inner EMPHASIS (scoped pass).
  - `[link [inner](u2)](u1)` → outer LINK containing inner LINK
    (legacy nested-link bug; pandoc-native would parse outer-wins
    with inner literal).
  - `![alt](u)` → IMAGE_LINK (with surrounding FIGURE wrapper).
  - `*[foo](u) bar*` → outer EMPHASIS containing LINK + TEXT.
  - `[foo](u) and [bar](v)` → two separate LINK nodes.

### Pre-existing bugs noted (NOT introduced by Phase 6)

Verification against `pandoc -f markdown -t native` reveals
THREE pre-existing divergences in the Pandoc dialect's link
handling that this session deliberately preserved (no
behavior change):

1. **`[foo]` with no refdef parses as a malformed LINK node**
   instead of literal text. Pandoc-native: `Str "[foo]"`.
   Cause: `delimiter_stack::reference_resolves` returns `true`
   under Pandoc dialect (shape-only opacity), so the legacy
   `try_parse_reference_link` emits LINK regardless of refdef
   resolution.
2. **`*foo [bar* baz]` produces TEXT + partial LINK** instead
   of all-literal. Pandoc-native: `Str "*foo", Space,
   Str "[bar*", Space, Str "baz]"` — emphasis cannot pair
   because `[` would create unmatched-bracket-like ambiguity,
   and `[bar*` is not a valid reference. Same root cause as
   #1: shape-only resolution.
3. **`[link [inner](u2)](u1)` allows nested links** under
   Pandoc. Pandoc-native: outer LINK contains LITERAL
   `[inner](u2)`, not a nested LINK. Cause: the dispatcher's
   recursive `parse_inline_text` for link text re-enters link
   parsing, finding `[inner](u2)` as an inner link.

All three would be addressed by enabling `process_brackets`
under Pandoc with proper refdef resolution and link-in-link
suppression — i.e., the SKILL.md's original Phase 6 vision.
This session deferred that work to keep the diff
zero-regression. A future phase (call it Phase 6b or a
dedicated bug-fix phase) can do the BracketPlan integration.

### Suggested next sub-targets, ranked

Phase 6 (simple variant) is done. Logical next:

1. **Phase 7** — delete legacy emphasis path. The `try_parse_emphasis`
   family (`try_parse_emphasis`, `try_parse_emphasis_nested`,
   `try_parse_one`, `try_parse_two`, `try_parse_three`,
   `parse_until_closer_with_nested_one`,
   `parse_until_closer_with_nested_two`, `parse_inline_range`,
   `parse_inline_range_nested`) is already dead code in the
   production flow (plan is always Some). Deletion is mostly
   mechanical:
   - Remove the legacy emphasis branch in
     `parse_inline_range_impl` (currently `core.rs:~2549+`,
     gated on `plan.is_none()`).
   - Update the in-file tests at `core.rs:2755`, `2781`, `2811`,
     `2862`, `3024` that call the legacy entry points directly.
   - Delete the `delimiter_stack` module and relocate
     `EmphasisPlan`, `DelimChar`, `EmphasisKind`, and
     `reference_resolves` into `inline_ir.rs`.
   This is the LARGEST diff in the migration so far (~1500 lines
   deleted) but mostly mechanical. Estimate: 1 session, mostly
   spent on test updates and rebuild verification.
2. **Phase 6b (optional)** — `process_brackets` under Pandoc.
   Would fix the three pre-existing bugs noted above. Higher
   risk because it changes algorithm. Probably warrants its own
   focused session AFTER Phase 7 lands (less scaffolding to
   preserve).

### Don't redo / known traps (carried forward + new)

All Phase 1-5 traps still apply. Plus:

- **NEW (Phase 6): the SKILL.md plan's claim "Pandoc allows
  link-in-link" is INCORRECT.** Verified against
  `pandoc -f markdown -t native`:
  `[link [inner](u2)](u1)` → `Link [Str "link", ..., Str
  "[inner](u2)"] ("u1", "")` (outer-wins, inner-literal). So
  any future BracketPlan integration under Pandoc must use
  outer-wins semantics, NOT the simple "don't deactivate earlier
  link openers" CommonMark-inverted behavior. The mechanism is
  greedy outer-first matching, not just the
  `deactivate_earlier_link_openers` toggle.
- **NEW (Phase 6): `try_pandoc_bracket_opaque` now produces
  `ConstructKind::PandocLinkOrImage` for bracket-shaped
  link/image bytes.** Math spans (recognized by
  `try_pandoc_math_opaque`) still produce
  `ConstructKind::PandocOpaque`. The two kinds are distinct in
  `build_construct_plan`: `PandocLinkOrImage` gets a dedicated
  `ConstructDispo`, `PandocOpaque` falls through to the `_ => {}`
  arm (no IR-driven dispatch — math has its own dispatcher
  branch).
- **NEW (Phase 6): the IR-driven dispatch arm for
  `PandocLinkOrImage` mirrors the legacy dispatcher's
  precedence chain exactly.** Don't reorder; don't skip steps.
  Inline image first, then reference image (gated on
  `reference_resolves`), then inline link, then reference link
  (also gated on `reference_resolves`). The dispatcher uses the
  same order; matching it ensures byte-identical CST output.
- **NEW (Phase 6): `try_parse_inline_link`'s `is_commonmark`
  arg under the IR-driven Pandoc branch must be `false`.** It
  controls whether the link parser uses CommonMark §6.3
  inline-suffix rules (allow `<dest>` autolink-shaped
  destination, etc.). The legacy CM-gated branch hard-codes
  `true`. Don't conflate the two — the dialect must match
  the call context.
- **NEW (Phase 6): Pandoc's `reference_resolves` returning
  `true` always is a PRE-EXISTING bug that produces malformed
  LINK nodes for unresolved shortcut refs.** Don't try to fix
  it in Phase 6 — it requires `process_brackets` integration
  and changes algorithm. Defer to Phase 6b. The current
  session preserved this bug to keep zero-regression.
- **NEW (Phase 6): Phase 7 does NOT depend on Phase 6.** The
  legacy emphasis path (`try_parse_emphasis*` family) is
  already dead code in the production flow — plan is always
  Some, so the legacy emphasis branch in
  `parse_inline_range_impl` never fires. Deletion is straight
  cleanup, not a behavior change. The recap-(vii) note that
  Phase 6 is "the LAST construct migration before the legacy
  emphasis path can be deleted" is misleading; both can land
  in either order.

### Files in current diff (committable)

- `crates/panache-parser/src/parser/inlines/inline_ir.rs`
- `crates/panache-parser/src/parser/inlines/core.rs`
- `.claude/skills/pandoc-ir-migrate/RECAP.md`

--------------------------------------------------------------------------------

## Earlier session — 2026-04-30 (vii)

**Workspace test count: 0 failing → 0 failing.** **Phase 5 LANDED
(simple-pattern variant).** Bracketed spans `[content]{attrs}` now
dispatched IR-first under `Dialect::Pandoc`; legacy dispatcher branch
gated to `Dialect::CommonMark`. Conformance allowlist preserved;
clippy + fmt clean. Diff is committable.

### Important: Phase 5 deviated from the SKILL.md plan

The SKILL.md description of Phase 5 calls for changing
`process_brackets` and adding `BracketResolutionKind::BracketedSpan`.
This session deliberately followed the simpler **Phase 2-4 pattern**
(`ConstructPlan`-driven dispatch) instead, because:

1. Phases 2-4 already deviated from the original 8-phase plan in the
   same direction: they used `ConstructPlan` (a separate byte-keyed
   construct dispatch table) rather than touching `process_brackets`.
   Continuing that pattern keeps the migration consistent.
2. `process_brackets` integration has not been needed for any
   construct so far. Pandoc-dialect bracket plan stays empty (Phase 1
   established `build_full_plans` skips `process_brackets` under
   Pandoc); changing that would be a much bigger blast radius for
   no current benefit.
3. Phase 6 (Pandoc links/images consume the IR bracket plan) is the
   first phase that genuinely *requires* bracket plan participation.
   When Phase 6 lands, it will introduce a Pandoc-dialect
   `process_brackets` invocation; bracketed spans can then move from
   `ConstructPlan` to `BracketPlan` if it reduces duplication. For
   now the `ConstructPlan` route is correct, simple, and analogous
   to footnote refs / inline footnotes / native spans / citations.

### What landed this session

1. **New `ConstructKind::BracketedSpan`** in `inline_ir.rs`. `build_ir`'s
   `[content]{attrs}` recognition (previously folded into
   `try_pandoc_bracket_opaque` and emitted as generic `PandocOpaque`)
   now has a dedicated branch BEFORE the generic bracket-opaque scan.
   Bracketed-span recognition was REMOVED from `try_pandoc_bracket_opaque`
   to keep the dedicated kind authoritative.

2. **New `ConstructDispo::BracketedSpan { end }`** alongside the
   Phase 2-4 variants. `build_construct_plan` extended with one more
   match arm.

3. **IR-driven dispatch branch** in `parse_inline_range_impl`: same
   shape as Phase 2's `InlineFootnote` / `NativeSpan` branches.
   Re-runs `try_parse_bracketed_span` to extract `(content, attrs)`,
   sanity-checks `pos + len == dispo_end`, calls `emit_bracketed_span`.

4. **Dispatcher's legacy `[text]{attrs}` branch** gated on
   `config.dialect == Dialect::CommonMark`. Under Pandoc the IR
   drives; under CommonMark dialect with the (rare) extension
   override, the legacy branch still fires.

### Why the new IR branch can come BEFORE `try_pandoc_bracket_opaque`

`try_parse_bracketed_span` requires `]` to be IMMEDIATELY followed by
`{` (no `(...)` allowed in between). So:
- `[foo](url){.cls}` → bracketed_span returns None (bytes[5]=`(`); falls
  through to `try_pandoc_bracket_opaque` → inline_link succeeds with
  the full thing including attrs. **Verified vs pandoc-native**:
  produces Link with `[ "cls" ]` attrs.
- `[foo]{.cls}` with `[foo]: /url` refdef → bracketed_span succeeds at
  position 0; pre-empts the (would-have-been-shortcut-ref) link.
  **Verified vs pandoc-native**: pandoc-native ALSO parses this as a
  Span, not a reference link followed by literal `{.cls}`. So the
  IR-first ordering matches pandoc semantics.

### Why this is "pure additive; no algorithm change"

Same as Phase 2-4: `try_parse_bracketed_span` and `emit_bracketed_span`
are unchanged; `build_ir` and the emission walk in
`parse_inline_range_impl` are augmented with a new kind/dispo pair,
but iteration logic for everything else is untouched. The IR's
recorded `[content]{attrs}` range and the dispatcher's recomputed range
must agree (both call `try_parse_bracketed_span`).

### Files in committed-ready diff

- `crates/panache-parser/src/parser/inlines/inline_ir.rs`:
  +1 `ConstructKind` variant, +1 `ConstructDispo` variant, +20 lines
  for the `[content]{attrs}` branch in `build_ir`, +3 lines in
  `build_construct_plan`, -5 lines (removed bracketed-span from
  `try_pandoc_bracket_opaque`).
- `crates/panache-parser/src/parser/inlines/core.rs`:
  +18 lines for the `BracketedSpan` dispatch arm in the IR-driven
  branch, +5 lines for the dialect gate on the legacy branch.

### Verification done

- Workspace tests: 0 → 0 failing.
- CommonMark conformance allowlist: green.
- clippy + fmt: clean.
- Manual probes (all match pandoc-native structurally):
  - `[foo]{.cls}` → BRACKETED_SPAN.
  - `[*foo*]{.cls}` → BRACKETED_SPAN containing EMPHASIS (inner
    inline-parsed via emit_bracketed_span's recursive
    parse_inline_text call).
  - `*[foo]{.cls}*` → outer EMPHASIS containing BRACKETED_SPAN.
  - `*emph [span]{.cls} still emph*` → outer EMPHASIS containing
    BRACKETED_SPAN (opacity confirmed).
  - `[foo](http://x){.cls}` → LINK (inline_link wins because
    bracketed_span requires `]{`, not `](`).
  - `**strong [span]{.cls} strong**` → STRONG containing
    BRACKETED_SPAN.
  - `[*foo* and **bar**]{.cls}` → BRACKETED_SPAN containing EMPHASIS
    + STRONG.
- Pre-existing divergence noted but OUT OF SCOPE: `[[nested]]{.cls}`
  produces BRACKETED_SPAN with inner LINK (legacy `parse_inline_text`
  finds a shortcut ref), but pandoc-native parses inner as plain
  `Str "[nested]"`. This was true before Phase 5 — it's a quirk of
  `emit_bracketed_span`'s recursive inline-parse, not introduced by
  this migration.

### Suggested next sub-targets, ranked

Phase 5 is done. Logical next:

1. **Phase 6** — Pandoc links/images consume the IR bracket plan.
   This is the LAST construct migration before the legacy emphasis
   path can be deleted. Replaces dispatcher branches at
   `core.rs:2113-2234` with bracket-plan-driven dispatch. Gates
   `deactivate_earlier_link_openers` on `Dialect::CommonMark` only
   (Pandoc allows link-in-link). The biggest dispatcher cleanup
   before Phase 7.
   - This is the FIRST phase that genuinely needs Pandoc-dialect
     `process_brackets` participation. Will need to enable
     `process_brackets` for Pandoc with links/images-only resolution
     (no shortcut-ref under Pandoc unless dialect-equivalent).
2. **Phase 7** — delete legacy emphasis path. Fall-out cleanup
   once Phase 6 lands.

### Don't redo / known traps (carried forward + new)

All Phase 1-4 traps still apply. Plus:

- **NEW (Phase 5): bracketed_span recognition can come BEFORE
  `try_pandoc_bracket_opaque` because `try_parse_bracketed_span`
  requires `]` immediately followed by `{`.** Inline links
  (`[text](url)`) and reference links (`[label][refdef]` /
  `[label]`) all have non-`{` characters after `]`, so the dedicated
  branch never shadows them. Verified against pandoc-native:
  `[foo]{.cls}` with refdef `[foo]: /url` produces Span (NOT
  reference link followed by literal `{.cls}`), confirming the
  IR-first precedence is correct.
- **NEW (Phase 5): `emit_bracketed_span` inline-parses inner content
  via `parse_inline_text`, which under Pandoc dialect can find
  shortcut reference links inside the span content even when no
  refdef exists.** Pandoc-native does NOT do this — it treats
  `[[nested]]{.cls}` as Span with literal `Str "[nested]"`. This
  divergence is pre-existing (predates the IR migration) and is
  OUT OF SCOPE for Phase 5. Worth noting as a follow-up improvement
  for the Pandoc inline-parsing semantics inside spans.
- **NEW (Phase 5): the SKILL.md plan's Phase 5 description does NOT
  match what was actually implemented.** The plan calls for
  `BracketResolutionKind::BracketedSpan` and changing
  `process_brackets`. The implementation followed the simpler
  Phase 2-4 pattern (`ConstructKind` + `ConstructDispo`). This is
  consistent with Phases 2-4 also deviating from the original plan
  toward `ConstructPlan`-driven dispatch. Bracket-plan integration
  is deferred to Phase 6 where it's actually needed.

### Files in current diff (committable)

- `crates/panache-parser/src/parser/inlines/inline_ir.rs`
- `crates/panache-parser/src/parser/inlines/core.rs`
- `.claude/skills/pandoc-ir-migrate/RECAP.md`

--------------------------------------------------------------------------------

## Earlier session — 2026-04-30 (vi)

**Workspace test count: 0 failing → 0 failing.** **Phase 4 LANDED.**
Bracketed citations (`[@cite]`) and bare citations (`@key` / `-@key`)
now dispatched IR-first under `Dialect::Pandoc`; legacy dispatcher
branches gated to `Dialect::CommonMark`. Two fixture updates landed
alongside (legacy bug fixes — see below). Conformance allowlist
preserved; clippy + fmt clean. Diff is committable.

### What landed this session

1. **Two new `ConstructKind` variants** in `inline_ir.rs`:
   `BracketedCitation`, `BareCitation`. `build_ir`'s `[@cite]`
   recognition (previously folded into `try_pandoc_bracket_opaque`
   and emitted as generic `PandocOpaque`) now has a dedicated branch
   BEFORE the generic bracket-opaque scan. Bare-citation recognition
   is NEW (no previous IR participation) — added as its own branch
   for `b == b'@'` or `b == b'-' && next == b'@'`. Bracketed-citation
   recognition was REMOVED from `try_pandoc_bracket_opaque` to keep
   the dedicated kind authoritative (Phase 3 trap pattern).

2. **Two new `ConstructDispo` variants** alongside the Phase 2-3
   variants. `build_construct_plan` extended with two more match arms.

3. **IR-driven dispatch branches** in `parse_inline_range_impl`:
   - `BracketedCitation` mirrors Phase 3's pattern exactly.
   - `BareCitation` re-detects via `try_parse_bare_citation` and
     dispatches to `emit_crossref` or `emit_bare_citation` based on
     `is_quarto_crossref_key(key)` and the `quarto_crossrefs` /
     `citations` extension flags — same logic as the legacy branches.

4. **Three legacy dispatcher branches** gated on
   `config.dialect == Dialect::CommonMark`: bracketed citation
   (`[@cite]`), bare `@cite`, and suppress-author `-@cite`.

### Why bare citation needed a NEW recognition site (vs. Phase 3)

Bare citations aren't bracket-shaped, so `try_pandoc_bracket_opaque`
never recognised them. They fell through `build_ir` as text events
and were picked up by the dispatcher's `byte == b'@'` / `byte == b'-'`
branches at emission time. The new IR branch at `b'@'` or `b'-@'`
adds first-class IR participation. Opacity is moot for bare citations
(no internal emphasis-eligible content) — IR participation is purely
for dispatch consolidation toward Phase 7.

### Fixture updates landed (legacy-bug fixes — see trap below)

Two cases in `tests/fixtures/cases/citations/` and three nodes in
`crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_citations.snap`
were misclassified as `LINK` (shortcut reference link) by the legacy
dispatcher when they should have been `CITATION`:

- `[see @doe99, pp. 33-35 and *passim*; @smith04, chap. 1]`
- `[*see* @item1 p. **32**]`
- `[see @item1 chap. 3; also @пункт3 p. 34-35]`

Pandoc-native confirms each is a `Cite`, not a `Link`. The legacy
dispatcher tried `try_parse_reference_link` BEFORE
`try_parse_bracketed_citation`; `reference_resolves` was permissive
enough to accept these as shortcut refs (even without a matching
refdef under Pandoc dialect's looser rules). The new IR-first
dispatch path recognises them as bracketed citations FIRST (the
build_ir branch fires before the dispatcher's reference-link branch),
matching pandoc-native.

The formatter's wrap behavior changes as a consequence: previously
the misparsed LINK exposed internal EMPHASIS/STRONG/CITATION
sub-nodes that the formatter could break across. The correctly-parsed
CITATION is one opaque unit (because `emit_bracketed_citation` emits
flat CITATION_CONTENT tokens, not inline-parsed prefix/suffix). So
the citation is now treated as an unbreakable token. This is
acceptable: a citation is one semantic unit; idempotency is
preserved (re-parsing the wrap output gets the same CITATION).

### Files in committed-ready diff

- `crates/panache-parser/src/parser/inlines/inline_ir.rs`:
  +2 `ConstructKind` variants, +2 `ConstructDispo` variants, +40
  lines for the `[@cite]` and `@cite`/`-@cite` branches in
  `build_ir`, +6 lines in `build_construct_plan`, -5 lines (removed
  bracketed-citation from `try_pandoc_bracket_opaque`).
- `crates/panache-parser/src/parser/inlines/core.rs`:
  +44 lines for the `BracketedCitation` and `BareCitation` dispatch
  arms in the IR-driven branch, +9 lines for dialect gates on the
  three legacy branches.
- `tests/fixtures/cases/citations/expected.md`: 6 lines changed
  (formatter wrap shifts for the 2 newly-correct CITATION cases).
- `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_citations.snap`:
  3 nodes changed from LINK to CITATION (legacy-bug fixes).

### Verification done

- Workspace tests: 0 → 0 failing.
- CommonMark conformance allowlist: green.
- clippy + fmt: clean.
- Manual probes (all match pandoc-native):
  - `*emph @doe99 still emph*` → outer EMPHASIS containing CITATION.
  - `*emph [@doe99] still emph*` → outer EMPHASIS containing
    bracketed CITATION.
  - `*emph -@doe99 still emph*` → outer EMPHASIS containing
    suppress-author CITATION (`-@` marker preserved).
  - `@doe99 says *hi*.` → CITATION + EMPHASIS, separate.
  - `[@doe99; @smith2000] for refs.` → multi-cite CITATION with
    CITATION_SEPARATOR.
  - `**strong @key strong**` → STRONG containing CITATION.

### Suggested next sub-targets, ranked

Phase 4 is done. Logical next:

1. **Phase 5** — bracketed spans `[text]{attrs}` into IR
   `process_brackets`. This is the FIRST phase that *changes*
   `process_brackets` (Phases 1-4 left it untouched under Pandoc).
   Adds a new `BracketResolutionKind::BracketedSpan` variant and
   gates `bracket_says_resolved` on kind. This is materially harder
   than Phases 1-4 because it intersects bracket resolution
   (current bracket plan is built only under CommonMark; Phase 5
   needs to enable a Pandoc-only `process_brackets` invocation
   that handles spans without trying to resolve as
   links/images/refs).
2. **Phase 6** — Pandoc links/images consume the IR bracket plan.
   Replaces dispatcher branches at `core.rs:2113-2234` with
   bracket-plan-driven dispatch. Gates
   `deactivate_earlier_link_openers` on `Dialect::CommonMark` only
   (Pandoc allows link-in-link). The biggest dispatcher cleanup
   before Phase 7.
3. **Phase 7** — delete legacy emphasis path. Fall-out cleanup.

### Don't redo / known traps (carried forward + new)

All Phase 1-3 traps still apply. Plus:

- **NEW (Phase 4): legacy dispatcher's reference-link branch can
  shadow bracketed citations that contain markup.** The legacy
  ordering (try inline link → try reference link → try bracketed
  citation) means `[*see* @item1 p. **32**]` can match as a
  shortcut reference link before reaching the citation branch.
  Under the IR-first dispatch this is correctly identified as a
  citation. This has been load-bearing on existing fixtures: any
  fixture with rich-content bracketed citations that was passing
  the legacy parser was relying on this misclassification. Always
  verify against pandoc-native when fixture diffs flip
  LINK→CITATION; the new IR output is correct.
- **NEW (Phase 4): `emit_bracketed_citation` does NOT inline-parse
  prefix/suffix.** It emits flat `CITATION_CONTENT` tokens. So
  `[*see* @key]` produces a CITATION with `*see*` as literal
  CITATION_CONTENT, not as nested EMPHASIS. Pandoc-native parses
  prefix/suffix as inline content; we don't. This is a known
  limitation OUT OF SCOPE for the IR migration but worth noting
  as a follow-up enhancement (would let the formatter wrap inside
  the brackets and would render emphasis/strong correctly in
  citation prefix/suffix).
- **NEW (Phase 4): bare-citation recognition is NEW IR
  participation, not a migration.** `try_pandoc_bracket_opaque`
  never handled bare citations (they're not bracket-shaped). So
  there's no `try_pandoc_bracket_opaque` cleanup to do for bare
  cites. The IR branch is additive.
- **NEW (Phase 4): both `@` and `-@` go through the same recognizer
  (`try_parse_bare_citation`) — the recognizer handles the
  optional `-` prefix internally.** Don't split into two IR
  branches; one branch with a combined byte-check is correct and
  matches the dispatcher's behavior.

### Files in current diff (committable)

- `crates/panache-parser/src/parser/inlines/inline_ir.rs`
- `crates/panache-parser/src/parser/inlines/core.rs`
- `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_citations.snap`
- `tests/fixtures/cases/citations/expected.md`
- `.claude/skills/pandoc-ir-migrate/RECAP.md`

--------------------------------------------------------------------------------

## Earlier session — 2026-04-30 (v)

**Workspace test count: 0 failing → 0 failing.** **Phase 3 LANDED.**
Footnote references (`[^id]`) now dispatched IR-first under
`Dialect::Pandoc`; legacy dispatcher branch gated to
`Dialect::CommonMark`. Conformance allowlist preserved; clippy + fmt
clean. Diff is committable.

### What landed this session

1. **New `ConstructKind::FootnoteReference`** in `inline_ir.rs`.
   `build_ir`'s `[^id]` recognition (previously folded into
   `try_pandoc_bracket_opaque` and emitted as generic `PandocOpaque`)
   now has a dedicated branch BEFORE the generic bracket-opaque scan.
   Footnote-ref recognition was REMOVED from `try_pandoc_bracket_opaque`
   to keep the dedicated kind authoritative.

2. **New `ConstructDispo::FootnoteReference { end }`** alongside
   `InlineFootnote` and `NativeSpan`. `build_construct_plan` extended
   with one more match arm.

3. **IR-driven dispatch branch** in `parse_inline_range_impl`: same
   shape as Phase 2's `InlineFootnote` / `NativeSpan` branches.
   Sanity-checks `pos + len == dispo_end`; falls through silently if
   they disagree.

4. **Dispatcher's legacy `[^id]` branch** gated on
   `config.dialect == Dialect::CommonMark`. Under Pandoc the IR
   drives; under CommonMark dialect with `extensions.footnotes`
   override (rare), the legacy branch still fires.

### Why this is "pure additive; no algorithm change"

Same as Phase 2: `try_parse_footnote_reference` and
`emit_footnote_reference` are unchanged; `build_ir` and the
emission walk in `parse_inline_range_impl` are augmented with a
new kind/dispo pair, but iteration logic for everything else is
untouched. The IR's recorded `[^id]` range and the dispatcher's
recomputed range must agree (both call `try_parse_footnote_reference`).

### Files in committed-ready diff

- `crates/panache-parser/src/parser/inlines/inline_ir.rs`:
  +1 `ConstructKind` variant, +1 `ConstructDispo` variant, +20
  lines for the `[^id]` branch in `build_ir`, +3 lines in
  `build_construct_plan`, -5 lines (removed footnote-ref from
  `try_pandoc_bracket_opaque`).
- `crates/panache-parser/src/parser/inlines/core.rs`:
  +15 lines for the `FootnoteReference` dispatch arm in the
  IR-driven branch, +3 lines for the dialect gate on the legacy
  branch.

### Verification done

- Workspace tests: 0 → 0 failing.
- CommonMark conformance allowlist: green.
- clippy + fmt: clean.
- Manual probes:
  - `See the note[^myref] for *details*.` → expected
    FOOTNOTE_REFERENCE + EMPHASIS, no interaction.
  - `*emph [^ref] still emph*` → outer EMPHASIS containing
    FOOTNOTE_REFERENCE (opacity confirmed).
  - `multiple refs: [^a] [^b] [^c] ok` → three FOOTNOTE_REFERENCE
    nodes, all extracted.

### Suggested next sub-targets, ranked

Phase 3 is done. Logical next:

1. **Phase 4** — `[@cite]` bracketed citations + bare `@cite`.
   Currently `[@cite]` is recognised in `build_ir`'s
   `try_pandoc_bracket_opaque` helper as generic `PandocOpaque`;
   need to break it out into a dedicated kind. Bare `@cite`
   (no brackets) is more involved — it needs a NEW recognition
   branch in `build_ir` since it's not bracket-shaped. Mirror
   Phase 3's pattern for the bracketed form first.
2. Phases 5-7 in order. Phase 5 is the first phase that
   *changes* `process_brackets` (for bracketed spans
   `[text]{attrs}`).

### Don't redo / known traps (carried forward + new)

All Phase 1-2 traps still apply. Plus:

- **NEW (Phase 3 pattern): when the dedicated kind takes over
  recognition of a construct, also REMOVE that construct from
  `try_pandoc_bracket_opaque`.** Otherwise the same bytes can be
  matched by both branches; the dedicated branch fires first
  (ordering in `build_ir`), but leaving the duplicate in
  `try_pandoc_bracket_opaque` is dead code that future phases
  might reactivate by accident. Removed `try_parse_footnote_reference`
  call from the helper.

### Files in current diff (committable)

- `crates/panache-parser/src/parser/inlines/inline_ir.rs`
- `crates/panache-parser/src/parser/inlines/core.rs`
- `.claude/skills/pandoc-ir-migrate/RECAP.md`

--------------------------------------------------------------------------------

## Earlier session — 2026-04-30 (iv)

**Workspace test count: 0 failing → 0 failing.** **Phase 2 LANDED.**
Inline footnotes (`^[note]`) and native spans (`<span>...</span>`)
now dispatched IR-first under `Dialect::Pandoc`; legacy dispatcher
branches gated to `Dialect::CommonMark`. Conformance allowlist
preserved; clippy + fmt clean. Diff is committable.

### What landed this session

1. **New `ConstructKind` variants** in `inline_ir.rs`:
   `InlineFootnote`, `NativeSpan`. `build_ir`'s `^[note]` and
   `<span>` recognition (added in Phase 1) now emits these kinds
   instead of generic `PandocOpaque`. The other Pandoc bracket-shaped
   opaques (links, ref-links, `[^id]`, `[@cite]`, `[span]{attrs}`,
   math) remain as `PandocOpaque` for now — they're consumed by the
   dispatcher's bracket chain and will move in Phases 3-6.

2. **`ConstructPlan` byte-keyed lookup** added alongside `BracketPlan`
   and `EmphasisPlan` on `InlinePlans`. Maps start-byte → `ConstructDispo`
   (`InlineFootnote { end }` or `NativeSpan { end }`).

3. **IR-driven dispatch** at the top of `parse_inline_range_impl`'s
   loop. When `construct_plan.lookup(pos)` hits, the dispatcher
   re-runs the same `try_parse_inline_footnote` / `try_parse_native_span`
   to extract content+attributes, sanity-checks `pos + len ==
   dispo_end`, then emits via the existing `emit_*` helpers. Same
   recognition algorithm, same emission helpers — the IR just gates
   *when* they fire.

4. **Dispatcher's legacy `^[` and `<span>` branches** gated on
   `config.dialect == Dialect::CommonMark`. Under Pandoc the IR
   drives; under CommonMark dialect with the (rare) extension
   override, the legacy branch still fires.

5. **Signature change**: `parse_inline_range_impl` gained a
   `construct_plan: Option<&ConstructPlan>` argument. All 5 call
   sites (entry points + nested helpers + recursive emphasis call
   at line 2408) updated.

### Why this is "pure additive; no algorithm change"

The `try_parse_*` recognition is unchanged. The `emit_*` emission
helpers are unchanged. The byte-by-byte iteration logic in
`parse_inline_range_impl` is unchanged for everything except the
new top-of-loop `construct_plan.lookup(pos)` early-out. The
sanity-check `pos + len == dispo_end` ensures the IR's recorded
range matches the dispatcher's recomputed range — if they ever
disagree (shouldn't, since both call the same helper), we fall
through to the rest of the loop without panicking.

### Files in committed-ready diff

- `crates/panache-parser/src/parser/inlines/inline_ir.rs`:
  + 2 new `ConstructKind` variants, +60 lines for `ConstructDispo`
    / `ConstructPlan` / `build_construct_plan`, plus `InlinePlans`
    field and `build_full_plans` wiring.
- `crates/panache-parser/src/parser/inlines/core.rs`:
  + import of `ConstructDispo` / `ConstructPlan`, +35 lines for
    the IR-driven dispatch branch at top of loop, signature change
    to `parse_inline_range_impl`, dialect gate on the two legacy
    branches.

### Verification done

- Workspace tests: 0 → 0.
- CommonMark conformance allowlist: green.
- clippy + fmt: clean.
- Manual probe: `*emph ^[*not-emph*] still emph*` produces outer
  EMPHASIS containing INLINE_FOOTNOTE containing inner EMPHASIS,
  matching `pandoc -f markdown -t native` exactly.
- Manual probe: `before <span class="x">*emph*</span> after`
  produces correct BRACKETED_SPAN with nested EMPHASIS.

### Suggested next sub-targets, ranked

Phase 2 is done. Logical next:

1. **Phase 3** — `[^id]` footnote refs into IR scan as
   `Construct::FootnoteReference` with IR-driven dispatch (mirror
   Phase 2's pattern: add `ConstructKind::FootnoteReference`,
   extend `build_construct_plan`, add a `ConstructDispo` variant,
   add a dispatch branch in `parse_inline_range_impl`, gate the
   dispatcher's `[^...]` branch to CommonMark dialect). Currently
   `[^id]` is recognised in `build_ir`'s `try_pandoc_bracket_opaque`
   helper as generic `PandocOpaque`; need to break it out into a
   dedicated kind.
2. **Phase 4** — `[@cite]` bracketed citations + bare `@cite` —
   same pattern as Phase 3.
3. Phases 5-7 in order.

### Don't redo / known traps (carried forward + new)

All Phase 1 traps still apply. Plus:

- **NEW (Phase 2 pattern): always sanity-check `pos + len ==
  dispo_end`** in the IR-driven dispatch branch. The IR records the
  range from `build_ir`'s scan; the dispatcher re-runs the same
  helper to extract the payload. If they ever disagree, fall through
  silently rather than panicking — protects against future drift.
- **NEW: emission helpers `emit_inline_footnote` and `emit_native_span`
  recursively call `parse_inline_text`** for inner content. That
  call constructs ITS OWN `InlinePlans` for the inner range — it
  doesn't reuse the outer plan. So nested constructs (footnote
  inside span, span inside footnote) work correctly without any
  special-case state passing.
- **NEW: don't gate the dispatcher's `^[` branch on
  `!is_commonmark`** alone — gate on the explicit
  `config.dialect == Dialect::CommonMark` check. The migration
  is on `Dialect`, not on a `!is_commonmark` boolean derived
  elsewhere. Same lesson for `<span>`.

### Files in current diff (committable)

- `crates/panache-parser/src/parser/inlines/inline_ir.rs`
- `crates/panache-parser/src/parser/inlines/core.rs`
- `.claude/skills/pandoc-ir-migrate/RECAP.md`

--------------------------------------------------------------------------------

## Earlier session — 2026-04-30 (iii)

**Workspace test count: 6 failing → 0 failing.** **Phase 1 LANDED.**
All previously-failing emphasis tests pass; CommonMark conformance
allowlist preserved; clippy + fmt clean. Diff is committable.

### What landed this session

Three Pandoc-only mechanisms in `inline_ir.rs`, all gated on
`Dialect::Pandoc`:

1. **Cascade-then-rerun loop** (replaces the single in-pass cascade
   call). After each `run_emphasis_pass`, collect cascade
   invalidations into a `rejected_pairs: Vec<(usize, usize)>` and
   re-run; iterate to fixed point. Lets the inner runs the
   between-removal stole back into the candidate pool. Fixes:
   - `*foo **bar* baz**` → `Strong[bar*, " ", baz]`.
   - `**foo *bar **nested** baz* qux**` →
     `Strong[foo, " ", Emph[bar, " ", Strong[nested], " ", baz], " ", qux]`.

2. **Strict-rerun gate** (iter 2+ only). Block a candidate pair if
   any unmatched same-char between-run has remaining count
   strictly greater than the pair's tentative consume. Mirrors
   pandoc-markdown's `one c → option2 → string [c,c]` greedy
   consumption: a stray `**` between two `*`s in a re-run would
   block an Emph that pandoc treats as literal. Fixes
   `**foo *bar** baz*` → all literal (was: spurious Emph in
   iter 2 from naive cascade-rerun).

3. **`pandoc_inner_strong_recovery` post-pass.** For each Emph
   match where opener and closer originally count >= 3, and
   closer has unmatched bytes >= 2, find the rightmost unmatched
   same-char between-run with count >= 2 and `can_close = true`
   and synthesise a Strong match: between-run becomes opener,
   closer's leftover becomes closer, AND the existing Emph
   match's offsets are shifted right (Emph close moves to
   rightmost byte; Strong takes leftmost 2 bytes of closer).
   This is byte-position rewriting, not pure addition. Fixes
   `***foo **bar** baz***` →
   `Emph[Strong[foo, " "], "bar", Strong[" ", baz]]`.

Plus: `parser_cst_emphasis_complex.snap` updated (TEXT-coalescence
diffs only — no structural changes per `diff | grep -v 'TEXT@' |
grep -v assertion_line` returning empty).

### Files in committed-ready diff

Same as session-(ii)'s WIP plus this session's edits:

- `.claude/rules/parser.md` (TEXT-coalescence rule paragraph from
  session-(ii)).
- `crates/panache-parser/src/parser.rs` (`populate_refdef_labels`
  widened, from session-(i)).
- `crates/panache-parser/src/parser/inlines/core.rs` (dispatcher
  fork dropped, from session-(i)).
- `crates/panache-parser/src/parser/inlines/inline_ir.rs` (~660
  net-added lines: dialect gates from session-(i), opaque scan
  from session-(i), cascade rule from session-(i), the three
  mechanisms above from this session).
- `crates/panache-formatter/src/formatter/inline_layout.rs`
  (`intraword_mid` fix, from session-(ii)).
- 9 modified `.snap` files (8 TEXT-coalescence from session-(ii) +
  `emphasis_complex` TEXT-coalescence from this session).

### Algorithm reference: pandoc's `three c` source

The breakthrough this session was reading
`pandoc/src/Text/Pandoc/Readers/Markdown.hs:1692-1718` — the
`three c`, `two c`, `one c` recursive descent. Key properties:

- `ender c n` for `c == '*'` succeeds purely on `count n (char c)`
  because `guard (c == '*')` short-circuits. **It does NOT
  require `notFollowedBy alphaNum`** — for underscore yes, for
  asterisk no. Earlier sessions assumed otherwise; this was the
  trace bug that made me think pandoc's algorithm couldn't
  produce the observed output.
- `enclosure c` consumes a delim run, then tries the
  whitespace-after alternative `(return (B.str cs) <>) <$>
  whitespace`. For `*foo *` the `*` opener is followed by `f`
  (no ws) so case-1 → `one c`; for `bar *` the `*` is followed
  by ` ` (ws) so the whitespace alternative fires and emits the
  `*` as literal. **This IS the can_open=!followed_by_ws gate.**
- `three c` reads content greedy until `ender c 1` candidate.
  Then tries `ender 3 → ender 2 → ender 1` in order:
  - `ender 3` succeeds → `Strong[Emph[content]]` (pandoc-markdown
    swaps Strong/Emph order vs CommonMark; `***foo***` is
    `Strong[Emph[foo]]` in markdown but `Emph[Strong[foo]]` in
    commonmark).
  - `ender 2` succeeds → `one c (B.strong <$> contents)` — wraps
    content as Strong, makes it `one c`'s prefix; outer wrap
    becomes Emph if `one c` finds its `ender 1`. **This is the
    `***A **B** C***` → `Emph[Strong[A], B, Strong[C]]` path.**
  - `ender 1` succeeds → `two c (B.emph <$> contents)` —
    symmetric variant.

### Trap reflections (session experience)

- **Don't trust your manual trace of pandoc's parser without
  re-checking `ender`'s guard chain.** I spent ~30 minutes
  convinced pandoc's algorithm couldn't produce its own output
  because I missed that `guard (c == '*')` short-circuits the
  `notFollowedBy alphaNum` check for asterisks.
- **Cascade-then-rerun without the strict-iter-2+ gate is too
  permissive.** Re-runs find pairs whose CONTENT has stray
  higher-count runs that pandoc would have absorbed via
  `option2`. The strict gate (`remaining > tentative_consume`
  blocks) is the canonical fix; iter-1 must NOT apply it (it
  would over-block legitimate cases like `***foo **bar**
  baz***`).
- **The Strong-recovery for `***A **B** C***` requires REWRITING
  the existing Emph's offsets**, not just appending matches.
  The Emph close has to shift right by 2 bytes so the Strong
  close fits in the leftmost 2 bytes (well-nested CST). I
  initially tried append-only and got CST emission with crossing
  markers (Strong's range extending past Emph's range).
- **`build_emphasis_plan` records by source byte position in a
  BTreeMap**, so two matches on the same run end up at distinct
  positions iff their `start + offset_in_run` differs. The
  emission walk then drives off Open's `partner_len` to know
  how far to skip — Close entries don't carry length.

### Suggested next sub-targets, ranked

Phase 1 is done. Logical next:

1. **Phase 2** — `^[note]` and `<span>...</span>` recognition out
   of the dispatcher's ordered-try chain into `build_ir` as
   `Construct` events. Pure additive; should be straightforward
   given Phase 1's groundwork.
2. **Phase 3** — `[^id]` footnote refs into IR scan as
   `Construct::FootnoteReference`.
3. Phases 4-7 in order.

### Don't redo / known traps (carried forward + new)

- **Don't try "two-pass + un-remove-between" for scoped emphasis.**
  (The `pandoc_inner_strong_recovery` here is NOT this approach
  — it's a targeted post-pass that synthesises specific matches
  with byte-offset rewriting, only firing on the precise outer-
  triple pattern.)
- **Don't widen the cascade rule to `can_open || can_close`.** Must
  be `&&`. Over-invalidates intraword `_` cases.
- **Don't widen the cascade rule to `can_close=true alone`.**
  Tested mentally on `**foo* bar**`: ev1 is right-flanking only
  (`can_open=false, can_close=true`), the cascade with
  `can_close=true alone` would invalidate the legitimate Strong;
  pandoc keeps it.
- **Don't tighten `pandoc_reject` to require strict count
  equality.** Rejects (1,3)/(3,1)/(2,3)/(3,2) which pandoc
  matches.
- **Don't add `can_open = false for count >= 4`.** Breaks
  `**foo****bar**`.
- **Don't preserve a legacy-fixture output that conflicts with
  pandoc-native.**
- **Math opacity in `build_ir` is non-negotiable for losslessness.**
- **`populate_refdef_labels` MUST be widened to Pandoc.**
- **TEXT-coalescence improvements may regress formatter
  escape-logic.** (intraword_mid fix in inline_layout.rs.)
- **NEW: Pandoc cascade-then-rerun must run strict-mode in iter
  2+ only.** Strict in iter 1 over-blocks; strict in iter 2+
  prevents naive re-runs from forming pairs pandoc rejects.
- **NEW: `***A **B** C***` recovery rewrites Emph offsets**, not
  just adds Strong. The Emph close must move right to make room
  for Strong's leftmost 2 bytes; otherwise CST is not
  well-nested.
- **NEW: `ender c n` for `c == '*'` does NOT check
  `notFollowedBy alphaNum`.** When tracing pandoc by hand, do
  NOT assume the alphanum check fires — it's gated behind
  `guard (c == '*') <|> ...` which short-circuits.

### Files in current diff (committable)

All as listed in "Files in committed-ready diff" above.

--------------------------------------------------------------------------------

## Earlier session — 2026-04-30 (ii)

**Workspace test count: 12 failing → 6 failing (uncommitted).**
**Phase 1 still partial — uncommitted; remaining 6 all cluster on
scoped-emphasis-on-strong-matched-inner-range (the keystone refactor).
Do NOT commit until that lands or the remaining tests are
deferred-with-fixture-update per the skill's protocol.** Pre-Phase-1
baseline (origin/main = `33a88e89`) is 0 failures, so this diff still
regresses main.

### What landed this session (on top of session-(i)'s WIP)

- **8 TEXT-coalescence parser-CST snapshot updates** accepted manually
  (no `cargo-insta` available; renamed `.snap.new` → `.snap` and
  stripped `assertion_line:` metadata lines). Verified each diff has
  zero non-TEXT structural lines via
  `diff <snap> <snap.new> | grep -v 'TEXT@' | grep -v assertion_line`.
  Files:
  - `parser_cst_emphasis`
  - `parser_cst_emphasis_intraword_underscore_closer`
  - `parser_cst_emphasis_nested_inlines` (recap-(i) was right to suggest
    re-categorizing — pure TEXT-coalescence after the cascade
    refinement)
  - `parser_cst_emphasis_same_delim_nested_pandoc`
  - `parser_cst_emphasis_skips_shortcut_reference_link`
  - `parser_cst_emphasis_split_runs_pandoc`
  - `parser_cst_equation_attributes_disabled`
  - `parser_cst_reference_definition_label_with_escaped_bracket`
- **Formatter intraword-underscore fix** at
  `crates/panache-formatter/src/formatter/inline_layout.rs:60-70`. The
  TEXT-coalescence improvement broke the formatter's escape logic:
  `escape_special_chars` only treated `_` as intraword when at the
  start or end of a TEXT chunk. With coalesced TEXT
  (`TEXT "quarto_crossrefs"` instead of three separate nodes), the
  `_` is mid-chunk and was being escaped to `\_` despite being
  between two alphanumerics. Added `intraword_mid` branch:
  `_` between two `is_alphanumeric()` chars in the same text run is
  treated as intraword and not escaped. Fixes
  `equation_attributes_disabled` formatter test.

### What remains uncommitted

Phase 0 + Phase 1 algorithm changes (still as in session-(i)):

- `crates/panache-parser/src/parser.rs` (`populate_refdef_labels`
  widened to both dialects)
- `crates/panache-parser/src/parser/inlines/core.rs` (dispatcher fork
  removed; `parse_inline_range` marked `#[allow(dead_code)]`)
- `crates/panache-parser/src/parser/inlines/inline_ir.rs` (~488 lines
  of dialect gates, opaque scan, cascade rule)
- `.claude/rules/parser.md` (TEXT-coalescence rule paragraph added)

This session's additions:

- 8 modified `.snap` files (TEXT-coalescence updates, listed above)
- `crates/panache-formatter/src/formatter/inline_layout.rs`
  (`intraword_mid` fix, ~10 lines)
- 1 leftover `parser_cst_emphasis_complex.snap.new` (kept as a working
  artifact for the structural-regression case; do not accept until
  scoped-emphasis lands)

### Remaining 6 failures (all scoped-emphasis-bucket)

| Test | File | Notes |
| --- | --- | --- |
| `emphasis_complex` | `tests/golden_cases.rs` | Formatter-side; cascades from parser |
| `emphasis_complex` | `crates/panache-parser/tests/golden_parser_cases.rs` | 4 specific input lines diverge structurally |
| `overlapping_emphasis_strong` | `crates/panache-parser/tests/emphasis_parser.rs` | `*foo **bar* baz**` → expects STRONG[bar* baz] |
| `test_deeply_nested_emphasis` | `crates/panache-parser/src/parser/inlines/tests.rs:1485` | `**foo *bar **nested** baz* qux**` |
| `test_triple_emphasis_pandoc_structure` | `tests.rs:1406` | `***foo **bar** baz***` → outer EM + 2 STRONG |
| `test_triple_emphasis_with_nested_strong` | `tests.rs:1254` | Same input as above; structural assertions |

### Trap discovered this session

- **TEXT-coalescence in the parser cascades into formatter escape-logic
  bugs.** The formatter's `escape_special_chars` made assumptions
  about TEXT-token granularity (intraword `_` detection only at
  chunk start/end). When coalescing the parser CST, audit *every*
  formatter-side place that branches on TEXT boundaries — there may
  be more such bugs latent. The fix landed in
  `inline_layout.rs:60-70` is the canonical pattern: detect intraword
  by checking actual neighboring chars in the text run, not by
  positional heuristics.

### Refining the proposed scoped-emphasis approach

Initial recap-(i) proposal: "strong-only pass first, then scoped
emphasis on each strong pair's inner range." This DOES NOT cleanly
solve `***foo **bar** baz***` (pandoc-native: `Emph[Strong[foo,
" "], "bar", Strong[" ", baz]]`). Reason: the outer `***...***`
matches via Pandoc's "consume 1 first" rule (EM outermost,
`***`-as-`*` + `**`), and the leftover `**`s on each side then
match independently with `**bar**`'s `**`s. A pure strong-only-first
pass would greedily pair `**foo `'s `**` with `**bar`'s closing
`**` (left-to-right closer walk, both eligible), preventing the
correct `***...***` outer match.

The right algorithm is closer to **mutual scoped recursion**:
when the outer `***...***` matches with consume=1 (EM), recursively
process emphasis on the inner range with separate state, including
the leftover `**`s from the outer's opener/closer as additional
runs. This is materially harder than the bracket scoped pass and
needs careful design — start the next session by sketching the
algorithm on paper before editing.

### Suggested next sub-targets, ranked

1. **Scoped emphasis pass design session** — read pandoc's
   `Text.Pandoc.Readers.Markdown.handleEmph` (or equivalent
   recursive-descent code) for the canonical algorithm, then
   sketch the IR-friendly translation. Don't start coding until
   the algorithm handles all 4 of: `***foo **bar** baz***`,
   `**foo *bar **nested** baz* qux**`, `*foo **bar* baz**`, and
   `**foo *bar* baz**` on paper.
2. **Implement scoped emphasis** in `build_full_plans`. Rough size:
   80-150 lines added, similar shape to the existing bracket scoped
   pass (`inline_ir.rs:1714-1746`).
3. **Audit other formatter escape-logic call sites** for TEXT-
   coalescence-driven bugs — likely candidates: `*` escape (similar
   pattern), maybe `[`/`]` near footnote refs.
4. **Phases 2-7** — gated on Phase 1 finishing clean.

### Don't redo / known traps (carried forward)

- **Don't try "two-pass + un-remove-between" for scoped emphasis.**
  See SKILL.md "Trap: two-pass + un-remove-between breaks pair
  crossing".
- **Don't widen the cascade rule to `can_open || can_close`.** Must
  be `&&`. (Recap-(i).)
- **Don't tighten `pandoc_reject` to require strict count equality.**
  Rejects legitimate (1,3)/(3,1)/(2,3)/(3,2) which pandoc DOES match.
- **Don't add `can_open = false for count >= 4`.** Breaks
  `**foo****bar**`.
- **Don't preserve a legacy-fixture output that conflicts with
  pandoc-native.** Verify with `pandoc -f markdown -t native`.
- **Math opacity in `build_ir` is non-negotiable for losslessness.**
- **`populate_refdef_labels` MUST be widened to Pandoc.**
- **TEXT-coalescence improvements may regress formatter tests via
  escape-logic.** Check `escape_special_chars` and similar.

### Files in current uncommitted diff

Same as session-(i)'s + 8 .snap updates + intraword_mid fix:

- `.claude/rules/parser.md`
- `crates/panache-parser/src/parser.rs`
- `crates/panache-parser/src/parser/inlines/core.rs`
- `crates/panache-parser/src/parser/inlines/inline_ir.rs`
- `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_*.snap` (8 files)
- `crates/panache-formatter/src/formatter/inline_layout.rs`
- `crates/panache-parser/tests/snapshots/golden_parser_cases__parser_cst_emphasis_complex.snap.new` (untracked, intentional)

--------------------------------------------------------------------------------

## Earlier session — 2026-04-30 (i)

**Workspace test count: 0 failing → 15 failing (+15, uncommitted).**
**Phase 0 LANDED clean and committed-ready.** **Phase 1 partial — diff
is uncommitted; do NOT commit until the remaining 15 regressions are
resolved or deferred-with-fixture-update per the skill's protocol.**

### Phase 0 — Thread `Dialect` through `inline_ir`

- Status: **complete, clean**.
- Diff: +21 / -9 in `crates/panache-parser/src/parser/inlines/inline_ir.rs`
  only.
- Changed `compute_flanking`, `process_emphasis`,
  `process_emphasis_in_range`, `process_emphasis_in_range_filtered`
  to accept `Dialect`. All branch points stubbed; no behavior change.
- Validation: workspace tests, clippy, fmt all green; CommonMark
  conformance unchanged.

### Phase 1 — Pandoc emphasis on the IR (PARTIAL)

What landed and works:

- **Pandoc opaque-construct scan in `build_ir`** under `!is_commonmark`:
  inline links, reference links, `[^id]`, `[@cite]`, `[text]{attrs}`,
  `^[note]`, `<span>...</span>`, `$math$` and other tex-math forms all
  emit `ConstructKind::PandocOpaque`. **Math opacity is load-bearing
  for losslessness** — without it, the IR pairs emphasis across `$a *
  b$` and the dispatcher's later math parse re-claims the bytes,
  producing `+9 byte` losslessness failures.
- **`build_full_plans` skips `process_brackets` under Pandoc.**
  Bracket plan stays empty; dispatcher's bracket branches keep firing.
  `parse_inline_text_recursive` / `parse_inline_text` always call
  `build_full_plans` (dispatcher fork dropped) but pass
  `bracket_plan = None` under Pandoc.
- **Dialect gates active**:
  - `compute_flanking` Pandoc branch: `can_open = !followed_by_ws`,
    `can_close = true` always (Pandoc's `ender` has no flanking gate),
    intraword underscore hard-rule on top.
  - `pandoc_reject` opener-finder gate: rejects (1,2), (2,1), and
    `count_o >= 4`.
  - Mod-3 rejection gated on `is_commonmark`.
  - Triple-emph nesting flip: under Pandoc, `count_o >= 3 &&
    count_c >= 3` consumes 1 first → STRONG(EM) for `***x***`.
- **`pandoc_cascade_invalidate` post-pass**: invalidates pairs
  containing unmatched same-ch runs with `can_open && can_close`.
  Iterates to fixed point. **The `&&` is load-bearing** — `||`
  over-invalidates intraword `_` cases.
- **`populate_refdef_labels` widened** to both dialects in
  `parser.rs`.

What's deferred (the 15 remaining workspace failures):

- 2 in `tests/golden_cases.rs` (top-level formatter):
  `equation_attributes_disabled`, `emphasis_complex`.
- 1 in `tests/emphasis_parser.rs`: `overlapping_emphasis_strong`
  (`*foo **bar* baz**`).
- 3 in lib `complex_emphasis_tests`: `test_deeply_nested_emphasis`,
  `test_triple_emphasis_pandoc_structure`,
  `test_triple_emphasis_with_nested_strong`.
- 9 in `tests/golden_parser_cases.rs`. Of these:
  - **7 are TEXT-coalescence only** (verified by
    `grep -E "^[<>] +(STRONG|EMPHASIS)"` on each diff). Safe to
    snapshot-update after pandoc-native confirms structural
    equivalence — this was the planned next step but session ended
    before completion.
  - **2 have residual structural divergences**: `emphasis_complex`
    (4 specific cases inside the fixture) and `emphasis_nested_inlines`
    (TEXT-coalescence only after the latest cascade refinement —
    re-categorize next session).

### Algorithmic divergences still needing work

All cluster on **scoped-emphasis-on-strong-matched-inner-range**:

- `**foo *bar* baz**` → Pandoc: STRONG[foo, EM[bar], baz]. IR currently:
  STRONG[...] with EM lost (between-removal eats the inner runs).
- `*foo **bar* baz**` → Pandoc: `*foo` literal + STRONG[bar* baz]. IR
  currently: all literal (cascade invalidates greedy outer emph).
- `***foo **bar** baz***` → Pandoc: EM[STRONG[foo], "bar", STRONG[baz]].
  IR currently: simpler / different nesting.
- `**foo *bar **nested** baz* qux**` → Pandoc:
  STRONG[foo, EM[bar STRONG[nested] baz], qux]. IR currently: all
  literal.

The structural fix is **scoped emphasis passes after pass-1 strong
matches**, mirroring the existing CommonMark bracket scoped pass at
`inline_ir.rs:1247-1306`. Implementation outline:

1. In `build_full_plans` under Pandoc, run a strong-only emphasis
   pass first (a new entry point that filters openers to count >= 2
   AND closers to count >= 2).
2. Collect resolved strong pairs.
3. For each pair, run `process_emphasis_in_range_filtered` over the
   inner event range with **separate state arrays** (count,
   source_start, removed) — NOT shared with the outer pass.
4. Run a final top-level emphasis pass with an exclusion bitmap on
   strong-matched inner ranges (analogous to the CommonMark bracket
   exclusion bitmap at `inline_ir.rs:1284-1289`).

### Don't redo / known traps

- **Don't try "two-pass + un-remove-between" for scoped emphasis.**
  An obvious-looking shortcut: pass 1 matches strong (closers
  count >= 2), apply between-removal as usual, then between passes
  un-remove the inner events so pass 2 can match nested emph. This
  was attempted and reverted because it **breaks the pair-crossing
  invariant** — pass 2 emph matches the un-removed inner opener
  with an opener whose strong partner is INSIDE the emph's range,
  producing crossing markers that emit invalid CST. The
  state-separation in scoped passes is the crucial bit; reusing
  the outer pass's state is what causes the crossing. See SKILL.md
  "Trap: two-pass + un-remove-between breaks pair crossing".
- **Don't widen the cascade rule to `can_open || can_close`.** Tested;
  over-invalidates intraword `_` (e.g. `_foo_bar_baz_`). The `&&`
  version is the verified-correct rule.
- **Don't tighten `pandoc_reject` to require strict count equality**
  (i.e., reject all `count_o != count_c` matches). Tested; rejects
  legitimate (1,3), (3,1), (2,3), (3,2) cases that pandoc DOES match.
  The current rule (`(1,2) || (2,1) || count_o >= 4`) is the
  verified-correct shape.
- **Don't add `can_open = false for count >= 4`.** Tested; breaks
  `**foo****bar**` which pandoc parses as STRONG[foo, bar] (the
  middle `****` splits naturally via consume rule).
- **Don't preserve a legacy-fixture output when it conflicts with
  `pandoc -f markdown -t native`.** The legacy
  `try_parse_emphasis` path is not the migration's reference. It
  approximates pandoc but has its own quirks. See SKILL.md
  "Pandoc vs legacy fixture trap".
- **Math opacity is non-negotiable for losslessness.** If you remove
  the `try_pandoc_math_opaque` call from `build_ir`, the parser
  crate test `emphasis_nested_inlines` fails with `+9 bytes` (the
  IR pairs emph across `$math$` and the dispatcher's math parse
  re-emits the math content).
- **`populate_refdef_labels` MUST be widened to Pandoc.** Without
  this, reference-link recognition under Pandoc would diverge from
  pandoc-native (which DOES require the refdef to exist for `[foo][bar]`
  to be a link).

### Suggested next sub-targets, ranked

1. **TEXT-coalescence snapshot updates** — quick win. Verify each
   diff in the 7 TEXT-coalescence-only fixtures against pandoc-native
   (structural shape unchanged), then run `cargo insta review` and
   accept. Reduces the failing-test count from 15 to ~8 with zero
   algorithm risk. Fixtures:
   - `emphasis_intraword_underscore_closer`
   - `emphasis_split_runs_pandoc`
   - `emphasis_same_delim_nested_pandoc`
   - `emphasis_skips_shortcut_reference_link`
   - `equation_attributes_disabled`
   - `reference_definition_label_with_escaped_bracket`
   - `emphasis` (just the "This is _ not emphasized _" line)

2. **Scoped emphasis passes** — the structural fix for the remaining
   ~8 algorithmic regressions. Outline above. This is the keystone
   work for finishing Phase 1; estimated 1-2 sessions because the
   algorithm refactor is non-trivial and each test case needs
   pandoc-native verification.

3. **Top-level golden_cases regressions** — `equation_attributes_disabled`
   and `emphasis_complex` (formatter side). Likely re-pass once
   parser-side TEXT-coalescence updates land; verify after #1.

4. **Phases 2-7** — gated on Phase 1 finishing clean. Don't start
   them while Phase 1 has uncommitted regressions.

### Carry-forward

(First session of this skill — nothing to carry forward yet.)

### Files in current uncommitted diff

- `crates/panache-parser/src/parser.rs` — `populate_refdef_labels`
  widened.
- `crates/panache-parser/src/parser/inlines/core.rs` —
  `parse_inline_text_recursive` / `parse_inline_text` dispatcher
  fork removed; `parse_inline_range` marked `#[allow(dead_code)]`
  for Phase 7 cleanup.
- `crates/panache-parser/src/parser/inlines/inline_ir.rs` — all
  the dialect gates, opaque-construct scan, cascade rule. Bulk of
  the diff (~488 lines added / 60 lines deleted).

### Initial migration plan (historical reference)

`/home/jola/.claude/plans/let-s-create-a-plan-noble-barto.md` — the
8-phase plan written at the start of the migration. Treat as
historical; the recap supersedes it for current state.
