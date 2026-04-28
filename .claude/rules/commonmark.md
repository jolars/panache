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
- Never add a number to `tests/commonmark/allowlist.txt` without first running
  `commonmark_full_report` and confirming it appears in the passing set. Group
  entries under section header comments matching the spec's section names.
- Parser fixes that unlock spec examples should land with a focused regression
  test in `crates/panache-parser/tests/fixtures/cases/` *before* the allowlist
  grows. The allowlist guards regressions; it is not where new behavior is
  asserted in detail.
- Keep `blocked.txt` reasons specific and actionable so future work can target
  concrete gaps. Do not use `blocked.txt` to silence regressions — that's the
  allowlist's job (by removing entries) plus a follow-up fix.
- `spec.txt` is pinned to the version recorded in
  `tests/fixtures/commonmark-spec/.panache-source`. Bumping the spec means
  re-running the full report and reviewing the diff intentionally; do not bump
  it as a side effect of other work.
- The `commonmark-hs/` checkout in the workspace root is read-only reference
  context. Do not vendor it as a dependency or alter its files.
- `report.txt` and `docs/development/commonmark-report.json` are generated
  artifacts. Regenerate them by running `commonmark_full_report`; do not
  hand-edit either file.
