# Panache TODO

This document tracks implementation status for Panache's features.

## Language Server

- [x] Incremental parsing and caching for LSP performance
- [x] Optimize incremental edit handling to avoid full-document reparses for
      multi-change or complex `didChange` updates.

### Performance

- [x] Introduce `#[salsa::interned]` for common keys (paths/labels).
- [x] Measure interned key impact (memory/cost) and decide whether to expand to
      additional key types.
- [x] Wire conservative salsa durability defaults (`set_with_durability`): open
      buffers LOW, config MEDIUM, dependency/disk-loaded files HIGH, watcher
      refreshes MEDIUM.
- [x] Add ignored durability measurement harness (`tests/durability_bench.rs`)
      for HIGH vs LOW revalidation cost.
- [x] Finalize and document durability update/invalidation policy based on
      measurements before broader rollout.

### Core LSP Capabilities

- [x] `textDocument/formatting` - Full document formatting
- [x] `textDocument/didOpen` - Track document opens
- [x] `textDocument/didChange` - Track document changes (incremental sync)
- [x] `textDocument/didClose` - Track document closes
- [x] Configuration discovery from workspace root (`.panache.toml`)

### Diagnostics

- [x] Syntax error diagnostics - Report parsing errors as diagnostics
- [x] Lint warnings - Configurable linting rules (e.g., heading levels, list
      consistency)
- [x] Citation validation - Validate citation keys against bibliography
- [x] Footnote validation - Check for undefined footnotes (also in linter)
- [x] Link validation - Check for broken internal links/references

### Code Actions

- [x] Convert between bullet/ordered lists
- [x] Convert loose/compact lists
- [x] Convert list to task list - Convert `- item`/`1. item` to `- [ ] item`
- [ ] Convert between table styles (simple, pipe, grid)
- [ ] Convert between inline/reference links
- [x] Convert between inline/reference footnotes

### Navigation & Symbols

- [x] Document outline - `textDocument/documentSymbol` for headings, tables,
      figures
- [x] Folding ranges - `textDocument/foldingRange`
      - [x] Code blocks
      - [x] Sections (headings)
- [x] Go to definition links, images, footnotes).
      - [x] Go to definition for reference links - Jump to `[ref]: url`
            definition
      - [x] Go to definition for citations - Jump to bibliography entry for
            `@cite` keys
      - [x] Go to definition for headings - Jump to heading target for internal
            links
      - [x] Go to definition for footnotes - Jump to footnote definition block
- [x] Find references - Find all uses of a reference link/footnote/citation
      - [x] Find references for citations - Find all `@cite` uses of a
            bibliography entry
      - [x] Find references for headings - Find all internal links to a heading
      - [ ] Find references for reference links - Find all `[text][ref]` links

### Completion

- [x] Citation completion - `textDocument/completion` for `@cite` keys from
      bibliography
- [ ] Reference link completion - Complete `[text][ref]` from defined references
- [ ] Heading link completion
- [ ] Attribute completion - Complete class names and attributes in
      `{.class #id}`
- [ ] Shortcode completion - Complete Quarto shortcode names in `{{< name >}}`
- [ ] Cross-reference completion - Complete `@fig-id` and `\@ref(fig-id)`
      cross-refs

### Inlay Hints (low priority)

Personally I think inlay hints are distracting and I am not sure what we want to
support.

- [ ] Link target hints - Show link targets as inlay hints
- [ ] Reference definition hints - Show reference definitions as inlay hints
- [ ] Citation key hints - Show bibliography entries for `@cite` keys
- [ ] Footnote content hints - Show footnote content as inlay hints

### Hover Information

- [x] Link preview - `textDocument/hover` to show link target
- [x] Reference preview - Show reference definition on hover
- [x] Footnote preview - Show footnote content inline
- [x] Citation preview - Show bibliography entry for citation
- [x] Heading preview - Show section content or summary on hover

### Advanced

- [x] Range formatting - `textDocument/rangeFormatting` for selected text only
- [ ] On-type formatting - `textDocument/onTypeFormatting` for auto-formatting
      triggers (not sure about this, low priority)
- [x] Document links - `textDocument/documentLink` for clickable links
- [ ] Semantic tokens - Syntax highlighting via LSP
- [ ] Rename
      - [x] Citations - Rename `@cite` keys and update bibliography
      - [x] Reference links - Rename `[ref]` labels and update definitions
      - [x] Headings - Rename heading text and update internal links
      - [x] Footnotes - Rename footnote labels and update definitions/links
      - [x] Files - Rename linked markdown files and update links
      - [ ] Files - Rename other linked files, shortcodes, etc.
- [x] Workspace symbols
      - [x] General support for pandoc etc
      - [x] Quarto - project-wide symbol search for figures, tables, sections
      - [x] Rmarkdown (Bookdown)
- [ ] Configuration via LSP - `workspace/didChangeConfiguration` to reload
      config

## Configuration System

- [x] Enable turning on or off linting rules in `[lint]` section
- [x] Per-flavor extension overrides - `[extensions.gfm]`,
- [x] Glob pattern flavor overrides - `[flavor_overrides]` with file patterns

## Linter

- [x] Add support for comments to disable linting on specific lines or blocks
      (e.g., `<!-- something -->`)
- [x] Auto-fixing for external code linters

### Future Lint Rules

#### Syntax correctness

- [x] Malformed fenced divs (unclosed, invalid attributes)
- [ ] Broken table structures
- [ ] Invalid citation syntax (`@citekey` malformations)
- [ ] Unclosed inline math/code spans
- [ ] Invalid shortcode syntax (Quarto-specific)

#### Style/Best practices

- [x] Inconsistent heading hierarchy (skip levels)
- [x] Duplicate reference labels
- [ ] Multiple top-level headings
- [ ] Empty links/images
- [ ] Unused reference definitions
- [ ] Hard-wrapped text in code blocks
- [ ] Use blanklines around horizontal rules

### Configuration

- [x] Per-rule enable/disable in `.panache.toml` `[lint]` section
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

## Formatter

- [x] Add support for comments to disable formatting on specific lines or blocks
      (e.g., `<!-- something-->`)

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
- [ ] `biome`
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
- [ ] `csharpier`
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
- [ ] `indent`
- [ ] `injected`
- [ ] `inko`
- [ ] `isort`
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
- [ ] `mix`
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
- [ ] `ormolu`
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
- [ ] `runic`
- [ ] `rustfmt`
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
- [ ] `stylua`
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

- [x] Add wrappers for block quotes, code blocks/chunks, display math, and
      fenced divs.
- [x] Add wrappers for footnote definitions/references.
- [x] Add wrapper for alert blocks.
- [x] Add wrappers for YAML metadata/title blocks.
- [x] Add wrappers for raw TeX blocks/commands.
- [x] Add a Paragraph convenience wrapper for repeated semantic checks.

### Tables

- [x] Simple tables
- [x] Pipe tables
- [x] Grid tables
- [x] Multiline tables

## Parser

### Architecture

- [x] Unify the line-prefix stripping vocabulary used by the block dispatcher's
      helpers. `ContainerPrefix` (in `parser/blocks/container_prefix.rs`)
      bundles `list_content_col`, `bq_depth`, and a
      `list_marker_consumed_on_line_0` flag. The three helpers
      `pandoc_html_open_tag_closes`, `find_multiline_open_end`, and
      `parse_html_block_with_wrapper` take it instead of bare `bq_depth`, so
      stacked-container dispatches strip both marker families. `BqPrefixState`
      and `LinePrefixState` collapsed into a unified `ContainerPrefixState` for
      graft re-injection. Unblocked pandoc-conformance 0452/0453 (`- > <div>...`
      single- and multi-line); html 257 → 259, total 452 → 454.
- [x] Follow-up to the `ContainerPrefix` work: remove the dual `ctx.content` /
      raw-`&self.lines, line_pos` redundancy in the `BlockParser` trait.
      Completed across three landed refactors: stack-walked `ContainerPrefix`
      (`SmallVec<[StripOp; 8]>` recipe of `ListAdvance` / `BlockQuoteMarker` /
      `ContentIndent` ops); migration of all 21 `BlockParser` impls to read the
      dispatch line via `lines.first()`; deletion of `BlockContext.content` and
      `BlockContext.list_marker_consumed_on_line_0` (the latter routed directly
      from `self.dispatch_list_marker_consumed` into
      `ContainerPrefix::from_stack`); deletion of the unused `BlockPrefixInfo`
      cache; and addition of the `footnote_with_blockquote` parser golden case
      locking in the bq-in-footnote strip-order semantics. A drive-by bug fell
      out: `shifted_blockquote_from_list` was sourcing `list_content_col` from
      `paragraphs::current_content_col` (which folds in the `FootnoteDefinition`
      content_col), double-counting the footnote indent for
      `[FootnoteDef, BlockQuote, Paragraph]` stacks and leaving
      continuation-line `>` bytes stranded as raw TEXT inside the paragraph.
      Source `list_content_col` from `ListItem` only and gate the probe on
      either `list_content_col > 0` or `current_blockquote_depth() > 0` so
      paragraph-continuation lines starting with `>` inside footnotes
      (angle-link variants) are not misclassified.
      - **Optional polish (deferred)**: delete the `from_ctx` shim by threading
        `&[Container]` or a pre-built prefix through `BlockContext` (4 callers
        remain: `parse_line` top-level dispatch, the in-flight bq-detect
        emission path, and two `ContinuationPolicy` helpers). The
        `dispatch_list_marker_consumed` toggle in `parse_line`'s shifted-bq
        paths is a side-band signal that could be replaced by a
        `StripOp::ListAdvanceConditional` variant so the prefix self-encodes the
        semantic.
- [x] Audit other multi-line-lookahead block parsers for the same misfire class.
      Audit complete; all four findings fixed (fenced code, definition lists,
      line blocks, pipe tables).
      - **Fenced code** --- fixed. `parse_fenced_code_block` /
        `parse_fenced_math_block` now take
        `(list_content_col,         list_marker_consumed_on_line_0, bq_outer, content_indent)`
        and apply the strip in container-stack order. The line-0 marker case
        uses `advance_columns` (so the non-whitespace list-marker bytes get
        skipped past silently) and the continuation case uses
        `byte_index_at_column` (so blank lines aren't eaten). Adjacent
        WHITESPACE emissions are coalesced for byte-range- equivalent CST
        stability. Locked in by parser golden case
        `fenced_code_in_list_blockquote`.
      - **Definition lists** --- fixed. The marker-line branch of
        `handle_definition_list_effect` was slicing the **raw**
        `self.lines[pos]` using `content_start_bytes` computed against the
        **container-stripped** `content` argument from `StrippedLines`, so the
        post-marker view still carried the outer `>` prefix and
        `count_blockquote_markers` fabricated a phantom inner blockquote (+4
        bytes). The fix slices `content` directly for `content_slice` /
        `content_line` (and for the fence-open `fence_line` in the same branch,
        proactively). Locked in by parser golden case
        `definition_list_in_list_blockquote`. Pandoc-native reads this as
        `BulletList → BlockQuote → DefinitionList`; our CST keeps Term as a
        PARAGRAPH outside the DEFINITION_LIST (same status quo as before; only
        the +4 losslessness break was in scope here).
      - **Pipe tables** --- fixed. `try_parse_pipe_table` now takes
        `(prefix: &ContainerPrefix, dispatch_line)` and builds a no-alloc
        container-stripped line view (`strip_line_0_for_emission` on the
        dispatch line, full `strip` elsewhere) that every detection scan
        (separator/cell shape, end-of-table, caption helpers) runs against, so a
        `- > | a | b |` shape is recognized instead of falling back to paragraph
        emission. Emission re-emits the per-continuation-line `>` prefix as
        `WHITESPACE`/`BLOCK_QUOTE_MARKER` tokens inside each `TABLE_SEPARATOR` /
        `TABLE_ROW` node (via `emit_content_line_prefixes`), parsing cells from
        the stripped tail; the `TABLE_HEADER` on the dispatch line emits no
        prefix (the core consumed it). With an empty prefix (non-nested tables)
        every strip is a no-op, so existing output stays byte-identical. The
        dispatcher threads `lines.prefix()` + `line_pos`. (Grid + simple tables
        were later migrated onto the same window --- see the follow-up below;
        multiline remains deferred because the container fragments it before
        dispatch.) Locked in by parser golden case
        `pipe_table_in_list_blockquote`. Pandoc-native reads this as
        `BulletList → BlockQuote → Table`, which our
        `LIST → LIST_ITEM → BLOCK_QUOTE → PIPE_TABLE` shape matches. (Formatter
        round-trip for the nested case is still imperfect, as with the
        line-block sibling: the separator-cell alignment scan trims the `>`
        prefix into a junk cell. Pre-existing class of issue, out of scope here;
        no test exercises that round-trip.)
      - **Line blocks** --- fixed. `parse_line_block` now takes
        `(bq_depth, list_content_col, list_marker_consumed_on_line_0, bq_outer, content_indent)`
        mirroring fenced code, with a `silent_strip_container_prefix` peek for
        continuation/next-marker detection and an `emit_open_line_prefixes`
        helper on the dispatch line (a near-clone of `prepare_fence_open_line`
        minus the final leading-space strip, since `LINE_BLOCK_MARKER` swallows
        any leading spaces before `|`). The dispatcher
        (`LineBlockParser::parse_prepared`) threads container geometry from
        `lines.prefix()` / `ctx`; the over-strict raw-line guard in
        `detect_prepared` was also removed. Helpers `strip_list_indent` /
        `emit_blockquote_prefix_tokens` / `emit_content_line_prefixes` were
        promoted to `pub(crate)` in `code_blocks.rs`. Formatter LINE_BLOCK
        walker updated to skip leading WS/BQ_MARKER prefix tokens before
        emitting the marker line, since the parser now emits those prefix tokens
        inside LINE_BLOCK_LINE. Locked in by parser golden case
        `line_block_in_list_blockquote`. Pandoc-native reads this as
        `BulletList → BlockQuote → LineBlock`. (Formatter round-trip for the
        nested case is still imperfect because the BLOCK_QUOTE walker doesn't
        re-emit `>` for continuation lines under a LIST_ITEM --- pre-existing,
        unrelated to this fix; no test exercises that round-trip.)
      - **Follow-up (window extraction) --- done for pipe tables, line blocks,
        grid tables, and simple tables.** The per-line container-stripping
        pattern is now consolidated onto the `StrippedLines` window
        (`container_prefix.rs`). The prefix emitters
        (`emit_content_line_prefixes`, `emit_blockquote_prefix_tokens`,
        `strip_list_indent`, `bq_outer_of_list`) were relocated from
        `code_blocks.rs` down into `container_prefix.rs` (the lower layer that
        hosts `StrippedLines`) and re-exported, so the window can own the emit
        step without an import cycle. `StrippedLines` gained a `dispatch` field
        plus `with_dispatch`, `strip_at(i)` (peek, absolute index),
        `emit_prefix_at(builder, i) -> tail` (continuation emit),
        `dispatch_tail`, and `iter_from_base`. Pipe tables dropped their
        hand-rolled materialized `Vec<&str>` for `strip_all()` and emit via the
        window; line blocks dropped `silent_strip_container_prefix` and route
        continuation lines through `strip_at` / `emit_prefix_at` (the bespoke
        dispatch-line emitter `emit_open_line_prefixes` stays). Every
        replacement routes through the same underlying functions the snapshots
        pin --- all four `*_in_list_blockquote` snapshots plus the `pipe_table*`
        and line-block snapshots are byte-identical. Definition lists are out of
        scope (single-line consumer of a pre-stripped view, not a forward
        scanner).
        - **Grid + simple tables --- done.** Verified (not assumed) that both
          misfired identically to pipe tables under `list → blockquote` nesting:
          their forward scans ran against raw `lines` still carrying `>` on
          continuation rows, so detection failed and they fell back to a
          `PARAGRAPH` (lossless but structurally wrong).
          `try_parse_simple_table` / `try_parse_grid_table` now take a
          `&StrippedLines` window: detection scans run on `window.strip_all()`,
          emission routes each line through a new
          `StrippedLines::emit_or_dispatch_tail(builder, i)` helper
          (`dispatch_tail` on the dispatch line, `emit_prefix_at` on
          continuation lines). `emit_table_row` / `emit_grid_table_row` take the
          window + absolute index/indices; grid's row accumulator switched from
          `Vec<&str>` to `Vec<usize>` (absolute indices) so each physical line
          of a multi-line row re-emits its prefix. Captions + internal blank
          lines still read raw `lines` (nested-caption is the untested corner,
          same as pipe tables). Empty prefix ⇒ every strip is a no-op, so all
          pre-existing grid/simple snapshots stay byte-identical. Locked in by
          parser golden cases `grid_table_in_list_blockquote` /
          `simple_table_in_list_blockquote` (`GRID_TABLE` / `SIMPLE_TABLE`, not
          `PARAGRAPH`).
        - **Remaining (deferred, standalone task)**: migrate fenced code / math
          (`parse_fenced_code_block` / `parse_fenced_math_block` in
          `code_blocks.rs`) onto the window. Kept separate because it carries
          structural snapshot-drift risk the table/line-block migrations did
          not: (1) the closing-fence detection loop runs
          `count_blockquote_markers(probe)` with a `line_bq_depth < bq_depth`
          break --- a forward-scan *termination* condition the window's
          `strip_at` can't express (would need a new `bq_survivors_at(i)` method
          or stay inline); (2) the open-fence emitter `prepare_fence_open_line`
          (with `first_line_override`) has no window equivalent and must stay
          bespoke. Recommend partial migration --- emission-loop
          `emit_content_line_prefixes` → `emit_prefix_at`, detection
          `inner_stripped` → `strip_at`, keep the bq-break inline, leave
          `prepare_fence_open_line` / `strip_content_line_prefixes` untouched
          --- gated hard on every fenced-code snapshot.
        - **Remaining (deferred, deeper than the window)**: multiline tables
          (`try_parse_multiline_table`) under `list → blockquote` nesting. The
          window is necessary but *not sufficient* here: a multiline table
          requires internal blank rows, and to stay inside the blockquote those
          must be `>`-only lines --- but a `>`-only "blank" line makes the
          **blockquote's** block parser split the content into two separate
          `PARAGRAPH`s *before* the table dispatcher ever sees a contiguous
          slice (verified: `BLOCK_QUOTE → PARAGRAPH, BLANK_LINE, PARAGRAPH`).
          That's container-level blank-line block segmentation, a layer above
          the table dispatcher. Grid/simple have no internal blank lines so they
          arrive contiguous and the window alone fixes them; multiline needs a
          deeper change to how `>`-prefixed blank lines terminate block
          accumulation inside containers (and only then a window migration of
          the parser itself). Out of scope for the window work.
- [ ] Stop letting `pandoc_ast.rs` drift into a second-stage parser. Load-
      bearing byte-walkers (`split_html_block_by_tags`, `parse_pandoc_blocks`
      and the refs/heading-id reparse helpers) re-tokenize source the CST should
      already encode. This violates the single-pass invariant in `AGENTS.md` and
      hides structural decisions from downstream consumers (linter, salsa, LSP,
      formatter) which all walk the CST, not the projector. Phase 6 of the HTML
      lift is the ongoing migration; the goal is for `pandoc_ast.rs` to be a
      pure CST → native projection with no byte reparse.
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

### Performance

- [ ] Avoid temporary green tree when injecting `BLOCK_QUOTE_MARKER` tokens into
      inline-parsed paragraphs (current approach parses inlines into a temp
      tree, then replays while inserting markers)

### Long-term YAML parser groundwork

- [x] Build an in-tree YAML parser module (`src/parser/yaml.rs`) as a long-term
      project with lossless CST goals.
- [x] Add shared YAML input/model groundwork for plain YAML files and
      hashpipe-prefixed YAML (frontmatter/chunk metadata), including host-range
      mapping scaffolding.
- [ ] Complete one production-grade shared parser core for plain + hashpipe YAML
      with full feature coverage.
- [x] Add shadow/read-only rollout scaffolding for in-tree YAML parsing.
- [ ] Add robust parity checks against existing YAML behavior before any
      formatter or edit-path replacement.
- [ ] Add first-class YAML formatting support after parser parity, using shared
      CST and idempotency-focused formatting tests for both plain YAML and
      hashpipe-prefixed YAML.
- [x] Add pinned yaml-test-suite fixtures under `tests/fixtures/yaml-test-suite`
      with an update script (`scripts/update-yaml-test-suite-fixtures.sh`).

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
- [ ] Add explicit extension-gated handling/config semantics for
      `four_space_rule`.

#### Non-Default: Line Breaks

- [x] Extension: `hard_line_breaks` - Newline = `<br>` (non-default)
- [ ] Extension: `ignore_line_breaks` - Ignore single newlines (non-default)
- [ ] Extension: `east_asian_line_breaks` - Smart line breaks for CJK
      (non-default)

#### Non-Default: GitHub/CommonMark

- [x] Extension: `alerts` - GitHub/Quarto alert/callout boxes (non-default)
- [x] Extension: `emoji` - `:emoji:` syntax (non-default)
- [ ] Extension: `wikilinks_title_after_pipe` - `[[link|title]]` (opt-in; no
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
- [ ] Extension: `spaced_reference_links` - Allow space in `[ref] [def]`
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

#### Smart Abbreviation Non-Breaking Spaces

- [x] Keep recognized abbreviations + following year together during wrapping
      (for example `M.A. 2007`) so wrapping does not split them.
- [x] Follow Pandoc `Ext_smart` behavior exactly by converting the post-
      abbreviation space to a non-breaking space.

## Architecture

- [x] Split out WASM support into a separate crate (`crates/panache-wasm`).
- [ ] Separate additional functionality into dedicated crates (long-term).

## dprint Plugin

A Wasm plugin so dprint users can install Panache via
`dprint add jolars/panache`. The new `crates/panache-dprint` crate wraps
`panache_formatter::format(..)` behind dprint's `SyncPluginHandler` protocol;
released independently of the main Panache version.

- [x] Add `crates/panache-dprint` crate (excluded from workspace, builds for
      `wasm32-unknown-unknown` only).
- [x] CI workflow `publish-dprint-wasm.yml` triggered on
      `dprint-plugin-panache-v*` tags; builds the `.wasm`, computes SHA256,
      uploads to the GitHub release.
- [x] Track in `versionary.jsonc` as its own package (independent versioning).
- [x] Local end-to-end smoke test against `dprint fmt`: parity with the panache
      CLI on `.md`/`.qmd`/`.Rmd` and idempotency confirmed.
- [ ] Generate `schema.json` from the plugin's `Configuration` struct (add
      `schemars` derive + a build/CI step), upload alongside the `.wasm` so
      `config_schema_url` resolves.
- [ ] Cut the first plugin release: land a `feat(dprint): ...` commit so
      versionary tags `dprint-plugin-panache-vX.Y.Z`, and confirm the publish
      workflow attaches `panache.wasm` + `schema.json` + `.sha256`.
- [ ] Open PR to `dprint/plugins` registry (separate repo from `dprint/dprint`):
      add `jolars/panache` to `info.json` and wire up the `latest.json`
      redirect. **Gating step** --- without this, `dprint add jolars/panache`
      cannot resolve.
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

- [ ] Implement a math parser that produces a CST for inline and display math
      (initial focus on TeX math with `$...$` and `$$...$$` delimiters).
- [ ] Implement a math formatter that can reformat math content while preserving
      semantics and idempotency (i.e., `format(format(math)) == format(math)`).
