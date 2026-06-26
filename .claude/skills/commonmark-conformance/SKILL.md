---
name: commonmark-conformance
description: Grow Panache's CommonMark spec conformance under
  `Flavor::CommonMark` by running every `spec.txt` example through the
  shared parser, comparing rendered HTML against the spec's expected
  HTML (byte-equality after `<li>` whitespace normalization), and
  triaging failures into renderer / parser-shape / flavor-leak /
  dialect-divergence / missing-feature buckets. Use when asked to
  advance the CommonMark pass rate, grow the allowlist, or unblock a
  CommonMark regression.
---

Panache's parser is shared across all flavors (Pandoc, Quarto,
RMarkdown, GFM, CommonMark, MultiMarkdown). Each flavor selects a
baseline set of extensions through `Extensions::for_flavor()`. Under
`Flavor::CommonMark`, almost every Pandoc-specific extension is
disabled, leaving only constructs that appear in the
[CommonMark spec](https://spec.commonmark.org/). Conformance is
tracked by running every `spec.txt` example through the parser and
comparing rendered HTML against the spec's expected HTML.

## Related rules to read first

The detailed triage workflow and invariants live in the rules — this
skill is the actionable entry point; the rules are the authority.

- `.claude/rules/commonmark-conformance.md` — failure-bucket triage
  (renderer gap / parser-shape gap / flavor leak / dialect divergence
  / missing feature), the `probe_examples` triage harness, and the
  step-by-step workflow. **Read this before triaging.**
- `.claude/rules/commonmark.md` — the invariants this work must not
  break (flavor gating, dialect handling, fixture-first discipline,
  HTML byte-equality comparison, allowlist hygiene).
- `.claude/rules/parser.md` — keep the parser CST-lossless and
  parser/formatter policy separate.

## Harness layout

  | Path                                                            | Purpose                                                        |
  | --------------------------------------------------------------- | -------------------------------------------------------------- |
  | `crates/panache-parser/tests/fixtures/commonmark-spec/spec.txt` | Vendored CommonMark spec.                                      |
  | `crates/panache-parser/tests/commonmark.rs`                     | Test runner (`commonmark_allowlist`, `commonmark_full_report`). |
  | `crates/panache-parser/tests/commonmark/spec_parser.rs`         | Parses `spec.txt` into example records.                        |
  | `crates/panache-parser/tests/commonmark/html_renderer.rs`       | Test-only CST → HTML renderer (not a public API).             |
  | `crates/panache-parser/tests/commonmark/allowlist.txt`          | Example numbers currently passing. CI fails on regression.     |
  | `crates/panache-parser/tests/commonmark/blocked.txt`            | Examples we deliberately do not target yet, with reasons.      |

The renderer mirrors the byte-equality comparison commonmark-hs uses:
the output is normalized via `<li>\n` → `<li>` and `\n</li>` →
`</li>` before comparison.

## Reports

Running `commonmark_full_report` writes two generated, git-tracked
artifacts beside the harness (no longer under `docs/`):

- `crates/panache-parser/tests/commonmark/report.txt` — per-section
  counts plus the full list of passing example numbers. Use this to
  grow the allowlist after fixing parser/renderer bugs.
- `crates/panache-parser/tests/commonmark/report.json` — the same
  data as a structured sidecar (`spec_version`, `total_examples`,
  `passing`, `failing`, `blocked`, per-section `sections`,
  `passing_numbers`).

Both are derived — regenerate them, don't hand-edit.

## Workflow (summary)

Full detail in `.claude/rules/commonmark-conformance.md`. In short:

```bash
# Regression guard: every allowlisted example must still pass.
cargo test -p panache-parser --test commonmark commonmark_allowlist

# Full report: regenerates report.txt + report.json.
cargo test -p panache-parser --test commonmark commonmark_full_report \
    -- --ignored --nocapture
```

To grow the allowlist: fix a parser or renderer bug (with a focused
regression test reproducing it), run `commonmark_full_report`, verify
the newly-passing numbers appear in `report.txt`, then add them to
`allowlist.txt` grouped under their section header. Run
`commonmark_allowlist` to confirm. If an example is intentionally not
targeted, add it to `blocked.txt` with a reason.

## Why an HTML renderer?

The CommonMark spec defines conformance as markdown-input →
HTML-output byte-equality. Panache's primary output is formatted
markdown, not HTML, so the harness needs a renderer to bridge the
gap. The renderer in `tests/commonmark/html_renderer.rs` is **test
code only** — it is not part of the public crate surface, and it
covers only the constructs `spec.txt` exercises. If a public
`--to html` mode is wanted later, the renderer can graduate from
test-only to a real module; until then it intentionally stays narrow.
