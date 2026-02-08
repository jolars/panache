# panache TODO - Comprehensive Pandoc Feature Coverage

This document tracks implementation status of Pandoc Markdown features based on the spec files in `docs/pandoc-spec/`.

**Focus**: Initial development prioritizes **default Pandoc extensions**. Non-default extensions are tracked separately for future consideration.

## Status Legend

- ✅ **Implemented** - Feature is fully or mostly implemented
- 🚧 **Partial** - Feature is partially implemented or needs work
- ❌ **Not Implemented** - Feature not yet started

---

## Block-Level Elements

### Paragraphs ✅

- ✅ Basic paragraphs
- ✅ Paragraph wrapping/reflow
- ✅ Extension: `escaped_line_breaks` (backslash at line end)

### Headings ✅

- ✅ ATX-style headings (`# Heading`)
- ✅ Setext-style headings (underlined with `===` or `---`)
- ✅ Heading identifier attributes (`# Heading {#id}`)
- ✅ Extension: `blank_before_header` - Require blank line before headings (default behavior)
- 🚧 Extension: `header_attributes` - Full attribute syntax `{#id .class key=value}`
- ❌ Extension: `implicit_header_references` - Auto-generate reference links

### Block Quotations ✅

- ✅ Basic block quotes (`> text`)
- ✅ Nested block quotes (`> > nested`)
- ✅ Block quotes with paragraphs
- ✅ Extension: `blank_before_blockquote` - Require blank before quote (default behavior)
- 🚧 Block quotes containing lists (works but may need polish)
- 🚧 Block quotes containing code blocks (needs testing)

### Lists 🚧

- ✅ Bullet lists (`-`, `+`, `*`)
- ✅ Ordered lists (`1.`, `2.`, etc.)
- ✅ Nested lists
- 🚧 List item continuation (works but formatter needs improvement)
- 🚧 Complex nested mixed lists (fragile, needs parser structure improvement)
- ❌ Extension: `fancy_lists` - Roman numerals, letters `(a)`, `A)`, etc.
- ❌ Extension: `startnum` - Start ordered lists at arbitrary number
- ❌ Extension: `example_lists` - Example lists with `(@)` markers
- ❌ Extension: `task_lists` - GitHub-style `- [ ]` and `- [x]`
- ✅ Extension: `definition_lists` - Term/definition syntax

### Code Blocks ✅

- ✅ Fenced code blocks (backticks and tildes)
- ✅ Code block attributes (language, etc.)
- ✅ Indented code blocks (4-space indent)
- ✅ Extension: `fenced_code_attributes` - `{.language #id}`
- ✅ Extension: `backtick_code_blocks` - Backtick-only fences
- ❌ Extension: `inline_code_attributes` - Attributes on inline code

### Horizontal Rules ✅

- ✅ Basic horizontal rules (`---`, `***`, `___`)

### Fenced Divs ✅

- ✅ Basic fenced divs (`::: {.class}`)
- ✅ Nested fenced divs
- ✅ Colon count normalization based on nesting
- ✅ Proper formatting with attribute preservation

### Tables ✅

- ✅ Extension: `simple_tables` - Simple table syntax (parsing complete, formatting deferred)
- ✅ Extension: `table_captions` - Table captions (both before and after tables)
- ✅ Extension: `pipe_tables` - GitHub/PHP Markdown tables (all alignments, orgtbl variant)
- ✅ Extension: `multiline_tables` - Multiline cell content (parsing complete, formatting deferred)
- ✅ Extension: `grid_tables` - Grid-style tables (parsing complete, formatting deferred)

### Line Blocks ❌

- ❌ Extension: `line_blocks` - Poetry/verse with `|` prefix

---

## Inline Elements

### Emphasis & Formatting ✅

- ✅ `*italic*` and `_italic_` - Full emphasis parsing with proper delimiter rules
- ✅ `**bold**` and `__bold__` - Strong emphasis with nesting support
- ✅ Nested emphasis (e.g., `***bold italic***`)
- ✅ Overlapping and adjacent emphasis handling
- ✅ Extension: `intraword_underscores` - `snake_case` handling
- ❌ Extension: `strikeout` - `~~strikethrough~~`
- ❌ Extension: `superscript`, `subscript` - `^super^` and `~sub~`
- ❌ Small caps - `[text]{.smallcaps}`
- ❌ Underline - `[text]{.underline}`

### Code & Verbatim ✅

- ✅ **Inline code** (`` `code` ``) - Full implementation with delimiter matching
- ✅ Multi-backtick code spans (``` `` ` `` ```)
- ✅ Code spans containing backticks
- ✅ Proper whitespace preservation in code spans
- ✅ **Fenced code blocks** (``` and ~~~) - Full implementation
- ✅ **Indented code blocks** (4 spaces or 1 tab) - Full implementation with blockquote support

### Links ✅

- ✅ Inline links `[text](url)`
- ✅ Automatic links `<http://example.com>`
- ✅ Nested inline elements in link text (code, emphasis, math)
- ❌ Reference links `[text][ref]`
- ❌ Extension: `shortcut_reference_links` - `[ref]` without second `[]`
- ❌ Extension: `link_attributes` - `[text](url){.class}`
- ❌ Extension: `implicit_header_references` - `[Heading Name]` links to header

### Images ✅

- ✅ Inline images `![alt](url)`
- ✅ Nested inline elements in alt text (code, emphasis, math)
- ❌ Reference images `![alt][ref]`
- ❌ Extension: `implicit_figures` - Paragraph with just image becomes figure

### Math ✅

- ✅ Inline math `$x = y$` - Full implementation with proper escaping
- ✅ Display math `$$equation$$` - Block and inline contexts, with proper formatting
- ✅ Multi-dollar math spans (e.g., `$$$ $$ $$$`)
- ✅ Math containing special characters
- ✅ Extension: `tex_math_dollars` - Dollar-delimited math

### Footnotes ✅

- ✅ Inline footnotes `^[note text]` - Full support with nested inline elements
- ❌ Reference footnotes `[^1]` with definition block
- ✅ Extension: `inline_notes` - Inline note syntax

### Citations ❌

- ❌ Extension: `citations` - `[@cite]` and `@cite` syntax

### Spans ✅

- ✅ Extension: `bracketed_spans` - `[text]{.class}` inline
- ❌ Extension: `native_spans` - HTML `<span>` elements

---

## Metadata & Front Matter

### Metadata Blocks ✅

- ✅ Extension: `yaml_metadata_block` - YAML frontmatter
- ✅ Extension: `pandoc_title_block` - Title/author/date at top

---

## Raw Content & Special Syntax

### Raw HTML ❌

- ❌ Extension: `raw_html` - Inline and block HTML
- ❌ Extension: `markdown_in_html_blocks` - Markdown inside HTML blocks
- ❌ Extension: `markdown_attribute` - `markdown="1"` attribute

### Raw LaTeX ⚠️

- ✅ Extension: `raw_tex` - Inline LaTeX commands (`\cite{ref}`, `\textbf{text}`, etc.)
- ❌ Extension: `raw_tex` - Block LaTeX environments (`\begin{tabular}...\end{tabular}`)
- ❌ Extension: `latex_macros` - Expand LaTeX macros

### Other Raw ❌

- ❌ Extension: `raw_attribute` - Generic raw blocks `{=format}`

---

## Escapes & Special Characters

### Backslash Escapes ✅

- ✅ Extension: `all_symbols_escapable` - Backslash escapes any symbol
- ✅ Extension: `angle_brackets_escapable` - Escape `<` and `>`
- ✅ Escape sequences in inline elements (emphasis, code, math)

### Line Breaks ✅

- ✅ Extension: `escaped_line_breaks` - Backslash at line end = `<br>`

---

## Non-Default Extensions (Future Consideration)

These extensions are **not enabled by default** in Pandoc and are lower priority for initial implementation.

### Non-Default: Emphasis & Formatting

- ❌ Extension: `mark` - `==highlighted==` text (non-default)

### Non-Default: Links

- ❌ Extension: `autolink_bare_uris` - Bare URLs as links (non-default)
- ❌ Extension: `mmd_link_attributes` - MultiMarkdown link attributes (non-default)

### Non-Default: Math

- ❌ Extension: `tex_math_single_backslash` - `\( \)` and `\[ \]` (non-default)
- ❌ Extension: `tex_math_double_backslash` - `\\( \\)` and `\\[ \\]` (non-default)
- ❌ Extension: `tex_math_gfm` - GitHub Flavored Markdown math (non-default)

### Non-Default: Metadata

- ❌ Extension: `mmd_title_block` - MultiMarkdown metadata (non-default)

### Non-Default: Headings

- ❌ Extension: `mmd_header_identifiers` - MultiMarkdown style IDs (non-default)

### Non-Default: Lists

- ❌ Extension: `lists_without_preceding_blankline` (non-default)
- ❌ Extension: `four_space_rule` - Four space vs two space list indent (non-default)

### Non-Default: Line Breaks

- ❌ Extension: `hard_line_breaks` - Newline = `<br>` (non-default)
- ❌ Extension: `ignore_line_breaks` - Ignore single newlines (non-default)
- ❌ Extension: `east_asian_line_breaks` - Smart line breaks for CJK (non-default)

### Non-Default: GitHub/Quarto-Specific

- ❌ Extension: `alerts` - GitHub/Quarto alert/callout boxes (non-default)
- ❌ Extension: `emoji` - `:emoji:` syntax (non-default)
- ❌ Extension: `wikilinks_title_after_pipe` - `[[link|title]]` (non-default)
- ❌ Quarto executable code cells with output
- ❌ Quarto cross-references `@fig-id`, `@tbl-id`
- ❌ Quarto callout blocks (`.callout-note`, etc.)

### Non-Default: Other

- ❌ Extension: `abbreviations` - Abbreviation definitions (non-default)
- ❌ Extension: `attributes` - Universal attribute syntax (non-default, commonmark only)
- ❌ Extension: `gutenberg` - Project Gutenberg conventions (non-default)
- ❌ Extension: `markdown_attribute` - `markdown="1"` in HTML (non-default)
- ❌ Extension: `old_dashes` - Old-style em/en dash parsing (non-default)
- ❌ Extension: `rebase_relative_paths` - Rebase relative paths (non-default)
- ❌ Extension: `short_subsuperscripts` - MultiMarkdown `x^2` style (non-default)
- ❌ Extension: `sourcepos` - Include source position info (non-default)
- ❌ Extension: `space_in_atx_header` - Allow no space after `#` (non-default)
- ❌ Extension: `spaced_reference_links` - Allow space in `[ref] [def]` (non-default)

---

## Won't Implement

- Format-specific output conventions (e.g., `gutenberg` for plain text output)
