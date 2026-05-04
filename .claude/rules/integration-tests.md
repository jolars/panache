---
paths:
  - "tests/**/*.rs"
  - "tests/fixtures/cases/**"
  - "crates/panache-formatter/tests/**/*.rs"
---

Integration tests should reflect user-visible behavior and minimize brittleness.

- Prefer focused assertions tied to one behavior change.
- Use fixtures under existing test directories rather than creating ad-hoc
  layouts.
- For new formatter golden scenarios (user-visible formatted output that
  exercises panache.toml config parsing end-to-end), add a new case directory
  under `tests/fixtures/cases/` and wire it into `tests/golden_cases.rs`. The
  goldens live in the top-level crate because their `panache.toml` fixtures
  use the host config schema (sub-tables like `[format]`, `[code-blocks]`,
  `[formatters.python]`) that the dependency-lean formatter Config doesn't
  parse on its own.
- For formatter unit tests that construct `Config` programmatically (no TOML
  parsing), add modules under `crates/panache-formatter/tests/format/` and
  wire them into `crates/panache-formatter/tests/format/main.rs`.
- For parser-only golden scenarios (CST/losslessness/parser behavior), add a new
  case directory under `crates/panache-parser/tests/fixtures/cases/` and wire it
  into `crates/panache-parser/tests/golden_parser_cases.rs`.
- Update expected outputs only when behavior intentionally changed and after
  manual diff review.
- For CLI diagnostics, prefer focused substring/order assertions over asserting
  entire rendered blocks (to reduce brittleness across renderer/layout tweaks).
- Keep tests deterministic (no timing or environment-sensitive assumptions
  unless already established by the suite).
