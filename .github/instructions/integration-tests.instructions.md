---
applyTo: "tests/**/*.rs,tests/fixtures/cases/**"
---

Integration tests should reflect user-visible behavior and minimize brittleness.

- Prefer focused assertions tied to one behavior change.
- Use fixtures under existing test directories rather than creating ad-hoc
  layouts.
- For new formatter golden scenarios (user-visible formatted output), add a new
  case directory under `tests/fixtures/cases/` and wire it into
  `tests/golden_cases.rs`.
- For parser-only golden scenarios (CST/losslessness/parser behavior), add a new
  case directory under `crates/panache-parser/tests/fixtures/cases/` and wire it
  into `crates/panache-parser/tests/golden_parser_cases.rs`.
- Update expected outputs only when behavior intentionally changed and after
  manual diff review.
- For CLI diagnostics, prefer focused substring/order assertions over asserting
  entire rendered blocks (to reduce brittleness across renderer/layout tweaks).
- Keep tests deterministic (no timing or environment-sensitive assumptions
  unless already established by the suite).
