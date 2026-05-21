---
paths:
  - "src/config/formatter_presets.rs"
  - "docs/reference/_formatter-presets-details.qmd"
---

Built-in external formatter presets are defined in
`src/config/formatter_presets.rs`.

- Add a new preset to BOTH the `PRESETS` metadata array and the
  `formatter_preset_names()` list. The `preset_names_list_matches_metadata`
  unit test fails if the two drift apart.
- Preset names are a free-form string in user config, not a schema enum, so
  adding or renaming a preset does NOT require regenerating
  `panache.schema.json`.
- `docs/reference/_formatter-presets-details.qmd` is auto-generated from the
  `PRESETS` array by `build.rs` on every build — never hand-edit it.
- For tools that need a filename hint while reading stdin, put the `{}`
  placeholder in `args` (e.g. `--stdin-file-path {}`). In stdin mode it resolves
  to `stdin.{ext}` via `temp_file_extension_for_language`, so confirm each
  supported language has an extension mapping there.
- Add a focused test asserting the preset resolves for its language(s) via
  `formatter_presets_for_language(...)`, and verify the real tool's stdin/args
  behavior first (e.g. `nix run nixpkgs#<tool>`).
