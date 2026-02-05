# Block Parser → Inline Parser Architecture

## Overview

The panache formatter now uses a two-stage parsing approach:

1. **Block Parser** (`src/block_parser.rs`) - Parses document structure into blocks
2. **Inline Parser** (`src/inline_parser.rs`) - Processes inline elements within block content
3. **Formatter** (`src/formatter.rs`) - Formats the final parsed CST

## Architecture Flow

```
Input Text
    ↓
Block Parser → Block-level CST (headings, paragraphs, code blocks, etc.)
    ↓
Inline Parser → Full CST (with inline elements: emphasis, links, math, etc.)
    ↓
Formatter → Formatted Output
```

## Implementation Status

### ✅ Block Parser (Implemented)
- ATX headings (`# Heading`)
- Paragraphs
- Fenced code blocks (``` and ~~~)
- Blockquotes (with nesting support)
- Lists (ordered and unordered)
- Horizontal rules (`---`, `***`, `___`)
- Metadata/frontmatter (YAML, TOML)
- Blank lines
- Container stack infrastructure for nested blocks

### 🔄 Inline Parser (Infrastructure Ready)
The inline parser infrastructure is set up but currently acts as a pass-through. The foundation is ready for implementing:

- **Emphasis**: `*text*`, `**text**`, `_text_`, `__text__`
- **Links**: `[text](url)`, `[text][ref]`
- **Inline Code**: `` `code` ``
- **Inline Math**: `$math$`
- **Escapes**: `\*`, `\[`, etc.
- **Images**: `![alt](url)`

### ✅ Formatter (Working)
- Paragraph wrapping
- Heading normalization
- Code block preservation
- Block quote formatting
- List formatting

## Adding Inline Parsing Features

To add a new inline element type:

1. **Add to SyntaxKind enum** (`src/syntax.rs`)
2. **Extend InlineElement enum** (`src/inline_parser.rs`)
3. **Implement parsing logic** in `InlineParser::parse_inline_content()`
4. **Add formatting logic** to `src/formatter.rs`
5. **Add tests** to `src/inline_parser.rs` tests module

## Current Benefits

- ✅ **Modular Architecture**: Clean separation between block and inline parsing
- ✅ **Incremental Development**: Can add inline features one at a time
- ✅ **Test Coverage**: Full test suite ensures stability
- ✅ **Working Formatter**: Basic functionality works without inline parsing
- ✅ **Foundation Ready**: Infrastructure is in place for rapid inline feature development

## Next Steps

1. Implement inline code parsing (`` `code` ``)
2. Implement emphasis parsing (`*text*`, `**text**`)
3. Implement link parsing (`[text](url)`)
4. Implement inline math parsing (`$math$`)
5. Add escape sequence handling (`\*`)

The architecture is now properly set up for systematic implementation of inline parsing features while maintaining full functionality of the existing block-level formatting.
