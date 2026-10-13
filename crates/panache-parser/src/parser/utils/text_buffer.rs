//! Interleaved buffer for accumulating multi-line block content.
//!
//! Used during paragraph and plain text parsing to collect lines before
//! emitting them with inline parsing applied. Structural bytes that must stay
//! out of the text handed to the inline parser (a blockquote marker, a
//! continuation line's gobbled container indent) are buffered as their own
//! segments and spliced back at emission.

use super::inline_emission;
use crate::options::ParserOptions;
use crate::parser::inlines::code_spans::pending_code_span_openers;
use crate::parser::inlines::sink::{InjectedMarker, MarkerInjectingSink};
use rowan::GreenNodeBuilder;

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
    /// A list item's or definition body's continuation indent, stripped off
    /// the buffered text so the inline parser sees the line from its content
    /// column.
    Indent(String),
}

impl ParagraphSegment {
    fn raw_len(&self) -> usize {
        match self {
            ParagraphSegment::Text(text) => text.len(),
            ParagraphSegment::Indent(indent) => indent.len(),
            ParagraphSegment::BlockquoteMarker {
                leading_spaces,
                has_trailing_space,
            } => leading_spaces + 1 + usize::from(*has_trailing_space),
        }
    }
}

/// Buffer for accumulating paragraph content with interleaved structural markers.
///
/// This enables proper inline parsing across line boundaries while preserving
/// the position of container-prefix (`LINE_PREFIX`) tokens for lossless reconstruction.
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

    /// Push a stripped list-item continuation indent to the buffer.
    ///
    /// The bytes are held out of the text handed to the inline parser and
    /// re-emitted as a `WHITESPACE` token at the same offset, so the parse
    /// stays lossless while inline constructs measure from the content column.
    pub(crate) fn push_indent(&mut self, indent: &str) {
        if indent.is_empty() {
            return;
        }
        self.segments
            .push(ParagraphSegment::Indent(indent.to_string()));
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

    /// Backtick runs in the buffered text that are still waiting for a closer.
    ///
    /// See [`pending_code_span_openers`]; measured on the inline-parser view,
    /// since that is the text the code-span scan will actually see.
    pub(crate) fn pending_code_span_openers(&self) -> Vec<usize> {
        pending_code_span_openers(&self.get_text_for_parsing())
    }

    /// The buffered bytes as they appear in the source — held-out indents and
    /// blockquote markers included.
    ///
    /// [`Self::get_text_for_parsing`] deliberately omits those, since the
    /// point of holding them out is to keep them away from the inline parser.
    /// A *block*-shape test (is this line an ATX heading?) is measured on the
    /// original line instead, so it reads this view.
    pub(crate) fn raw_text(&self) -> String {
        let mut result = String::new();
        for segment in &self.segments {
            match segment {
                ParagraphSegment::Text(text) => result.push_str(text),
                ParagraphSegment::Indent(indent) => result.push_str(indent),
                ParagraphSegment::BlockquoteMarker {
                    leading_spaces,
                    has_trailing_space,
                } => {
                    for _ in 0..*leading_spaces {
                        result.push(' ');
                    }
                    result.push('>');
                    if *has_trailing_space {
                        result.push(' ');
                    }
                }
            }
        }
        result
    }

    /// Split off everything from `raw_offset` (a byte offset into
    /// [`Self::raw_text`]) onward, leaving `self` untouched.
    ///
    /// Used when a block-shape test on the raw text claims a leading run of
    /// bytes — an ATX heading on the buffer's first line — and the remainder
    /// still has to be emitted as inlines with its indents re-injected.
    /// A cut that lands inside an indent or a marker keeps that segment whole:
    /// callers cut at a line boundary, where no such segment straddles.
    pub(crate) fn split_at_raw(&self, raw_offset: usize) -> ParagraphBuffer {
        let mut tail = ParagraphBuffer::new();
        let mut consumed = 0usize;
        for segment in &self.segments {
            let len = segment.raw_len();
            if consumed >= raw_offset {
                tail.segments.push(segment.clone());
            } else if let ParagraphSegment::Text(text) = segment
                && consumed + len > raw_offset
            {
                tail.push_text(&text[raw_offset - consumed..]);
            }
            consumed += len;
        }
        tail
    }

    pub(crate) fn split_off_trailing_markers(&mut self) -> ParagraphBuffer {
        let start = self
            .segments
            .iter()
            .rposition(|segment| matches!(segment, ParagraphSegment::Text(_)))
            .map_or(0, |i| i + 1);
        ParagraphBuffer {
            segments: self.segments.split_off(start),
        }
    }

    fn get_marker_positions(&self) -> Vec<(usize, InjectedMarker<'_>)> {
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
                    positions.push((
                        byte_offset,
                        InjectedMarker::BlockQuote {
                            leading_spaces: *leading_spaces,
                            has_trailing_space: *has_trailing_space,
                        },
                    ));
                }
                ParagraphSegment::Indent(indent) => {
                    positions.push((byte_offset, InjectedMarker::Indent(indent.as_str())));
                }
            }
        }
        positions
    }

    /// Emit the buffered content with inline parsing, interspersing markers at correct positions.
    ///
    /// `suppress_footnote_refs` cascades down into the inline parser. Block
    /// callers compute it from the container stack so paragraphs flushed
    /// from inside a `FOOTNOTE_DEFINITION` body silently drop `[^id]` refs
    /// (pandoc-native behavior).
    pub(crate) fn emit_with_inlines(
        &self,
        builder: &mut GreenNodeBuilder<'static>,
        config: &ParserOptions,
        suppress_footnote_refs: bool,
    ) {
        let text = self.get_text_for_parsing();
        if text.is_empty() && self.segments.is_empty() {
            return;
        }

        let marker_positions = self.get_marker_positions();

        if marker_positions.is_empty() {
            inline_emission::emit_inlines(builder, &text, config, suppress_footnote_refs);
        } else {
            self.emit_with_markers(
                builder,
                &text,
                &marker_positions,
                config,
                suppress_footnote_refs,
            );
        }
    }

    /// Buffered container content must hold a heading's shortcode arguments
    /// together, just as ordinary block dispatch does.
    pub(crate) fn try_emit_hugo_heading(
        &self,
        builder: &mut GreenNodeBuilder<'static>,
        config: &ParserOptions,
    ) -> Option<ParagraphBuffer> {
        if !config.extensions.hugo_shortcodes {
            return None;
        }
        let text = self.get_text_for_parsing();
        let first = text.split_inclusive('\n').next()?;
        let level = crate::parser::blocks::headings::try_parse_atx_heading(first)?;
        let raw_lines: Vec<_> = text.split_inclusive('\n').collect();
        let prefix = crate::parser::blocks::container_prefix::ContainerPrefix::default();
        let lines =
            crate::parser::blocks::container_prefix::StrippedLines::new(&raw_lines, 0, &prefix);
        let prepared = crate::parser::blocks::hugo::prepare(&lines, true, None)?;
        let cutoff = prepared.text.len();
        let mut markers = self.get_marker_positions();
        markers.retain(|(offset, _)| *offset < cutoff);
        let mut sink = MarkerInjectingSink::new(builder, &markers);
        crate::parser::blocks::headings::emit_atx_heading(&mut sink, &prepared.text, level, config);
        sink.finish();

        let mut raw_end = 0;
        let mut text_end = 0;
        for segment in &self.segments {
            if text_end >= cutoff {
                break;
            }
            if let ParagraphSegment::Text(text) = segment {
                let len = text.len().min(cutoff - text_end);
                text_end += len;
                raw_end += len;
            } else {
                raw_end += segment.raw_len();
            }
        }
        Some(self.split_at_raw(raw_end))
    }

    pub(crate) fn emit_setext_heading_text(
        &self,
        builder: &mut GreenNodeBuilder<'static>,
        config: &ParserOptions,
    ) {
        let text = self.get_text_for_parsing();
        let markers = self.get_marker_positions();
        let mut sink = MarkerInjectingSink::new(builder, &markers);
        crate::parser::blocks::headings::emit_setext_heading_text(&mut sink, &text, config);
        sink.finish();
    }

    fn emit_with_markers(
        &self,
        builder: &mut GreenNodeBuilder<'static>,
        text: &str,
        marker_positions: &[(usize, InjectedMarker<'_>)],
        config: &ParserOptions,
        suppress_footnote_refs: bool,
    ) {
        let mut sink = MarkerInjectingSink::new(builder, marker_positions);
        inline_emission::emit_inlines(&mut sink, text, config, suppress_footnote_refs);
        sink.finish();
    }

    /// Check if buffer is empty.
    pub(crate) fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// Clear the buffer for reuse.
    pub(crate) fn clear(&mut self) {
        self.segments.clear();
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
        assert_eq!(buffer.segments.len(), 1);
    }

    #[test]
    fn test_push_marker_separates_text() {
        let mut buffer = ParagraphBuffer::new();
        buffer.push_text("Line 1\n");
        buffer.push_marker(0, true);
        buffer.push_text("Line 2\n");
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
        assert!(matches!(
            positions[0],
            (
                7, // marker at byte 7
                InjectedMarker::BlockQuote {
                    leading_spaces: 0,
                    has_trailing_space: true
                }
            )
        ));
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
        assert!(matches!(
            positions[0],
            (
                2, // first marker at byte 2
                InjectedMarker::BlockQuote {
                    leading_spaces: 0,
                    has_trailing_space: true
                }
            )
        ));
        assert!(matches!(
            positions[1],
            (
                4, // second marker at byte 4
                InjectedMarker::BlockQuote {
                    leading_spaces: 1,
                    has_trailing_space: false
                }
            )
        ));
    }

    #[test]
    fn test_empty_text_ignored() {
        let mut buffer = ParagraphBuffer::new();
        buffer.push_text("");
        assert!(buffer.is_empty());
    }
}
