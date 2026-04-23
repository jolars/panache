---
paths:
  - "src/lsp/**/*.rs"
  - "src/lsp.rs"
  - "tests/lsp/**/*.rs"
  - "tests/lsp.rs"
---

LSP changes must prioritize protocol correctness and stable document state
transitions.

- Preserve request/notification behavior across open/change/save/close flows.
- Ensure UTF-16/UTF-8 position and range conversions remain correct.
- Prefer typed syntax wrappers for feature logic (symbols, diagnostics,
  definitions, etc.).
- Reuse shared conversion/state helpers (Salsa/document maps/range conversion)
  rather than re-implementing protocol mapping logic.
- Keep state updates explicit; avoid silent failure paths that hide
  diagnostics/actions.
- If lint/diagnostic payloads change, explicitly verify expected LSP diagnostic
  fields remain stable unless the protocol output change is intentional.
- Validate with targeted LSP integration tests before running the full suite.
- Keep the docs at `docs/guide/lsp.qmd` aligned with any user-visible behavior
  changes (e.g. new features, diagnostics, or client capabilities).
