use rowan::Language;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum SyntaxKind {
    // Tokens
    WHITESPACE = 0,
    NEWLINE,
    TEXT,
    Backslash,         // \ (for escaping)
    EscapedChar,       // Any escaped character
    NonbreakingSpace,  // \<space>
    HardLineBreak,     // \<newline>
    DivMarker,         // :::
    YamlMetadataDelim, // --- or ... (for YAML blocks)
    BlockQuoteMarker,  // >
    ImageLinkStart,    // ![
    ListMarker,        // - + *
    CommentStart,      // <!--
    CommentEnd,        // -->
    Attribute,         // {#label} for headings, math, etc.
    HorizontalRule,    // --- or *** or ___
    BlankLine,

    // Links and images
    LinkStart,           // [
    Link,                // [text](url)
    LinkText,            // text part of link
    LinkDest,            // (url) or (url "title")
    LinkRef,             // [ref] in reference links
    ImageLink,           // ![alt](url)
    ImageAlt,            // alt text in image
    AutoLink,            // <http://example.com>
    AutoLinkMarker,      // < and >
    ReferenceDefinition, // [label]: url "title"
    ReferenceLabel,      // [label] part
    ReferenceUrl,        // url part
    ReferenceTitle,      // "title" part

    // Math
    InlineMathMarker, // $
    BlockMathMarker,  // $$
    InlineMath,
    MathBlock,
    MathContent,

    // Footnotes
    InlineFootnoteStart, // ^[
    InlineFootnoteEnd,   // ]
    InlineFootnote,      // ^[text]

    // Citations
    Citation,           // [@key] or @key
    CitationMarker,     // @ or -@
    CitationKey,        // The citation key identifier
    CitationBraceOpen,  // { for complex keys
    CitationBraceClose, // } for complex keys
    CitationContent,    // Text content in bracketed citations
    CitationSeparator,  // ; between multiple citations

    // Spans
    BracketedSpan,    // [text]{.class}
    SpanContent,      // text inside span
    SpanAttributes,   // {.class key="val"}
    SpanBracketOpen,  // [
    SpanBracketClose, // ]

    // Code
    CodeSpan,
    CodeSpanMarker,  // ` or `` or ```
    CodeFenceMarker, // ``` or ~~~
    CodeBlock,

    // Inline emphasis and formatting
    Emphasis,          // *text* or _text_
    Strong,            // **text** or __text__
    Strikeout,         // ~~text~~
    Superscript,       // ^text^
    Subscript,         // ~text~
    EmphasisMarker,    // * or _ (for emphasis)
    StrongMarker,      // ** or __ (for strong)
    StrikeoutMarker,   // ~~ (for strikeout)
    SuperscriptMarker, // ^ (for superscript)
    SubscriptMarker,   // ~ (for subscript)

    // Composite nodes
    ROOT,
    DOCUMENT,
    YamlMetadata,
    PandocTitleBlock,
    FencedDiv,
    PARAGRAPH,
    BlockQuote,
    List,
    ListItem,
    DefinitionList,
    DefinitionItem,
    Term,
    Definition,
    DefinitionMarker, // : or ~
    Comment,

    // HTML blocks
    HtmlBlock,        // Generic HTML block
    HtmlBlockTag,     // Opening/closing tags
    HtmlBlockContent, // Content between tags

    // Headings
    Heading,
    HeadingContent,
    AtxHeadingMarker,       // leading #####
    SetextHeadingUnderline, // ===== or -----

    // LaTeX environments
    LatexCommand,     // \command{...}
    LatexEnvironment, // \begin{...}...\end{...}
    LatexEnvBegin,    // \begin{...}
    LatexEnvEnd,      // \end{...}
    LatexEnvContent,  //

    // Tables
    SimpleTable,
    MultilineTable,
    PipeTable,
    GridTable,
    TableHeader,
    TableFooter,
    TableSeparator,
    TableRow,
    TableCell,
    TableCaption,

    // Code block parts
    CodeFenceOpen,
    CodeFenceClose,
    CodeInfo,
    CodeContent,

    // Div parts
    DivFenceOpen,
    DivFenceClose,
    DivInfo,
    DivContent,
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

pub type SyntaxNode = rowan::SyntaxNode<QuartoLanguage>;
pub type SyntaxToken = rowan::SyntaxToken<QuartoLanguage>;
pub type SyntaxElement = rowan::SyntaxElement<QuartoLanguage>;
