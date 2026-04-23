---
paths:
  - "src/linter/**/*.rs"
  - "src/linter.rs"
  - "src/diagnostic_renderer.rs"
  - "tests/linting.rs"
  - "tests/cli/lint.rs"
  - "docs/guide/linting.qmd"
---

Linter changes should prioritize diagnostic correctness, actionable fixes, and
stable CLI/LSP behavior.

- Keep diagnostics precise: correct rule code, severity, and source range.
- Prefer adding or extending fix edits only when the replacement is reliable and
  preserves document intent.
- Maintain rustc-like diagnostic UX in CLI output (clear primary span, optional
  context, and concise help/note text).
- Keep lint docs in sync: update `docs/guide/linting.qmd` whenever lint rules,
  diagnostics, fix behavior, or CLI lint message format changes.
- Preserve LSP compatibility: CLI rendering changes must not alter
  `src/lsp/conversions.rs` behavior unless explicitly intended.
- Reuse shared orchestration paths (including Salsa-backed lint/diagnostic
  flows) between CLI and LSP where possible; avoid duplicating rule execution or
  diagnostic-mapping logic in parallel code paths.
- Add focused tests in `tests/linting.rs` and/or `tests/cli/lint.rs` for any
  user-visible lint behavior changes before full suite reruns.
