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
- [x] Debounce per-keystroke diagnostics and defer external linters to save.
      `did_change` now does only synchronous parse + state update and schedules
      a 200ms-debounced built-in lint; external linters (jarl, ruff, ...) run on
      `did_save`. Fixed format-on-save stalling behind a per-keystroke lint
      convoy on large docs (\~1.5s to \~0.11s round-trip).
- [x] Share a single CST pre-order walk across the built-in lint rules. Done for
      the rule corpus: rules declare `node_interests()`/`wants_text_tokens()`
      and the runner builds one `LintIndex` (`src/linter/index.rs`) bucketing
      nodes by `SyntaxKind` (and folding in ignore-range tracking), so each rule
      reads `cx.nodes(KIND)` instead of `tree.descendants()`. Measured via
      `examples/profile_lint.rs`: `pandoc_manual.md` 53.9ms → 47.9ms (-11%),
      `large_authoring.qmd` (Quarto) 3.53ms → 2.98ms (-16%), diagnostics
      unchanged.
- [ ] Follow-up: fold `symbol_usage_index_from_tree`'s \~12 internal
      `tree.descendants()` walks (`src/salsa.rs`) into one dispatching walk too
      --- the other named component of the old `Preorder` cost. Keep
      `SymbolUsageIndex`'s value-equality semantics byte-identical (salsa
      short-circuiting depends on it).
- [ ] Honor `experimental.incrementalParsing` from `settings` /
      `workspace/didChangeConfiguration`, not just initialize
      `initializationOptions`. Editors that send it via `settings` (observed in
      the wild) currently get a no-op toggle (every `did_change` logs
      `full_reparse...incremental_disabled`). Low priority --- parse is \~7ms
      either way --- but the toggle is silently ineffective. Pairs with the
      existing "Configuration via LSP" item below.
- [x] Run heavy background-lint salsa reads on a cloned db handle instead of
      under the global lock. `relint_with_dependents` and `lint_and_publish`
      held the single `Mutex<SalsaDb>` across the cold `project_graph` (\~120ms)
      eval, so interactive requests (formatting/hover) queued behind a
      background lint pass. salsa 0.26's db is `Send` but deliberately `!Sync`,
      so reads now clone a per-task handle (`clone_salsa_db`, a cheap `Arc`
      bump) and run the query with the lock released; `catch_cancelled` aborts a
      pass a concurrent edit cancels. (An `RwLock<SalsaDb>` was the first idea
      but the `!Sync` db rules it out --- `RwLock` needs `T: Sync` for
      concurrent readers.)
- [ ] Follow-up: offload the cold `project_graph` / `built_in_lint_plan` reads
      to `spawn_blocking` if a cold relint is shown to starve the async runtime
      (returns are owned/`Send`; pass `GreenNode`, not the red `SyntaxNode`,
      across the boundary).
- [ ] Follow-up: extend clone-out to the interactive navigation reads
      (goto-definition/references/hover reach `project_graph` under the lock via
      `get_definition_index_with_includes`) if profiling shows they stall behind
      a cold graph. Would need `Cancelled::catch` + retry on the interactive
      path rather than abort.
- [x] Revisit the LSP framework --- switched to rust-analyzer's `lsp-server` (PR
      #357). Synchronous main loop: `GlobalState` is the sole salsa writer (no
      lock), heavy reads dispatch to a small `TaskPool` over a cheap
      `StateSnapshot`, formatting runs on a dedicated single-thread `fmt_pool`,
      and cancellation rides `salsa::Cancelled` + `$/cancelRequest`. Main pool
      defaults to `num_cpus::get_physical()` (matching RA). Resolved the
      salsa-lock contention that prompted the question by construction rather
      than lock-granularity tricks. (Chose `lsp-server` over the earlier
      `async-lsp` candidate to drop the async runtime entirely.)
- [x] Follow-up (lsp-server): respond with `InternalError` when a pooled handler
      panics. `spawn_request_on` (`src/lsp/dispatch.rs`) now wraps the handler
      in `catch_unwind` and posts an `InternalError` `Task::Response` on a
      genuine panic (salsa cancellation still maps to `ContentModified`), so the
      client gets an answer instead of hanging on a never-resolved request id.
      Covered by a unit test
      (`panicking_handler_yields_internal_error_response`).
- [x] Follow-up (lsp-server): drop the dead `runtime_settings` field from
      `StateSnapshot`. It was `#[allow(dead_code)]`, never read off a snapshot,
      yet cloned into every per-request snapshot; removed it and the clone.
- [ ] Follow-up (lsp-server): avoid cloning the full `DocumentState` per
      request. `StateSnapshot::document_state()` and
      `definition_index_with_includes()` clone the whole `DocumentState`
      including the `Vec<ParsedYamlRegionSnapshot>` (path / salsa handles /
      `GreenNode` are cheap `Arc` bumps; the YAML-regions vec is a real
      allocation) on hot paths (hover/completion). Arc-wrap the regions vec or
      return a borrowed view. Profile-gated --- deferred: the vec is typically a
      single small region, and a fix ripples through `RequestContext` plus \~7
      handlers, so not worth it without a profile showing it matters.
- [ ] Follow-up (lsp-server): `$/cancelRequest` does not interrupt in-flight
      work. `on_cancel` only relabels the eventual reply as `RequestCanceled`;
      the pooled job still runs to completion (salsa cancellation fires on a
      write, not an explicit cancel), so a cancelled hover/format burns its full
      CPU cost. Largely inherent (RA is similar); investigate a cooperative
      cancel flag checked at coarse points. Lowest priority --- deferred.
- [x] Follow-up (external-tool budget): formatters (`run_formatters_parallel`)
      and the parallel external-linter loop (`src/linter/runner.rs`) each build
      an independent rayon pool capped at `external_max_parallel`, so a
      concurrent format + lint could spin up to 2x that many subprocess threads.
      Done: a process-wide counting semaphore (`ExternalToolBudget` in
      `src/external_tools_common.rs`) now bounds the live subprocess count
      across both paths. Each block (formatter) / job (linter) acquires a
      permit; the budget is sized from the user-configured
      `external_max_parallel`.
- [ ] Follow-up (external-tool budget, benchmarking): the CLI still forces
      `external_max_parallel = 1` per file when processing multiple files in
      parallel (`src/main.rs`). With the shared budget now providing a real
      global ceiling, this per-file forcing may no longer be the best throughput
      trade-off (N files could share the full budget instead of 1 each).
      Re-benchmark nested format/lint over many files and decide whether to drop
      the override.
- [ ] Follow-up (external-tool budget, benchmarking): the formatter invokes one
      subprocess *per code block* (`run_formatters_parallel` in
      `src/external_formatters_sync.rs` does
      `into_par_iter().filter_map(|block|       … format_code_sync …)`), whereas
      the linter concatenates all same-language blocks into a *single*
      invocation (`concatenate_with_blanks_and_mapping` in
      `src/linter/runner.rs`). For a doc with N same-language blocks that's N
      `black`/`shfmt` spawns vs. the linter's 1. Measure process-spawn +
      stdin/stdout overhead (each `format_with_stdin` also spawns a timeout
      thread) against actual format time for realistic block sizes; if spawn
      dominates, batch same-language blocks through one subprocess using the
      linter's concatenate-with-mapping approach to map output back per block.
      Likely a bigger throughput win than any pool tuning.
- [ ] Follow-up (external-tool budget, benchmarking): confirm the shared budget
      didn't regress the single-command path. On a doc with hundreds of code
      blocks the per-block `Mutex` acquire in `acquire_external_tool_permit`
      (`src/external_tools_common.rs`) is one lock op per block --- cheap vs. a
      subprocess, but verify it isn't contended via an
      `examples/profile_lint.rs`-style measurement. Pairs with building a small
      repeatable CLI harness (polyglot fixtures + `hyperfine`) that also serves
      the `--parallel`-override and batching benchmarks above.
- [x] Follow-up (lsp-server): minor cleanups. Done: (b) the
      `Duration::from_secs(3600)` `select!` fallback is now a documented
      `IDLE_TICK` const in `src/lsp.rs`; (c) `spawn_request_on`'s
      `use_fmt_pool: bool` is now a `RequestPool` enum. Deliberately left: (a)
      deduping `default_external_max_parallel` across the host and the
      dependency-lean formatter crate isn't worth the cross-crate coupling for a
      5-line default; (d) `DocumentMap` stays keyed by `String` --- a `Uri` key
      trips `clippy::mutable_key_type`, which is the whole reason it's a string.
- [ ] Follow-up (lsp-server, benchmarking): the dedicated `fmt_pool` is a single
      thread (`src/lsp/global_state.rs`), deliberately isolating format latency
      from hover/completion. Fine for interactive single-doc editing, but
      format-on-save across many open buffers would serialize through one
      worker. Only worth measuring if a user reports format-on-save lag ---
      lowest priority; the isolation rationale is sound. If revisited, bench a
      small N-thread `fmt_pool` against the current single-thread design under a
      multi-buffer save burst.

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
- [x] Language-aware sentence-wrap abbreviations (#307): select the no-break
      list from the document `lang:` (English, Czech, German, Spanish, French
      built-ins; primary-subtag fallback; `[format] lang` fallback) and let
      users extend it via `[format] no-break-abbreviations` (flat list or
      per-language table, merged with the built-ins). Spanish/French built-in
      lists are conservative starters --- extend as false splits surface.

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
      - **The four findings** --- each was a forward scanner whose raw-line scan
        tripped over the continuation `>` prefix under `list → blockquote`
        nesting (fenced code/math, definition lists, pipe tables, line blocks).
        All fixed and locked in by `*_in_list_blockquote` parser golden cases
        (`fenced_code`, `definition_list`, `pipe_table`, `line_block`);
        pandoc-native reads each as `BulletList → BlockQuote → <block>`. Pipe
        tables, line blocks, and fenced code/math were subsequently folded into
        the shared window (see the follow-up below); definition lists stay
        scalar-threaded. (Formatter round-trip for the nested pipe-table /
        line-block cases is still imperfect --- the BLOCK_QUOTE walker doesn't
        re-emit `>` for continuation lines under a LIST_ITEM; pre-existing, no
        test exercises it.) Implementation detail lives in git history.
      - **Follow-up (window extraction) --- COMPLETE.** Every multi-line-
        lookahead block parser (pipe / grid / simple / multiline tables, line
        blocks, fenced code / math) now scans + emits through the
        `StrippedLines` window (`container_prefix.rs`): detection on
        `strip_all()` / `prefix()`, continuation lines re-emitting their `>` /
        list-indent prefix via `emit_prefix_at` / `emit_or_dispatch_tail`. No
        hold-outs remain. Standalone (empty-prefix) output stays byte-identical
        throughout, and each parser is locked in by a `*_in_list_blockquote`
        parser golden case. Definition lists are out of scope (single-line
        consumer of a pre-stripped view, not a forward scanner). Multiline's
        earlier "deeper than the window" deferral was empirically refuted: the
        failure was raw-line detection, not blank-line segmentation, so the
        window alone fixes it. Per-parser implementation detail lives in git
        history.
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
      - [x] **References.** `[label]: url "title"` now emits `REFERENCE_URL` /
            `REFERENCE_TITLE` nodes via a shared `reference_definition_spans`
            walker that backs both detection and emission (no detect/emit
            drift). Projector reads the nodes; `parse_ref_url` deleted,
            `parse_reference_def` is a pure CST read. `ReferenceDefinition`
            gained `url()`/`title()`.
      - [x] **Attributes --- the `ATTRIBUTE` node.** The Pandoc `{...}`
            `ATTRIBUTE` node now emits `ATTR_ID` / `ATTR_CLASS` /
            `ATTR_KEY_VALUE` (`ATTR_KEY` + `ATTR_VALUE`) children via a shared
            `attribute_content_spans` walker that backs both detection
            (`parse_attribute_content`) and emission (`emit_attribute_node`) ---
            no detect/emit drift. All `ATTRIBUTE` emitters (headings, links,
            images, table captions, inline code, display math) route through it;
            the lossy `emit_attributes` reconstructor is gone. This fixed a live
            losslessness bug: inline-code / display-math attrs were reordered +
            re-quoted in the parser (`` `code`{.r #x key=v} `` →
            `` `code`{#x .r key="v"} ``); they now round-trip byte-for-byte and
            the formatter applies the normalization (via
            `normalize_attribute_text`, as headings already did).
            `AttributeNode` gained `classes()` / `key_values()` and reads
            structured children (precise `id_value_range` from the `ATTR_ID`
            token); the projector reads via `attr_from_attribute_node`.
      - [x] **Attributes --- `DIV_INFO`.** Fenced-div `::: {#id .class key=val}`
            bodies now emit `ATTR_*` children via a shared
            `emit_attribute_node_with_kinds` (factored out of
            `emit_attribute_node`; `emit_div_info_node` keeps bare-word
            shorthand and malformed/empty bodies as one opaque `TEXT` token).
            The projector reads the CST via `attr_from_attribute_node` (gated by
            the new `attr_node_is_structured`), falling back to `parse_div_info`
            for the bare-word case. `AttributeNode` already cast `DIV_INFO`, so
            it now reads structured children. Zero formatter churn (the
            formatter only reads `info_text()`, byte-identical after
            restructuring).
      - [ ] **Attributes --- remaining node kinds.** Apply the same structuring
            to the other attribute-bearing nodes so `parse_attr_block` /
            `parse_html_attrs` can finally be deleted: `SPAN_ATTRIBUTES`
            (bracketed spans) and `CODE_INFO` (code-block info strings,
            language-first semantics) still feed `parse_attr_block`;
            `HTML_ATTRS` (HTML `<div>`/`<span>`, distinct `class=""`/`id=""`
            syntax) feeds `parse_html_attrs`; and raw-inline `{=format}` still
            synthesizes its token (`raw_inline.rs`) rather than wrapping the
            source slice. Touch one node kind at a time.
      - [ ] **HTML opaque-block split.** Continue the HTML lift (Phase 6): lift
            the remaining *opaque* HTML splitting (comments, PI, verbatim, void
            / unmatched tags) into the parser so `split_html_block_by_tags` and
            the recursive `parse_pandoc_blocks` become vestigial. Largest
            bucket; coordinate with the `html-conformance` skill.
      - [ ] **Table separator tokenization.** The separator row is currently a
            coalesced `TEXT` blob (e.g. `TEXT "|:--|--:|"`), so
            `simple_table_aligns`, `grid_dash_widths`, and
            `pipe_separator_aligns` re-tokenize it. Split the markers (`|` /
            `+`, dash runs, colons) into distinct CST tokens so those
            derivations read structure instead of re-scanning a string. Note:
            this only structures the *syntax* --- the derived geometry (widths,
            alignment values) does NOT move into the CST; see below.
      - Legitimately stays in the projector (derived values with no source-byte
        form, not unencoded syntax): column **widths** (a normalized fraction of
        dash counts --- there is no byte that spells `0.33`); table
        **alignment** (the `AlignLeft`/... enum is computed --- from colons for
        pipe/grid tables, from content-vs-dash flushness for simple/multiline
        --- so even though its *evidence* is in the source, the value isn't a
        substring); implicit heading-id slugification (needs whole-document
        dedup); and smart-typography substitution (an output transform). Storing
        any of these as tokens would require synthetic tokens and break CST
        losslessness.
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
- [x] Gate table detection before the whole-buffer `StrippedLines::strip_all`.
      All four table parsers stripped the entire line buffer before their first
      shape check, and detection runs at every block start → O(block-starts ×
      lines), only under Pandoc/RMarkdown flavor (those table types enabled).
      Hoisted each parser's first-line predicate ahead of `strip_all` via
      `strip_at` (commit
      `perf(parser): gate table detection before       whole-buffer strip`).
      73KB math-heavy chapter parse/lint-plan recompute \~250ms → \~118ms;
      CommonMark unaffected. Behavior-preserving (goldens, CST snapshots,
      commonmark allowlist unchanged).
- [ ] Reduce green-tree build cost on large docs. After the table-gate fix, the
      residual \~118ms `built_in_lint_plan` is \~half rowan tree-build
      (`NodeCache::token`, `Arc::drop_slow`, malloc/free) proportional to token
      count. Coalescing per-line `TEXT`+`NEWLINE` pairs in raw/code/math blocks
      into single tokens would cut it. Invasive --- verify CST snapshots and the
      formatter round-trip before changing emitter shape (see the
      rowan-internals note in the `perf-investigation` skill).

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
