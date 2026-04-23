---
paths:
  - "src/config.rs"
  - "docs/guide/configuration.qmd"
---

Configuration changes should preserve predictable defaults, compatibility, and
clear migration paths.

- Preserve config discovery precedence and failure behavior for explicit
  `--config` paths.
- Keep flavor/extension merging deterministic: start from flavor defaults, then
  apply user overrides.
- Maintain backward compatibility for deprecated keys/sections where currently
  supported; keep warnings explicit and actionable.
- Use canonical kebab-case keys while preserving documented aliases. Existing
  aliases to snake_case are only there for backwards compatibility and should
  not be used in new code.
- Update `docs/guide/configuration.qmd` whenever defaults, keys, or deprecation
  behavior changes.
- Add focused tests in `src/config.rs` for parsing, precedence, merge behavior,
  and deprecation handling when config behavior changes.
