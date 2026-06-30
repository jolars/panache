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

- [x] Semantic tokens - Syntax highlighting via LSP (`semanticTokens/full`,
  additive + flavor-gated, custom legend;
  `src/lsp/handlers/semantic_tokens.rs`). Follow-ups: multi-line tokens
  (math/div bodies, per-line split); `full/delta`
  - `result_id`; `range` requests; widen the legend (emphasis/strong/links/
    headings --- only if we decide to contest the base grammar, which flips it
    to opt-in); raw-inline format tags (parser folds `{=fmt}` into a generic
    `ATTRIBUTE`, so a dedicated token kind is needed first).
- [ ] Rename
  - [x] Citations - Rename `@cite` keys and update bibliography
  - [x] Reference links - Rename `[ref]` labels and update definitions
  - [x] Headings - Rename heading text and update internal links
  - [x] Footnotes - Rename footnote labels and update definitions/links
  - [x] Files - Rename linked markdown files and update links
  - [x] Files - Rename other linked files, shortcodes, etc. Covers `embed`,
    `video`, and `placeholder` shortcode paths plus in-document frontmatter
    file paths (`bibliography`, `csl`, `css`). Deferred: raw HTML
    `src`/`href` and raw LaTeX `\input`/`\includegraphics` references;
    nested frontmatter paths such as `format.html.css`.
- [x] Configuration via LSP - `workspace/didChangeConfiguration` to reload
  config

### Spec coverage gaps

Markdown-relevant LSP methods we don't yet implement, surfaced by the 2026-06-18
spec-coverage audit (see `docs/guide/lsp.qmd` "LSP Specification Coverage").
`onTypeFormatting`, `semanticTokens`, `inlayHint`, and
`workspace/didChangeConfiguration` are tracked above and not repeated here.

- [x] Pull diagnostics - `textDocument/diagnostic` + `workspace/diagnostic` as a
  companion/alternative to the current push model (mode-switch: pull clients
  get pull only, push suppressed; cache + `workspace/diagnostic/refresh`)
  - [x] Populate `related_documents` in the document report for clients with
    `related_document_support` (the pulled document's project-graph closure
    carries its related files' cross-file diagnostics inline)
  - [x] Streaming/partial results (`DocumentDiagnosticReportPartialResult`,
    `WorkspaceDiagnosticReportPartialResult`): a `partialResultToken`
    streams the report's tail as `$/progress` chunks (response carries the
    first chunk). No token still returns the whole report
  - [ ] `workspace/diagnostic` only reports open documents + reachable project
    manifests, not every on-disk doc in the workspace (rust-analyzer pulls
    all workspace files). Decide whether closed-but-on-disk docs should
    surface.
- [ ] `textDocument/documentHighlight` - highlight every occurrence of the
  reference/citation/footnote/heading under the cursor
- [ ] `textDocument/selectionRange` - structural smart-select expansion (word →
  inline → block → section)
- [x] `textDocument/linkedEditingRange` - edit a reference label and its
  definition simultaneously
- [x] `completionItem/resolve` - defer expensive completion detail (e.g.
  citation previews) until an item is focused
- [ ] `codeAction/resolve` + advertise `codeActionKinds` - compute edits lazily
  and let clients filter actions by kind
- [ ] `workspace/didChangeWorkspaceFolders` - advertised
  (`change_notifications: true`) but currently unhandled
- [ ] `workspace/configuration` - pull settings from the client instead of
  relying only on discovered config files
- [ ] `workspace/executeCommand` - server-side commands backing complex code
  actions
- [x] File operations beyond `willRenameFiles`: `didRenameFiles`,
  `didCreateFiles`, `didDeleteFiles` (hygiene-only;
  `willCreate`/`willDelete` intentionally omitted)

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

#### YAML frontmatter semantics (yamllint gaps, #385)

Surveyed yamllint's 23 rules against what panache already does. The bulk are
**style** (`braces`/`brackets`/`colons`/`commas`/`hyphens` spacing,
`indentation`, `line-length`, `quoted-strings`, `trailing-spaces`,
`empty-lines`, comment formatting, `key-ordering`, `new-lines`, EOF newline) ---
those belong to the YAML formatter and `panache format --check`, **not** the
linter. Syntax validity and `key-duplicates` are already covered by
`yaml-parse-error` (consumer-aware). The genuine *linter* gaps are the
semantic-value footguns below; none are caught today (verified clean under both
default and `--flavor quarto`):

- [ ] **Undeclared alias (yamllint `anchors`, undeclared case).**
  `ref: *missing` with no matching `&missing` emits nothing today, but an
  undefined alias is a *hard error* in libyaml/js-yaml/PyYAML. This is
  arguably a **validator gap** (belongs in `yaml-parse-error`, not a new
  lint rule) --- requires resolving anchors/aliases during validation.

- [ ] **Duplicate/unused anchors (yamllint `anchors`, remaining cases).**
  Softer, lint-flavored; duplicate anchors (last-wins) and unused anchors.
  Lowest priority.

### Configuration

- [ ] Severity levels (error, warning, info)
- [ ] Auto-fix capability per rule (infrastructure exists, rules need
  implementation)
- [ ] Unwrap the CLI's top-level error print. `main() -> io::Result<()>` renders
  a returned error via `Debug`, so a config (or any other) error surfaces as
  `Error: Custom { kind: InvalidData, error: ... }`. The inner message is
  now readable (`ConfigError`'s `Debug` mirrors `Display`), but the
  `Custom { kind }` wrapper is noise. Fixing it properly means handling
  errors at the \~13 `load_config_for_cli(...)?` call sites (or switching
  `main` to a custom error type with a `Display`-based `Termination`) so the
  user sees just `Error: invalid config <path>: ...`. Affects all
  `io::Error`s, not only config.

### Open Questions

- How to balance parser error recovery vs. strict linting?
- LSP lint dispatch follow-ups (from the cancellation-race fix):
  - Larger redesign still open: shared thread pool + priority queue + lint cap
    (see the `lsp-shared-priority-pool` handoff plan). The all-docs-per-settle
    move left a `MAX_DOCS_PER_SETTLE` backstop hook in `dispatch_due_lints` for
    the lint-cap piece (currently unused; the raised salsa `lru` removes the
    cliff for realistic sessions).

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

### Performance

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

- [x] **Tabs as indentation --- DONE.** A space-vs-tab oracle audit corrected
  the premise: pandoc **never** rejects a tab as indentation (its
  Y79Y/006--009 failures are the separate non-string-key metadata rule,
  which fails with spaces too; pandoc's markdown reader expands tabs before
  YAML parsing). The tab checks (`check_tab_as_indent`,
  `check_quoted_scalar_continuation`) now gate per-consumer via
  `tab_indent_emits(ctx, rejecting)` with per-shape rejecting sets (`{}`,
  `{ryaml}`, `{jsyaml, ryaml}`); the substrate always emits, so
  yaml-test-suite verdicts are unchanged. The host metadata gate
  (`validate_doc_frontmatter`) was made context-aware too, so `panache lint`
  agrees with the parser and never double-reports. See
  `tests/yaml/consumer-matrix.md`.
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

## mdsvex / Svelte-flavored Markdown

MVP support for [mdsvex](https://mdsvex.pngwn.io) (`.svx`, `.svelte.md`). mdsvex
(≤0.12.x) builds on `remark-parse@8`, whose options default to `gfm: true`, so
tables, strikethrough, bare autolinks, and task lists work with **no plugins**
(confirmed by the getting-started example and real plugin-free
`svelte.config.js` setups; `remark-gfm` is only for modern remark). So
`Flavor::Mdsvex` is a CommonMark-*dialect* flavor with the GFM extension set +
`raw_html` + `yaml_metadata_block` + `svelte-template`, minus the extras mdsvex
does not enable by default (footnotes, math, emoji, alerts). The `{...}`
attribute "collision" with Pandoc syntax evaporates because the CommonMark
dialect leaves every attribute extension (`header_attributes`,
`bracketed_spans`, `fenced_divs`, `raw_attribute`) off, so `{` is free for
Svelte. `svelte-template` is off for every other flavor (zero behavior change
elsewhere).

- [x] MVP: `Flavor::Mdsvex` + `svelte-template` extension; `.svx`/`.svelte.md`
  detection; CLI/WASM/schema surfaces.
- [x] Opaque, sigil-distinguished inline spans (`SVELTE_BLOCK_LOGIC` for
  `{#…}`/`{:…}`/`{/…}`, `SVELTE_TAG` for `{@…}`, `SVELTE_EXPRESSION` for
  `{expr}`), content preserved verbatim. Balanced-brace scan reused from the
  shortcode parser. Parser golden + formatter golden + unit tests landed.
- [ ] **Tier 2: block-level `{#if}`/`{#each}` pairing.** Treat standalone
  block-logic lines as opaque *blocks* that act as block boundaries. Today a
  block-logic line that is a lone-node paragraph immediately followed by a
  *tight* list (no blank line) gets joined onto one line by the formatter
  and its inner whitespace collapsed---a **pre-existing formatter quirk
  shared with Quarto shortcodes**, idempotent and parser-lossless, but it
  mangles opaque content. The idiomatic blank-line-separated form is
  preserved verbatim.
- [ ] **Tier 3: format the JS/Svelte inside spans** (prettier-plugin-svelte
  territory). Likely out of scope.
- [ ] String-literal-aware brace matching: a `}` inside a JS string (`{ "}" }`)
  can terminate a span early (depth-counting only). Lossless fallback
  (literal `{`), but a real Svelte tokenizer would fix it.
- [ ] AST wrappers (`syntax/svelte.rs`), LSP semantic tokens, and lint rules for
  Svelte constructs.

## MyST

MyST (`mystmd.org`, `myst-parser`) support, modeled the same way as mdsvex: a
CommonMark-*dialect* flavor whose `myst_defaults` enables MyST-specific
extensions (`myst-directives`, `myst-roles`, `myst-targets`, `myst-comments`)
plus the GFM-superset rules `myst-parser` turns on (`pipe-tables`, `footnotes`).
Behavior is gated on those extension flags, never on `Flavor::Myst` directly, so
other flavors can borrow the same shapes. Markup extras (`myst-colon-fence`,
`myst-substitutions`, dollar-math, deflists, ...) stay opt-in.

- [x] Core constructs: directive parsing (backtick/colon fences, options,
  nesting), inline roles, targets `(label)=`, `%` comments, substitutions
  `{{ name }}`; directive-body formatting. Parser + formatter golden cases.
- [x] GFM-superset defaults: `pipe-tables` + `footnotes` on for `Flavor::Myst`
  (standalone tables and `[^ref]` footnotes were mangled without them).
  `inline-footnotes` stays off: `myst-parser` loads the footnote plugin with
  `inline=False`.
- [x] Smoke corpus vendored from `jupyter-book/myst-spec` with a losslessness +
  idempotency harness and a non-gating output-divergence triage report
  (`cargo test --test myst_corpus -- --ignored --nocapture`).
- [x] **Structural directive-option parsing.** The leading `:key: value` option
  block is parsed into structured `MYST_DIRECTIVE_OPTION` nodes
  (`name`/`value` tokens, colon markers), consumed in the directive opener
  and terminated by the first non-option line per MyST semantics, so a blank
  line is no longer required to keep them off the body (e.g.
  `:number-lines: 1` followed directly by `def five(): return 5` no longer
  merges). The formatter emits them canonically (`:name: value`, one blank
  line before the body), retiring the `is_myst_option_paragraph` stopgap. We
  require a well-formed `:name:` key rather than `myst-parser`'s looser
  "line starts with `:`", which avoids swallowing colon-fence closers and
  nested openers.
- [x] **Verbatim-bodied directives.** `{code}`, `{code-block}`, `{code-cell}`,
  and `{math}` now capture their body as a raw `MYST_DIRECTIVE_BODY` node at
  parse time (mirroring fenced code blocks), so the formatter passes it
  through byte-for-byte instead of markdown-reflowing it. Gated on the
  directive name in `try_parse_directive_open`; the opener parser consumes
  the whole verbatim directive single-pass and skips the markdown-body
  container.
  - [x] **External formatters/linters for verbatim directive bodies.** A
    `{code-block} python` body is now a real code body but is invisible to
    the `[formatters.*]`/`[linters.*]` external-tool path:
    `collect_code_blocks`
    (`crates/panache-formatter/src/formatter/code_blocks.rs`) only walks
    `CODE_BLOCK` nodes, so black/flake8 et al. never see it. Route
    `MYST_DIRECTIVE_BODY` through the same path, keyed by the directive
    argument (`MYST_DIRECTIVE_ARG`) as the language, like a fenced code
    block.
  - [ ] **Premature closer on a longer inner fence.** A body line whose fence
    run is at least the opener's count (e.g. a 4-backtick fence inside a
    3-backtick `{code-block}`) is treated as the closer. This matches the
    container path's behavior and is rare (MyST expects a longer *outer*
    fence), but a robust fix would track the opener width and only close on
    an exact or shorter-or-equal match per MyST semantics.
- [ ] Audit remaining `myst-parser` default-on rules beyond tables/footnotes
  (e.g. whether any other markdown-it core rule should default on for MyST).
- [ ] AST wrappers (`syntax/myst.rs`), LSP semantic tokens, and lint rules for
  MyST constructs.

## Architecture

- [ ] Separate additional functionality into dedicated crates (long-term).

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
