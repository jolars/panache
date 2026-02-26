//! Text buffer for accumulating multi-line block content.
//!
//! Used during paragraph and plain text parsing to collect lines before
//! emitting them with inline parsing applied.

use super::inline_emission;
use crate::config::Config;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

/// Buffer for accumulating text lines before emission.
///
/// Designed for minimal allocation overhead - reuses the same buffer
/// across multiple paragraph/plain blocks by clearing between uses.
#[derive(Debug, Default, Clone)]
#[allow(dead_code)] // Will be used in Subtask 3
pub(crate) struct TextBuffer {
    /// Accumulated lines (stored WITH trailing newlines if they had them in source).
    lines: Vec<String>,
}

#[allow(dead_code)] // Will be used in Subtask 3
impl TextBuffer {
    /// Create a new empty text buffer.
    pub(crate) fn new() -> Self {
        Self { lines: Vec::new() }
    }

    /// Push a line of text to the buffer.
    ///
    /// The line should include its trailing newline if it had one in the source.
    pub(crate) fn push_line(&mut self, text: impl Into<String>) {
        self.lines.push(text.into());
    }

    /// Get the accumulated text by concatenating all lines.
    ///
    /// Returns empty string if buffer is empty.
    /// Lines are concatenated as-is (they should include their own newlines if needed).
    pub(crate) fn get_accumulated_text(&self) -> String {
        self.lines.concat()
    }

    /// Clear the buffer for reuse.
    pub(crate) fn clear(&mut self) {
        self.lines.clear();
    }

    /// Check if buffer is empty.
    pub(crate) fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Get number of lines in buffer.
    pub(crate) fn len(&self) -> usize {
        self.lines.len()
    }

    /// Get an iterator over the buffered lines.
    pub(crate) fn lines(&self) -> impl Iterator<Item = &str> {
        self.lines.iter().map(|s| s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_buffer_is_empty() {
        let buffer = TextBuffer::new();
        assert!(buffer.is_empty());
        assert_eq!(buffer.len(), 0);
        assert_eq!(buffer.get_accumulated_text(), "");
    }

    #[test]
    fn test_push_single_line() {
        let mut buffer = TextBuffer::new();
        buffer.push_line("Hello, world!");
        assert!(!buffer.is_empty());
        assert_eq!(buffer.len(), 1);
        assert_eq!(buffer.get_accumulated_text(), "Hello, world!");
    }

    #[test]
    fn test_push_multiple_lines() {
        let mut buffer = TextBuffer::new();
        buffer.push_line("Line 1\n");
        buffer.push_line("Line 2\n");
        buffer.push_line("Line 3");
        assert_eq!(buffer.len(), 3);
        assert_eq!(buffer.get_accumulated_text(), "Line 1\nLine 2\nLine 3");
    }

    #[test]
    fn test_clear_buffer() {
        let mut buffer = TextBuffer::new();
        buffer.push_line("Line 1");
        buffer.push_line("Line 2");
        assert_eq!(buffer.len(), 2);

        buffer.clear();
        assert!(buffer.is_empty());
        assert_eq!(buffer.len(), 0);
        assert_eq!(buffer.get_accumulated_text(), "");
    }

    #[test]
    fn test_reuse_after_clear() {
        let mut buffer = TextBuffer::new();

        // First use
        buffer.push_line("First paragraph\n");
        buffer.push_line("continues here");
        assert_eq!(
            buffer.get_accumulated_text(),
            "First paragraph\ncontinues here"
        );

        // Clear and reuse
        buffer.clear();
        buffer.push_line("Second paragraph\n");
        buffer.push_line("also continues");
        assert_eq!(
            buffer.get_accumulated_text(),
            "Second paragraph\nalso continues"
        );
    }

    #[test]
    fn test_empty_lines() {
        let mut buffer = TextBuffer::new();
        buffer.push_line("\n");
        buffer.push_line("Non-empty\n");
        buffer.push_line("");
        assert_eq!(buffer.len(), 3);
        assert_eq!(buffer.get_accumulated_text(), "\nNon-empty\n");
    }

    #[test]
    fn test_whitespace_preserved() {
        let mut buffer = TextBuffer::new();
        buffer.push_line("  Leading spaces\n");
        buffer.push_line("Trailing spaces  \n");
        buffer.push_line("\tTab at start");
        assert_eq!(
            buffer.get_accumulated_text(),
            "  Leading spaces\nTrailing spaces  \n\tTab at start"
        );
    }

    #[test]
    fn test_default_is_empty() {
        let buffer = TextBuffer::default();
        assert!(buffer.is_empty());
        assert_eq!(buffer.get_accumulated_text(), "");
    }
}

// ============================================================================
// ParagraphBuffer - Interleaved buffer for paragraphs with structural markers
// ============================================================================

/// A segment in the paragraph buffer - either text content or a structural marker.
#[derive(Debug, Clone)]
pub(crate) enum ParagraphSegment {
    /// Text content (may include newlines)
    Text(String),
    /// A blockquote marker with its whitespace info
    BlockquoteMarker {
        leading_spaces: usize,
        has_trailing_space: bool,
    },
}

/// Buffer for accumulating paragraph content with interleaved structural markers.
///
/// This enables proper inline parsing across line boundaries while preserving
/// the position of BLOCKQUOTE_MARKER tokens for lossless reconstruction.
#[derive(Debug, Default, Clone)]
pub(crate) struct ParagraphBuffer {
    /// Interleaved segments of text and markers
    segments: Vec<ParagraphSegment>,
}

impl ParagraphBuffer {
    /// Create a new empty paragraph buffer.
    pub(crate) fn new() -> Self {
        Self {
            segments: Vec::new(),
        }
    }

    /// Push text content to the buffer.
    ///
    /// If the last segment is Text, appends to it. Otherwise creates a new Text segment.
    pub(crate) fn push_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        match self.segments.last_mut() {
            Some(ParagraphSegment::Text(existing)) => {
                existing.push_str(text);
            }
            _ => {
                self.segments.push(ParagraphSegment::Text(text.to_string()));
            }
        }
    }

    /// Push a blockquote marker to the buffer.
    pub(crate) fn push_marker(&mut self, leading_spaces: usize, has_trailing_space: bool) {
        self.segments.push(ParagraphSegment::BlockquoteMarker {
            leading_spaces,
            has_trailing_space,
        });
    }

    /// Get concatenated text for inline parsing (excludes markers).
    pub(crate) fn get_text_for_parsing(&self) -> String {
        let mut result = String::new();
        for segment in &self.segments {
            if let ParagraphSegment::Text(text) = segment {
                result.push_str(text);
            }
        }
        result
    }

    /// Get the byte positions where markers should be inserted in the concatenated text.
    ///
    /// Returns a list of (byte_offset, marker_info) pairs.
    fn get_marker_positions(&self) -> Vec<(usize, usize, bool)> {
        let mut positions = Vec::new();
        let mut byte_offset = 0;

        for segment in &self.segments {
            match segment {
                ParagraphSegment::Text(text) => {
                    byte_offset += text.len();
                }
                ParagraphSegment::BlockquoteMarker {
                    leading_spaces,
                    has_trailing_space,
                } => {
                    positions.push((byte_offset, *leading_spaces, *has_trailing_space));
                }
            }
        }
        positions
    }

    /// Emit the buffered content with inline parsing, interspersing markers at correct positions.
    pub(crate) fn emit_with_inlines(
        &self,
        builder: &mut GreenNodeBuilder<'static>,
        config: &Config,
    ) {
        let text = self.get_text_for_parsing();
        if text.is_empty() && self.segments.is_empty() {
            return;
        }

        let marker_positions = self.get_marker_positions();

        if marker_positions.is_empty() {
            // No markers - simple case, just emit inlines
            inline_emission::emit_inlines(builder, &text, config);
        } else {
            // Complex case: emit inlines with markers interspersed
            self.emit_with_markers(builder, &text, &marker_positions, config);
        }
    }

    /// Emit inline content with markers at specified byte positions.
    fn emit_with_markers(
        &self,
        builder: &mut GreenNodeBuilder<'static>,
        text: &str,
        marker_positions: &[(usize, usize, bool)],
        config: &Config,
    ) {
        let mut last_pos = 0;

        for &(byte_offset, leading_spaces, has_trailing_space) in marker_positions {
            // Emit text segment before this marker (if any)
            if byte_offset > last_pos {
                let segment = &text[last_pos..byte_offset];
                inline_emission::emit_inlines(builder, segment, config);
            }

            // Emit the marker
            if leading_spaces > 0 {
                builder.token(SyntaxKind::WHITESPACE.into(), &" ".repeat(leading_spaces));
            }
            builder.token(SyntaxKind::BLOCKQUOTE_MARKER.into(), ">");
            if has_trailing_space {
                builder.token(SyntaxKind::WHITESPACE.into(), " ");
            }

            last_pos = byte_offset;
        }

        // Emit remaining text after last marker
        if last_pos < text.len() {
            let segment = &text[last_pos..];
            inline_emission::emit_inlines(builder, segment, config);
        }
    }

    /// Clear the buffer for reuse.
    #[allow(dead_code)] // May be used for buffer reuse optimization
    pub(crate) fn clear(&mut self) {
        self.segments.clear();
    }

    /// Check if buffer is empty.
    pub(crate) fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }
}

#[cfg(test)]
mod paragraph_buffer_tests {
    use super::*;

    #[test]
    fn test_new_buffer_is_empty() {
        let buffer = ParagraphBuffer::new();
        assert!(buffer.is_empty());
        assert_eq!(buffer.get_text_for_parsing(), "");
    }

    #[test]
    fn test_push_text_single() {
        let mut buffer = ParagraphBuffer::new();
        buffer.push_text("Hello, world!");
        assert!(!buffer.is_empty());
        assert_eq!(buffer.get_text_for_parsing(), "Hello, world!");
    }

    #[test]
    fn test_push_text_concatenates() {
        let mut buffer = ParagraphBuffer::new();
        buffer.push_text("Hello");
        buffer.push_text(", ");
        buffer.push_text("world!");
        assert_eq!(buffer.get_text_for_parsing(), "Hello, world!");
        // Should be a single Text segment due to concatenation
        assert_eq!(buffer.segments.len(), 1);
    }

    #[test]
    fn test_push_marker_separates_text() {
        let mut buffer = ParagraphBuffer::new();
        buffer.push_text("Line 1\n");
        buffer.push_marker(0, true);
        buffer.push_text("Line 2\n");
        // Should be: Text, Marker, Text
        assert_eq!(buffer.segments.len(), 3);
        assert_eq!(buffer.get_text_for_parsing(), "Line 1\nLine 2\n");
    }

    #[test]
    fn test_marker_positions() {
        let mut buffer = ParagraphBuffer::new();
        buffer.push_text("Line 1\n"); // 7 bytes
        buffer.push_marker(0, true);
        buffer.push_text("Line 2\n"); // 7 bytes

        let positions = buffer.get_marker_positions();
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0], (7, 0, true)); // marker at byte 7
    }

    #[test]
    fn test_multiple_markers() {
        let mut buffer = ParagraphBuffer::new();
        buffer.push_text("A\n"); // 2 bytes
        buffer.push_marker(0, true);
        buffer.push_text("B\n"); // 2 bytes
        buffer.push_marker(1, false);
        buffer.push_text("C");

        let positions = buffer.get_marker_positions();
        assert_eq!(positions.len(), 2);
        assert_eq!(positions[0], (2, 0, true)); // first marker at byte 2
        assert_eq!(positions[1], (4, 1, false)); // second marker at byte 4
    }

    #[test]
    fn test_clear() {
        let mut buffer = ParagraphBuffer::new();
        buffer.push_text("Some text");
        buffer.push_marker(0, true);
        buffer.clear();
        assert!(buffer.is_empty());
        assert_eq!(buffer.get_text_for_parsing(), "");
    }

    #[test]
    fn test_empty_text_ignored() {
        let mut buffer = ParagraphBuffer::new();
        buffer.push_text("");
        assert!(buffer.is_empty());
    }
}
