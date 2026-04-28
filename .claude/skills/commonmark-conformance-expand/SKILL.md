---
name: commonmark-conformance-expand
description: Incrementally grow Panache's CommonMark spec.txt conformance by
  triaging one failing section (or a small group sharing a root cause) at a
  time, fixing the parser or test-only renderer, and growing the allowlist.
---

Use this skill when asked to push CommonMark conformance forward, fix a
specific failing spec example, or pick "the next best section" to work on.

## Scope boundaries

- Target is the conformance harness in
  `crates/panache-parser/tests/commonmark.rs` and the parser/renderer code
  it exercises under `Flavor::CommonMark`.
- This is a **long-horizon effort**. Each session moves the pass rate by a
  bounded amount; do not try to land sweeping rewrites in one go.
- Conformance work runs *only* under `Flavor::CommonMark`. Do not branch the
  harness on flavor — Pandoc-flavored or GFM-specific coverage belongs in
  golden cases under `tests/fixtures/cases/`, not here.
- The renderer at `crates/panache-parser/tests/commonmark/html_renderer.rs`
  is test-only. Do not promote it to a public API as part of this work.

## Key files

- `crates/panache-parser/tests/commonmark.rs` — runner. Two real tests:
  - `commonmark_allowlist` — regression guard against
    `tests/commonmark/allowlist.txt`.
  - `commonmark_full_report` (`#[ignore]`) — runs all 652 examples,
    writes `tests/commonmark/report.txt` and
    `docs/development/commonmark-report.json`.
- `crates/panache-parser/tests/commonmark/spec_parser.rs` — parses
  `spec.txt` into `SpecExample { number, section, markdown, expected_html }`.
  Rarely needs changes.
- `crates/panache-parser/tests/commonmark/html_renderer.rs` — test-only
  CST → HTML walker. Many divergences live here, not in the parser.
- `crates/panache-parser/tests/commonmark/allowlist.txt` — example numbers
  that must keep passing. Grouped by section header comments. Append-only
  in spirit; only remove an entry if you have a concrete reason and a
  follow-up plan.
- `crates/panache-parser/tests/commonmark/blocked.txt` — example numbers we
  intentionally do not target yet, with reasons. Not used to silence
  regressions.
- `crates/panache-parser/tests/fixtures/commonmark-spec/spec.txt` — vendored
  CommonMark spec. Do not edit directly; refresh via
  `scripts/update-commonmark-spec-fixtures.sh`.
- `crates/panache-parser/src/options.rs` — `Extensions::for_flavor()` is
  where flavor → extension defaults are resolved. Tightening the
  CommonMark flavor gate often happens here.
- `crates/panache-parser/src/parser/blocks/**`,
  `crates/panache-parser/src/parser/inlines/**` — where parser fixes land
  when the CST shape is wrong.

## Failure buckets

Every failing example is one of:

- **Renderer gap** — parser produces a sensible CST, but the test renderer
  doesn't emit the right HTML for it. Fix in `html_renderer.rs`.
- **Parser-shape gap** — parser CST shape doesn't match what the renderer
  needs (e.g. tokenization quirks, missing nested structure). Fix in
  `crates/panache-parser/src/parser/`.
- **Flavor leak** — a Pandoc-only behavior is firing under
  `Flavor::CommonMark` because an extension gate is missing or wrong.
  Fix by adding/tightening the gate in `parser/blocks/**` or
  `parser/inlines/**`, and verify `Extensions::for_flavor(CommonMark)`
  has the right defaults.
- **Dialect divergence** — the construct *parses differently* between
  Pandoc-markdown and CommonMark (not a single feature toggle, but a
  structural rule difference). Fix by branching on
  `config.dialect == Dialect::CommonMark` in the parser. Examples: backtick
  run matching, emphasis flanking edge cases, raw HTML recognition.
- **Genuine missing feature** — CommonMark construct not currently
  modeled. Less common; usually the largest fix.

### How to tell flavor leak from dialect divergence

A flavor leak means *Pandoc-flavored markdown* would also produce the
"wrong" output if the relevant extension were off. A dialect divergence
means even a fully-extensions-on Pandoc-markdown parse disagrees with
CommonMark on the construct.

Before classifying, verify with pandoc itself (assume it's available):

```
printf '<markdown>' > /tmp/probe.md
pandoc /tmp/probe.md -f commonmark -t native
pandoc /tmp/probe.md -f markdown   -t native
pandoc /tmp/probe.md -f gfm        -t native       # if GFM-relevant
```

- Outputs *agree* → not a flavor or dialect issue; it's a renderer or
  parser-shape gap that affects every flavor.
- Outputs *differ between commonmark/gfm and markdown* → **dialect
  divergence**. Gate on `Dialect`.
- Output for `markdown` matches CommonMark only when an extension is
  toggled → **flavor leak**, fix the extension default in
  `Extensions::for_flavor(...)`.

Reference for which extensions Pandoc itself ships under each flavor:
`pandoc/src/Text/Pandoc/Extensions.hs` (read-only checkout in the
workspace root). When designing a new `Extensions` flag default, check
that file for `getDefaultExtensions Markdown / CommonMark / GFM` to keep
panache aligned.

## Workflow

1. **Regenerate the report**:
   ```
   cargo test -p panache-parser --test commonmark commonmark_full_report \
     -- --ignored --nocapture
   ```
   Then look at `crates/panache-parser/tests/commonmark/report.txt` for
   per-section counts.

2. **Pick a section** — prefer high leverage, low risk:
   - Sections with many failures and a likely shared root cause
     (e.g. "Code spans 0/22" — backtick handling probably explains all of
     them) beat picking off one-offs.
   - Sections where the failures are spread across unrelated bugs are
     poor first targets.

3. **Probe one example.** Read the markdown and expected HTML for the
   smallest failing example in the section directly from
   `crates/panache-parser/tests/fixtures/commonmark-spec/spec.txt`. Then
   inspect what panache produces:
   ```
   printf '<markdown here>' | cargo run -- parse        # CST
   ```
   Compare the rendered output to the expected HTML by hand or with a
   throwaway `#[ignore]` test inside `commonmark.rs` that prints both
   sides. Remove the probe before finishing.

4. **Classify the fix** before editing — and **verify with pandoc** when
   the construct could plausibly differ between dialects:
   ```
   pandoc <case>.md -f commonmark -t native
   pandoc <case>.md -f markdown   -t native
   ```
   If the two disagree, the change is a dialect divergence, not a free
   parser fix. Then:
   - **Renderer gap**: edit `html_renderer.rs`. Keep changes narrow to the
     constructs the failing examples actually exercise.
   - **Parser-shape gap**: add a focused parser regression test under
     `crates/panache-parser/tests/fixtures/cases/<descriptive-name>/`
     (parser golden) that pins the desired CST shape, then make it pass.
     The conformance harness is *not* where parser invariants are
     authored.
   - **Flavor leak**: confirm by checking the flag in
     `Extensions::for_flavor(Flavor::CommonMark)`. Tighten the gate at
     the parser site that consults the flag.
   - **Dialect divergence**: gate the parser branch on
     `config.dialect == Dialect::CommonMark` (see
     `crates/panache-parser/src/options.rs`). Add **paired parser
     fixtures** — one with `parser-options.toml` set to
     `flavor = "commonmark"`, one set to `flavor = "pandoc"` — both
     pinning the dialect-specific CST shape. Pattern reference:
     `code_spans_unmatched_backtick_run_{commonmark,pandoc}`.
   - **Missing feature**: scope it carefully; if it's large, file it as
     follow-up rather than landing it in a conformance session.

5. **Apply the smallest focused change**, keeping the parser
   CST-lossless and parser/formatter policy separate (per
   `.claude/rules/parser.md`).

6. **Regenerate the report** and inspect which examples flipped to
   passing. A single root-cause fix often unlocks several.

7. **Allowlist the cleanly unlocked examples** in
   `tests/commonmark/allowlist.txt`. Group new entries under their
   section header comment. Do not allowlist examples whose pass status
   is fragile or dependent on side behavior — only crisp wins.

8. **Validate**:
   - `cargo test -p panache-parser --test commonmark commonmark_allowlist`
   - `cargo test -p panache-parser` (full parser-crate suite — catches
     parser regressions in goldens, snapshot, etc.)
   - `cargo clippy -p panache-parser --all-targets -- -D warnings`
   - `cargo fmt -p panache-parser -- --check`
   - Re-run `commonmark_full_report` so `report.txt` and
     `docs/development/commonmark-report.json` reflect the new state.

## Dos and don'ts

- **Do** prefer fixes that unlock multiple examples sharing a root cause.
- **Do** add a focused parser regression test before changing parser
  behavior.
- **Do** keep allowlist additions grouped under their CommonMark section
  header for readability.
- **Don't** add an example to the allowlist without verifying it appears
  in the latest `report.txt` passing set.
- **Don't** broaden the renderer to handle Pandoc-flavored constructs;
  this harness is `Flavor::CommonMark` only.
- **Don't** silence a regression by removing an allowlist entry — fix the
  underlying cause, or open a follow-up and document the gap in
  `blocked.txt`.
- **Don't** edit `report.txt` or `docs/development/commonmark-report.json`
  by hand. They are derived.
- **Don't** bump `spec.txt` to a new spec version as a side effect of a
  conformance session — that's its own intentional change.

## Report-back format

When done, report:

1. Pass count before and after (e.g. "66 → 81 / 652").
2. The section(s) targeted and the shared root cause behind the wins.
3. Files changed, classified by bucket (renderer / parser-shape / flavor
   leak / missing feature).
4. Examples unlocked but not yet allowlisted (candidates for follow-up,
   with the reason they were left off).
5. Suggested next targets grouped by likely shared root cause.
