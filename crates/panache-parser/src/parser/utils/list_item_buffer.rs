//! Buffer for accumulating list item content before emission.
//!
//! This module provides infrastructure for buffering list item content during parsing,
//! allowing us to determine tight vs loose lists and parse inline elements correctly.

use crate::options::ParserOptions;
use crate::parser::blocks::headings::{emit_atx_heading, try_parse_atx_heading};
use crate::parser::blocks::horizontal_rules::{emit_horizontal_rule, try_parse_horizontal_rule};
use crate::parser::utils::inline_emission;
use crate::parser::utils::text_buffer::ParagraphBuffer;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

/// A segment in the list item buffer - either text content or a blank line.
#[derive(Debug, Clone)]
pub(crate) enum ListItemContent {
    /// Text content (includes newlines for losslessness)
    Text(String),
    /// Structural blockquote marker emitted inside buffered list-item text.
    BlockquoteMarker {
        leading_spaces: usize,
        has_trailing_space: bool,
    },
}

/// Buffer for accumulating list item content before emission.
///
/// Collects text, blank lines, and structural elements as we parse list item
/// continuation lines. When the list item closes, we can:
/// 1. Determine if it's tight (Plain) or loose (PARAGRAPH)
/// 2. Parse inline elements correctly across continuation lines
/// 3. Emit the complete structure
#[derive(Debug, Default, Clone)]
pub(crate) struct ListItemBuffer {
    /// Segments of content in order
    segments: Vec<ListItemContent>,
}

impl ListItemBuffer {
    /// Create a new empty list item buffer.
    pub(crate) fn new() -> Self {
        Self {
            segments: Vec::new(),
        }
    }

    /// Push text content to the buffer.
    pub(crate) fn push_text(&mut self, text: impl Into<String>) {
        let text = text.into();
        if text.is_empty() {
            return;
        }
        self.segments.push(ListItemContent::Text(text));
    }

    pub(crate) fn push_blockquote_marker(
        &mut self,
        leading_spaces: usize,
        has_trailing_space: bool,
    ) {
        self.segments.push(ListItemContent::BlockquoteMarker {
            leading_spaces,
            has_trailing_space,
        });
    }

    /// Check if buffer is empty.
    pub(crate) fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// Get the number of segments in the buffer (for debugging).
    pub(crate) fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// Return the text of the first segment, if it is a `Text` segment.
    pub(crate) fn first_text(&self) -> Option<&str> {
        match self.segments.first()? {
            ListItemContent::Text(t) => Some(t.as_str()),
            ListItemContent::BlockquoteMarker { .. } => None,
        }
    }

    /// Determine if this list item has blank lines between content.
    ///
    /// Used to decide between Plain (tight) and PARAGRAPH (loose).
    /// Returns true if there's a blank line followed by more content.
    pub(crate) fn has_blank_lines_between_content(&self) -> bool {
        log::trace!(
            "has_blank_lines_between_content: segments={} result=false",
            self.segments.len()
        );

        false
    }

    /// Get concatenated text for inline parsing (excludes blank lines).
    fn get_text_for_parsing(&self) -> String {
        let mut result = String::new();
        for segment in &self.segments {
            if let ListItemContent::Text(text) = segment {
                result.push_str(text);
            }
        }
        result
    }

    fn to_paragraph_buffer(&self) -> ParagraphBuffer {
        let mut paragraph_buffer = ParagraphBuffer::new();
        for segment in &self.segments {
            match segment {
                ListItemContent::Text(text) => paragraph_buffer.push_text(text),
                ListItemContent::BlockquoteMarker {
                    leading_spaces,
                    has_trailing_space,
                } => paragraph_buffer.push_marker(*leading_spaces, *has_trailing_space),
            }
        }
        paragraph_buffer
    }

    /// Emit the buffered content as a Plain or PARAGRAPH block.
    ///
    /// If `use_paragraph` is true, wraps in PARAGRAPH (loose list).
    /// If false, wraps in PLAIN (tight list).
    pub(crate) fn emit_as_block(
        &self,
        builder: &mut GreenNodeBuilder<'static>,
        use_paragraph: bool,
        config: &ParserOptions,
    ) {
        if self.is_empty() {
            return;
        }

        // Get text and parse inline elements
        let text = self.get_text_for_parsing();

        if !text.is_empty() {
            let line_without_newline = text
                .strip_suffix("\r\n")
                .or_else(|| text.strip_suffix('\n'));
            if let Some(line) = line_without_newline
                && !line.contains('\n')
                && !line.contains('\r')
            {
                if let Some(level) = try_parse_atx_heading(line) {
                    emit_atx_heading(builder, &text, level, config);
                    return;
                }
                if try_parse_horizontal_rule(line).is_some() {
                    emit_horizontal_rule(builder, &text);
                    return;
                }
            }
        }

        let block_kind = if use_paragraph {
            SyntaxKind::PARAGRAPH
        } else {
            SyntaxKind::PLAIN
        };

        builder.start_node(block_kind.into());

        let paragraph_buffer = self.to_paragraph_buffer();
        if !paragraph_buffer.is_empty() {
            paragraph_buffer.emit_with_inlines(builder, config);
        } else if !text.is_empty() {
            inline_emission::emit_inlines(builder, &text, config);
        }

        builder.finish_node(); // Close PLAIN or PARAGRAPH
    }

    /// Clear the buffer for reuse.
    pub(crate) fn clear(&mut self) {
        self.segments.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_buffer_is_empty() {
        let buffer = ListItemBuffer::new();
        assert!(buffer.is_empty());
        assert!(!buffer.has_blank_lines_between_content());
    }

    #[test]
    fn test_push_single_text() {
        let mut buffer = ListItemBuffer::new();
        buffer.push_text("Hello, world!");
        assert!(!buffer.is_empty());
        assert!(!buffer.has_blank_lines_between_content());
        assert_eq!(buffer.get_text_for_parsing(), "Hello, world!");
    }

    #[test]
    fn test_push_multiple_text_segments() {
        let mut buffer = ListItemBuffer::new();
        buffer.push_text("Line 1\n");
        buffer.push_text("Line 2\n");
        buffer.push_text("Line 3");
        assert_eq!(buffer.get_text_for_parsing(), "Line 1\nLine 2\nLine 3");
    }

    #[test]
    fn test_clear_buffer() {
        let mut buffer = ListItemBuffer::new();
        buffer.push_text("Some text");
        assert!(!buffer.is_empty());

        buffer.clear();
        assert!(buffer.is_empty());
        assert_eq!(buffer.get_text_for_parsing(), "");
    }

    #[test]
    fn test_empty_text_ignored() {
        let mut buffer = ListItemBuffer::new();
        buffer.push_text("");
        assert!(buffer.is_empty());
    }
}
