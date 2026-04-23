---
paths:
  - "editors/code/**/*.ts"
  - "editors/code/package.json"
  - "editors/code/README.md"
---

VS Code extension changes should preserve reliable LSP startup and predictable
user configuration behavior.

- Keep extension settings in sync across implementation, `package.json`
  contributes schema, and `editors/code/README.md` documentation.
- Preserve activation behavior for supported languages/workspaces and avoid
  regressing startup reliability.
- Prefer reusing existing process/download/config helpers over duplicating
  command resolution or install logic.
- When changing server launch behavior, ensure `panache lsp` invocation and
  argument/environment wiring remain explicit and testable.
- Validate extension changes with `npm run compile` in `editors/code/`.
