---
applyTo: "src/lsp/**/*.rs,src/lsp.rs,tests/lsp/**/*.rs,tests/lsp.rs"
---

LSP changes must prioritize protocol correctness and stable document state
transitions.

- Preserve request/notification behavior across open/change/save/close flows.
- Ensure UTF-16/UTF-8 position and range conversions remain correct.
- Prefer typed syntax wrappers for feature logic (symbols, diagnostics,
  definitions, etc.).
- Keep state updates explicit; avoid silent failure paths that hide
  diagnostics/actions.
- Validate with targeted LSP integration tests before running the full suite.
