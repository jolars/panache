---
applyTo: "src/parser/yaml.rs,src/parser/yaml/**/*.rs,src/parser/**/*.rs,src/syntax/**/*.rs,tests/**/*yaml*"
---

Panache has a planned long-term in-tree YAML parser initiative. Treat current
work as groundwork unless explicitly marked as a production rollout.

Timeline expectation: this is a multi-month effort. Do not propose or imply
short-term replacement of the existing `yaml_parser` pipeline.

- Keep all YAML parser changes lossless and CST-first: preserve markers,
  whitespace, comments, and scalar style trivia.
- Design for both plain YAML files and hashpipe-prefixed YAML so one parser core
  can serve frontmatter, chunk metadata, bibliography YAML, and workspace YAML
  files (for example `_quarto.yml` and metadata files).
- Prefer explicit host↔embedded range mapping APIs, even when many spans are
  identity mappings, so LSP position conversion remains deterministic.
- Do not replace the existing YAML pipeline directly. New behavior should be
  introduced in shadow/read-only mode first, with parity checks before any
  formatter/edit-path adoption.
- Keep parser policy separate from formatter policy: parser captures syntax;
  formatting and normalization stay in formatter layers.
- Plan for first-class YAML formatting support as a follow-up phase after parser
  parity and mapping correctness are validated.
- Add focused tests for new YAML behavior and range mapping rules before broad
  integration; keep tests deterministic and feature-gated where appropriate.
