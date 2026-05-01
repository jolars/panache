---
name: pandoc-conformance-expand
description: Incrementally grow Panache's Pandoc-native conformance by triaging
  one failing corpus case (or a small group sharing a root cause) at a time,
  fixing the parser or test-only native projector, expanding the corpus, and
  growing the allowlist.
---

Use this skill when asked to push Pandoc-flavor conformance forward, fix a
specific failing pandoc-conformance case, pick "the next best section" to
work on, or extend the corpus with a new construct.

## Scope boundaries

- Target is the conformance harness in
  `crates/panache-parser/tests/pandoc.rs` and the parser/projector code it
  exercises under `Flavor::Pandoc`.
- This is a **long-horizon effort**. Each session moves the pass rate by a
  bounded amount; do not try to land sweeping rewrites in one go.
- Conformance work runs *only* under `Flavor::Pandoc`. Do not branch the
  harness on flavor — Quarto/RMarkdown/GFM/MMD coverage belongs in golden
  cases under `tests/fixtures/cases/`, not here.
- The projector at
  `crates/panache-parser/tests/pandoc/native_projector.rs` is **test-only**.
  Do not promote it to a public API as part of this work. If a public
  `--to native` mode is wanted later, plan that as its own change.
- The parser-level inline IR migration is its own long-horizon effort
  (`pandoc-ir-migrate`). Conformance fixes that exercise the IR should
  prefer the IR path over the legacy delimiter stack.

## Related rules to read first

These project rules apply directly to this skill's work; read them before
starting if you haven't already loaded them this session. **Skip if your
session is purely projector-only** — the failure-bucket triage in step 4
of the workflow tells you which session this is, and projector-only work
doesn't intersect with parser/test-organization rules.

- `.claude/rules/parser.md` — `Dialect` vs `Extensions` split, dialect
  pandoc-verification requirement, CST losslessness, parser-policy
  separation from formatter, **pandoc-native is the behavioral reference**
  for Pandoc-dialect parsing changes. *Material when fixing parser-shape
  or flavor gaps.*
- `.claude/rules/commonmark.md` — read for context: CommonMark conformance
  must stay green every session here too. Pandoc-side fixes that
  silently regress CommonMark are not allowed. *Always-on guard, but only
  triggers when the fix touches shared CommonMark code.*
- `.claude/rules/integration-tests.md` — formatter golden cases live under
  top-level `tests/fixtures/cases/`; parser-only cases live under
  `crates/panache-parser/tests/fixtures/cases/`. Conformance corpus is
  separate from both — it is *not* where parser invariants are authored.
  *Material when adding fixtures (parser-shape and missing-feature
  buckets).*

## Harness noise to ignore inside this skill

The runtime occasionally injects a `system-reminder` nudging you to use
`TaskCreate` / `TaskUpdate` for tracking. **Inside this skill, the workflow
below is already linear (probe → classify → fix → corpus/fixture →
allowlist → recap), so task tools add overhead without value.** Skip them
for conformance sessions unless the user explicitly asks for a task list.

## Key files

- `crates/panache-parser/tests/pandoc.rs` — runner. Three real tests:
  - `corpus_loader_reads_seed_corpus` — sanity guard that the corpus is
    discoverable.
  - `pandoc_allowlist` — regression guard against
    `tests/pandoc/allowlist.txt`.
  - `pandoc_full_report` (`#[ignore]`) — runs every corpus case, writes
    `tests/pandoc/report.txt` and `docs/development/pandoc-report.json`.
- `crates/panache-parser/tests/pandoc/corpus_loader.rs` — reads the corpus
  directory into `Vec<PandocCase { id, slug, section, markdown,
  expected_native }>`. Section is derived from the slug prefix; rarely
  needs changes.
- `crates/panache-parser/tests/pandoc/native_projector.rs` — test-only
  CST → pandoc-native text projector. **Many divergences live here, not in
  the parser.** Use the `Unsupported "<KIND>"` sentinel rather than
  silently dropping nodes; the report makes the gap visible.
- `crates/panache-parser/tests/pandoc/allowlist.txt` — case IDs that must
  keep passing. Grouped by section header comments. Append-only in spirit;
  only remove an entry if you have a concrete reason and a follow-up plan.
- `crates/panache-parser/tests/pandoc/blocked.txt` — cases we intentionally
  do not target yet, with reasons. Not used to silence regressions.
- `crates/panache-parser/tests/fixtures/pandoc-conformance/corpus/<NNNN>-<section>-<slug>/`
  — corpus directory. Each case has `input.md` and `expected.native`
  (pinned `pandoc -f markdown -t native` output). `<section>` is the slug
  prefix used for grouping in the report (e.g. `inline`, `block`).
- `crates/panache-parser/tests/fixtures/pandoc-conformance/.panache-source`
  — pinned pandoc version + extraction date. **Do not edit by hand**;
  refresh via `scripts/update-pandoc-conformance-corpus.sh`.
- `crates/panache-parser/scripts/update-pandoc-conformance-corpus.sh` —
  re-runs the locally-installed pandoc on every `input.md` to regenerate
  `expected.native`. Only run this when **intentionally bumping** the
  pinned pandoc version — review the diff before committing.
- `crates/panache-parser/src/options.rs` — `Extensions::for_flavor()` is
  where flavor → extension defaults are resolved. Tightening the Pandoc
  flavor gate often happens in `pandoc_defaults()`.
- `crates/panache-parser/src/parser/blocks/**`,
  `crates/panache-parser/src/parser/inlines/**` — where parser fixes land
  when the CST shape is wrong.
- `pandoc/src/Text/Pandoc/...` — read-only pandoc source checkout in the
  workspace root. **The ground truth for any algorithm question, not just
  flavor defaults.** When pandoc-native output disagrees with your
  intuition, *read the parser*. High-leverage spots:
  - `Extensions.hs` (`getDefaultExtensions Markdown`) — flavor defaults.
  - `Readers/Markdown.hs` (`simpleTableHeader`, `multilineTableHeader`,
    `pipeTable`, `gridTable`, `alignType`, `bulletList`, `definition`,
    etc.) — block parsers; check here when CST shape decisions need to
    match pandoc.
  - `Parsing/GridTable.hs` (`fractionalColumnWidths`,
    `widthsFromIndices`) — grid/multiline column-width math.
  - `URI.hs` (`schemes`) — autolink scheme allowlist.
  - `data/abbreviations` — pandoc's bundled abbreviation list.
  Probing `pandoc -f markdown -t native` is faster for shape questions;
  reading the source is faster for *why* a shape is what it is.

## Failure buckets

Every failing case is one of:

- **Projector gap** — parser produces a sensible CST, but
  `native_projector.rs` doesn't emit the right native AST text for it
  (missing kind, attribute formatting wrong, coalescing rule incorrect).
  Fix in the projector. The most common bucket — the seed projector
  intentionally covers a narrow construct set.
- **Parser-shape gap** — parser CST shape doesn't match what the projector
  needs (e.g. tokenization quirks, missing nested structure, wrong
  attribute attachment). Fix in `crates/panache-parser/src/parser/`.
- **Flavor gap** — a `Flavor::Pandoc` extension default doesn't match what
  pandoc itself enables for `markdown` input. Cross-check
  `pandoc/src/Text/Pandoc/Extensions.hs` (`getDefaultExtensions Markdown`)
  and tighten `pandoc_defaults()` in
  `crates/panache-parser/src/options.rs`.
- **Genuine missing feature** — Pandoc construct not currently modeled
  (e.g. tables of a kind we don't parse yet, definition lists in some
  configurations, raw blocks). Less common; usually the largest fix.

There is **no analog of the CommonMark "dialect divergence" bucket** here.
Conformance runs under `Flavor::Pandoc` only and pandoc-native *is* the
spec — divergence between commonmark- and markdown-flavored pandoc is
already pre-resolved before the case is added.

## Verification with pandoc

The corpus already commits `expected.native` per case, so the comparison
runs offline. When triaging a failure, also probe pandoc directly to
confirm the expected shape and to spot any `expected.native` that drifted
from the pinned pandoc version:

```
pandoc <case>/input.md -f markdown -t native
```

If your local pandoc disagrees with the committed `expected.native`,
either (a) the committed file was generated against a different pandoc
version (re-pin via `scripts/update-pandoc-conformance-corpus.sh` if
intentional), or (b) the case's expected shape has been miscaptured —
fix the case before fixing the projector.

## Session recap (`RECAP.md`)

This skill keeps a rolling recap at
`.claude/skills/pandoc-conformance-expand/RECAP.md`. It is the handoff
between sessions — short, judgment-call-only, not a duplicate of
`report.txt`.

- **At the start of a session**: read `RECAP.md` first. The "Suggested
  next targets" section is the recommended starting point, and the "Don't
  redo" list keeps you from reinventing fixes that already landed. If the
  user named a specific target, prefer that, but still skim the recap so
  you don't redo prior work.
- **At the end of a session**: rewrite the **Latest session** entry in
  `RECAP.md` with: pass count before → after, the targets and shared root
  causes, files changed (classified by bucket), what *not* to redo, and
  ranked next targets. Keep it terse — a fresh session should be able to
  pick up from this entry plus `report.txt` without scrolling the prior
  conversation.
- Treat `RECAP.md` as the canonical handoff. Only echo a recap into the
  conversation if the user asks for one explicitly.

## Workflow

1. **Regenerate the report**:
   ```
   cargo test -p panache-parser --test pandoc pandoc_full_report \
     -- --ignored --nocapture
   ```
   Then look at `crates/panache-parser/tests/pandoc/report.txt` for
   per-section counts and the "Failing case slugs" section.

2. **Pick a target** — prefer high leverage, low risk:
   - Failing cases sharing a likely root cause (e.g. all
     definition-list cases failing → one projector or parser fix unlocks
     several) beat picking off one-offs.
   - A small expansion of the corpus to cover an unmodeled construct is
     a valid target on its own — adding cases is how the harness grows
     beyond the seed.

3. **Probe one case.** Read its `input.md` and `expected.native`. Then
   inspect what panache produces:
   ```
   cat <input.md> | cargo run -- parse        # CST
   ```

   For triage across several failing case ids at once, the load-bearing
   tool is a throwaway `#[ignore]`-d `probe_cases` test inside
   `tests/pandoc.rs`. It prints markdown / expected native / actual
   projected native / match-status side-by-side so you can classify the
   failure bucket in seconds. Drop it in temporarily, edit the case ids
   as you triage, then **delete it before finishing** — it is not a
   permanent fixture. Template:

   ```rust
   #[test]
   #[ignore = "probe specific cases"]
   fn probe_cases() {
       let cases = read_corpus(&manifest_path(CORPUS_REL));
       let by_id: std::collections::HashMap<u32, &PandocCase> =
           cases.iter().map(|c| (c.id, c)).collect();
       for n in [/* failing case ids */] {
           let case = by_id[&n];
           let rendered = render_case(case);
           eprintln!("=== #{n} ({}) ===", case.slug);
           eprintln!("MD: {:?}", case.markdown);
           eprintln!("EXPECTED:\n{}", normalize_native(&case.expected_native));
           eprintln!("GOT:\n{}", normalize_native(&rendered));
           eprintln!("MATCH: {}", matches_expected(case, &rendered));
       }
   }
   ```

   Run with `cargo test -p panache-parser --test pandoc probe_cases --
   --ignored --nocapture`.

4. **Classify the fix** before editing — and **verify with pandoc** when
   the construct could plausibly drift from the pinned `expected.native`:
   ```
   pandoc <case>.md -f markdown -t native
   ```
   If pandoc's output disagrees with the committed `expected.native`,
   the case file is stale (re-pin intentionally) — fix that first.
   Then:
   - **Projector gap**: edit `native_projector.rs`. Keep changes narrow
     to the constructs the failing cases actually exercise. Replace any
     `Inline::Unsupported(...)` / `Block::Unsupported(...)` paths the
     case needs, but do not add coverage broader than the corpus
     demands.
   - **Parser-shape gap**: add a focused parser regression test under
     `crates/panache-parser/tests/fixtures/cases/<descriptive-name>/`
     (parser golden) that pins the desired CST shape, then make it
     pass. The conformance harness is *not* where parser invariants are
     authored. **Fixture-first is non-negotiable** — land the parser
     fixture before the conformance allowlist grows. The allowlist
     guards regressions; it does not assert new parsing behavior in
     detail.
   - **Projector-only gap**: a pure projector fix does not need a parser
     fixture if the CST shape it consumes is already pinned by an
     existing parser golden. If the projector fix is leaning on a CST
     shape with no parser golden yet, add one before allowlisting —
     otherwise the "invariant" lives only in the projector and rots
     silently.
   - **Flavor gap**: confirm by checking the flag in
     `Extensions::for_flavor(Flavor::Pandoc)` and cross-referencing
     `pandoc/src/Text/Pandoc/Extensions.hs`. Tighten the default in
     `pandoc_defaults()` and add/adjust paired parser fixtures if the
     extension change shifts CST shape under either Pandoc-default or
     Pandoc-extensions-off.
   - **Missing feature**: scope it carefully; if it's large, add a
     `blocked.txt` entry with a concrete description of what's missing
     and file follow-up rather than landing it in a conformance session.

   **Formatter golden case (when needed)** — if the parser change
   produces a *new structural shape* under Pandoc, add one formatter
   golden case under `tests/fixtures/cases/` (top-level, not the
   parser-crate one) without `panache.toml` changes (Pandoc is the
   default flavor). The formatter fixture pins the formatted output and
   exercises idempotency, which is how non-obvious round-trip bugs
   surface. Only add a formatter case when the new behavior produces a
   different block sequence than before; if formatting is unchanged, the
   parser fixture alone is sufficient.

5. **Apply the smallest focused change**, keeping the parser
   CST-lossless and parser/formatter policy separate (per
   `.claude/rules/parser.md`).

6. **Regenerate the report** and inspect which cases flipped to
   passing. A single root-cause fix often unlocks several.

7. **Allowlist the cleanly unlocked cases** in
   `tests/pandoc/allowlist.txt`. Group new entries under their section
   header comment. Do not allowlist cases whose pass status is fragile
   or dependent on side behavior — only crisp wins.

   Before adding any number, **verify it against the just-regenerated
   `report.txt`** so a stale memory of "this was passing" can't drift
   into the allowlist. Quick check:
   ```
   grep -E '^(N1|N2|N3)$' \
     crates/panache-parser/tests/pandoc/report.txt
   ```
   Each number must appear in the passing list. If a number doesn't
   show up there, the allowlist guard would later flip red as a
   regression for the wrong reason; do not add it.

8. **Validate** — and the CommonMark allowlist must stay green:
   - `cargo test -p panache-parser --test pandoc pandoc_allowlist`
   - `cargo test -p panache-parser --test commonmark commonmark_allowlist`
     (must remain green — pandoc-side fixes that silently regress
     CommonMark are not allowed)
   - `cargo test -p panache-parser` (full parser-crate suite — catches
     parser regressions in goldens, snapshot, etc.)
   - `cargo clippy -p panache-parser --all-targets -- -D warnings`
   - `cargo fmt -p panache-parser -- --check`
   - Re-run `pandoc_full_report` so `report.txt` and
     `docs/development/pandoc-report.json` reflect the new state.

## Dos and don'ts

- **Do** prefer fixes that unlock multiple cases sharing a root cause.
- **Do** add a focused parser regression test before changing parser
  behavior.
- **Do** keep allowlist additions grouped under their section header for
  readability.
- **Do** add new corpus cases when an under-covered construct comes up —
  the seed is intentionally narrow. New cases go under
  `tests/fixtures/pandoc-conformance/corpus/<NNNN>-<section>-<slug>/`,
  with `expected.native` regenerated by hand via `pandoc -f markdown -t
  native input.md > expected.native`.
- **Don't** add a case to the allowlist without verifying it appears in
  the latest `report.txt` passing set.
- **Don't** broaden the projector to handle non-Pandoc-flavored
  constructs — this harness is `Flavor::Pandoc` only.
- **Don't** silence a regression by removing an allowlist entry — fix
  the underlying cause, or open a follow-up and document the gap in
  `blocked.txt`.
- **Don't** edit `expected.native` files by hand. They are the pinned
  pandoc output. Either regenerate via the script (intentional version
  bump) or fix the parser/projector to match.
- **Don't** edit `report.txt` or `docs/development/pandoc-report.json`
  by hand. They are derived.
- **Don't** bump the pinned pandoc version as a side effect of a
  conformance session — that's its own intentional change.

## Report-back format

When done, report:

1. Pass count before and after (e.g. "25 → 31 / 33").
2. The section(s) targeted and the shared root cause behind the wins.
3. Files changed, classified by bucket (projector / parser-shape /
   flavor / missing feature).
4. Cases unlocked but not yet allowlisted (candidates for follow-up,
   with the reason they were left off).
5. Suggested next targets grouped by likely shared root cause.
