# Panache TODO

This document tracks implementation status for Panache's features.

## Language Server

### Code Actions

- [ ] Convert between table styles (simple, pipe, grid)
- [x] Convert between inline/reference links

### Navigation & Symbols

- [x] Find references - Find all uses of a reference link/footnote/citation
  - [x] Find references for citations - Find all `@cite` uses of a bibliography
    entry
  - [x] Find references for headings - Find all internal links to a heading
  - [x] Find references for reference links - Find all `[text][ref]` links

### Completion

- [ ] Reference link completion - Complete `[text][ref]` from defined references
- [ ] Heading link completion
- [ ] Attribute completion - Complete class names and attributes in
  `{.class #id}`
- [x] Shortcode completion - Complete Quarto shortcode names in `{{< name >}}`
- [x] Cross-reference completion - Complete `@fig-id` and `\@ref(fig-id)`
  cross-refs (also: file/shortcode path completion is implemented)

### Inlay Hints (low priority)

Personally I think inlay hints are distracting and I am not sure what we want to
support.

- [ ] Link target hints - Show link targets as inlay hints
- [ ] Reference definition hints - Show reference definitions as inlay hints
- [ ] Citation key hints - Show bibliography entries for `@cite` keys
- [ ] Footnote content hints - Show footnote content as inlay hints

### Advanced

- [ ] On-type formatting - `textDocument/onTypeFormatting` for auto-formatting
  triggers (not sure about this, low priority)
- [ ] Semantic tokens - Syntax highlighting via LSP
- [ ] Rename
  - [x] Citations - Rename `@cite` keys and update bibliography
  - [x] Reference links - Rename `[ref]` labels and update definitions
  - [x] Headings - Rename heading text and update internal links
  - [x] Footnotes - Rename footnote labels and update definitions/links
  - [x] Files - Rename linked markdown files and update links
  - [ ] Files - Rename other linked files, shortcodes, etc.
- [ ] Configuration via LSP - `workspace/didChangeConfiguration` to reload
  config

### Spec coverage gaps

Markdown-relevant LSP methods we don't yet implement, surfaced by the 2026-06-18
spec-coverage audit (see `docs/guide/lsp.qmd` "LSP Specification Coverage").
`onTypeFormatting`, `semanticTokens`, `inlayHint`, and
`workspace/didChangeConfiguration` are tracked above and not repeated here.

- [x] Pull diagnostics - `textDocument/diagnostic` + `workspace/diagnostic` as a
  companion/alternative to the current push model (mode-switch: pull clients
  get pull only, push suppressed; cache + `workspace/diagnostic/refresh`)
  - [ ] Populate `related_documents` in the document report for clients with
    `related_document_support` (currently `None`; cross-file diagnostics
    reach the client only via `workspace/diagnostic`)
  - [ ] Streaming/partial results (`DocumentDiagnosticReportPartialResult`,
    `WorkspaceDiagnosticReportPartialResult`) for large workspaces; reports
    are returned whole today
  - [ ] `workspace/diagnostic` only reports open documents + reachable project
    manifests, not every on-disk doc in the workspace (rust-analyzer pulls
    all workspace files). Decide whether closed-but-on-disk docs should
    surface.
- [ ] `textDocument/documentHighlight` - highlight every occurrence of the
  reference/citation/footnote/heading under the cursor
- [ ] `textDocument/selectionRange` - structural smart-select expansion (word →
  inline → block → section)
- [ ] `textDocument/linkedEditingRange` - edit a reference label and its
  definition simultaneously
- [ ] `completionItem/resolve` - defer expensive completion detail (e.g.
  citation previews) until an item is focused
- [ ] `codeAction/resolve` + advertise `codeActionKinds` - compute edits lazily
  and let clients filter actions by kind
- [ ] `workspace/didChangeWorkspaceFolders` - advertised
  (`change_notifications: true`) but currently unhandled
- [ ] `workspace/configuration` - pull settings from the client instead of
  relying only on discovered config files
- [ ] `workspace/executeCommand` - server-side commands backing complex code
  actions
- [ ] File operations beyond `willRenameFiles`: `didRenameFiles`,
  `willCreateFiles`/`didCreateFiles`, `willDeleteFiles`/`didDeleteFiles`
- [ ] `textDocument/willSave` / `willSaveWaitUntil` - server-driven
  format-before-write hook

#### Out of scope for prose

These spec methods target compiled-language tooling and have no useful Markdown
analogue; do not re-audit them: call hierarchy, type hierarchy,
`textDocument/implementation`, `typeDefinition`, `declaration`, `inlineValue`,
`moniker`, document color, and code lens.

### Future Lint Rules

#### Syntax correctness

- [ ] Broken table structures
- [ ] Invalid citation syntax (`@citekey` malformations)
- [ ] Unclosed inline math/code spans
- [ ] Invalid shortcode syntax (Quarto-specific)

#### Style/Best practices

- [ ] Multiple top-level headings
- [ ] Empty links/images
- [ ] Unused reference definitions
- [ ] Hard-wrapped text in code blocks
- [ ] Use blanklines around horizontal rules

### Configuration

- [ ] Severity levels (error, warning, info)
- [ ] Auto-fix capability per rule (infrastructure exists, rules need
  implementation)

### Shared utilities

- [ ] Lift the Levenshtein-based "did you mean...?" helper out of
  `src/linter/rules/html_entities.rs` into a shared utils module once a
  second rule wants fuzzy matching. Likely candidates: `citation-keys`
  (suggest the closest bibliography entry), `undefined-references` (suggest
  the closest defined label), and `unknown-emoji-alias` (suggest the closest
  emoji shortcode). Decide the API shape (raw `levenshtein` vs. a
  `nearest_match(target, candidates, max_distance)` helper that bundles the
  distance cap and alphabetical tie-break) at the second caller, not before.

### Open Questions

- How to balance parser error recovery vs. strict linting?
- Performance: incremental linting for LSP mode?
- LSP: incremental parsing cache (tree reuse on didChange)
- LSP lint dispatch follow-ups (from the cancellation-race fix):
  - `didOpen` schedules with `with_dependents: false`, so opening a child that
    an already-open parent includes won't refresh the parent's diagnostics until
    the parent is next edited/saved. Decide whether open should re-lint
    dependents.
  - The cancel→re-arm net (built-in only) leaves external-linter diagnostics
    stale when a save's pass is cancelled by a concurrent write; they refresh on
    the next save. Consider preserving `run_external` across the re-arm if it
    proves noticeable.
  - Larger redesign still open: shared thread pool + priority queue + lint cap
    (see the `lsp-shared-priority-pool` handoff plan).

### External formatter presets backlog (conform.nvim parity)

The list below tracks **non-deprecated** `conform.nvim` formatter preset names
that are not yet built-in Panache presets. Deprecated conform names are
intentionally excluded.

- [ ] `ansible-lint`
- [x] `asmfmt`
- [ ] `ast-grep`
- [x] `astyle`
- [ ] `auto_optional`
- [x] `autocorrect`
- [ ] `autoflake`
- [ ] `autopep8`
- [ ] `bake`
- [x] `bean-format`
- [x] `beautysh`
- [x] `bibtex-tidy`
- [ ] `bicep`
- [ ] `biome-check`
- [ ] `biome-organize-imports`
- [x] `biome`
- [ ] `blade-formatter`
- [ ] `blue`
- [x] `bpfmt`
- [x] `bsfmt`
- [x] `buf`
- [x] `buildifier`
- [x] `cabal_fmt`
- [ ] `caramel_fmt`
- [ ] `cbfmt`
- [ ] `cedar`
- [x] `cljfmt`
- [ ] `cljstyle`
- [x] `cmake_format`
- [ ] `codeql`
- [ ] `codespell`
- [ ] `commitmsgfmt`
- [ ] `crlfmt`
- [ ] `crystal`
- [x] `csharpier`
- [ ] `css_beautify`
- [x] `cue_fmt`
- [ ] `d2`
- [ ] `darker`
- [ ] `dart_format`
- [ ] `dcm_fix`
- [ ] `dcm_format`
- [ ] `deno_fmt`
- [x] `dfmt`
- [ ] `dioxus`
- [ ] `djlint`
- [ ] `docformatter`
- [ ] `dockerfmt`
- [ ] `docstrfmt`
- [ ] `doctoc`
- [ ] `dprint`
- [ ] `easy-coding-standard`
- [x] `efmt`
- [ ] `elm_format`
- [ ] `erb_format`
- [ ] `erlfmt`
- [ ] `eslint_d`
- [ ] `fantomas`
- [ ] `findent`
- [x] `fish_indent`
- [x] `fixjson`
- [ ] `fnlfmt`
- [ ] `forge_fmt`
- [ ] `format-dune-file`
- [ ] `fourmolu`
- [ ] `fprettify`
- [ ] `gawk`
- [ ] `gci`
- [x] `gdformat`
- [ ] `gdscript-formatter`
- [ ] `gersemi`
- [ ] `ghdl`
- [ ] `ghokin`
- [x] `gleam`
- [ ] `gluon_fmt`
- [ ] `gn`
- [x] `gofmt`
- [x] `gofumpt`
- [ ] `goimports-reviser`
- [ ] `goimports`
- [ ] `gojq`
- [ ] `golangci-lint`
- [ ] `golines`
- [x] `google-java-format`
- [ ] `grain_format`
- [ ] `hcl`
- [ ] `hindent`
- [ ] `hledger-fmt`
- [ ] `html_beautify`
- [ ] `htmlbeautifier`
- [x] `hurlfmt`
- [ ] `imba_fmt`
- [ ] `inko`
- [x] `isort`
- [ ] `janet-format`
- [ ] `joker`
- [ ] `jq`
- [ ] `js_beautify`
- [ ] `json_repair`
- [x] `jsonnetfmt`
- [ ] `just`
- [ ] `kcl`
- [ ] `kdlfmt`
- [ ] `keep-sorted`
- [x] `ktfmt`
- [ ] `ktlint`
- [ ] `kulala-fmt`
- [ ] `latexindent`
- [x] `leptosfmt`
- [ ] `liquidsoap-prettier`
- [ ] `llf`
- [ ] `lua-format`
- [ ] `mago_format`
- [ ] `mago_lint`
- [ ] `markdown-toc`
- [ ] `markdownfmt`
- [ ] `markdownlint-cli2`
- [ ] `markdownlint`
- [ ] `mdsf`
- [ ] `mdslw`
- [ ] `meson`
- [ ] `mh_style`
- [x] `mix`
- [ ] `mojo_format`
- [x] `nginxfmt`
- [ ] `nickel`
- [ ] `nimpretty`
- [x] `nixfmt`
- [ ] `nixpkgs_fmt`
- [ ] `nomad_fmt`
- [ ] `nph`
- [ ] `npm-groovy-lint`
- [ ] `nufmt`
- [ ] `ocamlformat`
- [ ] `ocp-indent`
- [ ] `odinfmt`
- [ ] `opa_fmt`
- [x] `ormolu`
- [ ] `oxfmt`
- [ ] `oxlint`
- [ ] `packer_fmt`
- [ ] `palantir-java-format`
- [ ] `pangu`
- [ ] `pasfmt`
- [ ] `perlimports`
- [ ] `perltidy`
- [ ] `pg_format`
- [ ] `php_cs_fixer`
- [ ] `phpcbf`
- [ ] `phpinsights`
- [ ] `pint`
- [ ] `pkl`
- [ ] `prettierd`
- [ ] `pretty-php`
- [ ] `prettypst`
- [ ] `prolog`
- [ ] `pruner`
- [ ] `puppet-lint`
- [ ] `purs-tidy`
- [x] `pycln`
- [ ] `pyink`
- [ ] `pymarkdownlnt`
- [x] `pyproject-fmt`
- [ ] `python-ly`
- [ ] `pyupgrade`
- [ ] `qmlformat`
- [x] `racketfmt`
- [ ] `reformat-gherkin`
- [ ] `reorder-python-imports`
- [ ] `rescript-format`
- [ ] `roc`
- [ ] `rstfmt`
- [ ] `rubocop`
- [x] `rubyfmt`
- [ ] `ruff_fix`
- [ ] `ruff_format`
- [ ] `ruff_organize_imports`
- [x] `rufo`
- [ ] `rumdl`
- [x] `runic`
- [x] `rustfmt`
- [ ] `rustywind`
- [ ] `scalafmt`
- [ ] `shellcheck`
- [ ] `shellharden`
- [ ] `sleek`
- [ ] `smlfmt`
- [ ] `snakefmt`
- [ ] `spotless_gradle`
- [ ] `spotless_maven`
- [ ] `sql_formatter`
- [ ] `sqlfluff`
- [ ] `sqruff`
- [ ] `squeeze_blanks`
- [ ] `standard-clj`
- [ ] `standardjs`
- [ ] `standardrb`
- [ ] `stylelint`
- [ ] `stylish-haskell`
- [x] `stylua`
- [ ] `superhtml`
- [ ] `swift`
- [ ] `swift_format`
- [ ] `swiftformat`
- [ ] `swiftlint`
- [ ] `syntax_tree`
- [x] `tclfmt`
- [ ] `templ`
- [ ] `terraform_fmt`
- [ ] `terragrunt_hclfmt`
- [x] `tex-fmt`
- [ ] `tlint`
- [ ] `tofu_fmt`
- [ ] `tombi`
- [ ] `treefmt`
- [ ] `trim_newlines`
- [ ] `trim_whitespace`
- [ ] `trunk`
- [ ] `twig-cs-fixer`
- [ ] `txtpbfmt`
- [ ] `typespec`
- [ ] `typos`
- [x] `typstyle`
- [ ] `ufmt`
- [ ] `uncrustify`
- [ ] `usort`
- [ ] `v`
- [ ] `verible`
- [ ] `vsg`
- [ ] `xmlformatter`
- [ ] `xmllint`
- [ ] `xmlstarlet`
- [ ] `yapf`
- [ ] `yew-fmt`
- [x] `yq`
- [ ] `zigfmt`
- [ ] `ziggy`
- [ ] `ziggy_schema`
- [ ] `zprint`

### Syntax AST wrappers

### Tables

- [x] Grid tables
  - [ ] Row-spanning grids (a `|` content row containing `+`, e.g.
    `grid_table_planets`) still use the older
    `format_spanning_grid_table_raw` passthrough, which re-pads but carries
    data-specific hacks (`col_count == 12` alignment guesses for the planets
    fixture) rather than modeling the rowspan structure. Fold this into the
    same span-aware engine so the special-cases go away; coordinate the
    colspan + rowspan geometry in one layout pass.

## Parser

### Architecture

- [ ] Stop letting `pandoc_ast.rs` drift into a second-stage parser. Load-
  bearing byte-walkers (`split_html_block_by_tags`, `parse_pandoc_blocks`
  and the refs/heading-id reparse helpers) re-tokenize source the CST should
  already encode. This violates the single-pass invariant in `AGENTS.md` and
  hides structural decisions from downstream consumers (linter, salsa, LSP,
  formatter) which all walk the CST, not the projector. The guiding
  principle: when the parser computes a structural fact during its single
  pass, it must emit that fact into the CST (wrapping existing source bytes,
  `HTML_ATTRS`-style --- never synthetic tokens) instead of forcing the
  projector to recompute it. Each bucket below is its own bounded step,
  verified against pandoc-native + CommonMark (both must stay byte-identical
  or improve). Roadmap:

  - [x] **Attributes --- remaining node kinds.** Structuring is done.
    `SPAN_ATTRIBUTES` (bracketed spans), `HTML_ATTRS` (HTML
    `<div>`/`<span>`; `parse_html_attrs` deleted), and raw-inline
    `{=format}` (`raw_inline.rs` now wraps the source slice via
    `emit_attribute_node`) all landed earlier. `CODE_INFO` DisplayExplicit
    (`{.python #id key=val}`) and DisplayShortcut (`lang {.cls}`) now emit
    `ATTR_*`/`CODE_LANGUAGE` children via `emit_code_info_attrs`, and
    `code_block_attr` reads them instead of re-parsing. Note:
    `parse_attr_block` is *not* deletable --- it is now the shared opaque
    fallback the structured readers intentionally retain (MMD `[#id]`,
    malformed bodies, bare-word divs, legacy table-caption scans, Plain/Raw
    info strings). Deferred follow-up: structure-read the Executable
    `CHUNK_OPTIONS` projection (Quarto/RMarkdown chunks), which still uses
    the `parse_attr_block` text path; the CST is already structured, only
    the projector lags.

  - [ ] **HTML opaque-block split.** Continue the HTML lift. **Phase 7a done**
    (2026-06-17): single-construct opaque shapes (comments, PI, verbatim
    `<pre>`/`<script>`/`<style>`/`<textarea>`) now retag to `HTML_BLOCK_RAW`
    at parse time, so the projector routes by kind and `emit_html_block`'s
    byte-sniff arm is dead for Pandoc. **Remaining (7b-7e):** standalone
    single-tag (close/void), single open + trailing, void sequences, and the
    hard multi-tag interleave (D3) still flow through
    `split_html_block_by_tags` / `parse_pandoc_blocks`. Note: D3's inter-tag
    markdown reparse *relocates* into the parser rather than disappearing
    --- the walker is not fully deletable. Largest bucket; coordinate with
    the `html-conformance` skill.

  - [x] **Table separator tokenization.** The separator row is currently a
    coalesced `TEXT` blob (e.g. `TEXT "|:--|--:|"`), so
    `simple_table_aligns`, `grid_dash_widths`, and `pipe_separator_aligns`
    re-tokenize it. Split the markers (`|` / `+`, dash runs, colons) into
    distinct CST tokens so those derivations read structure instead of
    re-scanning a string. Note: this only structures the *syntax* --- the
    derived geometry (widths, alignment values) does NOT move into the CST;
    see below.

  - Legitimately stays in the projector (derived values with no source-byte
    form, not unencoded syntax): column **widths** (a normalized fraction of
    dash counts --- there is no byte that spells `0.33`); table **alignment**
    (the `AlignLeft`/... enum is computed --- from colons for pipe/grid tables,
    from content-vs-dash flushness for simple/multiline --- so even though its
    *evidence* is in the source, the value isn't a substring); implicit
    heading-id slugification (needs whole-document dedup); and smart-typography
    substitution (an output transform). Storing any of these as tokens would
    require synthetic tokens and break CST losslessness.

- [x] Centralize position advancement. `parse_line`, `parse_inner_content`, and
  the dispatch helpers (`dispatch_bq_after_list_item`,
  `maybe_open_fenced_code_in_new_list_item`, the three `handle_*_effect`
  handlers, and `try_fold_list_item_buffer_into_setext`) now return a
  `LineDispatch` (or `usize` extras for effect handlers). The outer
  `parse_document_stack` is the sole site that mutates `self.pos`. The
  `self.pos -= 1` compensation hack inside `dispatch_bq_after_list_item` and
  two analogous `self.pos = new_pos - 1` hacks
  (`maybe_open_fenced_code_in_new_list_item`,
  `handle_definition_list_effect::Definition`) are gone.

- [x] De-duplicate table caption routing between `detect_prepared` and
  `parse_prepared` (`block_dispatcher.rs`). The
  `is_caption_followed_by_table` check, the `table_pos` computation (skip
  caption continuation lines + one blank), and the try-each-table-kind
  cascade are spelled out twice. Beyond the double-parse cost (see the
  table-IR item under Performance), the two copies can drift --- a fix to
  one (e.g. the raw-vs-stripped caption issue below) silently won't reach
  the other. Fold into one shared helper; pairs naturally with the table-IR
  change. Done: routing lives in `resolve_table_pos` / `try_parse_kind` /
  `first_kind_at`; `table_pos` cached on `TablePrepared`. The double-parse
  cost is still open (tracked under the table-IR Performance item).

- [x] Reconcile `is_caption_followed_by_table`'s raw-vs-stripped callers and its
  duplicate table sniffing (`blocks/tables.rs`). Not intended: pandoc
  recognizes caption-before/after tables inside a blockquote, and panache
  was *non-lossless* there (`> : cap` round-tripped to `> > : cap`) because
  caption emission read raw lines. Fixed: `emit_table_caption` and the
  caption blank-line emission now take the container-stripped window
  (`emit_or_dispatch_tail`), and `resolve_table_pos` + the deflist
  `detect_prepared` gate run caption detection on the stripped window (the
  multiline path only recognizes a caption-led table when dispatched at the
  border, so `resolve_table_pos` skipping the caption correctly inside a
  blockquote is what keeps it from leaking into a paragraph). The
  `next_line_is_definition_marker` / footnote lookahead gates intentionally
  stay raw (their marker detection is raw/indent-based, so the caption gate
  is top-level-only). The separator cascade is extracted into one named
  predicate `table_grid_starts_at`, documented as the cheap lookahead twin
  of `first_kind_at` (deliberately not a full parse, for perf) with an
  agreement unit test. Paired pandoc-verified fixtures landed under
  `crates/panache-parser/tests/fixtures/cases/` (pipe before/after, 2-line
  caption, multiline, + a commonmark counterpart).

- [x] Formatter drops the blockquote prefix on a table inside a blockquote.
  `> | a | b |\n> |---|---|\n> | 1 | 2 |` formats to `| a | b |` ... with no
  `>` (the table is lifted out of the blockquote). Lossless and idempotent,
  so it's a *formatter* policy bug, not a parser one --- surfaced while
  fixing the caption raw-vs-stripped reconcile. Caption-bearing tables
  inside blockquotes inherit the same drop. *Fixed:* blockquote child
  dispatch now handles `PIPE_TABLE`/`GRID_TABLE`/`SIMPLE_TABLE`/
  `MULTILINE_TABLE` (temp-format, strip self-indent, re-prefix), and
  `extract_table_caption_content` skips the losslessness
  `BLOCK_QUOTE_MARKER` tokens. Goldens `blockquote_pipe_table`,
  `blockquote_pipe_table_caption`.

- [x] Caption-before table as the *first line of a list item* (no blank line
  before it) was parsed twice. `- Table: cap\n\n  <table>` emitted the
  caption both as the item's `PLAIN` (the core claims the first list line
  before `TableParser`'s `has_blank_before`/`at_document_start` gate lets it
  fire) and again as the table's `TABLE_CAPTION` (backward
  `find_caption_*`), breaking losslessness. Fixed by
  `maybe_open_caption_table_in_new_list_item` in `parser/core.rs` ---
  mirrors `maybe_open_fenced_code_in_new_list_item`: when a fresh list
  item's buffered marker-line content is a caption that a table follows, it
  clears the buffer and emits the whole caption-led table via the forward
  caption path (Grid → Multiline → Pipe → Simple cascade). Fixture
  `list_item_pipe_table_caption_before`.

- [x] Formatter dropped the list marker when a list item's *sole/first child*
  was a table. **No-caption case**: `- | a | b |\n  …` keeps the marker by
  putting the first table line on the marker line --- `lists.rs`
  `format_list_item` has a `PIPE_TABLE | GRID_TABLE` arm that splices the
  marker prefix onto the table's first line (pipe + grid, bullet + ordered).
  **Caption-led case** (`- | a | b |\n  …\n\n  : cap`, both `: cap` and
  `Table:`/`table:` forms): fixed in the **parser** ---
  `maybe_open_table_with_trailing_caption_in_new_list_item` (core.rs) parses
  a marker-line table *with* its trailing caption at item open (via the
  `try_parse_*` cascade, whose `find_caption_after_table` absorbs the
  caption), so it never reaches the buffer-flush or the definition-list
  dispatch that previously mangled it. The formatter arm then splices the
  whole table (caption rendered below by
  `format_pipe_table`/`format_grid_table`). Matches pandoc, which always
  treats `: cap` after a table --- including in a list --- as the table's
  Caption, never a definition list. Goldens
  `list_item_table_first_{bullet,grid,caption_bullet,caption_ordered}`;
  parser fixtures `list_item_{pipe,grid}_table_caption_after_pandoc`,
  `list_item_pipe_table_caption_keyword_after_pandoc`. **Remaining edges**:
  `SIMPLE_TABLE`/`MULTILINE_TABLE` as a list item's first child (the
  open-time parse uses the full cascade, but the formatter arm and the
  buffer-lift allowlist cover only pipe/grid --- their formatters take no
  `indent`); and a table that sits *after a blank line* inside a list item
  followed by `: cap`, which is a separate pre-existing issue (the
  table-shaped lines parse as a `LINE_BLOCK`, not a table).

- [ ] Formatter trims leading/trailing spaces *inside* inline-code spans. A span
  whose backticks wrap content with leading spaces (two spaces, then
  `| a | b |`) is reformatted with those spaces removed, mutating the
  code-span content. The parse is lossless (`INLINE_CODE_CONTENT` keeps the
  spaces), so this is a *formatter* transformation. It matches pandoc's AST
  (`Code "| a | b |"`), which is more aggressive than CommonMark's "strip
  one space only when both sides have one" rule, so it may be intended ---
  but decide whether the formatter should preserve code-span content
  verbatim instead. (No literal example here: the hook would trim it.)
  Surfaced when the pre-commit `panache format` hook rewrote a backtick span
  in this file.

- [ ] Collapse the double `try_parse_definition_marker` call in
  `next_line_is_definition_marker` (`blocks/definition_lists.rs`). It parses
  the line once for `.is_some()` and again to destructure `marker`; a single
  `if let Some((marker, ..)) = ...` covers both. Runs in a per-block
  lookahead.

### Performance

- [x] Avoid temporary green tree when injecting `BLOCK_QUOTE_MARKER` tokens into
  inline-parsed paragraphs. Inline emission is now generic over an
  `InlineSink` trait (`inlines/sink.rs`); the common path writes straight
  into the `GreenNodeBuilder` (monomorphized, zero-cost) and blockquote
  paragraphs swap in `MarkerInjectingSink`, which splices markers at byte
  offsets during the single emission pass. No temp tree is built and
  replayed.

- [ ] Avoid temporary green tree in table detection.
  `TableParser::detect_prepared` (`block_dispatcher.rs`) fully parses the
  table into a throwaway `GreenNodeBuilder` just to validate the match
  (`.is_some()`), then `parse_prepared` parses it again into the real
  builder --- the table is parsed twice. Carry a reusable table IR
  (rows/cells/alignments) in `TablePrepared` so emission renders from the IR
  instead of re-parsing. Larger change than the blockquote-marker item
  above; same "build temp CST then discard" anti-pattern.

- [ ] Use `memchr` for the refdef-map newline scan. `memchr_newline`
  (`inlines/refdef_map.rs`) is a scalar `iter().position(|&b| b == b'\n')`
  despite its name; `collect_refdef_labels` calls it once per line over the
  whole document. Swap in the `memchr` crate (SIMD) for a free per-parse
  speedup --- and the name stops lying.

### YAML validation: consumer fidelity vs YAML 1.2 (needs design decision)

Surfaced 2026-06-08 while fixing a real bug: pandoc rejected a user's
frontmatter (`description:` with an empty inline value followed by an
*unindented* blurb) but `panache lint`/LSP stayed silent.

**Landed** (commits `f8f804c8`, `83a73a13`): the in-tree YAML scanner already
mirrors libyaml's required-simple-key rule and emits
`LEX_REQUIRED_SIMPLE_KEY_NOT_FOUND`, but `validate_yaml`
(`crates/panache-parser/src/parser/yaml/validator.rs`) never harvested it. Added
a harvest check (modeled on `check_unterminated_quoted`). 12 yaml-test- suite
error fixtures now report the libyaml-faithful message at libyaml's exact
line/column (verified against Ruby Psych). This bug class (scanner diagnostics
the validator drops) is now closed --- audited all 6 `LEX_*` codes.

**The bigger open question --- which YAML dialect should the validator target?**
panache currently validates against **YAML 1.2**, pinned by the allowlist
(`crates/panache-parser/tests/yaml/allowlist.txt`), which covers the **entire**
vendored yaml-test-suite --- all 402 cases (308 valid held to event-parity + 94
held to error-contract), NOT a curated subset. (The file's header comment "Keep
this list intentionally small" is stale; it was grown to full coverage.) So each
case's valid/error status is inherited from the upstream suite's `test.event` /
`error` files. But the *real consumers* are stricter and differ by context (see
memory `project_yaml_consumer_parsers.md`):

- **Frontmatter** → pandoc → Haskell `yaml`/**libyaml** (≈ YAML 1.1).
- **Hashpipe `#|` cell options** → Quarto → **js-yaml** (YAML 1.2). In a Quarto
  doc the frontmatter is read by BOTH, so it must satisfy the stricter one.
- They **agree** on big structural rejections (implicit empty keys, duplicate
  keys, reserved `@`); they **diverge** on e.g. **tabs as indentation** (pandoc
  ACCEPTS, quarto/js-yaml REJECTS --- confirmed in frontmatter and hashpipe).
  Scalar-typing differences (yes/no, sexagesimal, octal) are invisible to
  parse-acceptance (pandoc treats metadata scalars as strings), so they don't
  matter for "will it parse" validation.

**Policy landed (2026-06-08): consumer profiles on top of a 1.2 substrate.**

The validator now targets the real consumers, conditioned on (flavor, location),
without disturbing the 1.2 substrate. A `YamlValidationContext`
(`crates/panache-parser/src/parser/yaml/profile.rs`) maps (flavor, location) →
active consumer set; `validate_yaml_with_context` runs the existing \~25
substrate checks (Pool 1) followed by consumer-only checks (Pool 2) that never
run on the substrate path. `validate_yaml` stays a substrate wrapper, so the
yaml-test-suite verdicts are **unchanged** (no upstream-verdict override, no
allowlist churn). Flavor is threaded into `emit_yaml_block` /
`parse_fenced_code_block`; the parser's syntax-error channel carries the
consumer diagnostics straight to lint/LSP. The accept/reject ground truth came
from an empirical audit of every suite case through libyaml (Ruby Psych),
pandoc, and js-yaml --- see `scripts/yaml-oracle/` and the full classification
in `crates/panache-parser/tests/yaml/consumer-matrix.md`.

- [ ] **Tabs as indentation --- DEFERRED (needs parser surgery).** The audit
  disproved the blanket "pandoc accepts tabs" assumption: pandoc accepts
  tabs in scalar content / flow / after a block-seq dash but **rejects**
  them in explicit-key context (Y79Y/006--009). The panache check that fires
  (`PARSE_UNEXPECTED_INDENT`) is overloaded across 12 cases with mixed
  pandoc verdicts, so it cannot be suppressed at check granularity. Acting
  on it requires emitting a tab-context-specific diagnostic so the accepted
  contexts can be gated per-consumer. The (flavor, location) plumbing landed
  here is the prerequisite; a Pool-1 `suppressed_for` hook is the intended
  extension point (documented in `validate_yaml_with_context`).
- [ ] **pandoc metadata-must-be-a-mapping --- OUT OF SCOPE.** 11 frontmatter
  cases (e.g. `LX3P` `[flow]: block`, top-level sequences) where pandoc
  rejects but libyaml accepts. This is a frontmatter-shape rule, not YAML
  parse validity --- a candidate future lint. See
  `scripts/yaml-oracle/oracle-discrepancies.md`.
- [ ] **`? : x`** (one-line explicit empty key) is rejected by pandoc but the
  empty-key check intentionally skips explicit (`?`) keys. Low priority;
  characterize against both consumers before tightening.

## Parser - Coverage

This section tracks implementation status of Pandoc Markdown features based on
the spec files in `assets/pandoc-spec/`.

**Focus**: Prioritize **default Pandoc extensions**. Non-default extensions are
lower priority and may be deferred until after core formatting features are
implemented.

### Block-Level Elements

### Paragraphs ✅

- [x] Basic paragraphs
- [x] Paragraph wrapping/reflow
- [x] Extension: `escaped_line_breaks` (backslash at line end)

### Headings ✅

- [x] ATX-style headings (`# Heading`)
- [x] Setext-style headings (underlined with `===` or `---`)
- [x] Heading identifier attributes (`# Heading {#id}`)
- [x] Extension: `blank_before_header` - Require blank line before headings
  (default behavior)
- [x] Extension: `header_attributes` - Full attribute syntax
  `{#id .class key=value}`
- [x] Extension: `implicit_header_references` - Auto-generate reference links

### Block Quotations ✅

- [x] Basic block quotes (`> text`)
- [x] Nested block quotes (`> > nested`)
- [x] Block quotes with paragraphs
- [x] Extension: `blank_before_blockquote` - Require blank before quote (default
  behavior)
- [x] Block quotes containing lists
- [x] Block quotes containing code blocks

### Lists 🚧

- [x] Bullet lists (`-`, `+`, `*`)
- [x] Ordered lists (`1.`, `2.`, etc.)
- [x] Nested lists
- [x] List item continuation
- [x] Complex nested mixed lists
- [x] Extension: `fancy_lists` - Roman numerals, letters `(a)`, `A)`, etc.
- [ ] Extension: `startnum` - Start ordered lists at arbitrary number (low
  priority, if we even should support this)
- [x] Extension: `example_lists` - Example lists with `(@)` markers
- [x] Extension: `task_lists` - GitHub-style `- [ ]` and `- [x]`
- [x] Extension: `definition_lists` - Term/definition syntax

### Code Blocks

- [x] Fenced code blocks (backticks and tildes)
- [x] Code block attributes (language, etc.)
- [x] Indented code blocks (4-space indent)
- [x] Extension: `fenced_code_attributes` - `{.language #id}`
- [x] Extension: `backtick_code_blocks` - Backtick-only fences
- [x] Extension: `inline_code_attributes` - Attributes on inline code

### Horizontal Rules

- [x] Basic horizontal rules (`---`, `***`, `___`)

### Fenced Divs

- [x] Basic fenced divs (`::: {.class}`)
- [x] Nested fenced divs
- [x] Colon count normalization based on nesting
- [x] Proper formatting with attribute preservation

### Tables

- [x] Extension: `simple_tables` - Simple table syntax (parsing complete,
  formatting deferred)
- [x] Extension: `table_captions` - Table captions (both before and after
  tables)
- [x] Extension: `pipe_tables` - GitHub/PHP Markdown tables (all alignments,
  orgtbl variant)
- [x] Extension: `multiline_tables` - Multiline cell content (parsing complete,
  formatting deferred)
- [x] Extension: `grid_tables` - Grid-style tables (parsing complete, formatting
  deferred)

### Line Blocks

- [x] Extension: `line_blocks` - Poetry/verse with `|` prefix

### Inline Elements

#### Emphasis & Formatting

- [x] `*italic*` and `_italic_`
- [x] `**bold**` and `__bold__`
- [x] Nested emphasis (e.g., `***bold italic***`)
- [x] Overlapping and adjacent emphasis handling
- [x] Extension: `intraword_underscores` - `snake_case` handling
- [x] Extension: `strikeout` - `~~strikethrough~~`
- [x] Extension: `superscript` - `^super^`
- [x] Extension: `subscript` - `~sub~`
- [x] Extension: `bracketed_spans` - Small caps `[text]{.smallcaps}`, underline
  `[text]{.underline}`, etc.

#### Code & Verbatim

- [x] Inline code (`code`)
- [x] Multi-backtick code spans (\`\`\`\`\`)
- [x] Code spans containing backticks
- [x] Proper whitespace preservation in code spans
- [x] Fenced code blocks (\`\`\` and \~\~\~)
- [x] Indented code blocks

#### Links

- [x] Inline links `[text](url)`
- [x] Automatic links `<http://example.com>`
- [x] Nested inline elements in link text (code, emphasis, math)
- [x] Reference links `[text][ref]`
- [x] Extension: `shortcut_reference_links` - `[ref]` without second `[]`
- [x] Extension: `link_attributes` - `[text](url){.class}`
- [x] Extension: `implicit_header_references` - `[Heading Name]` links to header

#### Images

- [x] Inline images `![alt](url)`
- [x] Nested inline elements in alt text (code, emphasis, math)
- [x] Reference images `![alt][ref]`
- [x] Image attributes `![alt](url){#id .class key=value}`
- [x] Extension: `implicit_figures`

#### Math

- [x] Inline math `$x = y$`
- [x] Display math `$$equation$$`
- [x] Multi-dollar math spans (e.g., `$$$ $$ $$$`)
- [x] Math containing special characters
- [x] Extension: `tex_math_dollars` - Dollar-delimited math

#### Footnotes

- [x] Inline footnotes `^[note text]`
- [x] Reference footnotes `[^1]` with definition block
- [x] Extension: `inline_notes` - Inline note syntax
- [x] Extension: `footnotes` - Reference-style footnotes

#### Citations

- [x] Extension: `citations` - `[@cite]` and `@cite` syntax with complex key
  support

#### Spans

- [x] Extension: `bracketed_spans` - `[text]{.class}` inline
- [x] Extension: `native_spans` - HTML `<span>` elements with markdown content

### Metadata & Front Matter

#### Metadata Blocks

- [x] Extension: `yaml_metadata_block` - YAML frontmatter
- [x] Extension: `pandoc_title_block` - Title/author/date at top

### Raw Content & Special Syntax

#### Raw HTML

- [x] Extension: `raw_html` - Inline and block HTML
- [x] Extension: `markdown_in_html_blocks` - Markdown inside HTML blocks

#### Raw LaTeX

- [x] Extension: `raw_tex` - Inline LaTeX commands (`\cite{ref}`,
  `\textbf{text}`, etc.)
- [x] Extension: `raw_tex` - Block LaTeX environments
  (`\begin{tabular}...\end{tabular}`)
- [x] Extension: `latex_macros` - Expand LaTeX macros (conversion feature, not
  formatting concern)

#### Other Raw

- [x] Extension: `raw_attribute` - Generic raw blocks `{=format}`

### Escapes & Special Characters

#### Backslash Escapes

- [x] Extension: `all_symbols_escapable` - Backslash escapes any symbol
- [x] Extension: `angle_brackets_escapable` - Escape `<` and `>`
- [x] Escape sequences in inline elements (emphasis, code, math)

#### Line Breaks

- [x] Extension: `escaped_line_breaks` - Backslash at line end = `<br>`

### Non-Default Extensions (Future Consideration)

These extensions are **not enabled by default** in Pandoc and are lower priority
for initial implementation.

#### Non-Default: Emphasis & Formatting

- [x] Extension: `mark` - `==highlighted==` text (non-default)

#### Non-Default: Links

- [x] Extension: `autolink_bare_uris` - Bare URLs as links (non-default)
- [x] Extension: `mmd_link_attributes` - MultiMarkdown link attributes
  (non-default)

#### Non-Default: Math

- [x] Extension: `tex_math_single_backslash` - `\( \)` and `\[ \]` (non-default,
  enabled for RMarkdown)
- [x] Extension: `tex_math_double_backslash` - `\\( \\)` and `\\[ \\]`
  (non-default)
- [x] Extension: `tex_math_gfm` - GitHub Flavored Markdown math (non-default)

#### Non-Default: Metadata

- [x] Extension: `mmd_title_block` - MultiMarkdown metadata (non-default)

#### Non-Default: Headings

- [x] Extension: `mmd_header_identifiers` - MultiMarkdown style IDs
  (non-default)

#### Non-Default: Lists

- [x] Extension behavior: lists can start without a preceding blank line
  (non-default compatibility behavior).
- [x] Add explicit extension-gated handling/config semantics for
  `lists_without_preceding_blankline`.
- [x] Extension behavior: four-space list indentation rules are supported in
  compatibility mode.
- [x] Add explicit extension-gated handling/config semantics for
  `four_space_rule`.

#### Non-Default: Line Breaks

- [x] Extension: `hard_line_breaks` - Newline = `<br>` (non-default)
- [ ] Extension: `ignore_line_breaks` - Ignore single newlines (non-default)
- [x] Extension: `east_asian_line_breaks` - Smart line breaks for CJK
  (non-default)

#### Non-Default: GitHub/CommonMark

- [x] Extension: `alerts` - GitHub/Quarto alert/callout boxes (non-default)
- [x] Extension: `emoji` - `:emoji:` syntax (non-default)
- [x] Extension: `wikilinks_title_after_pipe` - `[[url|title]]` (opt-in; no
  flavor default)
- [x] Extension: `wikilinks_title_before_pipe` - `[[title|url]]` (opt-in; no
  flavor default)

#### Non-Default: Quarto-Specific

- [x] Quarto executable code cells with output
- [x] Quarto cross-references `@fig-id`, `@tbl-id`

#### Non-Default: RMarkdown-Specific

- [x] RMarkdown code chunks with output
- [x] Bookdown-style references (`\@ref(fig-id)`, etc.)

#### Non-Default: Other

- [ ] Extension: `abbreviations` - Abbreviation definitions (non-default)
- [ ] Extension: `attributes` - Universal attribute syntax (non-default,
  commonmark only)
- [ ] Extension: `gutenberg` - Project Gutenberg conventions (non-default)
- [ ] Extension: `markdown_attribute` - `markdown="1"` in HTML (non-default)
- [ ] Extension: `old_dashes` - Old-style em/en dash parsing (non-default)
- [ ] Extension: `rebase_relative_paths` - Rebase relative paths (non-default)
- [ ] Extension: `short_subsuperscripts` - MultiMarkdown `x^2` style
  (non-default)
- [ ] Extension: `sourcepos` - Include source position info (non-default)
- [ ] Extension: `space_in_atx_header` - Allow no space after `#` (non-default)
- [x] Extension: `spaced_reference_links` - Allow space in `[ref] [def]`
  (non-default)

### Won't Implement

- Format-specific output conventions (e.g., `gutenberg` for plain text output)

### Quarto Shortcodes

- [x] Parser support for `{{< name args >}}` syntax
- [x] Parser support for `{{{< name args >}}}` escape syntax
- [x] Formatter with normalized spacing
- [x] Extension flag `quarto_shortcodes` (enabled for Quarto flavor)
- [x] Golden test coverage
- [x] LSP diagnostics for malformed shortcodes
- [x] Completion for built-in shortcode names

### Known Differences from Pandoc

## Architecture

- [ ] Separate additional functionality into dedicated crates (long-term).

## dprint Plugin

A Wasm plugin so dprint users can install Panache via
`dprint config add jolars/dprint-plugin-panache`. The plugin wraps
`panache_formatter::format(..)` behind dprint's `SyncPluginHandler` protocol and
is released independently of the main Panache version.

**Relocated to its own repo (`jolars/dprint-plugin-panache`).** The plugin used
to live in `crates/panache-dprint` here, but its `panache.wasm` release asset
shadowed the CLI `v*` stream for the Zed extension's
`latest_github_release(require_assets: true)` lookup (see AGENTS.md "Release
Management"). It now depends on the published `panache-formatter` crate from
crates.io instead of a path dependency. The remaining open items below live in
that repo:

- [ ] Generate `schema.json` from the plugin's `Configuration` struct (add
  `schemars` derive + a build/CI step), upload alongside the `.wasm` so
  `config_schema_url` resolves.
- [ ] Cut the first plugin release and confirm the publish workflow attaches
  `panache.wasm` + `schema.json` + `.sha256`.
- [ ] Open PR to `dprint/plugins` registry (separate repo from `dprint/dprint`):
  add `jolars/dprint-plugin-panache` to `info.json` and wire up the
  `latest.json` redirect. **Gating step** --- without this,
  `dprint config add jolars/dprint-plugin-panache` cannot resolve.
- [ ] Open PR to `dprint/dprint` (docs only): add Panache to `README.md`'s
  third-party plugins list and `website/src/plugins.md`; add
  `website/src/plugins/panache.md` and
  `website/src/plugins/panache/config.md` (model on
  `website/src/plugins/malva.md` and the corresponding `malva/config.md`).
- [ ] Decide whether to expand the curated config surface (currently 9 keys)
  once the plugin has real usage feedback. Defer until requested.

## Caching

- [ ] Investigate caching strategies for improved performance, particularly for
  CLI linting.

## Math Parser and Formatter

Multi-session effort --- see the `math-parser-formatter` skill
(`.claude/skills/math-parser-formatter/`) for the phased roadmap, locked-in
design decisions, and per-session workflow. Parser invariants:
`.claude/rules/math-parser.md`.

- [x] Math parser producing a lossless structural TeX CST for inline and display
  math (`MATH_CONTENT` subtree; groups, environments, commands, alignment,
  scripts, comments) with a diagnostics side-channel. Landed in
  `crates/panache-parser/src/parser/math.rs`.
- [x] Surface math diagnostics (unclosed/mismatched braces and environments)
  through the linter and LSP. Landed as the always-on `math-syntax` lint
  rule (`src/linter/rules/math_content.rs`), surfaced via the registry to
  CLI + LSP. Derives the five diagnostics directly from the embedded
  `MATH_CONTENT` CST shape (no re-parse); spans are the offending tokens'
  host ranges.
- [x] Math formatter that reformats content semantics-safely (align `&` columns,
  indent environment bodies, normalize `\\`) while preserving idempotency
  (`format(format(math)) == format(math)`), behind an experimental gate.
  Landed as `[experimental] format-math` (default off) routing
  `$$`/`$`/`\[`/`\(` math content through
  `crates/panache-formatter/src/formatter/math/`. Standalone `\begin{env}`
  TeX blocks stay opaque (parser keeps them as `TEX_BLOCK`) --- a possible
  follow-up.
