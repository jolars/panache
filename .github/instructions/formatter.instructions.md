---
applyTo: "src/formatter/**/*.rs,src/formatter.rs,crates/panache-formatter/**/*.rs,tests/fixtures/cases/**/expected.md,tests/format/**/*.rs"
---

Formatter changes should preserve idempotency and avoid parser-side workarounds.

- Keep formatting deterministic: `format(format(x)) == format(x)`.
- Reuse existing wrapping/inline/list/table helpers instead of duplicating
  rendering logic.
- Keep formatter-core logic in `crates/panache-formatter`; keep host-specific
  process/runtime integrations in top-level `src/formatter.rs` and related host
  modules.
- Keep `crates/panache-formatter` dependency-lean (parser-style): avoid pulling
  config file parsing or LSP-only dependencies/features into the formatter core.
- If behavior changes, add or update the smallest relevant golden case in
  `tests/fixtures/cases/`.
- Keep docs aligned with formatter behavior: update `docs/guide/formatting.qmd`
  when user-visible formatting rules or examples change.
- Avoid touching unrelated golden cases; verify diffs are intentional.
- Prefer targeted tests (`cargo test --test golden_cases <case_name>`) before
  full suite reruns.
