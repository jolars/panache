//! Intermediate representation for inline elements
//!
//! This module defines the intermediate tree structure used during inline parsing.
//! The parsing process works in three stages:
//! 1. **Collect**: Parse inline elements into this IR (without resolving emphasis)
//! 2. **Resolve**: Run delimiter stack algorithm to find emphasis matches
//! 3. **Emit**: Traverse the IR tree and emit to GreenNodeBuilder
//!
//! This approach is necessary because emphasis delimiters are ambiguous - we can't
//! know if `*` is emphasis or plain text until we've scanned ahead and found (or
//! not found) matching closers.

use crate::parser::block_parser::attributes::AttributeBlock;

/// An inline element in the intermediate representation
#[derive(Debug, Clone, PartialEq)]
pub enum InlineElement {
    /// Plain text content
    Text {
        content: String,
        start: usize,
        end: usize,
    },

    /// Code span: `code`
    CodeSpan {
        content: String,
        backtick_count: usize,
        attributes: Option<AttributeBlock>,
        start: usize,
        end: usize,
    },

    /// Raw inline span: `raw`{=format}
    RawInline {
        content: String,
        format: String,
        backtick_count: usize,
        start: usize,
        end: usize,
    },

    /// Backslash escape: \*
    Escape {
        char: char,
        escape_type: EscapeType,
        start: usize,
        end: usize,
    },

    /// LaTeX command: \cite{ref}
    LaTeXCommand {
        full_text: String,
        start: usize,
        end: usize,
    },

    /// Math: $math$ or $$display$$
    InlineMath {
        content: String,
        start: usize,
        end: usize,
    },

    /// Display math: $$...$$ or \[...\]
    DisplayMath {
        content: String,
        dollar_count: Option<usize>, // None for \[...\]
        attributes: Option<String>,  // Quarto cross-ref
        start: usize,
        end: usize,
    },

    /// Single backslash math: \(...\) or \[...\]
    SingleBackslashMath {
        content: String,
        is_display: bool,
        start: usize,
        end: usize,
    },

    /// Double backslash math: \\(...\\) or \\[...\\]
    DoubleBackslashMath {
        content: String,
        is_display: bool,
        start: usize,
        end: usize,
    },

    /// Inline link: [text](url)
    InlineLink {
        full_text: String,
        link_text: String,
        dest: String,
        attributes: Option<String>,
        start: usize,
        end: usize,
    },

    /// Reference link: [text][ref]
    ReferenceLink {
        link_text: String,
        label: String,
        is_shortcut: bool,
        start: usize,
        end: usize,
    },

    /// Inline image: ![alt](url)
    InlineImage {
        full_text: String,
        alt_text: String,
        dest: String,
        attributes: Option<String>,
        start: usize,
        end: usize,
    },

    /// Reference image: ![alt][ref]
    ReferenceImage {
        alt_text: String,
        label: String,
        is_shortcut: bool,
        start: usize,
        end: usize,
    },

    /// Autolink: <url>
    Autolink {
        full_text: String,
        url: String,
        start: usize,
        end: usize,
    },

    /// Emphasis: *text* or _text_
    Emphasis {
        delim_char: char,
        children: Vec<InlineElement>,
        start: usize,
        end: usize,
    },

    /// Strong emphasis: **text** or __text__
    Strong {
        delim_char: char,
        children: Vec<InlineElement>,
        start: usize,
        end: usize,
    },

    /// Strikeout: ~~text~~
    Strikeout {
        content: String,
        start: usize,
        end: usize,
    },

    /// Superscript: ^text^
    Superscript {
        content: String,
        start: usize,
        end: usize,
    },

    /// Subscript: ~text~
    Subscript {
        content: String,
        start: usize,
        end: usize,
    },

    /// Inline footnote: ^[text]
    InlineFootnote {
        content: String,
        start: usize,
        end: usize,
    },

    /// Footnote reference: [^id]
    FootnoteReference {
        id: String,
        start: usize,
        end: usize,
    },

    /// Quarto shortcode: {{< name >}}
    Shortcode {
        content: String,
        is_escaped: bool,
        start: usize,
        end: usize,
    },

    /// Native span: <span>text</span>
    NativeSpan {
        content: String,
        attributes: String,
        start: usize,
        end: usize,
    },

    /// Bracketed span: [text]{attrs}
    BracketedSpan {
        content: String,
        attributes: String,
        start: usize,
        end: usize,
    },

    /// Citation: [@key] or @key
    BracketedCitation {
        content: String,
        start: usize,
        end: usize,
    },

    /// Bare citation: @key or -@key
    BareCitation {
        key: String,
        has_suppress: bool,
        start: usize,
        end: usize,
    },

    /// Delimiter run (for emphasis processing)
    /// This is temporary - will be converted to Emphasis/Strong or Text
    DelimiterRun {
        char: char,
        count: usize,
        can_open: bool,
        can_close: bool,
        start: usize,
        end: usize,
    },
}

/// Type of backslash escape
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscapeType {
    /// Regular escaped character: \*
    Literal,
    /// Nonbreaking space: \<space>
    NonbreakingSpace,
    /// Hard line break: \<newline>
    HardLineBreak,
}

impl InlineElement {
    /// Get the byte position where this element starts
    pub fn start(&self) -> usize {
        match self {
            Self::Text { start, .. }
            | Self::CodeSpan { start, .. }
            | Self::RawInline { start, .. }
            | Self::Escape { start, .. }
            | Self::LaTeXCommand { start, .. }
            | Self::InlineMath { start, .. }
            | Self::DisplayMath { start, .. }
            | Self::SingleBackslashMath { start, .. }
            | Self::DoubleBackslashMath { start, .. }
            | Self::InlineLink { start, .. }
            | Self::ReferenceLink { start, .. }
            | Self::InlineImage { start, .. }
            | Self::ReferenceImage { start, .. }
            | Self::Autolink { start, .. }
            | Self::Emphasis { start, .. }
            | Self::Strong { start, .. }
            | Self::Strikeout { start, .. }
            | Self::Superscript { start, .. }
            | Self::Subscript { start, .. }
            | Self::InlineFootnote { start, .. }
            | Self::FootnoteReference { start, .. }
            | Self::Shortcode { start, .. }
            | Self::NativeSpan { start, .. }
            | Self::BracketedSpan { start, .. }
            | Self::BracketedCitation { start, .. }
            | Self::BareCitation { start, .. }
            | Self::DelimiterRun { start, .. } => *start,
        }
    }

    /// Get the byte position where this element ends
    pub fn end(&self) -> usize {
        match self {
            Self::Text { end, .. }
            | Self::CodeSpan { end, .. }
            | Self::RawInline { end, .. }
            | Self::Escape { end, .. }
            | Self::LaTeXCommand { end, .. }
            | Self::InlineMath { end, .. }
            | Self::DisplayMath { end, .. }
            | Self::SingleBackslashMath { end, .. }
            | Self::DoubleBackslashMath { end, .. }
            | Self::InlineLink { end, .. }
            | Self::ReferenceLink { end, .. }
            | Self::InlineImage { end, .. }
            | Self::ReferenceImage { end, .. }
            | Self::Autolink { end, .. }
            | Self::Emphasis { end, .. }
            | Self::Strong { end, .. }
            | Self::Strikeout { end, .. }
            | Self::Superscript { end, .. }
            | Self::Subscript { end, .. }
            | Self::InlineFootnote { end, .. }
            | Self::FootnoteReference { end, .. }
            | Self::Shortcode { end, .. }
            | Self::NativeSpan { end, .. }
            | Self::BracketedSpan { end, .. }
            | Self::BracketedCitation { end, .. }
            | Self::BareCitation { end, .. }
            | Self::DelimiterRun { end, .. } => *end,
        }
    }

    /// Check if this element is a delimiter run
    pub fn is_delimiter_run(&self) -> bool {
        matches!(self, Self::DelimiterRun { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_element_positions() {
        let elem = InlineElement::Text {
            content: "hello".to_string(),
            start: 5,
            end: 10,
        };
        assert_eq!(elem.start(), 5);
        assert_eq!(elem.end(), 10);
    }

    #[test]
    fn test_delimiter_run_detection() {
        let delim = InlineElement::DelimiterRun {
            char: '*',
            count: 2,
            can_open: true,
            can_close: false,
            start: 0,
            end: 2,
        };
        assert!(delim.is_delimiter_run());

        let text = InlineElement::Text {
            content: "foo".to_string(),
            start: 0,
            end: 3,
        };
        assert!(!text.is_delimiter_run());
    }
}
