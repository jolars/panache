# CommonMark conformance — running session recap

This file is the rolling, terse handoff between sessions of the
`commonmark-conformance-expand` skill. Read it at the start of a session for
suggested next targets and known follow-ups; rewrite the **Latest session**
entry at the end with what changed and what to look at next.

Keep entries short. The full triage data lives in
`crates/panache-parser/tests/commonmark/report.txt` and
`docs/development/commonmark-report.json`; this file is for the *judgment
calls* a fresh session can't reconstruct from those artifacts (why a target
was picked, what was deliberately skipped, which fix unlocked which group).

---

## Latest session — 2026-04-28

**Pass count: 385 → 407 / 652 (59.0% → 62.4%)**

### Targets and root causes

- **Images: 7 → 22 / 22** — Pandoc `implicit_figures` flavor leak
  (`FigureParser` wasn't consulting `extensions.implicit_figures`); plus
  renderer-side gaps for reference / collapsed / shortcut image forms,
  alt-text extraction (nested URLs were leaking into `alt`), angle-bracketed
  URLs, and backslash-escape decoding in URLs/titles.
- **Links: 45 → 48 / 90** — same angle-bracket and backslash-escape fixes
  applied to `render_link`.
- **Backslash escapes: 7 → 9 / 13** — incidental wins from the URL/title
  backslash decoding.
- **Link reference definitions: 13 → 15 / 27** — same fix applied in
  `parse_reference_definition`.

### Files changed

- Parser (flavor leak): `crates/panache-parser/src/parser/block_dispatcher.rs`
  — `FigureParser::detect_prepared` now bails when
  `extensions.implicit_figures` is off. Pandoc-verified: `commonmark` reader
  yields `Para [Image …]`, `markdown` yields `Figure …`, so this is a
  single-feature toggle, not a `Dialect` divergence.
- Renderer (test-only): `crates/panache-parser/tests/commonmark/html_renderer.rs`
  — `render_image` reference forms; `collect_alt_text`/`push_alt_from` for
  plain-string alt; `strip_angle_brackets`; `decode_backslash_escapes` wired
  into `render_link`, `render_image`, `parse_reference_definition`.
- Parser fixture:
  `crates/panache-parser/tests/fixtures/cases/commonmark_image_paragraph_no_figure/`
  (input.md + `parser-options.toml` `flavor = "commonmark"`) + snapshot;
  registered in `golden_parser_cases.rs`. Pins that an image-only paragraph
  under CommonMark stays `PARAGRAPH > IMAGE_LINK` (no `FIGURE`).
- Allowlist: +22 entries (Images 572–591 minus 577/579/582/583/590/592/593
  which were already in; Links 486, 500, 506; Backslash escapes 22, 23; Link
  reference definitions 200, 202).

### Don't redo

- Image reference/shortcut/collapsed handling, alt-text extraction, angle
  URL stripping, backslash escape decoding in URLs/titles, and the
  `implicit_figures` gate on `FigureParser`. All in place.

### Suggested next targets, ranked

1. **Lists (5/21) + List items (17/31)** — biggest low-pass-rate section;
   likely shared root causes around loose/tight detection, lazy
   continuation, blank-line handling.
2. **Emphasis and strong emphasis (85/47)** — largest remaining absolute
   failure count; flanking-rule edge cases + intraword-underscore.
3. **HTML blocks (24/20) + Raw HTML (6/14)** — probably one shared fix in
   HTML-tag/comment recognition.
4. **Setext headings (16/11)** — small contained section, good for a
   targeted clean-up between bigger pushes.
5. Known link follow-ups still in `blocked.txt`: #488, #490, #493, #508,
   #523, #546 — angle-URL strictness and link-parser tightening (dialect
   divergences). Reference shape: `code_spans_unmatched_backtick_run_*`.
6. Entity reference follow-ups from prior session: #31 (HTML-block
   detection gap — `<a href="…">` stays a paragraph) and #41 (entity refs
   as structural quotes around link title — dialect divergence; needs
   paired fixtures).
