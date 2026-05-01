# Pandoc-conformance recap

Rolling handoff between sessions. Keep terse. Read `report.txt` for the full
state; this file is judgment calls only.

## Suggested next targets

Ranked by likely shared root cause and leverage. Numbers in parentheses are
the count of currently-failing imports clustered under that prefix in the
latest `report.txt`.

1. **Tables (~12)** — `grid_*`, `multiline_*`, `pipe_*`, `headerless_*`,
   `simple_table`, `tables_*`. Projector currently emits `Unsupported
   "PIPE_TABLE"` / `"GRID_TABLE"` / `"SIMPLE_TABLE"` / `"MULTILINE_TABLE"`.
   Adding even minimal `Table` projection (no col widths / alignments yet)
   unlocks the whole cluster. Highest leverage.
2. **Definition lists (~5)** — `definition_list*`, `*_definition_*`. CST has
   `DEFINITION_LIST` / `DEFINITION_ITEM` / `TERM` / `DEFINITION`; projector
   has no coverage. Pandoc native shape: `DefinitionList [ ([Inline],
   [[Block]]) ]`.
3. **Reference-style links and footnotes (~10)** — `reference_*`,
   `footnote_*`, `inline_footnotes`. Projector has no link-reference
   resolver and `FOOTNOTE_REFERENCE` / `FOOTNOTE_DEFINITION` / `INLINE_FOOTNOTE`
   are all `Unsupported`. Pandoc shapes: `Link` resolves through ref defs;
   footnotes become `Note [Block]`.
4. **Math (~5)** — `inline_math`, `display_math*`, `double_backslash_math`,
   `equation_*`. Projector lacks `Math InlineMath / DisplayMath "..."`
   coverage; CST has `INLINE_MATH` / `DISPLAY_MATH`.
5. **Emphasis edge cases (~7)** — `emphasis_*_pandoc`, `emphasis_complex`,
   `emphasis_nested_inlines`. Many of these are likely TEXT-token
   granularity diffs (per `.claude/rules/parser.md`: TEXT-coalescence diffs
   are benign — pandoc-native doesn't pin TEXT split points). Adjust
   `normalize_native()` to coalesce adjacent `Str "..." Str "..."` into a
   single `Str`, or sidestep by ensuring the projector always coalesces
   maximally before emitting.
6. **Lists with paragraphs / nested / ordered styles (~14)** — `lists_*`,
   `list_item_*`, `list_nested_*`. Mix of: (a) ordered-list style/delim
   beyond `Decimal/Period` (need `LowerRoman`, `UpperAlpha`, `OneParen`,
   `TwoParens`, `Example`, `DefaultStyle`); (b) multi-paragraph items
   not yet covered; (c) nested-list looseness handling.
7. **Raw blocks/inlines (~6)** — `latex_environment`, `raw_*`,
   `html_block*`, `inline_link_dest_*`. Pandoc emits `RawBlock (Format
   "tex") "..."` / `RawInline (Format "html") "..."`. CST kinds:
   `TEX_BLOCK`, `HTML_BLOCK`, `INLINE_HTML`, `RAW_INLINE`.
8. **Citations / spans / divs / line blocks (~6)** — `citations`,
   `bracketed_spans`, `fenced_divs`, `line_blocks`, `nested_headings_in_containers`.
   Each is a small projector addition: `Cite`, `Span`, `Div`, `LineBlock`.
9. **YAML metadata (~3)** — `yaml_metadata*`. Pandoc strips YAML metadata
   from the body block list (puts it in `Pandoc Meta {...}` wrapper which
   we don't emit). Easiest fix: in the projector, skip
   `SyntaxKind::YAML_METADATA` blocks rather than emitting `Unsupported`.
10. **Auto-id "section" fallback** — `imported-atx_empty_with_closing_fence`.
    Pandoc auto-numbers empty headings as `"section"`, `"section-1"`, etc.
    Our slugifier returns `""` for empty input. Add the same numbering
    fallback to `pandoc_slugify` in the projector.
11. **Email autolink + raw HTML autolink edge cases** — projector's
    `autolink_inline` always emits class `["uri"]`; pandoc emits
    `["email"]` with `mailto:` prefix for emails, and treats
    `<m:abc>` style as `RawInline (Format "html")`. Detect the email
    case via `@` presence in the URL.

Suggested first session: **#1 (Tables)** — single projector addition,
unlocks ~12 cases, no parser changes likely needed.

## Don't redo

- The CST → pandoc-native projector is **test-only** at
  `crates/panache-parser/tests/pandoc/native_projector.rs`. Do not move
  it under `src/` or wire it into the public API.
- Slugifier in the projector is intentionally a copy of
  `panache-formatter::utils::pandoc_slugify` — the parser crate cannot
  depend on the formatter crate (would cycle). Keep it inline.
- `expected.native` files are pinned to pandoc 3.9.0.2 (see
  `tests/fixtures/pandoc-conformance/.panache-source`). Regenerate via
  `scripts/update-pandoc-conformance-corpus.sh` only when intentionally
  bumping pandoc.
- The bulk-import script (`import-pandoc-conformance-from-parser-fixtures.sh`)
  uses leading-zero-stripped IDs to avoid POSIX shell octal interpretation
  in `$((...))`. Do not refactor it back to direct `$((0025 + 1))`-style
  arithmetic.
- `imported-*` cases live alongside hand-curated cases under
  `corpus/`. The script wipes prior `*-imported-*` dirs before re-running,
  so the import is idempotent — but **do not hand-edit** an imported case's
  `input.md` or `expected.native`. If a hand-curated variant is needed,
  copy it into a new `<NNNN>-<section>-<slug>/` dir with a non-`imported`
  section prefix.

## Latest session

- **Date**: 2026-05-01
- **Pass before → after**: 25 → 61 / 187 (+36 imports cleanly green
  out of 162 imported)
- **What landed**:
  - Bulk-import script:
    `crates/panache-parser/scripts/import-pandoc-conformance-from-parser-fixtures.sh`
  - 162 imported corpus cases (filter:
    `parser-options.flavor == pandoc | absent`,
    skip `*commonmark*` / `*gfm*` / `*disabled*` /
    `crlf_*` / `line_ending_*` / `tab_*`, skip non-`.md` inputs,
    cap at 4 KB).
  - Allowlist grew by 36 IDs under new `# imported` section.
- **Files changed (classified)**:
  - new script (Phase A bulk-import)
  - new corpus cases (162 `imported-*` dirs)
  - allowlist.txt (+36 IDs)
- **Don't redo**: see "Don't redo" above. The 126 currently-failing
  imports are the natural backlog; do not undo the import to "clean up"
  — they exist *to* fail until a projector/parser fix lands.
- **Next**: pick **Tables** (target #1) for the first reduction
  session — single projector function unlocks ~12 cases.
