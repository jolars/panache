---
applyTo: "src/formatter/**/*.rs,src/formatter.rs,tests/cases/**/expected.md,tests/format/**/*.rs"
---

Formatter changes should preserve idempotency and avoid parser-side workarounds.

- Keep formatting deterministic: `format(format(x)) == format(x)`.
- Reuse existing wrapping/inline/list/table helpers instead of duplicating
  rendering logic.
- If behavior changes, add or update the smallest relevant golden case in
  `tests/cases/`.
- Keep docs aligned with formatter behavior: update `docs/formatting.qmd` when
  user-visible formatting rules or examples change.
- Avoid touching unrelated golden cases; verify diffs are intentional.
- Prefer targeted tests (`cargo test --test golden_cases <case_name>`) before
  full suite reruns.
