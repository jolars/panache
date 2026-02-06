# panache TODO - Comprehensive Pandoc Feature Coverage

This document tracks implementation status of Pandoc Markdown features based on the spec files in `docs/pandoc-spec/`.

## Status Legend
- ✅ **Implemented** - Feature is fully or mostly implemented
- 🚧 **Partial** - Feature is partially implemented or needs work
- ❌ **Not Implemented** - Feature not yet started
- 🔵 **Formatter Only** - Parser may handle it, but formatter needs work
- ⚪ **Not Applicable** - Feature doesn't need special handling (passes through)

---

## Block-Level Elements

### Paragraphs ✅
- ✅ Basic paragraphs
- ✅ Paragraph wrapping/reflow
- ⚪ Extension: `escaped_line_breaks` (backslash at line end)

### Headings ✅
- ✅ ATX-style headings (`# Heading`)
- ✅ Setext-style headings (underlined with `===` or `---`)
- ✅ Heading identifier attributes (`# Heading {#id}`)
- 🔵 Extension: `header_attributes` - Full attribute syntax `{#id .class key=value}`
- ❌ Extension: `implicit_header_references` - Auto-generate reference links
- ❌ Extension: `mmd_header_identifiers` - MultiMarkdown style IDs
- ❌ Extension: `blank_before_header` - Require blank line before headings

### Block Quotations ✅
- ✅ Basic block quotes (`> text`)
- ✅ Nested block quotes (`> > nested`)
- ✅ Block quotes with paragraphs
- 🚧 Block quotes containing lists (works but may need polish)
- 🚧 Block quotes containing code blocks (needs testing)
- ❌ Extension: `blank_before_blockquote` - Require blank before quote

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
- ❌ Extension: `definition_lists` - Term/definition syntax
- ❌ Extension: `lists_without_preceding_blankline`
- ❌ Extension: `four_space_rule` - Four space vs two space list indent

### Code Blocks ✅
- ✅ Fenced code blocks (backticks and tildes)
- ✅ Code block attributes (language, etc.)
- ✅ Indented code blocks (4-space indent)
- ⚪ Extension: `fenced_code_attributes` - `{.language #id}`
- ⚪ Extension: `backtick_code_blocks` - Backtick-only fences
- ❌ Extension: `inline_code_attributes` - Attributes on inline code

### Horizontal Rules ✅
- ✅ Basic horizontal rules (`---`, `***`, `___`)
- 🔵 Distinguish from table syntax (potential ambiguity)

### Fenced Divs ✅
- ✅ Basic fenced divs (`::: {.class}`)
- ✅ Nested fenced divs
- ✅ Colon count normalization based on nesting
- ⚪ Extension: `native_divs` - HTML `<div>` elements

### Tables ❌
- ❌ Extension: `simple_tables` - Simple table syntax
- ❌ Extension: `multiline_tables` - Multiline cell content
- ❌ Extension: `grid_tables` - Grid-style tables
- ❌ Extension: `pipe_tables` - GitHub/PHP Markdown tables
- ❌ Extension: `table_captions` - Table captions

### Line Blocks ❌
- ❌ Extension: `line_blocks` - Poetry/verse with `|` prefix

---

## Inline Elements

### Emphasis & Formatting ⚪
- ⚪ `*italic*` and `_italic_`
- ⚪ `**bold**` and `__bold__`
- ⚪ Extension: `intraword_underscores` - `snake_case` handling
- ❌ Extension: `strikeout` - `~~strikethrough~~`
- ❌ Extension: `superscript`, `subscript` - `^super^` and `~sub~`
- ❌ Extension: `mark` - `==highlighted==` text
- ❌ Small caps - `[text]{.smallcaps}`
- ❌ Underline - `[text]{.underline}`

### Code & Verbatim ⚪
- ⚪ Inline code (`` `code` ``)
- ⚪ Verbatim - Pass through literal text

### Links 🔵
- 🔵 Inline links `[text](url)`
- 🔵 Reference links `[text][ref]`
- 🔵 Automatic links `<http://example.com>`
- ❌ Extension: `autolink_bare_uris` - Bare URLs as links
- ❌ Extension: `shortcut_reference_links` - `[ref]` without second `[]`
- ❌ Extension: `link_attributes` - `[text](url){.class}`
- ❌ Extension: `implicit_header_references` - `[Heading Name]` links to header
- ❌ Extension: `mmd_link_attributes` - MultiMarkdown link attributes

### Images 🔵
- 🔵 Inline images `![alt](url)`
- 🔵 Reference images `![alt][ref]`
- ❌ Extension: `implicit_figures` - Paragraph with just image becomes figure

### Math 🔵
- 🔵 Inline math `$x = y$`
- 🔵 Display math `$$equation$$`
- ⚪ Extension: `tex_math_dollars` - Dollar-delimited math
- ❌ Extension: `tex_math_single_backslash` - `\( \)` and `\[ \]`
- ❌ Extension: `tex_math_double_backslash` - `\\( \\)` and `\\[ \\]`
- ❌ Extension: `tex_math_gfm` - GitHub Flavored Markdown math

### Footnotes 🔵
- 🔵 Inline footnotes `^[note text]`
- ❌ Reference footnotes `[^1]` with definition block
- ❌ Extension: `inline_notes` - Inline note syntax

### Citations ❌
- ❌ Extension: `citations` - `[@cite]` and `@cite` syntax

### Spans ❌
- ❌ Extension: `bracketed_spans` - `[text]{.class}` inline
- ❌ Extension: `native_spans` - HTML `<span>` elements

---

## Metadata & Front Matter

### Metadata Blocks 🚧
- ✅ Extension: `yaml_metadata_block` - YAML frontmatter
- 🚧 Extension: `pandoc_title_block` - Title/author/date at top
- ❌ Extension: `mmd_title_block` - MultiMarkdown metadata

---

## Raw Content & Special Syntax

### Raw HTML ❌
- ❌ Extension: `raw_html` - Inline and block HTML
- ❌ Extension: `markdown_in_html_blocks` - Markdown inside HTML blocks
- ❌ Extension: `markdown_attribute` - `markdown="1"` attribute

### Raw LaTeX ⚪
- ⚪ Extension: `raw_tex` - LaTeX commands and environments
- ❌ Extension: `latex_macros` - Expand LaTeX macros

### Other Raw ❌
- ❌ Extension: `raw_attribute` - Generic raw blocks `{=format}`

---

## Escapes & Special Characters

### Backslash Escapes ⚪
- ⚪ Extension: `all_symbols_escapable` - Backslash escapes any symbol
- ⚪ Extension: `angle_brackets_escapable` - Escape `<` and `>`

### Line Breaks ⚪
- ⚪ Extension: `hard_line_breaks` - Newline = `<br>`
- ⚪ Extension: `escaped_line_breaks` - Backslash at line end = `<br>`
- ❌ Extension: `ignore_line_breaks` - Ignore single newlines
- ❌ Extension: `east_asian_line_breaks` - Smart line breaks for CJK

---

## Non-Default / Special Extensions

### Quarto-Specific ❌
- ❌ Extension: `alerts` - Quarto alert/callout boxes
- ❌ Executable code cells with output
- ❌ Cross-references `@fig-id`, `@tbl-id`
- ❌ Callout blocks (`.callout-note`, etc.)

### GitHub Flavored Markdown ❌
- ❌ Extension: `emoji` - `:emoji:` syntax
- ❌ Extension: `wikilinks_title_after_pipe` - `[[link|title]]`

### Other Extensions ❌
- ❌ Extension: `abbreviations` - Abbreviation definitions
- ❌ Extension: `gutenberg` - Project Gutenberg conventions
- ❌ Extension: `rebase_relative_paths` - Rebase relative paths
- ❌ Extension: `sourcepos` - Include source position info
- ❌ Extension: `space_in_atx_header` - Allow no space after `#`
- ❌ Extension: `spaced_reference_links` - Allow space in `[ref] [def]`
- ❌ Extension: `old_dashes` - Old-style em/en dash parsing

---

## Formatter-Specific Improvements

### High Priority 🚧
1. **List formatting improvements** - Better handling of continuation, nesting, alignment
2. **Inline element preservation** - Links, images, emphasis, code spans
3. **Table formatting** - Once tables are parsed, format them nicely
4. **Math block formatting** - Preserve math content properly

### Medium Priority 🔵
1. **Footnote formatting** - Once parsed, format reference-style footnotes
2. **Definition list formatting** - Format term/definition pairs
3. **Raw HTML passthrough** - Preserve HTML blocks/inline
4. **Citation formatting** - Format citation syntax consistently

### Low Priority 🔵
1. **Emoji passthrough** - Preserve `:emoji:` syntax
2. **Abbreviation expansion** - Handle abbreviation blocks
3. **Smart quote/dash handling** - Typography improvements

---

## Architecture Improvements

### Parser Structure 🚧
- 🚧 **List structure** - Emit explicit ListIndent, ListMarker, MarkerSpace nodes
- 🚧 **Fence structure** - Move newlines out of fence nodes, populate Info nodes properly
- ❌ **Inline parser** - Currently a placeholder, needs full implementation
- ❌ **Table parser** - No table parsing yet
- ❌ **HTML parser** - No HTML block/inline parsing

### Formatter Structure ✅
- ✅ **Reflow mode** - Paragraph wrapping works
- ✅ **Preserve mode** - Pass-through formatting
- ✅ **Configuration** - Line width, wrap mode, etc.
- 🔵 **Idempotency** - Ensure formatting is idempotent (mostly works)

### Testing 🚧
- ✅ **Golden tests** - Input/output comparison tests
- ✅ **Unit tests** - Parser and formatter units
- ❌ **Fuzzing** - cargo-fuzz for robustness
- ❌ **Property tests** - Token concatenation = input
- ❌ **Corpus testing** - Real Quarto documents

---

## Immediate Next Steps (Suggested Priority)

1. **Complete inline parser** - Currently a WIP placeholder
   - Links (inline, reference, automatic)
   - Images
   - Emphasis (bold, italic)
   - Code spans
   - Math (inline)

2. **Table support** - Critical for Quarto documents
   - Simple tables
   - Pipe tables (most common)
   - Grid tables (if time permits)

3. **List improvements** - Fragile currently
   - Parser: explicit marker/indent structure
   - Formatter: proper hanging indents

4. **Definition lists** - Common in documentation

5. **Task lists** - GitHub-style checkboxes (common)

6. **Footnotes** - Complete reference-style footnotes

7. **Citations** - Essential for academic Quarto docs

---

## Won't Implement (Low Value / Out of Scope)

- Old/deprecated extensions (e.g., `old_dashes`)
- Obscure formats (e.g., `gutenberg`)
- Editor-specific features (e.g., `sourcepos`)
- Format-specific raw content (leave as-is)

---

**Last Updated:** 2026-02-06
