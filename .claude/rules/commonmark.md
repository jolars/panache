---
paths:
  - "crates/panache-parser/tests/commonmark.rs"
  - "crates/panache-parser/tests/commonmark/**"
  - "crates/panache-parser/tests/fixtures/commonmark-spec/**"
  - "crates/panache-parser/scripts/update-commonmark-spec-fixtures.sh"
  - "docs/development/commonmark-conformance.qmd"
  - "docs/development/commonmark-report.json"
---

CommonMark conformance harness changes must stay fixture-driven and
flavor-gated.

- Treat upstream `spec.txt` as the source of truth. Conformance is measured by
  HTML byte-equality after the shared `<li>` / `</li>` whitespace normalization
  — do not relax the comparison.
- The renderer at `tests/commonmark/html_renderer.rs` is **test code only**.
  Do not move it under `src/`, re-export it, or otherwise put it on the public
  API surface without explicit discussion. If a public `--to html` mode is
  wanted later, plan that as its own change.
- All conformance runs use `Flavor::CommonMark`. Do not add cross-flavor
  branches into the harness. GFM-specific or Pandoc-specific behavior belongs
  in flavor-specific golden cases under `tests/fixtures/cases/`, not here.
- Parser changes unlocked by conformance work must not regress Pandoc-markdown.
  CommonMark and Pandoc disagree on more constructs than they agree on
  (backtick run matching, emphasis flanking, raw HTML recognition, ...).
  Before landing a parser-side fix, **verify against pandoc**:
  ```
  pandoc <case>.md -f commonmark -t native    # CommonMark expected shape
  pandoc <case>.md -f markdown   -t native    # Pandoc expected shape
  ```
  If the two outputs differ, the fix belongs behind the `Dialect` switch on
  `ParserOptions` (`Dialect::CommonMark` vs `Dialect::Pandoc`) — *not* an
  unconditional change in `parser/inlines/**` or `parser/blocks/**`. Add
  paired parser fixtures (one per dialect) under `tests/fixtures/cases/`
  with `parser-options.toml` pinning the flavor. See
  `code_spans_unmatched_backtick_run_{commonmark,pandoc}` for the canonical
  shape.
- Per-feature toggles (one bit, narrow scope) still belong on `Extensions`.
  `Dialect` is reserved for *structural* parser-identity differences. When in
  doubt: if multiple flavors share the same value, it's probably a `Dialect`
  knob; if it tracks one named feature, it's an extension flag.
- Never add a number to `tests/commonmark/allowlist.txt` without first running
  `commonmark_full_report` and confirming it appears in the passing set. Group
  entries under section header comments matching the spec's section names.
- Parser fixes that unlock spec examples should land with a focused regression
  test in `crates/panache-parser/tests/fixtures/cases/` *before* the allowlist
  grows. The allowlist guards regressions; it is not where new behavior is
  asserted in detail.
- Keep `blocked.txt` reasons specific and actionable so future work can target
  concrete gaps. Do not use `blocked.txt` to silence regressions — that's the
  allowlist's job (by removing entries) plus a follow-up fix. The one
  exception: examples that were *passing-by-accident* under prior flavor
  defaults (e.g. a construct was disabled, parser fell through to plain text,
  and that happened to match the spec) and that now fail with a *more
  correct* output once the construct is enabled. These are not regressions;
  they expose pre-existing parser laxity that the prior defaults masked.
  They may be moved from the allowlist to `blocked.txt`, but each entry
  must be labeled "passing-by-accident under prior defaults" along with the
  concrete parser gap it now exposes. This exception does not cover genuine
  regressions from parser/renderer changes — those still require a fix, not
  a `blocked.txt` entry.
- `spec.txt` is pinned to the version recorded in
  `tests/fixtures/commonmark-spec/.panache-source`. Bumping the spec means
  re-running the full report and reviewing the diff intentionally; do not bump
  it as a side effect of other work.
- The `commonmark-hs/` checkout in the workspace root is read-only reference
  context. Do not vendor it as a dependency or alter its files.
- `report.txt` and `docs/development/commonmark-report.json` are generated
  artifacts. Regenerate them by running `commonmark_full_report`; do not
  hand-edit either file.
