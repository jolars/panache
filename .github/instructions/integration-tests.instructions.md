---
applyTo: "tests/**/*.rs,tests/cases/**"
---

Integration tests should reflect user-visible behavior and minimize brittleness.

- Prefer focused assertions tied to one behavior change.
- Use fixtures under existing test directories rather than creating ad-hoc
  layouts.
- For new golden scenarios, add a new case directory and wire it into
  `tests/golden_cases.rs`.
- Update expected outputs/CST only when behavior intentionally changed and after
  manual diff review.
- Keep tests deterministic (no timing or environment-sensitive assumptions
  unless already established by the suite).
