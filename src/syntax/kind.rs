//! Syntax kinds and language definition for the Quarto/Pandoc CST.

use rowan::Language;

#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum SyntaxKind {
    // Tokens
    WHITESPACE = 0,
    NEWLINE,
    TEXT,
    BACKSLASH,           // \ (for escaping)
    ESCAPED_CHAR,        // Any escaped character
    NONBREAKING_SPACE,   // \<space>
    HARD_LINE_BREAK,     // \<newline>
    DIV_MARKER,          // :::
    YAML_METADATA_DELIM, // --- or ... (for YAML blocks)
    BLOCKQUOTE_MARKER,   // >
    IMAGE_LINK_START,    // ![
    LIST_MARKER,         // - + *
    TASK_CHECKBOX,       // [ ] or [x] or [X]
    COMMENT_START,       // <!--
    COMMENT_END,         // -->
    ATTRIBUTE,           // {#label} for headings, math, etc.
    HORIZONTAL_RULE,     // --- or *** or ___
    BLANK_LINE,

    // Links and images
    LINK_START,           // [
    LINK,                 // [text](url)
    LINK_TEXT,            // text part of link
    LINK_TEXT_END,        // ] closing link text
    LINK_DEST_START,      // ( opening link destination
    LINK_DEST,            // (url) or (url "title")
    LINK_DEST_END,        // ) closing link destination
    LINK_REF,             // [ref] in reference links
    IMAGE_LINK,           // ![alt](url)
    IMAGE_ALT,            // alt text in image
    IMAGE_ALT_END,        // ] closing image alt
    IMAGE_DEST_START,     // ( opening image destination
    IMAGE_DEST_END,       // ) closing image destination
    AUTO_LINK,            // <http://example.com>
    AUTO_LINK_MARKER,     // < and >
    REFERENCE_DEFINITION, // [label]: url "title"
    FOOTNOTE_DEFINITION,  // [^id]: content
    FOOTNOTE_REFERENCE,   // [^id]
    REFERENCE_LABEL,      // [label] part
    REFERENCE_URL,        // url part
    REFERENCE_TITLE,      // "title" part

    // Math
    INLINE_MATH_MARKER,  // $
    DISPLAY_MATH_MARKER, // $$
    INLINE_MATH,
    DISPLAY_MATH,
    MATH_CONTENT,

    // Footnotes
    INLINE_FOOTNOTE_START, // ^[
    INLINE_FOOTNOTE_END,   // ]
    INLINE_FOOTNOTE,       // ^[text]

    // Citations
    CITATION,             // [@key] or @key
    CITATION_MARKER,      // @ or -@
    CITATION_KEY,         // The citation key identifier
    CITATION_BRACE_OPEN,  // { for complex keys
    CITATION_BRACE_CLOSE, // } for complex keys
    CITATION_CONTENT,     // Text content in bracketed citations
    CITATION_SEPARATOR,   // ; between multiple citations

    // Spans
    BRACKETED_SPAN,     // [text]{.class}
    SPAN_CONTENT,       // text inside span
    SPAN_ATTRIBUTES,    // {.class key="val"}
    SPAN_BRACKET_OPEN,  // [
    SPAN_BRACKET_CLOSE, // ]

    // Shortcodes (Quarto)
    SHORTCODE,              // {{< name args >}} or {{{< name args >}}}
    SHORTCODE_MARKER_OPEN,  // {{< or {{{<
    SHORTCODE_MARKER_CLOSE, // >}} or >}}}
    SHORTCODE_CONTENT,      // content between markers

    // Code
    CODE_SPAN,
    CODE_SPAN_MARKER,  // ` or `` or ```
    CODE_FENCE_MARKER, // ``` or ~~~
    CODE_BLOCK,

    // Raw inline spans
    RAW_INLINE,         // `content`{=format}
    RAW_INLINE_MARKER,  // ` markers
    RAW_INLINE_FORMAT,  // format name (html, latex, etc.)
    RAW_INLINE_CONTENT, // raw content

    // Inline emphasis and formatting
    EMPHASIS,           // *text* or _text_
    STRONG,             // **text** or __text__
    STRIKEOUT,          // ~~text~~
    SUPERSCRIPT,        // ^text^
    SUBSCRIPT,          // ~text~
    EMPHASIS_MARKER,    // * or _ (for emphasis)
    STRONG_MARKER,      // ** or __ (for strong)
    STRIKEOUT_MARKER,   // ~~ (for strikeout)
    SUPERSCRIPT_MARKER, // ^ (for superscript)
    SUBSCRIPT_MARKER,   // ~ (for subscript)

    // Composite nodes
    DOCUMENT,
    YAML_METADATA,
    PANDOC_TITLE_BLOCK,
    FENCED_DIV,
    PARAGRAPH,
    PLAIN, // Inline content without paragraph break (tight lists, definition lists, table cells)
    BLOCKQUOTE,
    LIST,
    LIST_ITEM,
    DEFINITION_LIST,
    DEFINITION_ITEM,
    TERM,
    DEFINITION,
    DEFINITION_MARKER, // : or ~
    LINE_BLOCK,
    LINE_BLOCK_LINE,
    LINE_BLOCK_MARKER, // |
    COMMENT,
    FIGURE, // Standalone image (Pandoc figure)

    // HTML blocks
    HTML_BLOCK,         // Generic HTML block
    HTML_BLOCK_TAG,     // Opening/closing tags
    HTML_BLOCK_CONTENT, // Content between tags

    // Headings
    HEADING,
    HEADING_CONTENT,
    ATX_HEADING_MARKER,       // leading #####
    SETEXT_HEADING_UNDERLINE, // ===== or -----

    // LaTeX environments
    LATEX_COMMAND,     // \command{...}
    LATEX_ENVIRONMENT, // \begin{...}...\end{...}
    LATEX_ENV_BEGIN,   // \begin{...}
    LATEX_ENV_END,     // \end{...}
    LATEX_ENV_CONTENT, //

    // Tables
    SIMPLE_TABLE,
    MULTILINE_TABLE,
    PIPE_TABLE,
    GRID_TABLE,
    TABLE_HEADER,
    TABLE_FOOTER,
    TABLE_SEPARATOR,
    TABLE_ROW,
    TABLE_CELL,
    TABLE_CAPTION,
    TABLE_CAPTION_PREFIX, // "Table: ", "table: ", or ": "

    // Code block parts
    CODE_FENCE_OPEN,
    CODE_FENCE_CLOSE,
    CODE_INFO,     // Raw info string (preserved for lossless formatting)
    CODE_LANGUAGE, // Parsed language identifier (r, python, etc.)

    // Chunk options (for executable chunks like {r, echo=TRUE})
    CHUNK_OPTIONS,      // Container for all chunk options
    CHUNK_OPTION,       // Single option (key=value pair)
    CHUNK_OPTION_KEY,   // Option name (e.g., echo, fig.cap)
    CHUNK_OPTION_VALUE, // Option value (e.g., TRUE, "text")
    CHUNK_OPTION_QUOTE, // Quote character (" or ') if present
    CHUNK_LABEL,        // Special case: unlabeled first option in {r mylabel}

    CODE_CONTENT,

    // Div parts
    DIV_FENCE_OPEN,
    DIV_FENCE_CLOSE,
    DIV_INFO,
    DIV_CONTENT,
}

impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        Self(kind as u16)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum QuartoLanguage {}

impl Language for QuartoLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw.0) }
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        kind.into()
    }
}
