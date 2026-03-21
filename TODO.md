# panache TODO

This document tracks implementation status for panache's features.

## Language Server

- [x] Incremental parsing and caching for LSP performance
- [ ] Optimize incremental edit handling to avoid full-document reparses for
      multi-change or complex `didChange` updates.

### Performance

- [x] Introduce `#[salsa::interned]` for common keys (paths/labels).
- [ ] Measure interned key impact (memory/cost) and decide whether to expand to
      additional key types.
- [x] Wire conservative salsa durability defaults (`set_with_durability`): open
      buffers LOW, config MEDIUM, dependency/disk-loaded files HIGH, watcher
      refreshes MEDIUM.
- [x] Add ignored durability measurement harness (`tests/durability_bench.rs`)
      for HIGH vs LOW revalidation cost.
- [ ] Finalize and document durability update/invalidation policy based on
      measurements before broader rollout.

### Core LSP Capabilities

- [X] `textDocument/formatting` - Full document formatting
- [X] `textDocument/didOpen` - Track document opens
- [X] `textDocument/didChange` - Track document changes (incremental sync)
- [X] `textDocument/didClose` - Track document closes
- [X] Configuration discovery from workspace root (`.panache.toml`)

### Diagnostics

- [x] Syntax error diagnostics - Report parsing errors as diagnostics
- [x] Lint warnings - Configurable linting rules (e.g., heading levels, list
      consistency)
- [x] Citation validation - Validate citation keys against bibliography
- [x] Footnote validation - Check for undefined footnotes (also in linter)
- [x] Link validation - Check for broken internal links/references

### Code Actions

- [ ] Convert between bullet/ordered lists
- [X] Convert loose/compact lists
- [ ] Convert bullet list to task list - Convert `- item` to `- [ ] item`
- [ ] Convert between table styles (simple, pipe, grid)
- [ ] Convert between inline/reference links
- [X] Convert between inline/reference footnotes

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

### Inlay Hints (low priority)

Personally I think inlay hints are distractive and I am not sure what we want to
support.

- [ ] Link target hints - Show link targets as inlay hints
- [ ] Reference definition hints - Show reference definitions as inlay hints
- [ ] Citation key hints - Show bibliography entries for `@cite` keys
- [ ] Footnote content hints - Show footnote content as inlay hints

### Hover Information

- [ ] Link preview - `textDocument/hover` to show link target
- [ ] Reference preview - Show reference definition on hover
- [X] Footnote preview - Show footnote content inline
- [x] Citation preview - Show bibliography entry for citation (approximate)

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
      - [ ] Footnotes - Rename footnote labels and update definitions/links
      - [ ] Files - Rename files and update links throughout (maybe complex)
- [x] Workspace symbols
      - [x] General support for pandoc etc
      - [x] Quarto - project-wide symbol search for figures, tables, sections
      - [ ] Rmarkdown (Bookdown)
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

- [ ] Malformed fenced divs (unclosed, invalid attributes)
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

### Open Questions

- How to balance parser error recovery vs. strict linting?
- Performance: incremental linting for LSP mode?
- LSP: incremental parsing cache (tree reuse on didChange)

## Formatter

- [x] Add support for comments to disable formatting on specific lines or blocks
      (e.g., `<!-- something-->`)

### Tables

- [x] Simple tables
- [x] Pipe tables
- [x] Grid tables
- [x] Multiline tables

## Parser

### Performance

- [ ] Avoid temporary green tree when injecting `BLOCKQUOTE_MARKER` tokens into
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

- ✅ Basic paragraphs
- ✅ Paragraph wrapping/reflow
- ✅ Extension: `escaped_line_breaks` (backslash at line end)

### Headings ✅

- ✅ ATX-style headings (`# Heading`)
- ✅ Setext-style headings (underlined with `===` or `---`)
- ✅ Heading identifier attributes (`# Heading {#id}`)
- ✅ Extension: `blank_before_header` - Require blank line before headings
  (default behavior)
- ✅ Extension: `header_attributes` - Full attribute syntax
  `{#id .class key=value}`
- ✅ Extension: `implicit_header_references` - Auto-generate reference links

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
- [ ] Extension: `markdown_in_html_blocks` - Markdown inside HTML blocks

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

- [ ] Extension: `mark` - `==highlighted==` text (non-default)

#### Non-Default: Links

- [x] Extension: `autolink_bare_uris` - Bare URLs as links (non-default)
- [ ] Extension: `mmd_link_attributes` - MultiMarkdown link attributes
      (non-default)

#### Non-Default: Math

- [x] Extension: `tex_math_single_backslash` - `\( \)` and `\[ \]` (non-default,
      enabled for RMarkdown)
- [x] Extension: `tex_math_double_backslash` - `\\( \\)` and `\\[ \\]`
      (non-default)
- [x] Extension: `tex_math_gfm` - GitHub Flavored Markdown math (non-default)

#### Non-Default: Metadata

- [ ] Extension: `mmd_title_block` - MultiMarkdown metadata (non-default)

#### Non-Default: Headings

- [ ] Extension: `mmd_header_identifiers` - MultiMarkdown style IDs
      (non-default)

#### Non-Default: Lists

- [x] Extension behavior: lists can start without a preceding blank line
      (non-default compatibility behavior).
- [ ] Add explicit extension-gated handling/config semantics for
      `lists_without_preceding_blankline`.
- [x] Extension behavior: four-space list indentation rules are supported in
      compatibility mode.
- [ ] Add explicit extension-gated handling/config semantics for
      `four_space_rule`.

#### Non-Default: Line Breaks

- [ ] Extension: `hard_line_breaks` - Newline = `<br>` (non-default)
- [ ] Extension: `ignore_line_breaks` - Ignore single newlines (non-default)
- [ ] Extension: `east_asian_line_breaks` - Smart line breaks for CJK
      (non-default)

#### Non-Default: GitHub-specific

- [x] Extension: `alerts` - GitHub/Quarto alert/callout boxes (non-default)
- [x] Extension: `emoji` - `:emoji:` syntax (non-default)
- [ ] Extension: `wikilinks_title_after_pipe` - `[[link|title]]` (non-default)

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
- [ ] LSP diagnostics for malformed shortcodes
- [ ] Completion for built-in shortcode names

### Known Differences from Pandoc

#### Reference Link Parsing

**Status**: 🤔 **Deferred architectural decision**

**Issue**: Our CST structure differs from Pandoc's AST for undefined reference
links.

**Current behavior**:

- `[undefined]` (no definition exists) → Parsed as LINK node in CST
- Pandoc behavior: `[undefined]` → Parsed as literal text `Str "[undefined]"`
- **Formatting output is correct** (both produce `[undefined]`)

**Impact**:

- ✅ No impact on formatting (our primary use case)
- ✅ No impact on LSP features (uses CST traversal)
- ✅ No impact on linting (uses CST traversal)
- ⚠️ CST structure differs from Pandoc's AST (only matters for library users
  inspecting CST)

**Possible solutions** (if needed):

1. CST traversal during inline parsing to check if definition exists (O(n) cost
   per reference)
2. Minimal registry with `HashSet<String>` of definition labels (O(1) lookup)
3. Two-pass parsing (parse blocks first, then inline with definition knowledge)

**Decision**: Accept current behavior until a real-world use case requires
matching Pandoc's AST structure exactly.

#### Smart Abbreviation Non-Breaking Spaces

- [x] Keep recognized abbreviations + following year together during wrapping
      (for example `M.A. 2007`) so wrapping does not split them.
- [ ] Follow Pandoc `Ext_smart` behavior exactly by converting the post-
      abbreviation space to a non-breaking space.

## Architecture

- [x] Split out WASM support into a separate crate (`crates/panache-wasm`).
- [ ] Separate additional functionality into dedicated crates (long-term).

## Caching

- [ ] Investigate caching strategies for improved performance, particularly for
      CLI linting.
