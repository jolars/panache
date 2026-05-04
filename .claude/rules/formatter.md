---
paths:
  - "src/formatter.rs"
  - "crates/panache-formatter/**/*.rs"
  - "crates/panache-formatter/tests/format/**/*.rs"
  - "tests/fixtures/cases/**/expected.md"
---

Formatter changes should preserve idempotency and avoid parser-side workarounds.

- **Idempotency divergence is not automatically a formatter bug.** When pass1
  and pass2 disagree, the *symptom* surfaces in formatter output but the
  *cause* is often a parser CST shape that diverges from pandoc-native.
  Before writing any formatter fix for an idempotency or pass-mismatch
  failure, run `pandoc <repro>.md -f markdown -t native` on a minimal
  reproducer and compare against panache's CST. If the structural shape
  differs (different block types, missing/extra nesting, wrong attribute
  attachment), the bug is parser-side — route to `pandoc-conformance-expand`
  rather than reaching for a formatter helper (looseness propagation,
  marker normalization, separator injection, etc.) that papers over the
  upstream divergence.
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
