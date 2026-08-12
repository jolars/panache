# Panache TODO

This document tracks implementation status for Panache's features.

## Language Server

### Code Actions

- [ ] Convert between table styles (simple, pipe, grid)
- [x] Convert between inline/reference links

### Navigation & Symbols

- [x] Find references - Find all uses of a reference link/footnote/citation
  - [x] Find references for citations - Find all `@cite` uses of a bibliography
    entry
  - [x] Find references for headings - Find all internal links to a heading
  - [x] Find references for reference links - Find all `[text][ref]` links

### Completion

- [ ] Reference link completion - Complete `[text][ref]` from defined references
- [ ] Heading link completion
- [ ] Attribute completion - Complete class names and attributes in
  `{.class #id}`
- [x] Shortcode completion - Complete Quarto shortcode names in `{{< name >}}`
- [x] Cross-reference completion - Complete `@fig-id` and `\@ref(fig-id)`
  cross-refs (also: file/shortcode path completion is implemented)

### Inlay Hints (low priority)

Personally I think inlay hints are distracting and I am not sure what we want to
support.

- [ ] Link target hints - Show link targets as inlay hints
- [ ] Reference definition hints - Show reference definitions as inlay hints
- [ ] Citation key hints - Show bibliography entries for `@cite` keys
- [ ] Footnote content hints - Show footnote content as inlay hints

### Advanced

- [x] Semantic tokens - Syntax highlighting via LSP (`semanticTokens/full`,
  additive + flavor-gated, custom legend;
  `src/lsp/handlers/semantic_tokens.rs`). Follow-ups: multi-line tokens
  (math/div bodies, per-line split); `full/delta`
  - `result_id`; `range` requests; widen the legend (emphasis/strong/links/
    headings --- only if we decide to contest the base grammar, which flips it
    to opt-in); raw-inline format tags (parser folds `{=fmt}` into a generic
    `ATTRIBUTE`, so a dedicated token kind is needed first).
- [ ] Rename
  - [x] Citations - Rename `@cite` keys and update bibliography
  - [x] Reference links - Rename `[ref]` labels and update definitions
  - [x] Headings - Rename heading text and update internal links
  - [x] Footnotes - Rename footnote labels and update definitions/links
  - [x] Files - Rename linked markdown files and update links
  - [x] Files - Rename other linked files, shortcodes, etc. Covers `embed`,
    `video`, and `placeholder` shortcode paths plus in-document frontmatter
    file paths (`bibliography`, `csl`, `css`). Deferred: raw HTML
    `src`/`href` and raw LaTeX `\input`/`\includegraphics` references;
    nested frontmatter paths such as `format.html.css`.
- [x] Configuration via LSP - `workspace/didChangeConfiguration` to reload
  config

### Spec coverage gaps

Markdown-relevant LSP methods we don't yet implement, surfaced by the 2026-06-18
spec-coverage audit (see `docs/guide/lsp.qmd` "LSP Specification Coverage").
`onTypeFormatting`, `semanticTokens`, `inlayHint`, and
`workspace/didChangeConfiguration` are tracked above and not repeated here.

- [x] Pull diagnostics - `textDocument/diagnostic` + `workspace/diagnostic` as a
  companion/alternative to the current push model (mode-switch: pull clients
  get pull only, push suppressed; cache + `workspace/diagnostic/refresh`)
  - [x] Populate `related_documents` in the document report for clients with
    `related_document_support` (the pulled document's project-graph closure
    carries its related files' cross-file diagnostics inline)
  - [x] Streaming/partial results (`DocumentDiagnosticReportPartialResult`,
    `WorkspaceDiagnosticReportPartialResult`): a `partialResultToken`
    streams the report's tail as `$/progress` chunks (response carries the
    first chunk). No token still returns the whole report
  - [ ] `workspace/diagnostic` only reports open documents + reachable project
    manifests, not every on-disk doc in the workspace (rust-analyzer pulls
    all workspace files). Decide whether closed-but-on-disk docs should
    surface.
- [x] `textDocument/documentHighlight` - highlight every occurrence of the
  reference/citation/footnote/heading under the cursor
- [ ] `textDocument/selectionRange` - structural smart-select expansion (word →
  inline → block → section)
- [x] `textDocument/linkedEditingRange` - edit a reference label and its
  definition simultaneously
- [x] `completionItem/resolve` - defer expensive completion detail (e.g.
  citation previews) until an item is focused
- [ ] `codeAction/resolve` + advertise `codeActionKinds` - compute edits lazily
  and let clients filter actions by kind
- [x] `workspace/didChangeWorkspaceFolders` - multi-root workspaces; config
  resolves per-document against the containing folder, and add/remove
  re-resolves open documents live
- [x] `workspace/configuration` - pull runtime settings from the client (after
  `initialized` and on `didChangeConfiguration`) instead of relying only on
  discovered config files and pushed settings
- [ ] `workspace/executeCommand` - server-side commands backing complex code
  actions
- [x] File operations beyond `willRenameFiles`: `didRenameFiles`,
  `didCreateFiles`, `didDeleteFiles` (hygiene-only;
  `willCreate`/`willDelete` intentionally omitted)

#### Out of scope for prose

These spec methods target compiled-language tooling and have no useful Markdown
analogue; do not re-audit them: call hierarchy, type hierarchy,
`textDocument/implementation`, `typeDefinition`, `declaration`, `inlineValue`,
`moniker`, document color, and code lens.

### Future Lint Rules

#### Syntax correctness

- [ ] Broken table structures
- [ ] Invalid citation syntax (`@citekey` malformations)
- [ ] Unclosed inline math/code spans
- [ ] Invalid shortcode syntax (Quarto-specific)

#### Style/Best practices

- [ ] Multiple top-level headings
- [ ] Empty links/images
- [ ] Unused reference definitions
- [ ] Hard-wrapped text in code blocks
- [ ] Use blanklines around horizontal rules

### Linter bugs and performance (quarto-web triage, 2026-08)

#### False positives: `undefined-anchor` on render-generated anchors

- [ ] `undefined-anchor` flags anchors that only exist after render: Quarto
  listing categories (`gallery/index.qmd` → `#articles-reports`, etc.) and
  include-partials that link to a heading in their *parent*
  (`_incremental-pause.md` → `#creating-slides`, linted standalone). Static
  analysis can't see these targets; consider a heuristic or an opt-out for
  known render-time / cross-document anchors.

### Configuration

- [ ] Severity levels (error, warning, info)
- [ ] Auto-fix capability per rule (infrastructure exists, rules need
  implementation)
- [ ] Unwrap the CLI's top-level error print. `main() -> io::Result<()>` renders
  a returned error via `Debug`, so a config (or any other) error surfaces as
  `Error: Custom { kind: InvalidData, error: ... }`. The inner message is
  now readable (`ConfigError`'s `Debug` mirrors `Display`), but the
  `Custom { kind }` wrapper is noise. Fixing it properly means handling
  errors at the \~13 `load_config_for_cli(...)?` call sites (or switching
  `main` to a custom error type with a `Display`-based `Termination`) so the
  user sees just `Error: invalid config <path>: ...`. Affects all
  `io::Error`s, not only config.

## Incremental Parsing

Multi-session effort to harden, unify, and graduate incremental reparsing to
default-on, then add token/region tiers. Reference implementations audited for
this plan: rust-analyzer (`reparsing.rs`), `../arity`
(`crates/arity-parser/src/parser/reparse.rs`), and `../fatou`
(`crates/fatou-parser/src/parser/reparse.rs`, `src/incremental.rs`) --- fatou is
the primary model and both siblings are on disk for re-reading.

**Governing invariant** (fatou "Tenet 4 strong form"): a successful incremental
reparse must yield a green tree and syntax-error vector byte-identical to a full
parse of the edited text, enforced by a `#[cfg(debug_assertions)]` oracle on
every reparse. Every guard failure bails to full parse --- never an error.

Work happens on the `feat/incremental-parsing-graduation` branch; the user files
the PR themselves. Full design detail (phase entry/exit criteria, the
salsa-unification design, flip acceptance criteria) lives in the plan document
at `~/.claude/plans/i-want-to-promote-splendid-stardust.md`.

**Handover protocol:** a fresh session reads this section, picks the first
unchecked phase, verifies its entry criteria (previous phase's boxes checked,
workspace green on the branch), and works TDD with atomic conventional commits.
On completion it checks the phase box, updates the status line below, and
records any deviation or discovered follow-up as an indented bullet under the
phase. Never leave a phase half-landed: partial work is noted in the status line
with the exact next step.

**Current status / next step:** Phases 1--6a done. There is one authoritative
tree, the reparse lives inside salsa's `parsed_document`, the window-size cutoff
keeps a losing shape from ever being slower than a full parse, and the bench
thresholds are machine-checked (`task bench:incremental-gate`). Everything so
far sits behind `experimental.incrementalParsing`, still default-*off*, so none
of it has changed behavior for anyone --- which is what makes this the natural
PR boundary, with the flip landing separately.

Next step is Phase 6b (default flip). Its entry criteria are met; what it needs
is the gate *run*, not more code: oracle-clean fuzz at 10x iterations, the suite
green with the flag forced both ways, `task bench:incremental-gate` green, and
the week of oracle-live dogfooding. Run the gate at the default iteration count
(see the phase's own note on `multi_change_large_8`).

`incremental_regressions.rs` carries no ignored *incremental* tests; the three
`#[ignore]`d tests there pin two full-parser bugs (setext-after-setext, and a
trailing-`:` line promoting a list item's lazy continuation to a definition
term), both tracked on `main` under "Parser bugs found by the incremental
fuzzer" like the five the fuzzer found earlier. Neither fix has landed on `main`
yet --- the branch is rebased onto it and they are still ignored --- so they
stay ignored until they do.

Hardening applied after the phases above, from a review of the branch:

- Line endings. Every seam test in the cascade is textual, and the blank-line
  check was a `"\n\n"` suffix test, so a CRLF document (blank line `"\r\n\r\n"`)
  was refused at the first guard and never spliced at all --- safe, and a total
  loss of the feature for anything authored on Windows. `ends_with_blank_line`
  now strips one terminator and looks for another, which is line-ending
  agnostic. The fuzz corpus grew two CRLF snippets and two CRLF insert-alphabet
  entries, so the gap is measured rather than accidental.
- Guard parity. `reparse_section_window` ran a strictly weaker guard set than
  the suffix path. Two of the missing three cannot fire while the window is
  anchored at a top-level `HEADING`, but the thematic-break/dash-rule one can,
  and "the window starts at a heading" is a property of how the window is
  *chosen* --- which Phase 8 changes. All are applied on both paths now.
- Release-build safety. Both oracles are `cfg(debug_assertions)` (the host one
  also wants `PANACHE_REPARSE_ORACLE=1`), so a release build checked nothing,
  while `parsed_document` now feeds LSP formatting, which writes the user's
  file. `splice_length_agrees` in `src/salsa.rs` checks the one part of the
  invariant that is `O(1)` --- the spliced tree spans exactly its text --- in
  every build, and *falls back* to the full parse rather than panicking.
- The reshuffled corpus found one more full-parser bug: a trailing `:`/`~` line
  promoting a preceding list item's lazy continuation into a definition term,
  where the *splice* matched pandoc and the full parse did not. Declined by
  `first_block_has_trailing_definition_marker` so the splice keeps matching the
  full parse, pinned `#[ignore]`d in `incremental_regressions.rs`, and tracked
  on `main` under "Parser bugs found by the incremental fuzzer" (the entry says
  to delete the guard with the fix). It is not CRLF-specific --- the CRLF
  inserts only reshuffled the draws onto it --- and reproduces on LF.

No test in this section may read a document from `benches/documents/`: the
corpus is gitignored, and `download.sh` does not even produce every name the
repo still references (`medium_quarto.qmd` is gone). A corpus-reading test fails
on every clean checkout, and a corpus-*skipping* one never runs in CI at all, so
reproducers are synthetic and pin their strategy instead.

- [x] Phase 1: oracle --- `pub fn fingerprint` + debug
  `assert_matches_full_parse` on every non-fallback reparse; RA-style
  `do_check` structural tests (full `{:#?}` equality, pinned strategy +
  reparse-range length); delete the dead `src/range_utils.rs` copy of
  `find_incremental_restart_offset`.
  - Oracle lives in `crates/panache-parser/src/parser/verify.rs`; the existing
    suite (parser + LSP integration) already runs clean under it, so no
    divergence surfaced from the current strategies' happy paths.

- [x] Phase 2: seeded fuzz harness
  (`crates/panache-parser/tests/incremental_fuzz.rs`) with hazard-biased
  alphabet (setext, lazy continuation, fences, `:::` divs, list markers,
  table pipes, refdefs, YAML delimiters, HTML blocks, `$$`, footnotes) +
  commented hazard snippets + `benches/documents/` corpus;
  `PANACHE_FUZZ_ITERS` scaling. The known refdef-reuse bug is expected to
  surface here; capture divergences as minimized `#[ignore]`d red tests.
  - Delivered with deviations. The harness skips (and counts) inputs where the
    *full parser* itself is lossy or panics --- with a broken oracle the splice
    cannot be judged; every skip prints its reproducer, and the minimized cases
    are pinned in `crates/panache-parser/tests/incremental_regressions.rs` and
    tracked under "Parser bugs found by the incremental fuzzer" in the Parser
    section below.
  - Several incremental divergences the harness found were fixed in-session
    instead of parked (Phase 3 work pulled forward): restart-past-edit guard,
    textual + structural seam decoupling, fence-pairing parity over the prefix
    (heuristic; precise old-tree check deferred to Phase 8), list/blockquote
    continuation guard, and a refdef-proximity guard (`edit_may_touch_refdefs`,
    textual; the precise set comparison lands with the host layer in Phase 4).
    The section-window strategy was redesigned: it parses from the window start
    to EOF (list-item buffering depends on unbounded lookahead, so a bounded
    standalone window parse is untrustworthy) and re-adopts the old suffix
    children only on structural equality, else degrades to a suffix splice.

- [x] Phase 3: refdef-set-change guard (cheap bail to full parse); error
  carrying in the incremental result + three-bucket merge (RA recipe);
  oracle/fuzz extended to error equality; un-ignore red tests; error-matrix
  tests {unchanged/fixed/introduced} x strategy.
  - The merge has **two** buckets, not RA's three, as predicted when the phase
    was scoped: both strategies parse their window to EOF and both window starts
    are `<= edit.0`, where `map_old_offset_to_new` is the identity, so the seam
    sits at the same offset in the old and new text and nothing can straddle it.
    That case is a `debug_assert!` plus a bail. The real third bucket waits for
    the bounded region tier in Phase 8; the module doc says so.
  - `parse_incremental_suffix[_with_refdefs]` gained an `old_errors` parameter
    (the shape Phase 4's `reparse` already wanted), and `DocumentState` carries
    the errors beside its tree so `did_change` can feed the prefix's share to
    the next reparse. Both retire with `DocumentState.tree` in Phase 4. The LSP
    still serves diagnostics from salsa's independent full parse, so this is
    plumbing for the oracle, not a behavior change.
  - The **document-start-only construct guard** landed as a cheap textual bail
    on the window's first line (pandoc `%` title block, MultiMarkdown title
    block, CommonMark-dialect `---`). Splitting "byte 0 of the document" from
    "blank-line separated fragment start" in `BlockContext` is the principled
    fix and belongs with Phase 8 --- every other `at_document_start` consumer is
    `||`-ed with `has_blank_before`, which the seam guard already guarantees.
  - The fuzz harness runs four option tiers (pandoc, gfm, quarto,
    multimarkdown), chosen for reach: plain `commonmark` leaves
    `yaml_metadata_block` off and cannot reach the mid-document-YAML hazard at
    all, so `gfm` carries it. Budgets **split** the old pandoc-only counts
    rather than multiplying them, so a default `cargo test` costs about what it
    did before; `PANACHE_FUZZ_ITERS` scales every tier together.
  - The tiers found four more splice bugs, all fixed with regression tests:
    definition-marker and table-caption lines reaching back across the seam, a
    retained thematic break re-read as a multiline-table rule, a refdef-guard
    slice landing inside a multi-byte token, and an `old_edit` past the old
    tree's end. The last two were rowan panics, not divergences. The harness now
    also checks that the *base* parse round-trips, not only the parse of the
    edited text.
  - Nothing to un-ignore: the earlier red tests were fixed on `main` before the
    phase started. The tiers did surface one *new* full-parser bug
    (setext-after-setext), pinned `#[ignore]`d here and tracked on `main` under
    "Parser bugs found by the incremental fuzzer", where the fix belongs; it is
    not an incremental bug.

- [x] Phase 4: salsa unification --- reparse moves into `parsed_document` with a
  side-channel reparse base (fatou model, no staged edit chain: whole-text
  `diff_edit` recovers the single combined edit); base keyed on config +
  refdef set; admission-gated by the runtime flag; delete
  `DocumentState.tree` and the edit-range coalescing helpers; new
  `tests/salsa_incremental.rs`. Staged commits S1-S4, each green.
  - The design doc's three-bucket section-window error splice was already
    obsolete: Phase 3 shipped `merge_incremental_errors` with **two** buckets
    (both strategies parse to EOF), so S2 reused it unchanged.
  - `ReparseCache` has no admitted-set beside its map: presence *is* admission,
    so the two cannot drift apart. Fatou's hot/cold eviction classes are
    unnecessary here for the same reason --- a sweep or a sibling-config parse
    never enters the cache at all, so plain LRU over admitted entries suffices.
  - The parser's textual `edit_may_touch_refdefs` guard stays even though the
    host now compares the sets exactly: it is cheap, and it is the only refdef
    protection the parser-crate entry point has (it holds no refdef history).
  - The host oracle is gated on `cfg(debug_assertions)` +
    `PANACHE_REPARSE_ORACLE=1`, not `cfg(test)` as designed --- integration
    tests link the library built *without* `cfg(test)`, so a `cfg(test)` gate
    would have excluded exactly the suites (`tests/lsp.rs`,
    `tests/salsa_incremental.rs`) it exists to cover.
  - New: `PANACHE_INCREMENTAL_PARSING=1|0` overrides the client setting for the
    whole server process. Phase 6 wanted an escape hatch anyway, and it is what
    lets the suite run green with the feature forced on *and* forced off (the
    handful of tests that assert the setting's own plumbing skip under it).
  - Phase 7's `parser/incremental.rs` extraction was pulled forward into S4 as
    `parser/reparse.rs`, since S4 was already moving the surrounding code. The
    retired `parse_incremental_suffix*` fallback policy now lives with the
    callers that want it: `crates/panache-parser/tests/common/mod.rs` for the
    suites, a `#[cfg(test)]` shim in `reparse.rs` for the in-crate tests, and
    `try_reparse` in `benches/lsp_incremental.rs`.

- [x] Phase 5: benchmark repair --- fix `benches/lsp_incremental.rs`
  multi-change path (currently degenerates to full reparse), add
  fallback-rate + bail-cost accounting, commit results table in module doc.
  - The multi-change path is fixed by *mirroring* `parsed_document` rather than
    approximating it: whole-text `diff_edit` per notification, a base chained
    from step to step, and the host's refdef-set comparison ahead of the
    parser's textual guard. A case is now a *stream* of notifications, which is
    what makes a fallback rate mean anything --- the old per-case rate was 0 or
    1 by construction.
  - Three cases were measuring nothing. `synthetic_document` emitted adjacent
    lines, so every "paragraph" was one giant paragraph with no blank line for
    the seam guard to decouple at, and every synthetic edit got a window
    starting at byte 0; it now separates paragraphs and emits a `##` heading
    every ten, so the section-window strategy has something to find.
    `pandoc_manual_single_edit` edited line 200, which is ``[`setspace`]: ...``,
    and rewrote the *label* --- it is kept, renamed to
    `pandoc_manual_refdef_label_edit`, as the host-side-decline case, and a
    genuine early-prose edit was added beside it. `fallback_invalid_range` was
    dropped: the server validates client ranges before touching its buffer, so
    it modelled nothing, and with `diff_edit` it merely duplicated
    `full_replace`.
  - The accounting distinguishes **window** bytes (window start to EOF, what
    both strategies actually re-parse) from **spliced** bytes (green children
    replaced). Only the former predicts the speedup; printing the latter is what
    made `tables_single_edit` look like "7% reparsed, 0.98x". The bench also
    reports a per-strategy histogram and a fallback-reason histogram, and
    verifies the governing invariant at every step of every case before timing
    anything.
  - Headline: speedup is a function of window share and nothing else --- 5.6x on
    a late edit or a typing stream in the pandoc manual (7% window), 1.0x where
    the window is \~98%. A *successful* wide reparse is 5-10% slower than a full
    parse (`pandoc_manual_early_edit` at 0.9x, `full_replace` at 0.2x).
    Guard-cascade bail cost is 15.7% of a full parse; a host-side decline costs
    one refdef scan.
  - Three consequences for the phases below, folded into them: the window-size
    cutoff is promoted out of Phase 8 into its own phase *ahead of the flip*
    (5b), Phase 6's gate grows a regression ceiling and names its cases, and
    Phase 7's exit criterion is restated in the harness's terms.

- [x] Phase 5b: window-size cutoff --- decline in `reparse_ranges` when the
  window start leaves more than a threshold share of the document
  downstream, before the guard cascade and the window parse run. Threshold
  picked from the bench (the crossover is around 85-90% window, where the
  5-10% splice surcharge stops being repaid); a bench case per side of it.
  - Promoted out of Phase 8 and ahead of Phase 6 because the flip is what makes
    the losing shapes the *default*. Today a whole-document replace measures
    0.2x and an early edit in the pandoc manual 0.9x; the cutoff turns both into
    a clean \~1.0x fallback, so the flip ships something that is never worse
    than the status quo. `full_replace` is not an exotic shape: a client that
    answers format-on-save with a whole-document replace takes that path on
    every save.
  - Independent of the region tier --- it compares `reparse_range.0` against a
    fraction of the document length and returns `None`, which is the existing
    refusal-first contract. Phase 8 keeps the cutoff and re-tunes the threshold
    once regions change what a window costs.
  - Landed as `MAX_WINDOW_SHARE_PERCENT = 85` and **two** checks, not one. The
    cheap one runs before the old tree is touched at all: every window this
    entry point can choose starts at or before the edit, so the edit offset is a
    sound upper bound on the window start, and a whole-document replace declines
    there for a tenth of a microsecond (0.2x -> 0.9x). The precise one runs
    after the restart is known, because a single top-level block spanning most
    of the document puts the restart arbitrarily far ahead of the edit.
  - A too-wide *section* window declines the strategy, not the reparse: the
    section anchor is the previous top-level heading, which can sit far earlier
    than the edited block, so the suffix window below it is often narrower and
    still admissible.
  - Bench, before -> after: `full_replace` 0.2x -> 0.9x,
    `pandoc_manual_early_edit` 0.9x -> 1.0x, `multi_change_large_8` 0.9x -> 0.9x
    (see below), `tables_single_edit` / `math_single_edit` /
    `large_authoring_single_edit` 1.0-1.1x -> 1.0x. Nothing that won lost: the
    typing streams and `pandoc_manual_late_edit` are unchanged at 2.8x and 5.4x.
    New cases `window_cutoff_accepted` (79.9% window) and
    `window_cutoff_declined` (87.8%) bracket the threshold on one document.
  - **Phase 6's 0.95x ceiling is not met by three cases, and none of them is a
    wide-window splice.** `bail_refdef_edit` (0.9x) exists to price a decline,
    which is definitionally slower than the full parse it wraps --- the ceiling
    needs the same explicit exemption the fallback-rate threshold already gives
    it. `multi_change_utf16_4` (0.7x) is a 74-byte document against a 1.8 us
    fixed attempt cost; it lost at 0.8x *while splicing successfully* before
    this phase, so it is Phase 7's fixed-overhead problem, not a threshold
    problem. `multi_change_large_8` (0.9x) declines in under a microsecond and
    still costs \~100 us more than a full parse of its 76 KB: that residual is
    host-side per-step work (whole-text `diff_edit` plus the 67 KB `insert` it
    allocates, the refdef-set clone, the base text copy), it is unattributed
    between those, and it wants a profile in Phase 6 rather than a threshold.
  - A document-size floor was tried for the 74-byte case and reverted: it would
    also refuse small documents with *narrow* windows, which do win
    (`single_change_small`, 1.6 KB, 1.3x), and it made every reuse assertion in
    the suite untestable through the production entry point.
  - The cutoff cost the fuzz harness two thirds of its coverage --- its hazard
    snippets are tens of bytes, so nearly every window is a wide one, and the
    share of edits reaching a splice fell from 78% to 23% while every assertion
    still passed. `CostGuards::{Enforced,Ignored}` on a new
    `reparse_with_cost_guards` is the opt-out: the snippets fuzz with the cost
    guard off (the seams they encode occur mid-document in real files, where the
    cutoff admits them), the real-document corpus keeps the production setting,
    and each driver now asserts a floor on its splice rate so a future guard
    cannot silently empty the harness again.

- [x] Phase 6a: mechanize the gate --- the thresholds Phase 6b is gated on were
  printed but never checked, so "the gate passed" was an eyeball judgement.
  `PANACHE_LSP_BENCH_ASSERT=1` now checks every case and exits non-zero on a
  violation; `task bench:incremental-gate` fetches the corpus and runs it.
  - **The fallback-rate criterion was stale and had to be replaced, not
    implemented.** It read "< 20% on every case except the two that price a
    decline", which was written before Phase 5b: the window-size cutoff makes a
    decline the *correct* outcome for a wide-window edit, and ten of the
    eighteen cases now fall back on every step by design. A global rate rule
    cannot express that. Each case instead declares an `Expect` (`Reuse::Always`
    or `Reuse::Never`, plus an optional speedup floor), so the old exemption
    list is gone: the exempted cases are simply the ones that declare `Never`,
    and a new case cannot be added without saying what it is for.
  - Every ratio rule carries an absolute-microsecond escape
    (`MAX_ABSOLUTE_OVERHEAD_US = 20`), because a ratio on a 2 us baseline is
    noise. That retires the by-name exemptions Phase 5b recorded for
    `full_replace` (+0.3 us) and `multi_change_utf16_4` (+1.7 us, 44% bail on a
    3.7 us parse) and puts them on a stated principle, and it lets
    `bail_refdef_edit` (+15.8 us) pass the ceiling without the carve-out the
    roadmap reserved for it. Presence is checked too: the real-document corpus
    is gitignored and `load_document` skips silently, so without it a gate run
    on a fresh checkout passes by not measuring exactly the strictest cases.
  - **`multi_change_large_8`'s \~95 us is profiled, and Phase 5b's guess at it
    was wrong.** It is not host-side per-step work: measured directly on the
    case, `diff_edit` is 7.1 us, the config clone 0.1 us, and the declined
    attempt 0.2 us --- under 8 us of the \~95, and the base text copy the guess
    also named was never inside the timed region at all. The rest is the
    *fallback full parse itself* running \~5% slower on the incremental path
    (1861 us vs 1963 us for the same call on the same text), with the previous
    green tree and the 64 KB edit buffer resident across it. That residual sits
    inside the run-to-run spread of the same parse, which is why the case
    straddles 0.95x rather than failing outright; it carries a documented 0.90
    ceiling naming the profile, and the printed reason keeps the exemption
    visible on every run.
    - A real mis-attribution *was* found and fixed on the way, but it is not
      this one. The bench modelled `refdef_set` as a bare scan and then compared
      whole `RefdefMap`s on the incremental path only, while in production the
      comparison happens inside the query, is charged to both paths, and hands
      back the same `Arc` when the set is unchanged --- which is what makes
      `parsed_document`'s check a pointer compare. `refdef_query` now models the
      backdating. It moves this case by nothing measurable (its synthetic
      document has no reference definitions, so the sets are empty and the
      comparison was free); it matters for refdef-carrying documents, and for
      the harness continuing to mirror the query it claims to mirror.
  - Deferred deliberately: no CI workflow yet. The gate needs
    `benches/documents/download.sh` and a release build, and a timing-assert job
    on shared runners would land flaky next to the flip. The mode and the task
    target are what make wiring it a later one-liner.

- [ ] Phase 6b: default flip --- incremental parsing is **always on**, with no
  new setting. `panache.experimental.incrementalParsing` stays exactly where
  it is and keeps working, but inverts its meaning: absent means on, and the
  only reason to write it is `false`, which turns the side channel off for
  debugging. No `panache.incrementalParsing` key, no alias, no
  `deprecationMessage`, no setting migration --- a second key buys nothing
  when the only value anyone would set is the one the existing key already
  accepts.
  - Work: default the setting to on where it is read
    (`experimental_incremental_parsing_from_initialize` in
    `src/lsp/dispatch.rs`, and the `workspace/configuration` pull and
    `didChangeConfiguration` paths in `src/lsp/handlers/configuration.rs`, which
    share `runtime_incremental_parsing_from_value`); flip the VS Code
    `package.json` default to `true` and reword its description, which currently
    sells it as an unstable experiment rather than a debug switch; update
    `docs/guide/lsp.qmd` (opt-in -> opt-out), `docs/development/lsp.qmd`, and
    the AGENTS.md admission sentence; flip the LSP tests that assert the default
    is off (`tests/lsp/test_incremental_edits.rs`,
    `tests/lsp/test_config_pull.rs`, `tests/lsp/test_config_reload.rs`).
    `PANACHE_INCREMENTAL_PARSING=1|0` keeps overriding the setting in both
    directions and needs no change.
  - The server-side default is only *one* of three:
    `editors/code/src/extension.ts` passes its own hard-coded `false` fallback
    into `initializationOptions`, and `editors/code/README.md` documents
    `default: false`. Both need flipping with `package.json`, or VS Code keeps
    sending an explicit `false` and the server default never governs that client
    at all.
  - `apply_runtime_settings` in `src/lsp/handlers/configuration.rs` applies no
    default: an absent key keeps the current value rather than resetting. That
    is correct and stays, but it means the flip cannot be made there --- only
    the initialize path decides the default.
  - The `experimental.` prefix becomes a misnomer the day this lands. Renaming
    it is deliberately declined: a rename needs precisely the alias and
    migration this phase drops, and the cost of a stale prefix on a debug-only
    switch is lower than the cost of carrying two keys forever. The *internal*
    name (`runtime_settings.experimental_incremental_parsing`) has no wire
    impact and can be renamed freely --- Phase 9 material.
  - Gate: oracle-clean fuzz at 10x iterations; workspace + LSP suite green with
    the flag forced on and off; 1 week oracle-live dogfooding with zero panics;
    and `task bench:incremental-gate` green.
  - **The bench thresholds are no longer restated here.** Phase 6a moved them
    into the cases themselves (`Expect` in `benches/lsp_incremental.rs`), which
    is the only copy: a number in this file could not be checked and drifted
    from the harness within one phase. The floors the roadmap named survive
    verbatim as declarations --- `typing_stream_medium` >= 2x,
    `pandoc_manual_late_edit` and `pandoc_manual_typing_stream` >= 5x --- as
    does the 0.95x regression ceiling and the 20%-of-a-full-parse bail budget.
    Read the gate's output, not this bullet.
    - Both 5x floors measure 5.4-5.7x, run to run. That is a thin margin by
      design: the floors are the roadmap's, and the margin printed on every run
      is what makes drift visible before it fails.
    - Run the gate at the default iteration count. `multi_change_large_8` fails
      at 4 iterations and passes at 80 --- its margin is a few percent on a 1.9
      ms parse, so a shortened run measures the sampling noise, not the feature.

- [ ] Phase 7: token tier --- edit inside plain `TEXT`; newline ban,
  construct-character ban list kept honest by a grammar-grepping test, relex
  kind stability, join probes, error non-touch; char-by-char typing test.
  (The module extraction this phase also carried landed early, in Phase 4's
  S4: `crates/panache-parser/src/parser/reparse.rs`.)
  - Phase 5's typing-stream numbers *raise* this phase's value rather than
    lowering it. The section window already gets streams to 2.7x/5.6x, but
    `pandoc_manual_typing_stream` still costs 1.9 ms per keystroke, because a 7%
    window on a 300 KB document is still 21 KB re-parsed for a one-character
    insert. This tier is O(token) instead of O(document tail) and is the only
    thing that removes that.
  - Exit criterion in the harness's terms: the typing-stream cases must show a
    step change, not an improvement. And the tier has to skip the *fixed*
    overhead too, not only the block parse --- `full_replace` puts a \~10 us
    floor on a 1.6 KB document for machinery that ultimately parsed 27 bytes
    (materializing the root from green, walking the cascade).

- [ ] Phase 8: region tier over top-level `DOCUMENT` children replacing
  section/suffix windows --- symmetric newline-decoupling scans in old and
  new text, `no_straddle` seam primitive, fence/div balance,
  setext/lazy-continuation/list-tightness/HTML-block coupling guards. Fixes
  the suffix-window reparse-to-EOF gap. (The too-wide bail this phase used
  to carry is Phase 5b; re-tune its threshold here.)
  - Phase 5 confirms the premise: window share is the only lever on speedup, and
    the current tier gets 92-98% windows on all three real documents, so it wins
    nothing on early or mid-document edits in real files.
  - The payoff depends on top-level *child* granularity, not on headings.
    `tables_single_edit` edits line 40, the section window fires, and the window
    still starts \~450 bytes in, because that is where the nearest top-level
    heading sits. More headings would not help; regions over `DOCUMENT` children
    are what does.

- [ ] Phase 9: closeout --- architecture docs, dead-path pruning, record
  deferrals.

**Deferred (explicit non-goals):** nested-container regions (inside list items,
blockquotes, divs) are unsound without a context-parameterized fragment-parser
entry point carrying container stack, open fences, and refdef scope --- fatou's
recorded lesson (fatou `TODO.md`, "Incremental" section). Regions stay
restricted to top-level `DOCUMENT` children until that exists. NodePtr
re-anchoring across edits (arity's `map_range_through_edits`) is only needed if
panache starts caching NodePtrs across edits.

## Parser

### Parser bugs found by the incremental fuzzer (2026-08)

The incremental reparse fuzz harness
(`crates/panache-parser/tests/incremental_fuzz.rs`, on
`feat/incremental-parsing-graduation`) checks that a full parse round-trips its
bytes before it judges a splice --- a broken oracle cannot judge anything. That
precondition check found the following; none of them are incremental-parser
bugs. Minimized reproducers live in
`crates/panache-parser/tests/incremental_regressions.rs` on that branch.

Round-trip failures (`input` -> lossy output):

- [x] Display-math closing fence *drops* bytes (content loss, not a reorder):
  `$$\nx^2 + y\n$$$\n` -> `$$\nx^2 + y\n$$\n`. Also `$$\nx^2 + y\n$$$ra\n`
  -> `$$\nx^2 + y\n$$ra\n` and `$$\nx^2 +$$$$\n\npara\n` ->
  `$$\nx^2 +$$\n\npara\n`. `try_parse_display_math` consumed a whole closing
  dollar run while `emit_display_math` only wrote back an opener-length
  marker. Both delimiters are now exactly `$$` (pandoc's
  `mathDisplayWith "$$" "$$"`), so surplus dollars stay content or text.

- [x] Fenced-div opener mangles the whitespace run before its label, and
  duplicates part of the label: `:::  te\nbody\n:::\n\npara\n` ->
  `::: tee\nbody\n:::\n\npara\n`; `:::: \ty\n:::\n::::\n\npara\n` ->
  `:::: yy\n:::\n::::\n\npara\n`;
  `::::     outer\n::: inner\nbody\n:::\n::::\n` ->
  `:::: outeruter\n::: inner\nbody\n:::\n::::\n`. Detection trims the whole
  whitespace run after the colons, but `emit` consumed a single space before
  slicing the label off the rest, so the leftover run shifted the slice and
  the tail was re-emitted as a label suffix. `emit` now consumes the same
  run.

- [x] A setext underline directly after a setext-*shaped* pair of lines broke
  the block dispatcher's open-paragraph contract. Both `a\nb\n---\nc\n---\n`
  and ```` ```\nx\n---\ny\n---\n ```` tripped the `debug_assert!` in
  `parser/core.rs` ("block parser `setext_heading` returned `Yes` while a
  paragraph or buffered list-item content is open"); so did the blockquote
  and list-item forms. `SetextHeadingParser::detect_prepared` computed its
  `follows_setext_heading` escape by re-lexing two *raw source lines*, so it
  could not tell "the parser emitted a HEADING there" from "the parser
  absorbed those bytes as text" --- it fired while the paragraph was still
  buffered, and the heading was emitted before those bytes. Debug builds
  panicked; release builds reordered silently, making this a losslessness
  bug rather than only an assertion failure. The escape is now gated on
  `!ctx.paragraph_open && !ctx.list_item_content_open` (the latter is a new
  `BlockContext` field), which leaves its one live case --- consecutive
  setext headings inside a list item, where the item's buffer is already
  flushed --- intact. All five shapes now match
  `pandoc -f markdown -t native`.
  - Surfaced on `feat/incremental-parsing-graduation` by the flavor-tiered fuzz
    harness, once `---\nk: v\n---\n` entered its insert alphabet. Both shapes
    are pinned `#[ignore]`d there in `incremental_regressions.rs` as
    `full_parse_lossless_setext_underline_after_setext_heading` and
    `full_parse_lossless_setext_pair_after_unterminated_fence`; un-ignore them
    when that branch rebases onto this fix.

Pandoc divergences. The parse round-trips its bytes, so the harness's
lossy-or-panic precondition check does not catch these --- they surface the
other way round, as an incremental splice that is *right* diverging from a full
parse that is wrong:

- [x] A line ending in ` :` or ` ~` promotes a preceding list item's
  continuation line into a definition-list `TERM`, swallowing the blank line
  between them. `- a\nb\n\nc :\n` parsed as a `LIST_ITEM` containing a
  `DEFINITION_LIST`, where pandoc has
  `BulletList [[Plain [Str "a", SoftBreak, Str "b"]]]` followed by
  `Para [Str "c", Space, Str ":"]`.
  - Root cause: `ContainerPrefix::strip` walks a list item's content column with
    `advance_columns`, which counts *any* character as a column, so on a line
    short of that column it slices content off as if it were indent — inside a
    two-column item `"c :"` became `":"`. Fixed by
    `ContainerPrefix::line_carries_list_indent` plus a
    `LineView::carries_container_prefix` gate in
    `next_line_is_definition_marker`. That root cause also explains the original
    qualifiers: an ordered marker gives `content_col` 3, so the faked slice is
    just the newline, and a tab overshoots column 2 and survives the slice. The
    lazy continuation was *not* required — an indented continuation
    (`- a\n  b\n\nc :\n`) fired too.
  - `feat/incremental-parsing-graduation` still owns un-ignoring
    `full_parse_definition_list_from_trailing_colon_after_lazy_list_item` in
    `incremental_regressions.rs` and deleting the incremental-side workaround
    `first_block_has_trailing_definition_marker` in `parser/reparse.rs` with its
    two call sites, when that branch rebases onto this. Neither file exists on
    `main`.
  - Conformance did not move: commonmark 652/652 and pandoc 524/524 both held
    across the whole series (the earlier note expecting a delta was wrong — both
    suites were already at 100%).

Further definition-list divergences fixed alongside it:

- [x] A term had to be a **one-line block**, as pandoc requires: `a\nb\n\n: def`
  is two `Para`s, not a definition list on `b`. Panache promoted the last
  line of a multi-line paragraph. The orphan guard lost its `!ctx.in_list`
  carve-out at the same time, since a refused term otherwise reached the
  definition arm and emitted `DefinitionList [([], ...)]` outside the item.
  Needed a formatter guard (`guard_definition_marker_start`) because reflow
  can manufacture the one-line block above a `: def` paragraph.

- [x] A term on the **list-marker line** now nests: `- Term\n  : def` is
  `BulletList [[DefinitionList [(Term, ...)]]]`. Detected in the list-item
  content path (`maybe_open_definition_term_in_new_list_item`), mirroring
  the footnote branch, since `ListParser` outranks `DefinitionListParser`
  and the dispatcher never sees the marker line. Required reading markers at
  the item's content column (`definition_marker_in_list_frame`) so the
  four-space rule and nested items keep working.

- [x] A **dedented** marker after a list item folded into the item as a lazy
  continuation: `- Term\n: def\n` and `- Term\n : def\n` gave
  `BulletList [[Plain [Term, SoftBreak, ":", Space, "def"]]]` where pandoc
  closes the list and emits `Para [Str ":", Space, Str "def"]`. Same for
  `- a\n  b\n  : def\n`, where pandoc splits the item into two `Plain`s and
  panache kept one. A definition marker is a *block start*, and pandoc's
  `endline` refuses to cross one while it is reading a list item, so it can
  be neither a soft nor a lazy continuation there
  (`definition_marker_breaks_open_list_item_block`); the marker's own indent
  then decides whether it stays in the item or drops out of the list. Needed
  two formatter guards, since neither shape survived a round-trip otherwise:
  a blank line between a `LIST` and a `PARAGRAPH` pressed against it, and
  preserved line breaks for an item whose reflow would collapse the block
  above a marker into a one-line term
  (`reflow_would_promote_a_definition_term`).

- [x] A marker **inside a definition body** opened a second `Definition` instead
  of a second block in the same one: `T\n:   a\n    b\n    : def\n` is
  `DefinitionList [(T, [[Plain "a b", Plain ": def"]])]` for pandoc, which
  reads the body in the same frame as a list item's, so the marker ends the
  block above without starting a definition
  (`definition_marker_breaks_open_definition_block`). It runs in the
  definition-continuation branch of `parse_inner_content`, which flushes the
  buffered `PLAIN` before the dispatcher gets a look — the `Definition` arm
  of `DefinitionListParser::detect_prepared` would otherwise claim the line.
  A marker *dedented* below the content column still opens a sibling
  definition. The formatter takes the same preserved-line-breaks guard a
  tight list item does (`reflow_would_promote_a_definition_term`), since
  collapsing the block above the marker to one line would make it a term.

- [x] A marker under a **single buffered line** of a definition body now makes
  that line a term of a *nested* definition list: `T\n:   a\n    : def\n` is
  `DefinitionList [(T, [[DefinitionList [(a, [[Plain "def"]])]]])]`, and so
  is a marker following one that already broke the block
  (`T\n:   a\n    b\n    : def\n    : def2\n`, where `: def` is the term).
  Both promote a term out of already-buffered body content, so the old
  break-or-nothing predicate became
  `definition_marker_over_open_body_block`, returning which of the two the
  buffered block's shape calls for; the buffer has not reached the green
  builder yet, so `promote_buffered_definition_term` just opens the nested
  `DEFINITION_LIST` around it and leaves the marker to the dispatcher.
  Nesting recurses (`T\n:   a\n    : b\n        : c`) and the blank-line
  variant (`T\n:   a\n\n    b\n    : def`) came with it. Two
  container-unwinding rules had to follow, since a `DEFINITION_LIST` inside
  a `DEFINITION` was previously unreachable: a term across a blank line only
  keeps a nested list open while it reaches the body holding it
  (`next_is_definition_term_below` now stacks the content-container frame on
  the list-item column, or `T2` became a term of the nested list), and a
  *dedented* marker unwinds to the level whose frame it lands in
  (`close_dedented_definition_lists`, so `T\n:   a\n    : def\n  : sibling`
  puts the sibling back on `T`). Plain text is not a block start and stays a
  lazy continuation of the innermost body. Two formatter guards keep the
  round-trip stable: `reflow_would_promote_a_definition_term` now also fires
  on a `PLAIN` followed by a nested `DEFINITION_LIST` (collapsing the block
  above hands the nested marker *that* line), and a body that is only a
  definition list is `is_compact`, since a blank line between the term and
  the nested marker detaches them on reparse. Looseness is unobservable
  there — such a body holds no paragraph to render either way.

- [x] A one-line body block separated from the marker by a **blank line** is
  promoted too: `T\n\n:   a\n\n    :   b\n` and
  `T\n:   a\n\n    b\n\n    : c\n` now nest, matching pandoc. A blank line
  does not detach a term from its definition, so the question the marker
  line asks in `definition_marker_over_open_body_block` is asked one line
  early, by lookahead, in the blank-line branch of `parse_inner_content`
  (`blank_line_promotes_buffered_definition_term`) — before the flush, while
  the block is still a buffer that `promote_buffered_definition_term` can
  reopen as a `TERM`. Two frame details the shared
  `next_line_is_definition_marker` cannot settle on its own: it must report
  *zero* further blanks, since the lookahead already stands on one and a
  term keeps at most one; and the dedent test is re-measured here, because
  `line_carries_list_indent` vets only the list component of the strip while
  `strip_content_indent` degrades gracefully instead of reporting a short
  line. Both frames are absolute, so the promotion also fires inside a list
  item and a blockquote.

- [x] **Two** blank lines detach a term from its first definition marker, at
  every level. Pandoc's `definitionListItem` reads the term and then
  `option False $ blankline` — a *single* optional blank — so `T\n\n\n: b`
  is two paragraphs, and the same rule recursing through a body's reparse
  makes `T\n:   a\n\n\n    :   b\n` one definition holding
  `Plain "a", Plain ": b"`. Three lookaheads had to learn the limit
  (`next_line_is_definition_marker`, `first_content_line_term_lookahead`,
  and — already correct — the blank-line promotion), and
  `definition_marker_over_open_body_block` had to stop requiring an *open*
  block: reaching the body's content column is the whole test, so a marker
  over a closed or not-yet-started body is body text rather than a sibling
  definition (`T\n:\n    : b`, `T\n:   a\n    b\n\n    : c`). Only a
  *dedented* marker is a second definition, however many blanks precede it.
  The formatter needed the escape guard on the paths that emit paragraph
  lines themselves and so never ran it — list continuations, footnote and
  admonition bodies — plus two fixes to the guard's own frame: a
  content-container indent is prefix, not the marker's 0-3 space allowance,
  and a fenced-div opener ends the block above it.

- [x] The same reflow-promotes-a-term idempotency failure across a **blank
  line** (`- x\n\n  a\n  b\n\n  : def\n`, which reflowed to a term on `a b`)
  came with the guard wiring above: `format_list_continuation_paragraph` now
  runs `guard_definition_marker_start`, which escapes the marker instead.

- [x] A definition list followed by a **thematic break** dropped the nested
  marker's padding (`- T\n\n  :   a\n\n      :   def\n\n---\n` reformatting
  to `: def`). Filed as a formatter marker-width heuristic; it was not one.
  The formatter emits `:   ` unconditionally, so the `: def` was literal
  paragraph text: the *parser* had demoted the nested marker, and
  `next_line_is_definition_marker`'s table-caption escape
  (`is_caption_followed_by_table`) was what demoted it. That probe scanned
  forward out of the container it was called in, so a dash run at column 0 —
  the thematic break *closing* the definition body — read as the multiline
  table a `:` line inside the body could caption. Pandoc nests a definition
  list on `a` in both shapes; the trailing break is a `HorizontalRule`
  either way. The bound is `ContainerPrefix::line_reaches_content_column`,
  the stricter twin of `line_carries_list_indent`: the latter vets only the
  `ListAdvance` ops, because `strip_content_indent` degrades gracefully
  rather than reporting a short line, so a definition or footnote body (a
  `ContentIndent` op with no list advance) had no boundary at all — which is
  why the unnested shape broke on a bare `---` while the list-nested one
  needed four dashes. Exposed on `LineView` as `stays_inside_container`,
  defaulting to `true` so raw line slices and `UniformStripView` are
  unaffected. Covered by
  `definition_marker_caption_probe_container_boundary` in both golden
  suites; the suppression side (a dash run that really is at the body's
  content column) is pinned by
  `caption_probe_still_fires_inside_the_container`.

Still open in the same area, found while fixing the above and **pre-existing**
(both reproduce at `4fedf093`, unchanged by the caption-probe bound). These are
losslessness failures, so they outrank the cosmetic issues above:

- [ ] A table **caption inside a container** loses bytes on the round trip.
  `T\n\n:   a\n\n    :   def\n\n    ----\n    x\n    ----\n` — a caption
  plus simple table at a definition body's content column — reparses with
  the caption line duplicated and the table's indent dropped
  (`panache debug format --checks losslessness` shows `-    ----` /
  `+    :   def\n+\n+----`). The parse matches pandoc structurally apart
  from a spurious extra `Para [Str ":", Space, Str "def"]` alongside the
  `Table`, so the caption line is being consumed twice: once by the caption
  and once as body text.

- [ ] The same shape in a **list item** parses to pandoc's AST exactly, but the
  *formatter* mangles the round trip: it rewrites the `----` rules to `--`
  and re-emits the caption below the table. Parser-side is clean here, so
  this one is genuinely a formatter bug, unlike the item above it. Repro:
  `- item\n\n  : cap\n\n  ----\n  x\n  ----\n`

Also still open, but **do not fix these individually** — they are input to
"Consolidate container-frame resolution behind a single typed verdict" under
`Parser > Architecture`, not competition for it. Each is one of the shapes where
the eight overlapping strip helpers disagree today, so patching them separately
means making the same judgment three times in three different helpers, which is
the pathology performed once more. Feed them to that item's step 1 (the
behavior-pinning table test) and let the verdict close them:

- [ ] `next_line_is_definition_marker` still cannot see a marker whose indent is
  a **tab straddling** the item's content column: `- a\n\n  b\n\n\t: def\n`
  yields three paragraphs where pandoc yields a definition list. The strip,
  not the detection frame, is what fails — `advance_columns`
  (`container_prefix.rs:1050`) hits `next > target` on the tab and returns
  the slice *starting at* it rather than reporting the straddle.
  `emit_definition_marker` already emits literal indent bytes, so the
  emission side is ready for it. Design input for the verdict enum: a
  straddling tab is its own case, distinct from a genuinely short line,
  since the line *does* reach the content column — there is just no byte
  boundary to split on.

- [ ] `is_caption_followed_by_table` runs on the same over-stripping view as the
  term lookahead and has the same latent asymmetry. It is a *suppression*
  gate, so an over-strip can only fail to open a definition list, and it is
  shared with the table parsers — no reproducer yet.

- [ ] `StrippedLines::peek_prefix_at` is not equivalent to
  `ContainerPrefix::strip` for the
  `[ContentIndent, ListAdvance, BlockQuoteMarker]` shape (the `issue_209`
  fixture). Its doc claims the divergence is dormant while
  `content_indent == 0`, which does not hold for definition-list lookaheads,
  where the definition container contributes the content indent.

Adjacent, found while fixing the losslessness bugs the same harness turned up:

- [x] `parse_line` honours the block registry only for `OpenBlockQuote` effects,
  throwing away every other prepared match and re-deriving container
  structure from a raw marker count. The setext-over-a-blockquote fix
  special-cases one parser name; the general cleanup is to let the
  registry's verdict stand. Any non-blockquote verdict on the raw line now
  wins over the marker count (registry order mirrors pandoc's reader order),
  and the CommonMark same-container rule moved into
  `SetextHeadingParser::detect_prepared`, where it belongs. Still excluded:
  shifted blockquotes inside list items, where `dispatcher_ctx` carries no
  `list_indent_info` and the verdict is therefore untrustworthy.
- [x] Under CommonMark, `> a\n> ---\n` should be `BlockQuote [Header 2 "a"]`
  (pandoc's commonmark reader agrees); panache gave `BlockQuote [Para, HR]`.
  The same-container rule in `SetextHeadingParser::detect_prepared` counted
  `>` markers on `ctx.next_line`, which the blockquote-carrying dispatch
  path in `parse_line` hands over already stripped of every marker --- so
  the count was always 0. It now reads the depth off the raw lookahead line.
  The same root cause made `a\n> ---\n` a top-level `Header 2` instead of
  `Para, BlockQuote [HR]`.
- [x] Under Pandoc, `a\n> ---\n` should be one `Para` (pandoc reads it as
  `[Str "a", SoftBreak, Str ">", Space, Str "\8212"]`); panache gave
  `Header 2 "a"`. Same shape as the CommonMark bug above, but not the same
  rule: Pandoc treats a leading marker run on the *text* line as literal
  text (`> foo\n---\n` is a top-level H2 with the marker included), so the
  underline's raw depth is now compared against `ctx.blockquote_depth`
  alone. Also fixes `> a\n> > ---\n` and the indented `a\n   > ---\n`.
- [x] Under Pandoc, `> > a\n> ---\n` should be
  `BlockQuote [Header 2 [">", Space, "a"]]`; panache gave
  `BlockQuote [BlockQuote [Para]]`. Pandoc's `blockQuote` strips exactly
  *one* `>` per line and recursively re-parses the rest, so every parser
  ahead of it in the reader order --- `setextHeader` among them --- gets a
  shot at content that still begins with `>`. `Parser::blockquote_depth_cap`
  now probes the registry at each depth the line would pass through and
  stops at the first winner that outranks `BlockQuoteParser`; that depth
  caps the open and the surplus markers stay in the content as literal text.
  Rank, not effect, is the test: `BlockQuoteParser` declines outright at a
  non-zero `ctx.blockquote_depth`, so lower-ranked parsers would otherwise
  win by default (`> > a\n: b\n` keeps both quotes in pandoc).
  Pandoc-dialect only --- CommonMark reads the surplus `>` as a real
  container, not as text.
- [x] Under Pandoc, `> > a | b\n> ---|---\n` should be `BlockQuote [Table …]`
  with `> a` as the first cell (`table` outranks `blockQuote`, so the depth
  cap above applies); panache gave `BlockQuote [BlockQuote [Para]]`. Not the
  depth cap after all: `try_parse_pipe_table` required at least one row
  after the delimiter, so it declined at every depth. Pandoc reads those
  rows with `many`, so a header plus a delimiter row is already a whole
  table --- the requirement is dropped, which also makes top-level
  `a | b\n---|---\n` and `| a\n|---\n` (previously a `LineBlock`) tables, as
  pandoc has them.
- [x] Under Pandoc, `> > a\n: b\n` should be
  `BlockQuote [BlockQuote [DefinitionList [(a, [[Plain b]])]]]`; panache
  dropped the definition body and let `: b` escape the quotes entirely as a
  top-level `Para`. Two independent causes, both fixed. (1) The term
  look-ahead ran on raw lines, so a `> : b` under a quote never matched the
  0-3-space marker test and the term above it lost its body --- it now runs
  on the container-stripped window, which also retires the
  `blockquote_depth == 0` carve-out in the orphan guard. (2) Pandoc's
  blockquote reader folds lazy lines into the quote's raw content before
  parsing blocks, so an unquoted `: b` still belongs to the open definition
  list; the lazy gate in `core.rs` only recognized an open `Paragraph` or
  `ListItem`, and now recognizes an open `DEFINITION_LIST` too.
- [x] A line block inside a list item loses its indent through the formatter
  (`- x\n\n  | a\n b\n` re-formats to a line block at column 0), which then
  absorbs the following line on reparse and breaks idempotency. The
  `LINE_BLOCK` arm in the formatter ignored its `indent` argument and
  emitted every `|` line at column 0; it now prefixes each line with the
  container indent. The CST already matched pandoc here, so this was
  formatter-only.
- [x] A line block inside a blockquote escapes the quote through the formatter:
  `> | foo` formatted to `| foo` (the `>` was dropped, so the `BlockQuote`
  was lost), and `- > | foo` formatted to `- \n  | foo` and then to
  `- \| foo`, an idempotency break. The `BLOCK_QUOTE` arm re-prefixes
  paragraphs, lists, and definition lists explicitly but sent `LINE_BLOCK`
  down the `_ =>` fallback, which emitted the lines unprefixed. `LINE_BLOCK`
  now gets the same `append_blockquote_prefixed_block` treatment
  `DEFINITION_LIST` gets, rendering to a temp buffer at indent 0 because
  `content_prefix` already carries the container indent. The CST already
  matched pandoc, so this was formatter-only.
- [x] Pandoc folds an under-indented lazy line into a line block
  (`- x\n\n  | a\n b |\n` is one `LineBlock [[a, b, |]]`); panache ended the
  line block and emitted the lazy line as its own block. Two causes. (1)
  Pandoc gobbles a list item's continuation indent all-or-nothing
  (`optional (gobbleSpaces n)`), so a lazy line indented *less* than the
  content column keeps every leading space it had and still reads as a
  continuation; panache's strip is greedy, so the classification now also
  consults the raw line (`continues_previous_line`). A line indented
  *exactly* to the content column still opens a new block. (2) Continuation
  lines were projected and formatted as lines of their own. A
  `LINE_BLOCK_LINE` with no `LINE_BLOCK_MARKER` now folds into the line
  above --- with a `Space` in `pandoc_ast.rs`, and by appending to the
  rendered line in the formatter's `LINE_BLOCK` arm, which previously gave
  the continuation its own `|` and so turned one line into two. Corpus 506
  and 507.
- [x] Pandoc drops a lazy line's indentation when its blockquote reader folds it
  back into the quote, so the line cannot continue a line block:
  `> | a\n b |\n` is `BlockQuote [LineBlock [[a]], Para [b, |]]`. Panache
  kept the leading space and read a continuation, and it only held a quote
  open across a lazy line for an open `Paragraph`, `ListItem`, or
  `DEFINITION_LIST`, so the same gap showed up after a heading
  (`> # h\n b |\n` let `b |` escape to the top level). Fixed by the general
  pandoc-dialect blockquote-laziness rule:
  `Parser::fold_lazy_line_into_blockquote` gobbles every non-blank lazy line
  back into the open quote with its indentation dropped (kept in the tree as
  a `WHITESPACE` token, and taken off `self.lines[self.pos]` so the
  container-prefix window and the dispatcher see the same text), and
  `Parser::blockquote_gobble_ends_at` holds pandoc's `endline` guards ---
  each anchored at byte 0 of the raw line, which is why `> para\n# head`
  ends the quote under `-blank_before_header` but `> para\n # head` does
  not. `LineKind::Continuation` now declines a lazy line for the same
  reason. Corpus 508.
- [x] Pandoc tries `many (char ' ' >> anyLine)` *before* the next `| ` marker
  line, so an indented marker line continues the line above: `| a\n  | b\n`
  is one `LineBlock [[a, |, b]]`. Panache checked for a marker first and
  read two lines. Fixed by the ordering swap in `parse_line_block`:
  `continues_previous_line` is consulted before
  `parse_line_block_line_marker`, and it no longer excludes lines whose
  trimmed start is `| ` --- pandoc's rule looks at nothing but the leading
  space. The blockquote-laziness guard (`lazy_in_quote`) still runs first,
  so `> | a\n  | b\n` stays two lines. Corpus 509.
- [x] A line block opening on a list-marker line was read as literal text:
  `- | a\n  | b\n` is `BulletList [[LineBlock [[a], [b]]]]` in pandoc, which
  parses an item's content as a fresh block sequence, but panache buffered
  the post-marker text and emitted a `PLAIN` of two `|` lines. The
  dispatcher's `LineBlockParser` never sees the marker line --- the list
  parser consumes it first --- so the gap is bridged by
  `maybe_open_line_block_in_new_list_item`, alongside the existing
  fenced-code and caption-table marker-line helpers. It runs after them and
  declines when a pipe table starts on the line (tables outrank line blocks
  in the registry, 10 vs 13, and a header row satisfies
  `try_parse_line_block_start`), leaving that to the buffer's structural
  lift. The formatter's same-line leading-block case in `lists.rs` grew
  `LINE_BLOCK` next to `FENCED_DIV`/`CODE_BLOCK`, without which the item's
  marker was dropped. Fixed two parser fixtures that had pinned the old
  divergence (`*_pipe_table_*_no_separator_pandoc`). Corpus 510.
- [x] The blockquote fold reached one line at a time, so a *multi-line*
  construct opened on a lazy line kept the indent on its body lines:
  ````> # h\n ```\n code\n ``` ```` was `CodeBlock " code"` where pandoc has
  `"code"`. Checking it showed the fold was never the whole story --- the
  de-indent is a property of pandoc's quote reader, which skips the leading
  whitespace of every lazy line while extracting the raw content, so
  ````> ```\n code\n> ``` ```` diverged too with no fold in sight. Fixed by
  moving the gobble into `ContainerPrefix`: a `lazy_blockquote_gobble` flag
  set from the dialect makes the blockquote strip drop the leading
  whitespace of any line short of its `>` markers, and every block parser
  inherits it through the one strip (`strip`, `strip_line_0_*`, `split`, and
  the `content_line_prefix_tail` / `emit_content_line_prefixes` pair, which
  hands the bytes to the pending `WHITESPACE` run so the tree stays
  lossless). The trim is unbounded and covers tabs, per
  ````> ```\n         deep\n> ``` ```` -> `CodeBlock "deep"`.
  `parse_fenced_math_block` grew the `dialect` parameter its forward scan
  needed for the same `gobbled_lazily` exemption the other scans had. The
  html body-lift path needed `split` taught too, otherwise a lazy `<div>`
  body line indented four columns became an indented code block where pandoc
  reads a paragraph. Corpus 511; 512 and 513 are blocked (see below and
  `tests/pandoc/blocked.txt`).
- [x] A lazy line that gives a quoted list item a second block left the list
  tight: `> - item\n # head` under `-blank_before_header` was
  `BulletList [[Plain item, Header]]` where pandoc has `Para`. The looseness
  rule was indeed what was missing, but not the one guessed here --- "an
  item holding more than one block" is wrong, since `- a\n  - n` keeps two
  blocks and stays tight. Reading `para` in pandoc's `Readers/Markdown.hs`
  gives the real rule: a paragraph is a `Plain` unless it is terminated by a
  blank line or by one of the block starts `para` looks ahead for --- a
  fenced code block (`+backtick_code_blocks`), an ATX header
  (`-blank_before_header`), or a blockquote (`-blank_before_blockquote`).
  One such `Para` anywhere makes the whole list loose. Fixed in the
  projector (`has_paragraph_broken_by_block`), which is where panache
  already derives looseness; no `ParserOptions` needed, because none of
  those blocks can exist as a sibling after a `PLAIN` unless its gate is on ---
  the CST answers the question. The `listStart` alternative is excluded
  because pandoc suppresses it when already `inList`, and the `</div>` /
  `:::` ones because they are never visible while pandoc reparses an item's
  extracted content; definition lists do not share the promotion at all.
  Also fixed the blank-line-free fence twin
  (````- a\n  ```\n  c\n  ```\n- b````, corpus 514), which diverged under
  default extensions.
- [x] Found while checking the fold, pre-existing and unrelated: a fenced code
  block inside a blockquote keeps the `>` markers in its content ---
  ````> ```\n> code\n> ``` ```` projects `CodeBlock "> code"`. `code_block`
  rebuilds the payload from `CODE_CONTENT`'s raw text, so every
  container-prefix token the emitter peeled off lands back in the string.
  This is what blocks corpus 512, where the CST is already correct. Fixed in
  the projector: `code_content_text` walks the tokens instead of taking the
  node's text. The recorded ambiguity --- a line-leading `WHITESPACE` inside
  `CODE_CONTENT` is a container prefix for a fenced block but the
  significant indent of an indented one --- does resolve from the emitters,
  but the split is not fenced-vs-indented. `BLOCK_QUOTE_MARKER` and its one
  padding space are always prefix; remaining line-start whitespace is
  dropped token-wise only for a fenced block directly inside a blockquote,
  where the emitter consumes exactly what pandoc does (marker padding, or
  the lazy-line gobble). Everywhere else the host indent has to come off by
  *column*, since the emitter's token boundary is not column-exact with
  tabs: for a `:\t` definition marker (content column 4) it peels `"\t\t"` ---
  8 columns --- into the prefix token, so dropping the token loses 4 columns
  of code (corpus 44 caught this). Tabs expand against the raw line column
  before the prefix comes off, mirroring pandoc's `tabFilter`, so
  ````> ```\n> \tcode```` is `"  code"`. Also cured the same leak in
  indented code inside a quote, fenced code in a footnote body, and a quote
  nested in a list item. Corpus 512 + new 515-519; total 512 -> 518 passing.
- [x] The fold declined *entry* inside a quoted list item, so a construct opened
  on such a lazy line never formed: ````> - a\n   ```\n   c\n   ``` ```` is
  `BlockQuote [BulletList [[Plain a]], CodeBlock "c"]` in pandoc, but the
  lazy gates in `parse_line` ran ahead of `fold_lazy_line_into_blockquote`
  and buffered the lines as `PLAIN`, so the fence degraded to an
  `INLINE_CODE` span. The gap was two-sided. Pandoc's `rawListItem` stops
  collecting at a line `codeBlockFenced` would claim, so a fence ends the
  item where a heading or thematic break is still swallowed as lazy item
  text (`> - item\n # head` keeps the `Header` *inside* the item) --- that
  asymmetry is why `close_lists_above_indent` is now gated on the
  fenced-code parser under Pandoc instead of CommonMark's wider `!OpenList`,
  which also fixed the unquoted twin ````- a\n```rust\nc\n``` ````. And both
  lazy gates had to stop claiming the line: the paragraph one declines for a
  backtick fence only (`endline`'s guard is backtick-anchored, so
  `> a\n   ~~~` stays lazy text), the list one for any fence. Both probe the
  *de-indented* content and require a matching closer, since
  `codeBlockFenced` fails without one and an info string is no substitute.
  The fold closes the list itself, before emitting the gobbled indent ---
  forced, not cosmetic: a `~~~` fence only detects as a block when
  `has_blank_before`, true for a `BlockQuote` on the stack but false for a
  `ListItem`. The dispatcher's closer scan also had to drop a gobbled line's
  whole indent, not the three columns `is_closing_fence` tolerates, or a
  fence at four spaces declined and fell back to a paragraph.
  `debug format --checks idempotency` now passes on the shape (it used to
  re-quote the lines as `>   >   c`). Corpus 513; block 39 -> 40, total 518
  -> 519 (100%).
- [x] Formatting a fence that a lazy fold opened rewrote its payload:
  ````> # h\n   ```\n   c\n   ``` ```` is `CodeBlock "c"` but formatted to
  ````> ```\n>    c\n> ``` ````, i.e. `CodeBlock "   c"`. Pre-dated the 513
  work, which only made one more input reach it. `parse_fenced_code_block`
  emits a *content* line's gobbled prefix bytes inside `CODE_CONTENT` (via
  `window.emit_prefix_at`), and the formatter fed those bytes to its
  column-based `strip_indent_columns` with a base of 0 --- the fold consumes
  the fence's indent upstream, so from `CODE_BLOCK`'s view the fence sits at
  column 0 and there is no base to strip against. Hoisting the prefix out of
  `CODE_CONTENT` was the first idea but does not survive contact with the
  shape: `CODE_CONTENT` is one node over *all* content lines, so only line
  one's prefix could become a sibling. Fixed instead by mirroring the
  projector's `fenced_in_blockquote` rule in the formatter --- for a fenced
  block whose parent is a `BLOCK_QUOTE` the whole line-start `WHITESPACE`
  token is container syntax (marker padding or the lazy gobble) and drops
  token-wise; everywhere else the tab-inexact token boundary still forces
  the column strip. Golden case `blockquote_lazy_fence_payload`.
- [x] Nobody stripped the *opening fence's indent* from a fenced block's
  payload, in the formatter or the projector: ````   ```\n   c\n   ``` ````
  is `CodeBlock "c"` in pandoc but was `"   c"` in both, and
  ````>   ```\n>   c\n>   ``` ```` is `"c"` but came out `"  c"`. Unlike a
  container prefix, the emitter leaves a content line's share of that indent
  inside the line's `TEXT` token, so it can only come off by column. Both
  consumers already had the machinery and only needed the number: the
  `WHITESPACE` run before `CODE_FENCE_OPEN`, which is whatever host indent
  the parser left in the block *followed by* the fence's own (a list item at
  content column 2 whose fence is indented one further emits `"  "` then
  `" "`). Projector callers take the larger of that run and the host indent
  they already know, rather than summing, because the parser peels the host
  indent into the container's own tokens in some shapes and leaves it here
  in others; the formatter spends the same count as a budget, against the
  line's `WHITESPACE` first and then its `TEXT`. That in turn exposed
  `append_blockquote_prefixed_block` skipping its re-indent on already-
  indented lines: with a correct payload that shifts the body down the fence
  and loses the columns on the next parse, so the re-indent is now uniform.
  Corpus 520/521, golden case `fenced_code_indented_fence`. The CommonMark
  twin below was the same root cause and is fixed with it --- the spec suite
  has its own §4.5-correct renderer, which is why it never saw the bug.
- [x] An *over*-indented fence after a list item is lazy text in pandoc:
  ````- a\n   ```\n   c\n   ``` ```` (indent 3, content column 2) is
  `Plain [a, SoftBreak, Code "c"]`, because `listLine` gobbles only
  `continuationIndent` columns and the leftover space defeats `endline`'s
  backtick anchor. Panache nested a `CodeBlock` in the item. The rule turned
  out not to be list-specific at all --- `endline` anchors on the fence
  *character*, so any leftover column keeps the fence inside the paragraph,
  and ````a\n ```r\nc\n``` ```` and ````> a\n>  ```r\n> c\n> ``` ```` were
  wrong the same way (the top-level bare-fence case only looked right
  because the bare-fence heuristic declined it for an unrelated reason).
  Fixed in `FencedCodeBlockParser::detect_prepared`: under Pandoc, with no
  blank line before and a paragraph or list-item buffer open, a fence whose
  container-stripped line still starts with whitespace declines detection
  and stays paragraph text. CommonMark keeps interrupting from up to three
  columns (§4.5), so the gate is dialect-scoped, and a blank line before
  still opens the block. Corpus 521/521. Parser cases
  `list_item_overindented_fence_is_lazy_pandoc`,
  `list_item_overindented_fence_interrupts_commonmark`,
  `overindented_fence_after_paragraph_is_lazy_pandoc`; golden case
  `overindented_fence_stays_paragraph_text`. Two adjacent defects surfaced
  and are filed separately below.
- [x] A lazy continuation line inside a list item kept its *full* indent in the
  inline text instead of being gobbled to the content column, which is
  invisible until an inline construct preserves interior whitespace:
  ``- a\n   `x\n   y` `` is `Code "x  y"` in pandoc (`listLine` eats the 2
  continuation columns, leaving one space per line) but was `Code "x    y"`
  here. The blockquote path already stripped correctly, so this was the list
  path only. Pre-dated the over-indented-fence work above; not in the
  corpus. Fixed in `ListItemBuffer::to_paragraph_buffer`, which now holds
  each continuation line's `content_col` indent *out* of the text handed to
  the inline parser and re-injects it as a `WHITESPACE` token at emission ---
  the same trick the blockquote path uses for its `>` markers, so the parse
  stays byte-lossless. `MarkerInjectingSink` grew an `InjectedMarker` enum
  to carry both marker kinds. Verified against pandoc for indents 1--5
  columns past the content column; the only AST change across all 677 parser
  fixtures was display math in a list item, which now matches pandoc too
  (`Math DisplayMath "\n\\begin{bmatrix}...\n"`, indent no longer baked in).
  Parser case `list_continuation_indent_stripped_pandoc`.
- [x] A **tab** straddling a list item's content column is not gobbled at all,
  so ``- a\n\t`x\n\ty` `` was `Code "x \ty"` here but `Code "x   y"` in
  pandoc, which expands the tab to a 4-column stop and eats 2 of those
  columns. Splitting the tab means emitting residual *spaces* that do not
  exist in the source, which byte-losslessness forbids, so no parser change
  can fix it: both byte attributions are wrong (leaving the tab in the
  payload keeps 4 columns, gobbling it whole keeps 0, pandoc keeps 2), and
  the CST cannot hold a string the source does not contain. Fixed a layer
  down instead --- pandoc's `tabFilter` is a *pre-reader* pass over the
  whole input, so the projector is where panache models it.
  `inline_code_payload` now rebuilds a code span by walking its tokens with
  a running source column, expanding tabs to 4-column stops from that
  column, and subtracting the enclosing item's content column from a tab
  still inside the line's indent run (`list_gobble_columns`). That also
  fixed the tab expansion the projector never did at all: ``a`x\ty`b `` is
  `Code "x y"` (the tab starts at column 3), `` `x\n\ty` `` is
  `Code "x     y"`, and `` > a\n> \t`x\n> \ty` `` counts the `> ` prefix
  `tabFilter` had not yet stripped. Spaces need no such treatment --- the
  parser already peels every gobbled space out of the payload, so the floor
  only ever compensates for the unsplittable tab. Parser fixture
  `list_continuation_tab_indent_pandoc` pins the two CST shapes the
  projector leans on; unit tests in `pandoc_ast.rs` pin the payloads against
  pandoc 3.9.0.2. Corpus unchanged (no case has a tab in a code span). Split
  out of the item above.
- [x] A definition body's continuation indent is not gobbled at all, so
  ``a\n:   d\n    `x\n    y` `` is `Code "x y"` in pandoc but
  `Code "x     y"` here --- pandoc's `defListIndent` gobble is the
  definition's content column, the same rule `listLine` applies. This is the
  definition-list twin of the list-item indent bug fixed above, and it
  wanted the same parser fix (hold the indent out of the text handed to the
  inline parser, re-inject it as `WHITESPACE` at emission), *not* a floor in
  the projector --- that would paper over a real parser gap. Found while
  fixing the tab item above. Not in the corpus. Fixed in three parts:
  - `Container::Definition`'s `plain_buffer` is now a `ParagraphBuffer` instead
    of a `TextBuffer`, so `parse_inner_content` can hold each continuation
    line's `content_col` indent out of the buffered text as an `Indent` segment
    rather than re-prepending it. A *lazy* line never reaches the content column
    and pandoc takes nothing off it, so its whitespace stays payload --- that
    case already matched and still does. `emit_definition_plain_or_heading`
    keeps reading the buffer's *raw* bytes for its block-shape tests (ATX
    heading, standalone image), so block structure is untouched; only the inline
    emission changed. `TextBuffer` had no other caller left and was deleted.
  - The tab variants turned out to be two separate defects, both fixed here
    rather than deferred. `try_parse_definition_marker` measured the post-marker
    whitespace with `leading_indent` (column 0), so `:\td` got a content column
    of 5 where `tabFilter` reaches the stop at 4; it now uses
    `leading_indent_from(after_marker, indent_cols + 1)`. And a tab *straddling*
    the content column still cannot be split (one byte, lossless CST), so
    `list_gobble_columns` in the projector was generalized from `LIST_ITEM` to
    "innermost `LIST_ITEM` or `DEFINITION`" --- the two share the
    marker/trailing-space/content-column shape, and the column is absolute, so a
    definition nested in a list item folds in automatically.
  - Verified against pandoc 3.9.0.2 across marker widths, surplus indent, lazy
    lines, `~` markers, bodies reopening after a blank line or a heading,
    definitions nested in a list item, and all seven tab shapes: 17 of 18 probe
    cases now match where 13 diverged before. Parser fixtures
    `definition_continuation_indent_stripped_pandoc` (space indents, display
    math, lazy) and `definition_continuation_tab_indent_pandoc` (the two CST
    shapes the projector leans on); payload unit tests in `pandoc_ast.rs`.
    Formatter output is unchanged --- no golden expectation moved, and the
    losslessness/idempotency sweep over all 1070 fixtures has the same seven
    pre-existing failures as before. The other 11 CST snapshots that moved are
    `TEXT` retagged as `WHITESPACE` over identical byte ranges; two were checked
    against pandoc individually and one
    (`blockquote_no_interrupt_def_plain_continuation_pandoc`) is a payload fix,
    `Code "{{< include >}}"` where it used to be five spaces.
- [x] A **footnote** definition's continuation indent is not gobbled either:
  ``x[^1]\n\n[^1]: d\n    `x\n    y` `` is `Code "x y"` in pandoc but
  `Code "x     y"` here. Same bug as the definition-list item above and the
  list-item one before it, but a third code path --- a footnote body's
  continuation lines land in a `Container::Paragraph`'s `ParagraphBuffer`
  via `append_paragraph_line`, which buffers the *raw* line. Fixed in three
  parts:
  - `parse_inner_content` now computes the gobble once, next to the existing
    `strip_content_indent` call, and hands it to a new
    `append_paragraph_line_gobbling`, which holds those bytes out as an `Indent`
    segment instead of buffering the line verbatim. Keying it on
    `content_indent` (the sum `content_container_indent_to_strip` already
    maintains) means `Container::Admonition` and `Container::Definition` are
    covered by the same change --- the admonition half was unverified in the
    original note and is now pinned by unit tests, since python-markdown
    admonitions have no pandoc oracle.
  - A *lazy* line never reaches the content column and pandoc takes nothing off
    it, so its whitespace stays payload; the guard is
    `leading_indent(content).0 >= content_indent`.
  - The projector's `list_gobble_columns` grew a `FOOTNOTE_DEFINITION` arm. It
    cannot reuse the marker-width rule the other two share: `noteBlock` strips a
    fixed `indentSpaces` regardless of how wide `[^label]:` is, so the column is
    the definition's start column plus 4 (`FOOTNOTE_INDENT_COLUMNS`, now shared
    with the parser's container push).
  - Removing the indent from the `TEXT` token made the linter's
    `swallowed-list-marker` `baseline = 4` hack obsolete --- it existed only
    because footnote bodies used to bake the indent into `TEXT`. Dropped; marker
    indent is now measured from the content column for every container
    uniformly.
  - Verified against pandoc 3.9.0.2 over 20 probe cases (marker-line spans,
    surplus indent, lazy lines, second paragraphs, blockquote nesting, math,
    multiple notes, and all four tab shapes --- note that no tab can straddle a
    content column of 4, since a tab starting before column 4 always stops
    exactly there): 18 of 20 match where 4 did before. The two that remain are
    separate defects, filed below. Parser fixtures
    `footnote_continuation_indent_stripped_pandoc` and
    `footnote_continuation_tab_indent_pandoc`, plus payload unit tests in
    `parser/blocks/tests/content_containers.rs`. Formatter output is unchanged ---
    no golden expectation moved, and the losslessness/idempotency sweep over all
    1073 fixtures has the same seven pre-existing failures, byte for byte. The
    10 CST snapshots that moved are all `TEXT` retagged as `WHITESPACE` over
    identical byte ranges, in footnote and admonition bodies.
- [x] A footnote definition inside a **list item** is not recognized as a
  footnote definition at all: `- x[^1]\n\n  [^1]: d\n` is
  `Plain [Str "x", Note ...]` in pandoc but two `Para`s here, with the
  marker left as literal text (`Str "[^1]:"`). This is block *detection*,
  not the indent gobble --- the definition never opens, so no
  `FOOTNOTE_DEFINITION` node exists. Found while fixing the footnote indent
  above. Not in the corpus. Fixed:
  - `FootnoteDefinitionParser::detect_prepared` required `[^` at byte 0 of the
    container-prefix-stripped line, but the dispatcher does not strip a list
    item's content column --- block parsers handle that themselves (cf.
    `content_for_fenced_div_detection`). A new `footnote_marker_indent_len` does
    both halves of what `noteBlock` sees: the item's content column when the
    line reaches it, plus `nonindentSpaces` (<=3) on top, in that same frame.
    Four spaces past the frame stays an indented code block, which the registry
    reaches first anyway.
  - This also fixes plain `   [^1]: d` at the top level, which pandoc accepts
    and panache used to leave as paragraph text.
  - The indent is carried on `FootnoteDefinitionPrepared` and emitted as a
    leading `WHITESPACE` token inside `FOOTNOTE_DEFINITION`, matching what
    `REFERENCE_DEFINITION` and `HEADING` do with theirs.
  - Verified against pandoc over the indent window (0..=3 and 4 at top level,
    content column +0..=3 and +4 in an item), multi-item lists, and a definition
    followed by a second paragraph: all match. Parser + formatter fixtures
    `footnote_definition_in_list_item`, plus three unit tests in
    `parser/blocks/reference_links.rs`. No existing test moved.
  - One divergence survived at the time, and it was not footnote-specific: a
    *single*-item list whose only block is the note's paragraph was `Plain` in
    pandoc and `Para` here. Fixed since, by implementing `compactify`.
- [x] `is_loose_list` cannot express pandoc's `compactify`. Fixed: looseness is
  per paragraph now, and `compactify` runs after block extraction.
  - `paragraph_is_para` classifies each `PLAIN` child on its own --- a `Para`
    when a blank line follows it inside the item, when one of pandoc's `para`
    terminators follows, or when the item itself is followed by a blank line
    (pandoc's `rawListItem` swallows those into the item's own content).
    `compactify` then applies all three of pandoc's branches, including the
    demotion of a sole trailing `Para`.
  - Per-paragraph is load-bearing, not an implementation detail: with a
    list-wide flag `- a\n- b\n\n  [^1]: d\n` sees two `Para`s and skips the
    demotion, where pandoc sees one and emits `Plain`, `Plain`. Corpus cases
    522--524; the unit tests sit under the `compactify` banner in
    `pandoc_ast.rs`.
  - The second defect in the same shape is fixed too: `- [x]` was tagged a task
    checkbox with nothing after it, so the item projected to nothing. Pandoc
    converts task markers *after* inline parsing (`taskListItemFromAscii`),
    matching only `Str "[x]" : Space : rest`, so the parser now requires a
    literal space or tab and content on the same line.
  - Still divergent, and much more obscure: `- [x] foo\n\n  [x]: /url\n` is
    `Plain [Link ..., Space, Str "foo"]` in pandoc, because reference resolution
    beats the task marker. Panache decides the checkbox structurally in the
    parser, before references resolve, so it emits `Str "\9746"`. Not in the
    corpus.
- [x] A lazy `> q` after list-item text became a sibling block here but stays
  text in pandoc: `- a\n  > q\n` is one
  `Plain [Str "a", SoftBreak, Str ">", Space, Str "q"]` there, two blocks
  here. Pandoc's `blank_before_blockquote` is on by default for `markdown`,
  so a blockquote cannot interrupt a paragraph without a blank line, and the
  lazy line is a continuation instead. Fixed in the parser: the
  no-blank-before arm now folds the line into the open `ListItemBuffer`
  instead of flushing it and starting a `PARAGRAPH` sibling. No projector
  change was needed --- `interrupts_paragraph`'s premise (a `BLOCK_QUOTE`
  cannot follow a `PLAIN` unless the extension that lets it interrupt is
  off) holds again.
- [x] A list nested inside a footnote body gobbled the wrong number of columns:
  the item's continuation lines went through
  `ListItemBuffer::to_paragraph_buffer` with only the item's own
  `content_col`, which is \*relative\* to the content the footnote already
  stripped (2, not the absolute 6), while the buffered lines are raw --- so
  the footnote's 4 columns leaked into the payload. Fixed by handing the
  buffer the whole enclosing container chain
  (`ContainerStack::gobble_chain`) instead of a single column, and by making
  each level of that chain \*all-or-nothing\*, which is what pandoc's
  `listLine` does (`optional (gobbleSpaces n)`): a line too shallow for a
  level keeps every column it has, and the levels inside it still get their
  turn on the same residue. That second half was a bug of its own --- the
  old `min(indent, content_col)` gobble was wrong for lazy continuation
  lines in plain and nested lists too, with no footnote involved. Three
  parser snapshots moved (all toward pandoc); the pandoc corpus stayed at
  524/524.
- [x] The formatter expanded a code span's tabs from column 0 of the span's own
  content rather than from its source column, so it rewrote `` a`x\ty`b ``
  to `` a`x   y`b `` --- pandoc reads 1 space in the input and 3 in the
  output, i.e. formatting changed the document's meaning. The list cases
  happened to come out right (the joined `"x "` prefix is exactly the
  content column that was gobbled), which is why this never surfaced. Found
  while fixing the tab item above. Fixed by lifting the projector's column
  bookkeeping out of `pandoc_ast.rs` into
  `syntax::code_span::code_span_payload` (tab width is a parameter now: the
  projector pins pandoc's 4, the formatter passes `tab-width`), so both read
  a span the same way. `expand_tabs_code_span` is left owning only the line
  join, which is the one place the two disagree --- the projector trims the
  padding pandoc trims, the formatter preserves it.
- [x] `has_matching_closer` scans for a fence's closer past the end of the
  enclosing list item, so a top-level fence adopts an item's paragraph text:
  ````- a ```\n  c\n  ```\n\nb ```r\nc\n``` ```` is two `Plain`/`Para` runs
  of inline code in pandoc, but here the item's third line opens a
  `CodeBlock` that swallows ````\nb ```r\nc````. The forward scan in
  `FencedCodeBlockParser::detect_prepared` breaks only on a *blockquote*
  depth drop --- nothing ends it when the list item does, and the column
  slice it applies to candidate lines does not stand in for that. Shows up
  as an idempotency failure, since panache's own output for the entry above
  is exactly this shape. Not in the corpus. Fixed with `ContainerExitScan`
  in `blocks/code_blocks.rs`, which both closer scans
  (`FencedCodeBlockParser::detect_prepared` and
  `Parser::has_matching_fence_closer`) now consult: it reproduces pandoc's
  `listItem` rule that a blank line arms the indent requirement, so the
  first non-blank line below the container's content column after a blank
  ends the scan. Lazy gobbling without an intervening blank is untouched,
  and the guard only bites for *bare* fences, which are the only ones whose
  detection consults a closer. Pandoc corpus stayed at 524/524.
- [x] A bare closed fence after a plain paragraph did not interrupt it:
  ````a\n```\nc\n``` ```` is `Para "a"` + `CodeBlock "c"` in pandoc but was
  one inline-code `Para` here. The extra transcript/list contexts guarding
  the heuristic in `FencedCodeBlockParser::detect_prepared` are gone --- a
  matching closer is the whole condition, as in pandoc, where
  `codeBlockFenced` simply fails without one. What the contexts were
  standing in for is the *inline* side: a bare fence that closes a code span
  opened earlier in the buffered paragraph is that span's closer, since
  pandoc reaches `endline` only between inlines, never from inside a code
  span. That is now modelled directly by `pending_code_span_openers`,
  threaded to the detector as `BlockContext::open_code_span_openers` (gated
  on the line opening with a backtick, so the buffer scan stays off the hot
  path), which keeps ````b ```r\nc\n``` ```` a single
  `Para [Str "b", Code "r c"]`. Pandoc corpus stayed at 524/524, CommonMark
  at 652/652.
- [x] Under `flavor = "commonmark"` a lazy quoted fence kept its indent in the
  payload: ````> - a\n   ```\n   c\n   ``` ```` ends the quote (correct) but
  yielded `CodeBlock "   c"` where CommonMark strips up to the opening
  fence's indent and gives `"c"`. Same root cause as the entry above and
  fixed with it; the CST that
  `blockquote_lazy_fence_ends_quoted_list_item_commonmark` pins is
  unchanged, and the formatter now emits ````> - a\n```\nc\n``` ````.

### Architecture

- [ ] Stop letting `pandoc_ast.rs` drift into a second-stage parser. Load-
  bearing byte-walkers (`split_html_block_by_tags`, `parse_pandoc_blocks`
  and the refs/heading-id reparse helpers) re-tokenize source the CST should
  already encode. This violates the single-pass invariant in `AGENTS.md` and
  hides structural decisions from downstream consumers (linter, salsa, LSP,
  formatter) which all walk the CST, not the projector. The guiding
  principle: when the parser computes a structural fact during its single
  pass, it must emit that fact into the CST (wrapping existing source bytes,
  `HTML_ATTRS`-style --- never synthetic tokens) instead of forcing the
  projector to recompute it. Each bucket below is its own bounded step,
  verified against pandoc-native + CommonMark (both must stay byte-identical
  or improve).

- [ ] Consolidate container-frame resolution behind a single typed verdict.
  There is no authoritative answer to "which container frame is this line
  in, and does it reach the content column?", so every lookahead site picks
  one of several overlapping helpers and a wrong pick fails only under a
  specific container nesting.

  The bug stream says this is where the defects are. Of the last 300 commits,
  120 are fixes and 77 of those are `fix(parser)`; 44 of the 77 (57%) name a
  frame concept (indent, column, list, item, blockquote, definition, lazy,
  continuation). Keyword histogram: `list` 19, `item` 14, `definition` 11,
  `indent` 9, `marker` 9, `table` 9, `lazy` 8. That is one question answered
  wrongly many times, not a scatter of unrelated defects.

  Current answers to that one question, all subtly different:

  - `ContainerPrefix::strip` (`container_prefix.rs:337`) --- walks ops via
    `advance_columns` (`:1050`), which counts *any* character as a column, so on
    a short line it slices content off as if it were indent.
  - `ContainerPrefix::line_carries_list_indent` (`:402`) --- vets only the
    `ListAdvance` component; blockquote and content indent are stripped but
    their absence is deliberately not reported.
  - `ContainerPrefix::peek_prefix_at` (`:775`) --- already has its own open TODO
    below for not being equivalent to `strip`.
  - `ContainerPrefix::strip_line_0_for_emission` (`:435`) --- skips the
    innermost `ListAdvance` under a flag.
  - `strip_content_indent` --- degrades gracefully instead of reporting a short
    line, which is exactly the signal callers need.
  - `Parser::content_container_indent_to_strip` (`core.rs:4759`) --- sums
    content columns into an *absolute*.
  - `gobbled_indent_prefix_len` (`container_stack.rs:219`).
  - Hand-rolled `leading_indent(x).0 >= content_col` at `core.rs:1106`,
    `core.rs:1192`, `core.rs:4836`, `core.rs:4969`, and
    `definition_lists.rs:101`.

  Two live traps inside that list. First, `content_col` carries two conventions:
  absolute (what `content_container_indent_to_strip` sums, and what every
  hand-rolled dedent test compares against) versus relative-per-container (what
  `from_stack` pushes as `ContentIndent` ops). Inside a list item the same
  definition body reads 4 one way and 2 the other. Second, the source already
  documents the divergences it cannot reconcile: `from_ctx` "may diverge from
  `from_stack`" (`:161`), and `peek_prefix_at`'s dormancy claim rests on
  `content_indent == 0`, which is false for every definition-list lookahead.

  The guiding principle: a strip must not be able to report *what is left of a
  line* without also reporting *whether the container indent it consumed was
  real*. Replace the `&str` return with a typed verdict (`InsideFrame` /
  `Dedented(cols)` / `FakedIndent`) so that today's class of bug becomes a type
  error rather than a missing test. Fix the `content_col` convention at the same
  time and state it once, in one doc comment.

  Bounded steps, each landable on its own:

  - [ ] Pin the current behavior of all eight paths as a table test over one
    corpus of `(container stack, line)` pairs, including the shapes where
    they disagree today. This is the safety net; nothing else starts without
    it. Seed the corpus from the three deferred items under
    `Parser bugs found by the incremental fuzzer` --- the tab straddle, the
    `is_caption_followed_by_table` over-strip, and the `peek_prefix_at`
    non-equivalence are known disagreement shapes held back for exactly
    this.
  - [ ] Settle the `content_col` convention (absolute or relative) and make
    `from_stack` and `content_container_indent_to_strip` agree.
  - [ ] Introduce the typed verdict alongside the existing API and migrate the
    five hand-rolled `leading_indent` sites first --- they are the smallest
    and each has existing coverage.
  - [ ] Migrate the lookahead callers (`next_line_is_definition_marker`,
    `next_is_definition_term_below`, the caption and term probes), then
    delete whichever of `peek_prefix_at` / `line_carries_list_indent` the
    verdict subsumes. Closes the `peek_prefix_at` item below.

  Secondary, and worth folding in once the above lands: containers hold text in
  `ParagraphBuffer` / `ListItemBuffer` *outside* the green builder, and a
  classification can only be revised while it is still buffered --- once flushed
  it is a `PLAIN` that cannot be retagged. The definition-list series has now
  asked "is this line a term?" at three different moments (the marker line, the
  dispatcher's `Term` arm, and blank-line flush time), each a separate site to
  keep in sync. The single-pass rule in `AGENTS.md` is right; what is missing is
  one mechanism for plumbing the lookahead, so each site hand-rolls it with a
  different strip helper.

  Two honest caveats for whoever picks this up. Only 9 of 55 open TODO items are
  frame-related, well under the 57% in the fix history --- frame bugs are
  reachable from real documents and so get found and fixed, meaning this
  dominates past work more than it does the remaining backlog. And the claim
  that consolidation cuts the bug rate is judgment, not measurement; what is
  measured is that the divergences exist and are already documented in the
  source.

## Parser - Coverage

This section tracks implementation status of Pandoc Markdown features based on
the spec files in `assets/pandoc-spec/`.

**Focus**: Prioritize **default Pandoc extensions**. Non-default extensions are
lower priority and may be deferred until after core formatting features are
implemented.

### Block-Level Elements

### Paragraphs ✅

- [x] Basic paragraphs
- [x] Paragraph wrapping/reflow
- [x] Extension: `escaped_line_breaks` (backslash at line end)

### Headings ✅

- [x] ATX-style headings (`# Heading`)
- [x] Setext-style headings (underlined with `===` or `---`)
- [x] Heading identifier attributes (`# Heading {#id}`)
- [x] Extension: `blank_before_header` - Require blank line before headings
  (default behavior)
- [x] Extension: `header_attributes` - Full attribute syntax
  `{#id .class key=value}`
- [x] Extension: `implicit_header_references` - Auto-generate reference links

### Block Quotations ✅

- [x] Basic block quotes (`> text`)
- [x] Nested block quotes (`> > nested`)
- [x] Block quotes with paragraphs
- [x] Extension: `blank_before_blockquote` - Require blank before quote (default
  behavior)
- [x] Block quotes containing lists
- [x] Block quotes containing code blocks

### Lists 🚧

- [x] Bullet lists (`-`, `+`, `*`)
- [x] Ordered lists (`1.`, `2.`, etc.)
- [x] Nested lists
- [x] List item continuation
- [x] Complex nested mixed lists
- [x] Extension: `fancy_lists` - Roman numerals, letters `(a)`, `A)`, etc.
- [ ] Extension: `startnum` - Start ordered lists at arbitrary number (low
  priority, if we even should support this)
- [x] Extension: `example_lists` - Example lists with `(@)` markers
- [x] Extension: `task_lists` - GitHub-style `- [ ]` and `- [x]`
- [x] Extension: `definition_lists` - Term/definition syntax

### Code Blocks

- [x] Fenced code blocks (backticks and tildes)
- [x] Code block attributes (language, etc.)
- [x] Indented code blocks (4-space indent)
- [x] Extension: `fenced_code_attributes` - `{.language #id}`
- [x] Extension: `backtick_code_blocks` - Backtick-only fences
- [x] Extension: `inline_code_attributes` - Attributes on inline code

### Horizontal Rules

- [x] Basic horizontal rules (`---`, `***`, `___`)

### Fenced Divs

- [x] Basic fenced divs (`::: {.class}`)
- [x] Nested fenced divs
- [x] Colon count normalization based on nesting
- [x] Proper formatting with attribute preservation
- [x] Top-level indented lone `:::` diverges from pandoc. Panache accepted up to
  3 leading spaces on a closing fence, so `::: outer\ntext\n  :::` closed
  the div; pandoc instead treats the indented `:::` as paragraph text and
  leaves the div implicitly closed at EOF (`Str ":::"`). Fixed by tracking
  the opener's indent on `Container::FencedDiv` and rejecting a closer more
  indented than its opener in `FencedDivCloseParser` (scoped to the no-list
  frame so the #439 in-list handling is untouched). Surfaced 2026-07-28
  while fixing #439.
- [ ] Nested fenced divs inside a list item are mis-parsed: the outer div is
  left unclosed and its trailing `:::` becomes stray text, which surfaces as
  a `stray-fenced-div-markers` lint false positive (e.g.
  `docs/authoring/markdown-basics.qmd`). Minimal repro: a `- -` nested list
  whose inner item opens a `pad` div, contains a fully closed `light` div,
  then a trailing `:::` to close `pad`. Pandoc closes both divs
  (`Div .pad [ Div .light ]`); panache leaves `pad` open and strays the
  closer. Surfaced 2026-08 in the quarto-web triage.

### Tables

- [x] Extension: `simple_tables` - Simple table syntax (parsing complete,
  formatting deferred)
- [x] Extension: `table_captions` - Table captions (both before and after
  tables)
- [x] Extension: `pipe_tables` - GitHub/PHP Markdown tables (all alignments,
  orgtbl variant)
- [x] Extension: `multiline_tables` - Multiline cell content (parsing complete,
  formatting deferred)
- [x] Extension: `grid_tables` - Grid-style tables (parsing complete, formatting
  deferred)

### Line Blocks

- [x] Extension: `line_blocks` - Poetry/verse with `|` prefix

### Inline Elements

#### Emphasis & Formatting

- [x] `*italic*` and `_italic_`
- [x] `**bold**` and `__bold__`
- [x] Nested emphasis (e.g., `***bold italic***`)
- [x] Overlapping and adjacent emphasis handling
- [x] Extension: `intraword_underscores` - `snake_case` handling
- [x] Extension: `strikeout` - `~~strikethrough~~`
- [x] Extension: `superscript` - `^super^`
- [x] Extension: `subscript` - `~sub~`
- [x] Extension: `bracketed_spans` - Small caps `[text]{.smallcaps}`, underline
  `[text]{.underline}`, etc.

#### Code & Verbatim

- [x] Inline code (`code`)
- [x] Multi-backtick code spans (\`\`\`\`\`)
- [x] Code spans containing backticks
- [x] Proper whitespace preservation in code spans
- [x] Fenced code blocks (\`\`\` and \~\~\~)
- [x] Indented code blocks

#### Links

- [x] Inline links `[text](url)`
- [x] Automatic links `<http://example.com>`
- [x] Nested inline elements in link text (code, emphasis, math)
- [x] Reference links `[text][ref]`
- [x] Extension: `shortcut_reference_links` - `[ref]` without second `[]`
- [x] Extension: `link_attributes` - `[text](url){.class}`
- [x] Extension: `implicit_header_references` - `[Heading Name]` links to header

#### Images

- [x] Inline images `![alt](url)`
- [x] Nested inline elements in alt text (code, emphasis, math)
- [x] Reference images `![alt][ref]`
- [x] Image attributes `![alt](url){#id .class key=value}`
- [x] Extension: `implicit_figures`

#### Math

- [x] Inline math `$x = y$`
- [x] Display math `$$equation$$`
- [x] Multi-dollar math spans (e.g., `$$$ $$ $$$`)
- [x] Math containing special characters
- [x] Extension: `tex_math_dollars` - Dollar-delimited math

#### Footnotes

- [x] Inline footnotes `^[note text]`
- [x] Reference footnotes `[^1]` with definition block
- [x] Extension: `inline_notes` - Inline note syntax
- [x] Extension: `footnotes` - Reference-style footnotes

#### Citations

- [x] Extension: `citations` - `[@cite]` and `@cite` syntax with complex key
  support
- [x] Pandoc `notAfterString` for bare `@key`: a citation glued to a preceding
  word character is literal text, not a citation (`word@key`,
  `user@example.com`, `違法編訂@jzkhl`). Handled at the shared detection
  site via a char-before-`@` check (alphanumeric or `.` suppresses); the
  `-@` suppress-author form is exempt. Backs the `unspaced-citation` lint
  rule. Closes #448.
- [x] `notAfterString` delimiter-adjacent corner: a bare `@key` glued to a
  *resolved closing emphasis/strong delimiter* is now suppressed to match
  pandoc (`*em*@key` and `**strong**@key` are `Emph`/`Strong` +
  `Str "@key"`). The IR consults the emphasis pass's result when building
  the construct plan (`demote_bare_citation_after_emphasis_closer`), keying
  off resolved closers only, so `*@key*` (opener) and `*em*-@key`
  (suppress-author) keep the citation. No extra scan: the correction reads
  already-computed delimiter state rather than re-classifying.
- [ ] `unspaced-citation` covers citations only. A crossref glued to a word
  (`x@fig-plot`) is likewise left as text by the parser but not flagged by
  the rule; extend it to crossref keys (gated on `quarto_crossrefs`) as a
  follow-up.

#### Spans

- [x] Extension: `bracketed_spans` - `[text]{.class}` inline
- [x] Extension: `native_spans` - HTML `<span>` elements with markdown content

### Metadata & Front Matter

#### Metadata Blocks

- [x] Extension: `yaml_metadata_block` - YAML frontmatter
- [x] Extension: `pandoc_title_block` - Title/author/date at top

### Raw Content & Special Syntax

#### Raw HTML

- [x] Extension: `raw_html` - Inline and block HTML
- [x] Extension: `markdown_in_html_blocks` - Markdown inside HTML blocks

#### Raw LaTeX

- [x] Extension: `raw_tex` - Inline LaTeX commands (`\cite{ref}`,
  `\textbf{text}`, etc.)
- [x] Extension: `raw_tex` - Block LaTeX environments
  (`\begin{tabular}...\end{tabular}`)
- [x] Extension: `latex_macros` - Expand LaTeX macros (conversion feature, not
  formatting concern)

#### Other Raw

- [x] Extension: `raw_attribute` - Generic raw blocks `{=format}`

### Escapes & Special Characters

#### Backslash Escapes

- [x] Extension: `all_symbols_escapable` - Backslash escapes any symbol
- [x] Extension: `angle_brackets_escapable` - Escape `<` and `>`
- [x] Escape sequences in inline elements (emphasis, code, math)

#### Line Breaks

- [x] Extension: `escaped_line_breaks` - Backslash at line end = `<br>`

### Non-Default Extensions (Future Consideration)

These extensions are **not enabled by default** in Pandoc and are lower priority
for initial implementation.

#### Non-Default: Emphasis & Formatting

- [x] Extension: `mark` - `==highlighted==` text (non-default)

#### Non-Default: Links

- [x] Extension: `autolink_bare_uris` - Bare URLs as links (non-default)
- [x] Extension: `mmd_link_attributes` - MultiMarkdown link attributes
  (non-default)

#### Non-Default: Math

- [x] Extension: `tex_math_single_backslash` - `\( \)` and `\[ \]` (non-default,
  enabled for RMarkdown)
- [x] Extension: `tex_math_double_backslash` - `\\( \\)` and `\\[ \\]`
  (non-default)
- [x] Extension: `tex_math_gfm` - GitHub Flavored Markdown math (non-default)

#### Non-Default: Metadata

- [x] Extension: `mmd_title_block` - MultiMarkdown metadata (non-default)

#### Non-Default: Headings

- [x] Extension: `mmd_header_identifiers` - MultiMarkdown style IDs
  (non-default)

#### Non-Default: Lists

- [x] Extension behavior: lists can start without a preceding blank line
  (non-default compatibility behavior).
- [x] Add explicit extension-gated handling/config semantics for
  `lists_without_preceding_blankline`.
- [x] Extension behavior: four-space list indentation rules are supported in
  compatibility mode.
- [x] Add explicit extension-gated handling/config semantics for
  `four_space_rule`.

#### Non-Default: Line Breaks

- [x] Extension: `hard_line_breaks` - Newline = `<br>` (non-default)
- [ ] Extension: `ignore_line_breaks` - Ignore single newlines (non-default)
- [x] Extension: `east_asian_line_breaks` - Smart line breaks for CJK
  (non-default)

#### Non-Default: GitHub/CommonMark

- [x] Extension: `alerts` - GitHub/Quarto alert/callout boxes (non-default)
- [x] Extension: `emoji` - `:emoji:` syntax (non-default)
- [x] Extension: `wikilinks_title_after_pipe` - `[[url|title]]` (opt-in; no
  flavor default)
- [x] Extension: `wikilinks_title_before_pipe` - `[[title|url]]` (opt-in; no
  flavor default)

#### Non-Default: Quarto-Specific

- [x] Quarto executable code cells with output
- [x] Quarto cross-references `@fig-id`, `@tbl-id`

#### Non-Default: RMarkdown-Specific

- [x] RMarkdown code chunks with output
- [x] Bookdown-style references (`\@ref(fig-id)`, etc.)

#### Non-Default: Other

- [ ] Extension: `abbreviations` - Abbreviation definitions (non-default)
- [ ] Extension: `attributes` - Universal attribute syntax (non-default,
  commonmark only)
- [ ] Extension: `gutenberg` - Project Gutenberg conventions (non-default)
- [ ] Extension: `markdown_attribute` - `markdown="1"` in HTML (non-default)
- [ ] Extension: `old_dashes` - Old-style em/en dash parsing (non-default)
- [ ] Extension: `rebase_relative_paths` - Rebase relative paths (non-default)
- [ ] Extension: `short_subsuperscripts` - MultiMarkdown `x^2` style
  (non-default)
- [ ] Extension: `sourcepos` - Include source position info (non-default)
- [ ] Extension: `space_in_atx_header` - Allow no space after `#` (non-default)
- [x] Extension: `spaced_reference_links` - Allow space in `[ref] [def]`
  (non-default)

### Won't Implement

- Format-specific output conventions (e.g., `gutenberg` for plain text output)

### Quarto Shortcodes

- [x] Parser support for `{{< name args >}}` syntax

- [x] Parser support for `{{{< name args >}}}` escape syntax

- [x] Formatter with normalized spacing

- [x] Extension flag `quarto_shortcodes` (enabled for Quarto flavor)

- [x] Golden test coverage

- [x] LSP diagnostics for malformed shortcodes

- [x] Completion for built-in shortcode names

## Additional Markdown flavors

### mdsvex / Svelte-flavored Markdown

MVP support for [mdsvex](https://mdsvex.pngwn.io) (`.svx`, `.svelte.md`). mdsvex
(≤0.12.x) builds on `remark-parse@8`, whose options default to `gfm: true`, so
tables, strikethrough, bare autolinks, and task lists work with **no plugins**
(confirmed by the getting-started example and real plugin-free
`svelte.config.js` setups; `remark-gfm` is only for modern remark). So
`Flavor::Mdsvex` is a CommonMark-*dialect* flavor with the GFM extension set +
`raw_html` + `yaml_metadata_block` + `svelte-template`, minus the extras mdsvex
does not enable by default (footnotes, math, emoji, alerts). The `{...}`
attribute "collision" with Pandoc syntax evaporates because the CommonMark
dialect leaves every attribute extension (`header_attributes`,
`bracketed_spans`, `fenced_divs`, `raw_attribute`) off, so `{` is free for
Svelte. `svelte-template` is off for every other flavor (zero behavior change
elsewhere).

- [x] MVP: `Flavor::Mdsvex` + `svelte-template` extension; `.svx`/`.svelte.md`
  detection; CLI/WASM/schema surfaces.
- [x] Opaque, sigil-distinguished inline spans (`SVELTE_BLOCK_LOGIC` for
  `{#…}`/`{:…}`/`{/…}`, `SVELTE_TAG` for `{@…}`, `SVELTE_EXPRESSION` for
  `{expr}`), content preserved verbatim. Balanced-brace scan reused from the
  shortcode parser. Parser golden + formatter golden + unit tests landed.
- [x] **Tier 2: block-level `{#if}`/`{#each}` pairing.** Standalone Svelte spans
  (block logic `{#if}`/`{:else}`/`{/each}`, tags `{@html}`, and expressions
  `{expr}`) that occupy a whole line are now emitted as an opaque
  `SVELTE_BLOCK` leaf block (mirroring the MyST leaf-block pattern) that
  acts as a block boundary. This fixes the prior quirk where a lone-span
  paragraph adjacent to a *tight* list (no blank line) got joined onto one
  line and its inner whitespace collapsed. The equivalent quirk for Quarto
  shortcode lines (`{{< ... >}}`) is still a separate pre-existing issue and
  is not addressed here.
- [ ] **Tier 3: format the JS/Svelte inside spans** (prettier-plugin-svelte
  territory). Likely out of scope.
- [ ] String-literal-aware brace matching: a `}` inside a JS string (`{ "}" }`)
  can terminate a span early (depth-counting only). Lossless fallback
  (literal `{`), but a real Svelte tokenizer would fix it.
- [ ] AST wrappers (`syntax/svelte.rs`), LSP semantic tokens, and lint rules for
  Svelte constructs.

### MyST

MyST (`mystmd.org`, `myst-parser`) support, modeled the same way as mdsvex: a
CommonMark-*dialect* flavor whose `myst_defaults` enables MyST-specific
extensions (`myst-directives`, `myst-roles`, `myst-targets`, `myst-comments`,
`myst-block-breaks`) plus the GFM-superset rules `myst-parser` turns on
(`pipe-tables`, `footnotes`, `yaml-metadata-block`). Behavior is gated on those
extension flags, never on `Flavor::Myst` directly, so other flavors can borrow
the same shapes. Markup extras (`myst-colon-fence`, `myst-substitutions`,
dollar-math, deflists, ...) stay opt-in.

- [x] **AST wrappers (`syntax/myst.rs`).** Typed wrappers over the existing
  `MYST_*` CST kinds, wired through `syntax.rs`, each with cast-from-`parse`
  unit tests (follow the `syntax/shortcodes.rs` pattern). Landed:
  - `MystTarget` (`label()`) --- the anchor side of MyST's cross-reference
    graph; keystone for goto-def/rename/undefined-target lint.
  - `MystRole` (`name()` brace-stripped, `content()`) --- the reference side
    (`` {ref}`label` ``); pairs with `MystTarget` for reference resolution.
  - `MystDirective` (`name()`, `argument()`, `options()` over
    `MystDirectiveOption` `name()`/`value()`, `body()`) --- richest construct;
    unlocks the most lint rules.
  - `MystSubstitution` (`name()`, trimmed) --- enables the "key not defined in
    frontmatter `substitutions:`" lint.
  - Skipped `MystComment`/`MystBlockBreak` wrappers (no name/label semantics to
    expose yet); add when a rule needs them.
- [ ] **LSP semantic tokens for MyST.** Wrapper-driven classification of
  directive/role names, target labels, and substitution names. Depends on
  the AST wrappers.
- [ ] **Lint rules for MyST constructs.** Gate on the `myst-*` extension flags
  (never `Flavor::Myst` directly), via the `add-lint-rule` skill. Start with
  `undefined-references` (role target resolves to a `MystTarget`) and an
  unknown-directive/role check. Depends on the AST wrappers.

## Math Parser and Formatter

Multi-session effort --- see the `math-parser-formatter` skill
(`.claude/skills/math-parser-formatter/`) for the phased roadmap, locked-in
design decisions, and per-session workflow. Parser invariants:
`.claude/rules/math-parser.md`.

- [x] Math parser producing a lossless structural TeX CST for inline and display
  math (`MATH_CONTENT` subtree; groups, environments, commands, alignment,
  scripts, comments, and `\left`/`\right` delimiter pairs). Landed in
  `crates/panache-parser/src/parser/math.rs`.
- [x] Surface math diagnostics (unclosed/mismatched braces and environments,
  unbalanced `\left`/`\right`) through the linter and LSP. Landed as the
  always-on `math-syntax` lint rule (`src/linter/rules/math_content.rs`),
  surfaced via the registry to CLI + LSP. All diagnostics derive from the
  embedded `MATH_CONTENT` CST shape via the single shared
  `syntax::math_diagnostics` (no re-parse, no side-channel; also consumed by
  the formatter to leave malformed math verbatim); spans are the offending
  tokens' host ranges.
- [ ] Migrate the math formatter's `\left`/`\right` line-break tracking to the
  `MATH_DELIMITED` node. The break-candidate scan
  (`crates/panache-formatter/src/formatter/math/linebreak.rs`) and
  `command_class` (`operators.rs`) still track delimiter depth by command
  *text* (`name == "left"`/`"right"`), which is now partly redundant with
  the structural node. Harmless as a fallback today (formatter goldens are
  byte-identical), but node-awareness would let the scan treat a delimited
  run as one opaque operand instead of re-deriving depth.
- [x] Math formatter that reformats content semantics-safely (align `&` columns,
  indent environment bodies, normalize `\\`) while preserving idempotency
  (`format(format(math)) == format(math)`), behind an experimental gate.
  Landed as `[experimental] format-math` (default off) routing
  `$$`/`$`/`\[`/`\(` math content through
  `crates/panache-formatter/src/formatter/math/`. Standalone `\begin{env}`
  TeX blocks stay opaque (parser keeps them as `TEX_BLOCK`) --- a possible
  follow-up.
