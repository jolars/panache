# panache TODO

This document tracks implementation status for panache's features.

**Status Legend**

- ✅ **Implemented** - Feature is fully or mostly implemented
- 🚧 **Partial** - Feature is partially implemented or needs work
- ❌ **Not Implemented** - Feature not yet started
- ⏹️ **Won't Implement** - Feature intentionally not implemented

## Language Server Protocol (LSP)

### Core LSP Capabilities

- ✅ `textDocument/formatting` - Full document formatting
- ✅ `textDocument/didOpen` - Track document opens
- ✅ `textDocument/didChange` - Track document changes (incremental sync)
- ✅ `textDocument/didClose` - Track document closes
- ✅ Configuration discovery from workspace root (`.panache.toml`)

### Future LSP Features

#### Diagnostics

- ❌ **Syntax error diagnostics** - Report parsing errors as diagnostics
- ❌ **Lint warnings** - Configurable linting rules (e.g., heading levels, list
  consistency)
- ❌ **Link validation** - Check for broken internal links/references
- ❌ **Citation validation** - Validate citation keys against bibliography
- ❌ **Footnote validation** - Check for undefined footnotes (also in linter)

#### Code Actions

- ❌ **Convert lists** - Convert between bullet/ordered lists
- ✅ **Convert loose/compact lists** - Toggle between loose and compact list
- ❌ Convert bullet list to task list - Convert `- item` to `- [ ] item`
- ❌ **Convert table** - Convert between table styles (simple, pipe, grid)
- ❌ **Convert link styles** - Convert between inline/reference links
- ✅ **Convert footnote styles** - Convert between inline/reference footnotes

#### Navigation & Symbols

- ✅ **Document outline** - `textDocument/documentSymbol` for headings, tables,
  figures
- ✅ **Folding ranges** - `textDocument/foldingRange` for headings, code blocks,
  fenced divs, YAML frontmatter
- ✅ **Go to definition** - Jump to reference link/footnote definitions
  (reference links, images, footnotes)
- ❌ Go to definition for citations - Jump to bibliography entry for `@cite`
  keys
- ❌ **Find references** - Find all uses of a reference link/footnote/citation

#### Completion

- ❌ **Citation completion** - `textDocument/completion` for `@cite` keys from
  bibliography
- ❌ **Reference link completion** - Complete `[text][ref]` from defined
  references
- ❌ **Heading link completion** - Complete internal links to headings
- ❌ **Attribute completion** - Complete class names and attributes in
  `{.class #id}`

#### Inlay Hints (low priority)

- ❌ **Link target hints** - Show link targets as inlay hints
- ❌ **Reference definition hints** - Show reference definitions as inlay hints
- ❌ **Citation key hints** - Show bibliography entries for `@cite` keys
- ❌ **Footnote content hints** - Show footnote content as inlay hints

#### Hover Information

- ❌ **Link preview** - `textDocument/hover` to show link target
- ❌ **Reference preview** - Show reference definition on hover
- ❌ **Citation preview** - Show bibliography entry for citation
- ✅ **Footnote preview** - Show footnote content inline

#### Advanced

- ✅ **Range formatting** - `textDocument/rangeFormatting` for selected text
  only
- ❌ **On-type formatting** - `textDocument/onTypeFormatting` for
  auto-formatting triggers (not sure about this, low priority)
- ❌ **Document links** - `textDocument/documentLink` for clickable links
- ❌ **Semantic tokens** - Syntax highlighting via LSP
- ❌ **Rename** - Rename reference links/footnotes/citations across document and bibliography
- ❌ **Workspace symbols** - Search for headings across all workspace documents
- ❌ **Configuration via LSP** - `workspace/didChangeConfiguration` to reload
  config

---

## Configuration System

### Current Features

- ✅ Hierarchical config loading (explicit → local → XDG → defaults)
- ✅ Auto-detect flavor from file extension (.qmd → Quarto, .Rmd → RMarkdown)
- ✅ `flavor` config field affects .md files and stdin
- ✅ Global `[extensions]` overrides for all flavors
- ✅ `[formatters.<formatter>]` configuration for external code formatters
- ✅ `[linters]` configuration for external code linters

### Future Enhancements

#### Per-Flavor Extension Configuration

- ❌ **Per-flavor extension overrides** - `[extensions.gfm]`,
  `[extensions.quarto]`, `[extensions.rmarkdown]`, etc.
  - Allow fine-grained control of extensions for specific flavors
  - Example: Enable `task_lists` only for GFM, disable `citations` for
    CommonMark
  - Falls back to global `[extensions]` settings when not specified

#### Per-File Pattern Overrides

- ❌ **Glob pattern flavor overrides** - `[flavor_overrides]` with file patterns
  - Override flavor for specific files or patterns
  - Example: `"README.md" = "gfm"` or `"docs/**/*.md" = "gfm"`
  - Useful for projects with mixed Markdown files (e.g., README.md as GFM, docs
    as Pandoc)
  - Could potentially extend to per-pattern extension overrides:
    `[pattern_overrides."docs/**/*.md".extensions]`

## Linter

- ❌ Auto-fixing for external code linters

### Current Status

**✅ Implemented** - Basic linter with CLI and one rule.

**Current features:**

- ✅ `panache lint` CLI subcommand
- ✅ `--check` mode for CI (exit 1 if violations found)
- ✅ `--fix` mode for auto-fixing violations
- ✅ Diagnostic output with file locations
- ✅ Pluggable rule system with `RuleRegistry`
- ✅ **Implemented rule:** `heading-hierarchy` - Warns when heading levels are
  skipped (e.g., h1 → h3)

### Architecture

Follows the ruff/clippy pattern: separate concerns, shared infrastructure

```
src/linter/           # Core linting logic
  ├── rules/          # Individual lint rules
  │   └── heading_hierarchy.rs  # Heading level checking
  ├── diagnostics.rs  # Diagnostic types (Diagnostic, Severity, Fix, Edit)
  ├── rules.rs        # Rule trait and registry
  └── runner.rs       # Rule execution
src/main.rs           # CLI: `panache lint` subcommand
src/lsp.rs            # LSP: TODO - integrate diagnostics
```

Both linter and formatter:

- ✅ Share the same parser and AST
- ✅ Use the same config system
- ✅ Benefit from rowan's CST

### CLI Commands

```bash
panache lint document.qmd           # Report violations
panache lint --fix document.qmd     # Auto-fix what's possible
panache lint --check document.qmd   # CI mode: exit non-zero if violations
panache lint --config cfg.toml      # Custom config
```

### Future Lint Rules

**Syntax correctness:**

- ❌ Malformed fenced divs (unclosed, invalid attributes)
- ❌ Broken table structures
- ❌ Invalid citation syntax (`@citekey` malformations)
- ❌ Unclosed inline math/code spans
- ❌ Invalid shortcode syntax (Quarto-specific)

**Style/Best practices:**

- ✅ Inconsistent heading hierarchy (skip levels)
- ❌ Multiple top-level headings
- ❌ Empty links/images
- ✅ Duplicate reference labels
- ❌ Unused reference definitions
- ❌ Hard-wrapped text in code blocks

**Configuration:**

- ❌ Per-rule enable/disable in `.panache.toml` `[lint]` section
- ❌ Severity levels (error, warning, info)
- ❌ Auto-fix capability per rule (infrastructure exists, rules need
  implementation)

### Next Steps

- [ ] Add more lint rules (empty links, duplicate refs, etc.)
- [ ] Make rules configurable via `[lint]` section in config
- [ ] LSP integration with `textDocument/publishDiagnostics`
- [ ] Add auto-fix implementations for fixable rules

### Open Questions

- Should linter rules be pluggable (external crates)?
- How to balance parser error recovery vs. strict linting?
- Performance: incremental linting for LSP mode?

## Formatter

### Tables

- ✅ Simple tables
- ✅ Pipe tables
- ✅ Grid tables
- ✅ Multiline tables

## Parser

### Performance

- 🚧 Avoid temporary green tree when injecting `BLOCKQUOTE_MARKER` tokens into
  inline-parsed paragraphs (current approach parses inlines into a temp tree,
  then replays while inserting markers)

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
- ⏹️ Extension: `implicit_header_references` - Auto-generate reference links
  (conversion feature, not formatting concern)

### Block Quotations ✅

- ✅ Basic block quotes (`> text`)
- ✅ Nested block quotes (`> > nested`)
- ✅ Block quotes with paragraphs
- ✅ Extension: `blank_before_blockquote` - Require blank before quote (default
  behavior)
- ✅ Block quotes containing lists
- ✅ Block quotes containing code blocks


### Lists 🚧

- ✅ Bullet lists (`-`, `+`, `*`)
- ✅ Ordered lists (`1.`, `2.`, etc.)
- ✅ Nested lists
- ✅ List item continuation
- ✅ Complex nested mixed lists
- ✅ Extension: `fancy_lists` - Roman numerals, letters `(a)`, `A)`, etc.
- ❌ Extension: `startnum` - Start ordered lists at arbitrary number (low
  priority)
- ✅ Extension: `example_lists` - Example lists with `(@)` markers
- ✅ Extension: `task_lists` - GitHub-style `- [ ]` and `- [x]`
- ✅ Extension: `definition_lists` - Term/definition syntax

### Code Blocks ✅

- ✅ Fenced code blocks (backticks and tildes)
- ✅ Code block attributes (language, etc.)
- ✅ Indented code blocks (4-space indent)
- ✅ Extension: `fenced_code_attributes` - `{.language #id}`
- ✅ Extension: `backtick_code_blocks` - Backtick-only fences
- ✅ Extension: `inline_code_attributes` - Attributes on inline code

### Horizontal Rules ✅

- ✅ Basic horizontal rules (`---`, `***`, `___`)

### Fenced Divs ✅

- ✅ Basic fenced divs (`::: {.class}`)
- ✅ Nested fenced divs
- ✅ Colon count normalization based on nesting
- ✅ Proper formatting with attribute preservation

### Tables ✅

- ✅ Extension: `simple_tables` - Simple table syntax (parsing complete,
  formatting deferred)
- ✅ Extension: `table_captions` - Table captions (both before and after tables)
- ✅ Extension: `pipe_tables` - GitHub/PHP Markdown tables (all alignments,
  orgtbl variant)
- ✅ Extension: `multiline_tables` - Multiline cell content (parsing complete,
  formatting deferred)
- ✅ Extension: `grid_tables` - Grid-style tables (parsing complete, formatting
  deferred)

### Line Blocks ✅

- ✅ Extension: `line_blocks` - Poetry/verse with `|` prefix

## Inline Elements

### Emphasis & Formatting ✅

- ✅ `*italic*` and `_italic_`
- ✅ `**bold**` and `__bold__`
- ✅ Nested emphasis (e.g., `***bold italic***`)
- ✅ Overlapping and adjacent emphasis handling
- ✅ Extension: `intraword_underscores` - `snake_case` handling
- ✅ Extension: `strikeout` - `~~strikethrough~~`
- ✅ Extension: `superscript` - `^super^`
- ✅ Extension: `subscript` - `~sub~`
- ✅ Extension: `bracketed_spans` - Small caps `[text]{.smallcaps}`, underline
  `[text]{.underline}`, etc.

### Code & Verbatim ✅

- ✅ Inline code (`` `code` ``)
- ✅ Multi-backtick code spans (``` `` ` `` ```)
- ✅ Code spans containing backticks
- ✅ Proper whitespace preservation in code spans
- ✅ Fenced code blocks (``` and ~~~)
- ✅ Indented code blocks

### Links ✅

- ✅ Inline links `[text](url)`
- ✅ Automatic links `<http://example.com>`
- ✅ Nested inline elements in link text (code, emphasis, math)
- ✅ Reference links `[text][ref]`
- ✅ Extension: `shortcut_reference_links` - `[ref]` without second `[]`
- ✅ Extension: `link_attributes` - `[text](url){.class}`
- ⏹️ Extension: `implicit_header_references` - `[Heading Name]` links to header
  (conversion feature, not formatting concern)

### Images ✅

- ✅ Inline images `![alt](url)`
- ✅ Nested inline elements in alt text (code, emphasis, math)
- ✅ Reference images `![alt][ref]`
- ✅ Image attributes `![alt](url){#id .class key=value}`
- ⏹️ Extension: `implicit_figures` - Conversion feature, not formatting concern

### Math ✅

- ✅ Inline math `$x = y$`
- ✅ Display math `$$equation$$`
- ✅ Multi-dollar math spans (e.g., `$$$ $$ $$$`)
- ✅ Math containing special characters
- ✅ Extension: `tex_math_dollars` - Dollar-delimited math

### Footnotes ✅

- ✅ Inline footnotes `^[note text]`
- ✅ Reference footnotes `[^1]` with definition block
- ✅ Extension: `inline_notes` - Inline note syntax
- ✅ Extension: `footnotes` - Reference-style footnotes

### Citations ✅

- ✅ Extension: `citations` - `[@cite]` and `@cite` syntax with complex key
  support

### Spans ✅

- ✅ Extension: `bracketed_spans` - `[text]{.class}` inline
- ✅ Extension: `native_spans` - HTML `<span>` elements with markdown content

---

## Metadata & Front Matter

### Metadata Blocks ✅

- ✅ Extension: `yaml_metadata_block` - YAML frontmatter
- ✅ Extension: `pandoc_title_block` - Title/author/date at top

## Raw Content & Special Syntax

### Raw HTML ✅

- ✅ Extension: `raw_html` - Inline and block HTML
- ❌ Extension: `markdown_in_html_blocks` - Markdown inside HTML blocks

### Raw LaTeX ✅

- ✅ Extension: `raw_tex` - Inline LaTeX commands (`\cite{ref}`,
  `\textbf{text}`, etc.)
- ✅ Extension: `raw_tex` - Block LaTeX environments
  (`\begin{tabular}...\end{tabular}`)
- ⏹️ Extension: `latex_macros` - Expand LaTeX macros (conversion feature, not
  formatting concern)

### Other Raw

- ✅ Extension: `raw_attribute` - Generic raw blocks `{=format}` (blocks ✅,
  inline spans ✅)

## Escapes & Special Characters

### Backslash Escapes ✅

- ✅ Extension: `all_symbols_escapable` - Backslash escapes any symbol
- ✅ Extension: `angle_brackets_escapable` - Escape `<` and `>`
- ✅ Escape sequences in inline elements (emphasis, code, math)

### Line Breaks ✅

- ✅ Extension: `escaped_line_breaks` - Backslash at line end = `<br>`

## Non-Default Extensions (Future Consideration)

These extensions are **not enabled by default** in Pandoc and are lower priority
for initial implementation.

### Non-Default: Emphasis & Formatting

- ❌ Extension: `mark` - `==highlighted==` text (non-default)

### Non-Default: Links

- ❌ Extension: `autolink_bare_uris` - Bare URLs as links (non-default)
- ❌ Extension: `mmd_link_attributes` - MultiMarkdown link attributes
  (non-default)

### Non-Default: Math

- ✅ Extension: `tex_math_single_backslash` - `\( \)` and `\[ \]` (non-default,
  enabled for RMarkdown)
- ✅ Extension: `tex_math_double_backslash` - `\\( \\)` and `\\[ \\]`
  (non-default)
- ❌ Extension: `tex_math_gfm` - GitHub Flavored Markdown math (non-default)

### Non-Default: Metadata

- ❌ Extension: `mmd_title_block` - MultiMarkdown metadata (non-default)

### Non-Default: Headings

- ❌ Extension: `mmd_header_identifiers` - MultiMarkdown style IDs (non-default)

### Non-Default: Lists

- ❌ Extension: `lists_without_preceding_blankline` (non-default)
- ❌ Extension: `four_space_rule` - Four space vs two space list indent
  (non-default)

### Non-Default: Line Breaks

- ❌ Extension: `hard_line_breaks` - Newline = `<br>` (non-default)
- ❌ Extension: `ignore_line_breaks` - Ignore single newlines (non-default)
- ❌ Extension: `east_asian_line_breaks` - Smart line breaks for CJK
  (non-default)

### Non-Default: GitHub-specific

- ❌ Extension: `alerts` - GitHub/Quarto alert/callout boxes (non-default)
- ❌ Extension: `emoji` - `:emoji:` syntax (non-default)
- ❌ Extension: `wikilinks_title_after_pipe` - `[[link|title]]` (non-default)

### Non-Default: Quarto-Specific

- ✅ Quarto executable code cells with output
- ❌ Quarto cross-references `@fig-id`, `@tbl-id`

### Non-Default: RMarkdown-Specific

- ❌ RMarkdown code chunks with output
- ❌ Bookdown-style references (`\@ref(fig-id)`, etc.`)

### Non-Default: Other

- ❌ Extension: `abbreviations` - Abbreviation definitions (non-default)
- ❌ Extension: `attributes` - Universal attribute syntax (non-default,
  commonmark only)
- ❌ Extension: `gutenberg` - Project Gutenberg conventions (non-default)
- ❌ Extension: `markdown_attribute` - `markdown="1"` in HTML (non-default)
- ❌ Extension: `old_dashes` - Old-style em/en dash parsing (non-default)
- ❌ Extension: `rebase_relative_paths` - Rebase relative paths (non-default)
- ❌ Extension: `short_subsuperscripts` - MultiMarkdown `x^2` style
  (non-default)
- ❌ Extension: `sourcepos` - Include source position info (non-default)
- ❌ Extension: `space_in_atx_header` - Allow no space after `#` (non-default)
- ❌ Extension: `spaced_reference_links` - Allow space in `[ref] [def]`
  (non-default)

---

## Won't Implement

- Format-specific output conventions (e.g., `gutenberg` for plain text output)

## Quarto Shortcodes

- ✅ Parser support for `{{< name args >}}` syntax
- ✅ Parser support for `{{{< name args >}}}` escape syntax
- ✅ Formatter with normalized spacing
- ✅ Extension flag `quarto_shortcodes` (enabled for Quarto flavor)
- ✅ Golden test coverage
- ❌ LSP diagnostics for malformed shortcodes (future)
- ❌ Completion for built-in shortcode names (future)

### Known Differences from Pandoc

#### Reference Link Parsing

**Status**: 🤔 **Deferred architectural decision**

**Issue**: Our CST structure differs from Pandoc's AST for undefined reference links.

**Current behavior**:

- `[undefined]` (no definition exists) → Parsed as LINK node in CST
- Pandoc behavior: `[undefined]` → Parsed as literal text `Str "[undefined]"`
- **Formatting output is correct** (both produce `[undefined]`)

**Impact**: 

- ✅ No impact on formatting (our primary use case)
- ✅ No impact on LSP features (uses CST traversal)
- ✅ No impact on linting (uses CST traversal)
- ⚠️ CST structure differs from Pandoc's AST (only matters for library users inspecting CST)

**Possible solutions** (if needed):

1. CST traversal during inline parsing to check if definition exists (O(n) cost per reference)
2. Minimal registry with `HashSet<String>` of definition labels (O(1) lookup)
3. Two-pass parsing (parse blocks first, then inline with definition knowledge)

**Decision**: Accept current behavior until a real-world use case requires matching Pandoc's AST structure exactly.

