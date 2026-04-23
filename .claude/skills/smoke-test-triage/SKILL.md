---
name: smoke-test-triage
description: Triage and fix panache smoke-test regressions (idempotency,
  losslessness, parse/format checks) from CI debug-format reports and linked
  issues.
---

Use this skill when asked to investigate failures reported by smoke-test scans
or `debug format` CI issues (especially idempotency/losslessness regressions).

## Goals

1. Reproduce the exact failure from the report.
2. Minimize to a stable local fixture.
3. Add regression coverage in the right test surface.
4. Fix root cause (not symptom).
5. Validate targeted cases, then full repository checks.

## Triage workflow

1. Read the issue/report details first:
   - failing check type (`idempotency`, `losslessness`, parse, etc.)
   - sample file path
   - upstream repo + commit SHA
   - panache commit/version used by scan
   - diff excerpt and approximate line

2. Reproduce in a local clone of the target repository:
   - checkout the exact target commit from the report
   - run:
     - `panache debug format --checks all --report <sample-file>`
   - if needed, collect pass artifacts with:
     - `panache debug format --checks all --dump-dir <dir> --dump-passes <file>`

3. Minimize:
   - reduce to smallest snippet that still reproduces
   - keep source syntax realistic (especially chunk/hashpipe/YAML edge cases)
   - confirm reproduction is deterministic across repeated runs

4. Classify the failure before fixing:
   - First determine whether this is primarily a parser issue or formatter
     issue.
   - Quick heuristic:
     - parser issue: losslessness mismatch, CST/marker/trivia dropped, parse
       shape changes, syntax not captured correctly.
     - formatter issue: idempotency drift, wrapping/whitespace churn,
       hashpipe/YAML reflow instability, output normalization changing across
       passes.
   - Use debug artifacts (`--dump-passes`) to compare:
     - input vs parsed output (parser/losslessness boundary)
     - first format vs second format (formatter/idempotency boundary)
   - If uncertain, state the best hypothesis and why before implementing.

5. Add regression fixture(s):
   - Formatter user-visible cases:
     - `tests/fixtures/cases/<case-name>/input.{md,qmd,Rmd}`
     - `tests/fixtures/cases/<case-name>/expected.{md,qmd,Rmd}`
     - add case name to `tests/golden_cases.rs`
   - Parser-only behavior:
     - add parser golden case under
       `crates/panache-parser/tests/fixtures/cases/<case-name>/`
     - wire in parser golden runner

6. Fix implementation at root cause:
   - parser lossless/CST bugs -> parser crate
   - formatting/idempotency bugs -> formatter crate
   - avoid papering over by changing expected outputs only
   - preserve existing behavior for unrelated fixtures

7. Validate:
   - targeted first:
     - `cargo test --test golden_cases <case-name>`
     - `panache debug format --checks all --report <fixture-input>`
   - then full validation:
     - `cargo check --workspace`
     - `cargo test --workspace`
     - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
     - `cargo fmt -- --check`

## Panache-specific guidance

- For `.qmd` smoke regressions, ensure tests run with correct flavor context
  (Quarto detection matters for hashpipe/chunk behavior).
- Prefer adding one focused regression fixture per bug.
- Do not update unrelated golden fixtures.
- Be careful with trailing whitespace and wrapping in hashpipe/YAML paths: these
  are common idempotency triggers.
- Use Pandoc behavior as reference for ambiguous syntax.

## Report-back format

When done, report:

1. Whether the issue reproduced (and exact command).
2. Minimal reproducer summary.
3. Fixture(s) added/updated.
4. Root cause and code path changed.
5. Validation commands run and outcomes.
