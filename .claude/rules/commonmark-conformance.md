---
paths:
  - "crates/panache-parser/tests/commonmark.rs"
  - "crates/panache-parser/tests/commonmark/**"
  - "crates/panache-parser/tests/fixtures/commonmark-spec/**"
  - "docs/development/commonmark-report.json"
---

CommonMark conformance work — failure-bucket triage and workflow. Pair
with `.claude/rules/commonmark.md`, which covers the invariants this
work must not break.

## Scope

- Target is the conformance harness in
  `crates/panache-parser/tests/commonmark.rs` and the parser/renderer
  code it exercises under `Flavor::CommonMark`.
- This is a **long-horizon effort**. Each session moves the pass rate
  by a bounded amount; do not try to land sweeping rewrites in one go.
- Conformance work runs *only* under `Flavor::CommonMark`. Do not
  branch the harness on flavor — Pandoc-flavored or GFM-specific
  coverage belongs in golden cases under `tests/fixtures/cases/`, not
  here.

## Key files

- `crates/panache-parser/tests/commonmark.rs` — runner. Two real tests:
  - `commonmark_allowlist` — regression guard against
    `tests/commonmark/allowlist.txt`.
  - `commonmark_full_report` (`#[ignore]`) — runs all 652 examples,
    writes `tests/commonmark/report.txt` and
    `docs/development/commonmark-report.json`.
- `crates/panache-parser/tests/commonmark/spec_parser.rs` — parses
  `spec.txt` into `SpecExample { number, section, markdown,
  expected_html }`. Rarely needs changes.
- `crates/panache-parser/tests/commonmark/html_renderer.rs` —
  test-only CST → HTML walker. Many divergences live here, not in the
  parser.
- `crates/panache-parser/tests/commonmark/allowlist.txt` — example
  numbers that must keep passing. Grouped by section header comments.
  Append-only in spirit; only remove an entry if you have a concrete
  reason and a follow-up plan.
- `crates/panache-parser/tests/commonmark/blocked.txt` — example
  numbers we intentionally do not target yet, with reasons. Not used
  to silence regressions.
- `crates/panache-parser/tests/fixtures/commonmark-spec/spec.txt` —
  vendored CommonMark spec. Do not edit directly; refresh via
  `scripts/update-commonmark-spec-fixtures.sh`.
- `crates/panache-parser/src/options.rs` — `Extensions::for_flavor()`
  is where flavor → extension defaults are resolved. Tightening the
  CommonMark flavor gate often happens here.
- `crates/panache-parser/src/parser/blocks/**`,
  `crates/panache-parser/src/parser/inlines/**` — where parser fixes
  land when the CST shape is wrong.

## Failure buckets

Every failing example is one of:

- **Renderer gap** — parser produces a sensible CST, but the test
  renderer doesn't emit the right HTML for it. Fix in
  `html_renderer.rs`.
- **Parser-shape gap** — parser CST shape doesn't match what the
  renderer needs (e.g. tokenization quirks, missing nested structure).
  Fix in `crates/panache-parser/src/parser/`.
- **Flavor leak** — a Pandoc-only behavior is firing under
  `Flavor::CommonMark` because an extension gate is missing or wrong.
  Fix by adding/tightening the gate in `parser/blocks/**` or
  `parser/inlines/**`, and verify
  `Extensions::for_flavor(CommonMark)` has the right defaults.
- **Dialect divergence** — the construct *parses differently* between
  Pandoc-markdown and CommonMark (not a single feature toggle, but a
  structural rule difference). Fix by branching on
  `config.dialect == Dialect::CommonMark` in the parser. Examples:
  backtick run matching, emphasis flanking edge cases, raw HTML
  recognition.
- **Genuine missing feature** — CommonMark construct not currently
  modeled. Less common; usually the largest fix.

### How to tell flavor leak from dialect divergence

A flavor leak means *Pandoc-flavored markdown* would also produce the
"wrong" output if the relevant extension were off. A dialect
divergence means even a fully-extensions-on Pandoc-markdown parse
disagrees with CommonMark on the construct.

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
workspace root). When designing a new `Extensions` flag default,
check that file for `getDefaultExtensions Markdown / CommonMark / GFM`
to keep panache aligned.

## Workflow

1. **Regenerate the report**:
   ```
   cargo test -p panache-parser --test commonmark commonmark_full_report \
     -- --ignored --nocapture
   ```
   Then look at `crates/panache-parser/tests/commonmark/report.txt`
   for per-section counts.

2. **Pick a section** — prefer high leverage, low risk:
   - Sections with many failures and a likely shared root cause
     (e.g. "Code spans 0/22" — backtick handling probably explains
     all of them) beat picking off one-offs.
   - Sections where the failures are spread across unrelated bugs are
     poor first targets.

3. **Probe one example.** Read the markdown and expected HTML for the
   smallest failing example in the section directly from
   `crates/panache-parser/tests/fixtures/commonmark-spec/spec.txt`.
   Then inspect what panache produces:
   ```
   printf '<markdown here>' | cargo run -- parse        # CST
   ```

   For triage across several failing example numbers at once, the
   load-bearing tool is a throwaway `#[ignore]`-d `probe_examples`
   test inside `commonmark.rs`. It prints markdown / expected HTML /
   actual rendered HTML / match-status side-by-side so you can
   classify the failure bucket in seconds. Drop it in temporarily,
   edit the example numbers as you triage, then **delete it before
   finishing** — it is not a permanent fixture. Template:

   ```rust
   #[test]
   #[ignore = "probe specific examples"]
   fn probe_examples() {
       let examples = read_spec(&manifest_path(SPEC_FIXTURE_REL));
       let by_number: std::collections::HashMap<u32, &SpecExample> =
           examples.iter().map(|e| (e.number, e)).collect();
       for n in [/* failing example numbers */] {
           let example = by_number[&n];
           let rendered = render_example(example);
           eprintln!("=== #{n} ({}) ===", example.section);
           eprintln!("MD: {:?}", example.markdown);
           eprintln!("EXPECTED:\n{}", example.expected_html);
           eprintln!("GOT:\n{}", rendered);
           eprintln!("MATCH: {}", matches_expected(example, &rendered));
       }
   }
   ```

   Run with `cargo test -p panache-parser --test commonmark
   probe_examples -- --ignored --nocapture`.

4. **Classify the fix** before editing — and **verify with pandoc**
   when the construct could plausibly differ between dialects:
   ```
   pandoc <case>.md -f commonmark -t native
   pandoc <case>.md -f markdown   -t native
   ```
   If the two disagree, the change is a dialect divergence, not a
   free parser fix. Then:
   - **Renderer gap**: edit `html_renderer.rs`. Keep changes narrow
     to the constructs the failing examples actually exercise.
   - **Parser-shape gap**: add a focused parser regression test under
     `crates/panache-parser/tests/fixtures/cases/<descriptive-name>/`
     (parser golden) that pins the desired CST shape, then make it
     pass. The conformance harness is *not* where parser invariants
     are authored. **Fixture-first is non-negotiable** — land the
     fixture before the allowlist grows. The allowlist guards
     regressions; it is not where new parsing behavior is asserted in
     detail.
   - **Renderer-only gap**: a pure renderer fix (e.g. emitting
     `<br />`, trimming heading whitespace) does not need a parser
     fixture if the CST shape it consumes is already pinned by an
     existing parser golden. If the renderer fix is leaning on a CST
     shape that has no parser golden yet, add one before allowlisting
     — otherwise the "invariant" lives only in `html_renderer.rs` and
     rots silently.
   - **Flavor leak**: confirm by checking the flag in
     `Extensions::for_flavor(Flavor::CommonMark)`. Tighten the gate
     at the parser site that consults the flag.
   - **Dialect divergence**: gate the parser branch on
     `config.dialect == Dialect::CommonMark` (see
     `crates/panache-parser/src/options.rs`). Add **paired parser
     fixtures** — one with `parser-options.toml` set to
     `flavor = "commonmark"`, one set to `flavor = "pandoc"` — both
     pinning the dialect-specific CST shape. Pattern reference:
     `code_spans_unmatched_backtick_run_{commonmark,pandoc}`.
   - **Missing feature**: scope it carefully; if it's large, file it
     as follow-up rather than landing it in a conformance session.

   **Formatter golden case (when needed)** — if the parser change
   produces a *new structural shape* under CommonMark (e.g. paragraph
   + thematic break + paragraph where there was previously a single
   paragraph), also add one formatter golden case under
   `tests/fixtures/cases/` (top-level, not the parser-crate one) with
   `panache.toml` setting `flavor = "commonmark"`. The formatter
   fixture pins the formatted output and exercises idempotency, which
   is how non-obvious round-trip bugs surface (the formatter's HR
   style colliding with setext underlines is the canonical example).
   **Skip the paired Pandoc formatter case** — the existing top-level
   fixture suite already covers Pandoc-default behavior, and adding
   duplicates is churn. Only add a CommonMark formatter case when the
   new CommonMark behavior produces a different block sequence than
   the Pandoc path; if both dialects format identically, the parser
   fixture alone is sufficient.

   Reference:
   `tests/fixtures/cases/thematic_break_interrupts_paragraph_commonmark/`.

   Note on `panache.toml` flavor strings: the formatter config uses
   serde's kebab-case rename. `Flavor::CommonMark` accepts both
   `"commonmark"` and `"common-mark"` (alias). Other variants follow
   kebab-case strictly (`"rmarkdown"`, `"multimarkdown"`, `"gfm"`,
   `"pandoc"`, `"quarto"`).

5. **Apply the smallest focused change**, keeping the parser
   CST-lossless and parser/formatter policy separate (per
   `.claude/rules/parser.md`).

6. **Regenerate the report** and inspect which examples flipped to
   passing. A single root-cause fix often unlocks several.

7. **Allowlist the cleanly unlocked examples** in
   `tests/commonmark/allowlist.txt`. Group new entries under their
   section header comment. Do not allowlist examples whose pass
   status is fragile or dependent on side behavior — only crisp wins.

   Before adding any number, **verify it against the just-regenerated
   `report.txt`** so a stale memory of "this was passing" can't drift
   into the allowlist. Quick check:
   ```
   grep -E '^(N1|N2|N3)$' \
     crates/panache-parser/tests/commonmark/report.txt
   ```
   Each number must appear (twice — once in the flat passing list,
   once in its section block). If a number doesn't show up there, the
   allowlist guard would later flip red as a regression for the wrong
   reason; do not add it.

8. **Validate**:
   - `cargo test -p panache-parser --test commonmark commonmark_allowlist`
   - `cargo test -p panache-parser` (full parser-crate suite —
     catches parser regressions in goldens, snapshot, etc.)
   - `cargo clippy -p panache-parser --all-targets -- -D warnings`
   - `cargo fmt -p panache-parser -- --check`
   - Re-run `commonmark_full_report` so `report.txt` and
     `docs/development/commonmark-report.json` reflect the new state.

## Historical session knowledge

`.claude/conformance-archive/commonmark.md` preserves the rolling
recap from prior conformance sessions — accumulated "don't redo"
notes about load-bearing parser/renderer code (delimiter-stack
pass-ordering, exclusion bitmap, link-scanner skip flags, setext
list-item indent guard, etc.). Skim it before touching the inline IR
or the emphasis/link resolution paths.
