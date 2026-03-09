# panache TODO

This document tracks implementation status for panache's features.

## Language Server

- [ ] Incremental parsing and caching for LSP performance (Implemented, but
      crude and often reparses entire document on change; needs optimization)

### Salsa/LSP refactor follow-ups

- [x] Remove `DocumentState.metadata` and replace it with
      a minimal YAML-frontmatter status (`yaml_ok: bool` or
      `yaml_error: Option<YamlError>`). Salsa (`crate::salsa::metadata`) is now
      the single source of truth for metadata + bibliography parsing.
- [x] [correctness] Decide watcher policy for uncached dependency files:
      keep `update_file_text_if_cached(...)` (bounded memory) or selectively
      insert (`update_file_text(...)`) for workspace dependency types
      (bibs/includes/metadata).
- [x] [correctness] Model YAML frontmatter parsing as a salsa query (e.g.
      `yaml_metadata_parse_result(...) -> Result<...>`) so diagnostics/handlers
      don’t need a separate pre-check.
- [ ] [optional] Move more LSP diagnostics/lint derivations behind salsa where
      it makes sense (be careful with non-`Send` rowan/CST types; keep external
      linter execution at the async boundary, outside salsa queries).
      - [x] Built-in lint + YAML/metadata diagnostics now derived via salsa
            query (`built_in_lint_plan`); external linters remain async in LSP.
      - [x] Remove duplicate parse in diagnostics path by deriving external
            linter jobs from the same salsa lint-plan query.
- [x] [performance] Apply salsa LRU tuning for long-running LSP sessions
      (see `salsa/book/src/tuning.md`): add `#[salsa::tracked(lru = N)]`
      to high-churn tracked queries where appropriate (`project_graph`,
      `definition_index`).
- [ ] [performance] Evaluate `#[salsa::interned]` for common keys (paths/labels)
      if it reduces memory/cost.
- [ ] [performance] Evaluate salsa durability policy (`set_with_durability`)
      with measurements (e.g., open buffers LOW, stable dependency files
      MEDIUM/HIGH), and only roll out with a clear update/invalidation policy.
      - [x] Wire conservative defaults: open buffers LOW, config MEDIUM,
            dependency/disk-loaded files HIGH, watcher refreshes MEDIUM.
      - [x] Add ignored durability measurement harness
            (`tests/durability_bench.rs`) for HIGH vs LOW revalidation cost.
- [x] [performance] Audit long-running query loops and add cancellation checks
      (`db.unwind_if_revision_cancelled()` in current salsa) where appropriate
      to improve cancellation responsiveness.

### Core LSP Capabilities

- [X] `textDocument/formatting` - Full document formatting
- [X] `textDocument/didOpen` - Track document opens
- [X] `textDocument/didChange` - Track document changes (incremental sync)
- [X] `textDocument/didClose` - Track document closes
- [X] Configuration discovery from workspace root (`.panache.toml`)

### Future LSP Features

#### Diagnostics

- [x] Syntax error diagnostics - Report parsing errors as diagnostics
- [x] Lint warnings - Configurable linting rules (e.g., heading levels, list
      consistency)
- [X] Citation validation - Validate citation keys against bibliography
- [ ] Footnote validation - Check for undefined footnotes (also in linter)
- [ ] Link validation - Check for broken internal links/references

#### Code Actions

- [ ] Convert between bullet/ordered lists
- [X] Convert loose/compact lists
- [ ] Convert bullet list to task list - Convert `- item` to `- [ ] item`
- [ ] Convert between table styles (simple, pipe, grid)
- [ ] Convert between inline/reference links
- [X] Convert between inline/reference footnotes

#### Navigation & Symbols

- [x] Document outline - `textDocument/documentSymbol` for headings, tables,
      figures
- [x] Folding ranges - `textDocument/foldingRange`
      - [x] Code blocks
      - [x] Sections (headings)
- [ ] Go to definition links, images, footnotes). Enabled for some, but not all,
      reference types.
      - [x] Go to definition for reference links - Jump to `[ref]: url`
            definition
      - [x] Go to definition for citations - Jump to bibliography entry for
            `@cite` keys
      - [ ] Go to definition for headings - Jump to heading target for internal
            links
- [ ] Find references - Find all uses of a reference link/footnote/citation
      - [ ] Find references for citations - Find all `@cite` uses of a
            bibliography entry
      - [ ] Find references for headings - Find all internal links to a heading
      - [ ] Find references for reference links - Find all `[text][ref]` links

#### Completion

- [x] Citation completion - `textDocument/completion` for `@cite` keys from
      bibliography
- [ ] Reference link completion - Complete `[text][ref]` from defined references
- [ ] Heading link completion
- [ ] Attribute completion - Complete class names and attributes in
      `{.class #id}`

#### Inlay Hints (low priority)

Personally I think inlay hints are distractive and I am not sure what we want
to support.

- [ ] Link target hints - Show link targets as inlay hints
- [ ] Reference definition hints - Show reference definitions as inlay hints
- [ ] Citation key hints - Show bibliography entries for `@cite` keys
- [ ] Footnote content hints - Show footnote content as inlay hints

#### Hover Information

- [ ] Link preview - `textDocument/hover` to show link target
- [ ] Reference preview - Show reference definition on hover
- [x] Footnote preview - Show footnote content inline
- [x] Citation preview - Show bibliography entry for citation (approximate)

#### Advanced

- [x] Range formatting - `textDocument/rangeFormatting` for selected text only
- [ ] On-type formatting - `textDocument/onTypeFormatting` for auto-formatting
      triggers (not sure about this, low priority)
- [ ] **Document links** - `textDocument/documentLink` for clickable links
- [ ] **Semantic tokens** - Syntax highlighting via LSP
- [ ] Rename
      - [x] Citations - Rename `@cite` keys and update bibliography
      - [x] Reference links - Rename `[ref]` labels and update definitions
      - [ ] Headings - Rename heading text and update internal links
- [ ] Workspace symbols
      - [ ] General support for pandoc etc
      - [ ] Quarto - project-wide symbol search for figures, tables, sections
      - [ ] Rmarkdown (Bookdown)
- [ ] Configuration via LSP - `workspace/didChangeConfiguration` to reload
      config

## Pandoc Test Adoption

- [ ] Pandoc Reader Tests (Markdown.hs)
      - [x] Autolinks/bare URIs
      - [ ] Links/references edge cases
      - [ ] Headers/implicit refs
      - [ ] Emphasis/strong
      - [ ] Lists/definition lists
      - [ ] Footnotes/citations
- [ ] Pandoc Reader Tests (markdown-reader-more.txt)
      - [ ] URLs with spaces/punctuation
      - [ ] Multilingual URLs
      - [ ] Entities in links/titles
      - [ ] Parentheses/backslashes in URLs
      - [ ] Reference link fallbacks

## Configuration System

- [ ] Enable turning on or off linting rules in `[lint]` section

### Per-Flavor Extension Configuration

- [ ] Per-flavor extension overrides - `[extensions.gfm]`,
      `[extensions.quarto]`, `[extensions.rmarkdown]`, etc.
      - Allow fine-grained control of extensions for specific flavors
      - Example: Enable `task_lists` only for GFM, disable `citations` for
        CommonMark
      - Falls back to global `[extensions]` settings when not specified

### Per-File Pattern Overrides

- [ ] Glob pattern flavor overrides - `[flavor_overrides]` with file patterns
      - Override flavor for specific files or patterns
      - Example: `"README.md" = "gfm"` or `"docs/**/*.md" = "gfm"`
      - Useful for projects with mixed Markdown files (e.g., README.md as GFM,
        docs as Pandoc)
      - Could potentially extend to per-pattern extension overrides:
        `[pattern_overrides."docs/**/*.md".extensions]`

## Linter

- [ ] Add support for comments to disable linting on specific lines or blocks
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

- [ ] Per-rule enable/disable in `.panache.toml` `[lint]` section
- [ ] Severity levels (error, warning, info)
- [ ] Auto-fix capability per rule (infrastructure exists, rules need
      implementation)

### Open Questions

- How to balance parser error recovery vs. strict linting?
- Performance: incremental linting for LSP mode?
- LSP: incremental parsing cache (tree reuse on didChange)

## Formatter

- [ ] Add support for comments to disable formatting on specific lines or blocks
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

## Parser - Coverage

This section tracks implementation status of Pandoc Markdown features based on
the spec files in `assets/pandoc-spec/`.

**Focus**: Prioritize **default Pandoc extensions**. Non-default extensions
are lower priority and may be deferred until after core formatting features
are implemented.

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
- [ ] Extension: `implicit_figures`

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

- [ ] Extension: `autolink_bare_uris` - Bare URLs as links (non-default)
- [ ] Extension: `mmd_link_attributes` - MultiMarkdown link attributes
      (non-default)

#### Non-Default: Math

- [x] Extension: `tex_math_single_backslash` - `\( \)` and `\[ \]` (non-default,
      enabled for RMarkdown)
- [x] Extension: `tex_math_double_backslash` - `\\( \\)` and `\\[ \\]`
      (non-default)
- [ ] Extension: `tex_math_gfm` - GitHub Flavored Markdown math (non-default)

#### Non-Default: Metadata

- [ ] Extension: `mmd_title_block` - MultiMarkdown metadata (non-default)

#### Non-Default: Headings

- [ ] Extension: `mmd_header_identifiers` - MultiMarkdown style IDs
      (non-default)

#### Non-Default: Lists

- [ ] Extension: `lists_without_preceding_blankline` (non-default)
- [ ] Extension: `four_space_rule` - Four space vs two space list indent
      (non-default)

#### Non-Default: Line Breaks

- [ ] Extension: `hard_line_breaks` - Newline = `<br>` (non-default)
- [ ] Extension: `ignore_line_breaks` - Ignore single newlines (non-default)
- [ ] Extension: `east_asian_line_breaks` - Smart line breaks for CJK
      (non-default)

#### Non-Default: GitHub-specific

- [ ] Extension: `alerts` - GitHub/Quarto alert/callout boxes (non-default)
- [ ] Extension: `emoji` - `:emoji:` syntax (non-default)
- [ ] Extension: `wikilinks_title_after_pipe` - `[[link|title]]` (non-default)

#### Non-Default: Quarto-Specific

- [x] Quarto executable code cells with output
- [x] Quarto cross-references `@fig-id`, `@tbl-id`

#### Non-Default: RMarkdown-Specific

- [x] RMarkdown code chunks with output
- [x] Bookdown-style references (`\@ref(fig-id)`, etc.\`)

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
- [ ] LSP diagnostics for malformed shortcodes (future)
- [ ] Completion for built-in shortcode names (future)

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

## Architecture

- [ ] Separate out some functionality into separate crates (long-term)
