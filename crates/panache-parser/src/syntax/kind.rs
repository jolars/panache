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
    BACKSLASH,         // \ (for escaping)
    ESCAPED_CHAR,      // Any escaped character
    NONBREAKING_SPACE, // \<space>
    HARD_LINE_BREAK,   // \<newline>
    DIV_MARKER,        // :::

    // YAML tokens (metadata and in-tree YAML CST parser)
    YAML_METADATA_DELIM, // --- or ... (for YAML blocks)
    YAML_KEY,            // YAML mapping key token
    YAML_COLON,          // YAML mapping key-value separator
    YAML_TAG,            // YAML explicit tag token (e.g. !!str)
    YAML_ANCHOR,         // YAML anchor token (e.g. &name)
    YAML_ALIAS,          // YAML alias token (e.g. *name)
    YAML_SCALAR_TEXT,    // YAML scalar content fragment (one physical line of a scalar)
    YAML_FLOW_INDICATOR, // YAML flow structural punctuation ([ ] { } ,)
    YAML_DIRECTIVE,      // YAML directive line (%YAML, %TAG)
    YAML_COMMENT,        // YAML inline comment token
    YAML_LINE_PREFIX,    // Embedded-YAML per-line prefix trivia (hashpipe `#|`)
    YAML_DOCUMENT_START, // YAML document start marker (---)
    YAML_DOCUMENT_END,   // YAML document end marker (...)

    BLOCK_QUOTE_MARKER, // >
    ALERT_MARKER,       // [!NOTE], [!TIP], etc.
    IMAGE_LINK_START,   // ![
    LIST_MARKER,        // - + *
    TASK_CHECKBOX,      // [ ] or [x] or [X]
    COMMENT_START,      // <!--
    COMMENT_END,        // -->
    ATTRIBUTE,          // {#label} for headings, math, etc.
    // Structured children of a Pandoc `{...}` ATTRIBUTE. Each wraps the
    // existing source bytes (markers/quotes included); the projector strips
    // them. Absent on opaque ATTRIBUTE forms (MMD `[#id]`, raw-inline
    // `{=format}`, fallback), which keep a single inner ATTRIBUTE token.
    ATTR_ID,         // #id (token text includes the leading '#')
    ATTR_CLASS,      // .class (token text includes the leading '.')
    ATTR_KEY_VALUE,  // key=value (node grouping the pieces below)
    ATTR_KEY,        // key (token, no '=')
    ATTR_VALUE,      // value or "value"/'value' (token text includes quotes)
    HORIZONTAL_RULE, // --- or *** or ___
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
    FOOTNOTE_LABEL_START, // [^
    FOOTNOTE_LABEL_ID,    // id in [^id] or [^id]:
    FOOTNOTE_LABEL_END,   // ]
    FOOTNOTE_LABEL_COLON, // :
    REFERENCE_LABEL,      // [label] part
    REFERENCE_URL,        // url part
    REFERENCE_TITLE,      // "title" part

    // Wikilinks (Pandoc `wikilinks_title_{after,before}_pipe` extensions)
    WIKI_LINK,       // [[url]] or [[url|title]]
    IMAGE_WIKI_LINK, // ![[url]] or ![[url|title]]
    WIKI_LINK_OPEN,  // [[ or ![[
    WIKI_LINK_URL,   // URL slot (raw TEXT child, no inline parsing)
    WIKI_LINK_PIPE,  // | separator
    WIKI_LINK_TITLE, // title slot (raw TEXT child, no inline parsing)
    WIKI_LINK_CLOSE, // ]]

    // Math
    INLINE_MATH_MARKER,  // $
    DISPLAY_MATH_MARKER, // $$
    INLINE_MATH,
    DISPLAY_MATH,
    MATH_CONTENT, // wrapper node for parsed TeX math content (subtree root)

    // Math content (structural TeX CST under MATH_CONTENT)
    MATH_GROUP,       // { ... } brace group (node)
    MATH_ENVIRONMENT, // \begin{env} ... \end{env} (node)
    MATH_GROUP_OPEN,  // {
    MATH_GROUP_CLOSE, // }
    MATH_COMMAND,     // \foo control word or \% control symbol
    MATH_LINE_BREAK,  // \\
    MATH_ALIGN,       // & alignment tab
    MATH_SCRIPT,      // ^ or _
    MATH_COMMENT,     // % to end of line (TeX comment)
    // `+ - * = < >` operator atom; one token per char. Class/precedence are
    // contextual (unary minus, `\mathbin`) and deferred to the formatter.
    MATH_OPERATOR,
    // Delimiters and punctuation whose TeX mathcode class is fixed at the
    // character level (unlike operator class, which is contextual): `( [` open,
    // `) ]` close, `, ;` punctuation. One token per char. The ambiguous `| . /`
    // stay in MATH_TEXT — their class needs macro context.
    MATH_OPEN,    // ( or [
    MATH_CLOSE,   // ) or ]
    MATH_PUNCT,   // , or ;
    MATH_TEXT,    // run of ordinary atoms
    MATH_SPACE,   // run of spaces/tabs
    MATH_NEWLINE, // newline within math content
    // Bookdown equation label `(\#eq:label)`, recognized only when the
    // `bookdown_equation_references` extension is enabled. A single token over
    // the whole `(\#eq:...)` span so the indexer/LSP can target it precisely.
    MATH_EQUATION_LABEL,

    // Footnotes
    INLINE_FOOTNOTE_START, // ^[
    INLINE_FOOTNOTE_END,   // ]
    INLINE_FOOTNOTE,       // ^[text]

    // Citations
    CITATION,                // [@key] or @key
    CITATION_MARKER,         // @ or -@
    CITATION_KEY,            // The citation key identifier
    CITATION_BRACE_OPEN,     // { for complex keys
    CITATION_BRACE_CLOSE,    // } for complex keys
    CITATION_CONTENT,        // Text content in bracketed citations
    CITATION_SEPARATOR,      // ; between multiple citations
    CROSSREF,                // Quarto cross-reference: @fig-*, @eq-*, etc.
    CROSSREF_MARKER,         // @ or -@ for cross-references
    CROSSREF_KEY,            // Cross-reference key identifier
    CROSSREF_BRACE_OPEN,     // { for braced cross-reference keys
    CROSSREF_BRACE_CLOSE,    // } for braced cross-reference keys
    CROSSREF_BOOKDOWN_OPEN,  // \@ref(
    CROSSREF_BOOKDOWN_CLOSE, // )

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

    // Svelte template syntax (mdsvex). Opaque, lossless spans: the content
    // between the braces is preserved verbatim. The parent kind records the
    // category (block logic / tag / expression) by leading sigil.
    SVELTE_BLOCK_LOGIC,  // {#if}, {:else}, {/each}, ...
    SVELTE_TAG,          // {@html ...}, {@const ...}, {@debug ...}
    SVELTE_EXPRESSION,   // {expr}
    SVELTE_MARKER_OPEN,  // {
    SVELTE_MARKER_CLOSE, // }
    SVELTE_CONTENT,      // verbatim content between the braces

    // Code
    INLINE_CODE,
    INLINE_CODE_MARKER,  // ` or `` or ```
    INLINE_CODE_CONTENT, // Literal inline code content
    INLINE_EXEC,         // Inline executable code span variants
    INLINE_EXEC_MARKER,  // Backtick markers delimiting inline executable code
    INLINE_EXEC_LANG,    // Runtime marker (`r` or `{r}`)
    INLINE_EXEC_CONTENT, // Executable inline code expression
    CODE_FENCE_MARKER,   // ``` or ~~~
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
    MARK,               // ==text==
    SUPERSCRIPT,        // ^text^
    SUBSCRIPT,          // ~text~
    EMPHASIS_MARKER,    // * or _ (for emphasis)
    STRONG_MARKER,      // ** or __ (for strong)
    STRIKEOUT_MARKER,   // ~~ (for strikeout)
    MARK_MARKER,        // == (for mark/highlight)
    SUPERSCRIPT_MARKER, // ^ (for superscript)
    SUBSCRIPT_MARKER,   // ~ (for subscript)

    // Composite nodes
    DOCUMENT,

    // YAML nodes
    YAML_METADATA,
    YAML_METADATA_CONTENT,    // Content lines inside YAML metadata block
    YAML_STREAM, // YAML 1.2 stream wrapper (zero or more YAML_DOCUMENT children + trivia)
    YAML_DOCUMENT, // a single YAML document (markers + body)
    YAML_SCALAR, // YAML scalar value node (wraps YAML_SCALAR_TEXT content + NEWLINE/prefix leaves)
    YAML_BLOCK_MAP, // YAML block mapping container
    YAML_BLOCK_MAP_ENTRY, // YAML block mapping entry (key: value)
    YAML_BLOCK_MAP_KEY, // YAML block mapping key wrapper
    YAML_BLOCK_MAP_VALUE, // YAML block mapping value wrapper
    YAML_FLOW_MAP, // YAML flow mapping container ({key: value, ...})
    YAML_FLOW_MAP_ENTRY, // YAML flow mapping entry
    YAML_FLOW_MAP_KEY, // YAML flow mapping key wrapper
    YAML_FLOW_MAP_VALUE, // YAML flow mapping value wrapper
    YAML_FLOW_SEQUENCE, // YAML flow sequence container ([a, b, ...])
    YAML_FLOW_SEQUENCE_ITEM, // YAML flow sequence item wrapper
    YAML_BLOCK_SEQUENCE, // YAML block sequence container (- item ...)
    YAML_BLOCK_SEQUENCE_ITEM, // YAML block sequence item wrapper
    YAML_BLOCK_SEQ_ENTRY, // YAML block sequence entry marker (-)

    PANDOC_TITLE_BLOCK,
    MMD_TITLE_BLOCK,
    FENCED_DIV,
    PARAGRAPH,
    PLAIN, // Inline content without paragraph break (tight lists, definition lists, table cells)
    BLOCK_QUOTE,
    ALERT,
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
    // Pandoc-dialect lift: a matched <div ...>...</div> block.
    HTML_BLOCK_DIV,
    // Pandoc-dialect lift: a single-construct opaque HTML block that
    // projects to exactly one `RawBlock "html"` — an HTML comment
    // (`<!-- -->`), a processing instruction (`<? ?>`), or a verbatim
    // raw-text element (`<pre>`/`<script>`/`<style>`/`<textarea>`). The
    // wrapper kind lets the pandoc-native projector route by CST kind
    // instead of re-sniffing the leading bytes. CommonMark dialect keeps
    // the opaque HTML_BLOCK shape.
    HTML_BLOCK_RAW,
    // Structural region inside an HTML opening tag holding the
    // attribute-list bytes — i.e. everything between the tag name and
    // the closing `>`, exclusive. Recognized by `AttributeNode::cast`,
    // so the salsa anchor index sees `id`/`class`/key=val attrs from
    // `<div id="x">` blocks via the same walk that handles fenced-div
    // and heading attributes.
    HTML_ATTRS,

    // Inline raw HTML (CommonMark §6.6 / Pandoc raw_html). One node per HTML
    // tag/comment/declaration/PI/CDATA span; child token holds the verbatim
    // bytes of the span.
    INLINE_HTML,
    INLINE_HTML_CONTENT,
    // Pandoc-dialect inline lift: a matched <span ...>...</span> tag pair,
    // mirroring HTML_BLOCK_DIV at the inline level. The open tag's
    // attribute region is exposed structurally as HTML_ATTRS so the
    // existing AttributeNode walk picks up `<span id>` ids automatically.
    INLINE_HTML_SPAN,

    // TeX blocks
    TEX_BLOCK, // Raw tex block (e.g., LaTeX commands)

    // Headings
    HEADING,
    HEADING_CONTENT,
    ATX_HEADING_MARKER,       // leading #####
    SETEXT_HEADING_UNDERLINE, // ===== or -----

    // LaTeX inline commands
    LATEX_COMMAND, // \command{...}

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
    // Separator-row markers (split out of the coalesced separator TEXT so
    // alignment/width derivations read structure instead of re-scanning a
    // string). One `TABLE_SEP_DELIM` covers both `|` and `+`.
    TABLE_SEP_DELIM,      // single column delimiter: `|` (pipe) or `+` (grid)
    TABLE_SEP_DASHES,     // a run of `-`
    TABLE_SEP_EQUALS,     // a run of `=` (grid `+===+` header divider)
    TABLE_SEP_COLON,      // single `:` alignment marker
    TABLE_SEP_WHITESPACE, // interior spaces/tabs between dash runs

    // Code block parts
    CODE_FENCE_OPEN,
    CODE_FENCE_CLOSE,
    CODE_INFO,     // Raw info string (preserved for lossless formatting)
    CODE_LANGUAGE, // Parsed language identifier (r, python, etc.)

    // Chunk options (for executable chunks like {r, echo=TRUE})
    CHUNK_OPTIONS,          // Container for all chunk options
    CHUNK_OPTION,           // Single option (key=value pair)
    CHUNK_OPTION_KEY,       // Option name (e.g., echo, fig.cap)
    CHUNK_OPTION_VALUE,     // Option value (e.g., TRUE, "text")
    CHUNK_OPTION_QUOTE,     // Quote character (" or ') if present
    CHUNK_LABEL,            // Special case: unlabeled first option in {r mylabel}
    HASHPIPE_YAML_PREAMBLE, // Hashpipe YAML option preamble region inside CODE_CONTENT
    HASHPIPE_YAML_CONTENT,  // Content lines belonging to hashpipe YAML preamble
    HASHPIPE_PREFIX,        // Hashpipe option marker prefix (e.g., #|, //|, --|)

    CODE_CONTENT,

    // Div parts
    DIV_FENCE_OPEN,
    DIV_FENCE_CLOSE,
    DIV_INFO,
    DIV_CONTENT,
    EMOJI, // :alias:

    // Bracket-shape pattern that did not resolve as a link/image.
    // Distinct from LINK/IMAGE_LINK so downstream tools (linter, LSP) can
    // walk a typed wrapper without the parser having to lie about
    // resolution. `is_image()` on the typed wrapper distinguishes
    // `[foo]` from `![foo]` shapes.
    UNRESOLVED_REFERENCE,
}

impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        Self(kind as u16)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PanacheLanguage {}

impl Language for PanacheLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw.0) }
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        kind.into()
    }
}
