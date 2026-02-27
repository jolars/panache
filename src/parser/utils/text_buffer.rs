//! Text buffer for accumulating multi-line block content.
//!
//! Used during paragraph and plain text parsing to collect lines before
//! emitting them with inline parsing applied.

use super::inline_emission;
use crate::config::Config;
use crate::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use rowan::{GreenNodeBuilder, NodeOrToken};

/// Buffer for accumulating text lines before emission.
///
/// Designed for minimal allocation overhead - reuses the same buffer
/// across multiple paragraph/plain blocks by clearing between uses.
#[derive(Debug, Default, Clone)]
pub(crate) struct TextBuffer {
    /// Accumulated lines (stored WITH trailing newlines if they had them in source).
    lines: Vec<String>,
}

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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_buffer_is_empty() {
        let buffer = TextBuffer::new();
        assert!(buffer.is_empty());
        assert!(buffer.is_empty());
        assert_eq!(buffer.get_accumulated_text(), "");
    }

    #[test]
    fn test_push_single_line() {
        let mut buffer = TextBuffer::new();
        buffer.push_line("Hello, world!");
        assert!(!buffer.is_empty());
        assert_eq!(buffer.get_accumulated_text(), "Hello, world!");
    }

    #[test]
    fn test_push_multiple_lines() {
        let mut buffer = TextBuffer::new();
        buffer.push_line("Line 1\n");
        buffer.push_line("Line 2\n");
        buffer.push_line("Line 3");
        assert_eq!(buffer.get_accumulated_text(), "Line 1\nLine 2\nLine 3");
    }

    #[test]
    fn test_clear_buffer() {
        let mut buffer = TextBuffer::new();
        buffer.push_line("Line 1");
        buffer.push_line("Line 2");
        buffer.clear();
        assert!(buffer.is_empty());
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
        assert!(!buffer.is_empty());
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
    ///
    /// Important: we must parse the full text *once* so multiline inlines (like STRONG)
    /// can span across blockquote marker boundaries.
    fn emit_with_markers(
        &self,
        builder: &mut GreenNodeBuilder<'static>,
        text: &str,
        marker_positions: &[(usize, usize, bool)],
        config: &Config,
    ) {
        // Parse inlines once into a temporary tree.
        let mut temp_builder = GreenNodeBuilder::new();
        temp_builder.start_node(SyntaxKind::HEADING_CONTENT.into());
        inline_emission::emit_inlines(&mut temp_builder, text, config);
        temp_builder.finish_node();
        let inline_root = SyntaxNode::new_root(temp_builder.finish());

        struct MarkerEmitter<'a> {
            marker_positions: &'a [(usize, usize, bool)],
            idx: usize,
            offset: usize,
        }

        impl<'a> MarkerEmitter<'a> {
            fn emit_markers_at_current(&mut self, builder: &mut GreenNodeBuilder<'static>) {
                while let Some(&(byte_offset, leading_spaces, has_trailing_space)) =
                    self.marker_positions.get(self.idx)
                    && byte_offset == self.offset
                {
                    if leading_spaces > 0 {
                        builder.token(SyntaxKind::WHITESPACE.into(), &" ".repeat(leading_spaces));
                    }
                    builder.token(SyntaxKind::BLOCKQUOTE_MARKER.into(), ">");
                    if has_trailing_space {
                        builder.token(SyntaxKind::WHITESPACE.into(), " ");
                    }
                    self.idx += 1;
                }
            }

            fn emit_token_with_markers(
                &mut self,
                builder: &mut GreenNodeBuilder<'static>,
                token: &SyntaxToken,
            ) {
                let kind = token.kind();
                let token_text = token.text();

                let mut start = 0;
                while start < token_text.len() {
                    // Markers at the current offset must be emitted before emitting any bytes.
                    self.emit_markers_at_current(builder);

                    let remaining = token_text.len() - start;

                    let next_marker_offset = self
                        .marker_positions
                        .get(self.idx)
                        .map(|(byte_offset, _, _)| *byte_offset);

                    if let Some(next) = next_marker_offset
                        && next > self.offset
                        && next < self.offset + remaining
                    {
                        let split_len = next - self.offset;
                        let end = start + split_len;
                        if end > start {
                            builder.token(kind.into(), &token_text[start..end]);
                            self.offset += split_len;
                            start = end;
                            continue;
                        }
                    }

                    builder.token(kind.into(), &token_text[start..]);
                    self.offset += remaining;
                    break;
                }
            }

            fn emit_element(
                &mut self,
                builder: &mut GreenNodeBuilder<'static>,
                el: NodeOrToken<SyntaxNode, SyntaxToken>,
            ) {
                match el {
                    NodeOrToken::Node(n) => {
                        builder.start_node(n.kind().into());
                        for child in n.children_with_tokens() {
                            self.emit_element(builder, child);
                        }
                        builder.finish_node();
                    }
                    NodeOrToken::Token(t) => self.emit_token_with_markers(builder, &t),
                }
            }
        }

        let mut emitter = MarkerEmitter {
            marker_positions,
            idx: 0,
            offset: 0,
        };

        // Emit the inline parse result, injecting markers at the recorded offsets.
        for el in inline_root.children_with_tokens() {
            emitter.emit_element(builder, el);
        }

        // Emit any markers at the end.
        emitter.emit_markers_at_current(builder);
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
    fn test_empty_text_ignored() {
        let mut buffer = ParagraphBuffer::new();
        buffer.push_text("");
        assert!(buffer.is_empty());
    }
}
