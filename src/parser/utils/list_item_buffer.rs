//! Buffer for accumulating list item content before emission.
//!
//! This module provides infrastructure for buffering list item content during parsing,
//! allowing us to determine tight vs loose lists and parse inline elements correctly.

use crate::config::Config;
use crate::parser::utils::inline_emission;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

/// A segment in the list item buffer - either text content or a blank line.
#[derive(Debug, Clone)]
pub(crate) enum ListItemContent {
    /// Text content (includes newlines for losslessness)
    Text(String),
    /// A blank line (affects tight/loose determination)
    #[allow(dead_code)]
    BlankLine,
    /// A task list checkbox (must be first content after marker)
    #[allow(dead_code)]
    TaskCheckbox { checked: bool },
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

    /// Push a blank line to the buffer.
    #[allow(dead_code)]
    pub(crate) fn push_blank_line(&mut self) {
        self.segments.push(ListItemContent::BlankLine);
    }

    /// Push a task checkbox to the buffer.
    #[allow(dead_code)]
    pub(crate) fn push_task_checkbox(&mut self, checked: bool) {
        self.segments
            .push(ListItemContent::TaskCheckbox { checked });
    }

    /// Check if buffer is empty.
    pub(crate) fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// Get the number of segments in the buffer (for debugging).
    pub(crate) fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// Determine if this list item has blank lines between content.
    ///
    /// Used to decide between Plain (tight) and PARAGRAPH (loose).
    /// Returns true if there's a blank line followed by more content.
    pub(crate) fn has_blank_lines_between_content(&self) -> bool {
        let mut seen_blank = false;
        let mut seen_content_after_blank = false;

        for segment in &self.segments {
            match segment {
                ListItemContent::BlankLine => {
                    seen_blank = true;
                }
                ListItemContent::Text(_) | ListItemContent::TaskCheckbox { .. } => {
                    if seen_blank {
                        seen_content_after_blank = true;
                        break;
                    }
                }
            }
        }

        log::trace!(
            "has_blank_lines_between_content: segments={} seen_blank={} seen_content_after={} result={}",
            self.segments.len(),
            seen_blank,
            seen_content_after_blank,
            seen_content_after_blank
        );

        seen_content_after_blank
    }

    /// Get concatenated text for inline parsing (excludes blank lines).
    fn get_text_for_parsing(&self) -> String {
        let mut result = String::new();
        for segment in &self.segments {
            match segment {
                ListItemContent::Text(text) => {
                    result.push_str(text);
                }
                ListItemContent::TaskCheckbox { checked } => {
                    // Emit checkbox syntax so it can be parsed
                    if *checked {
                        result.push_str("[x]");
                    } else {
                        result.push_str("[ ]");
                    }
                }
                ListItemContent::BlankLine => {
                    // Skip blank lines in text extraction
                }
            }
        }
        result
    }

    /// Emit the buffered content as a Plain or PARAGRAPH block.
    ///
    /// If `use_paragraph` is true, wraps in PARAGRAPH (loose list).
    /// If false, wraps in PLAIN (tight list).
    pub(crate) fn emit_as_block(
        &self,
        builder: &mut GreenNodeBuilder<'static>,
        use_paragraph: bool,
        config: &Config,
    ) {
        if self.is_empty() {
            return;
        }

        let block_kind = if use_paragraph {
            SyntaxKind::PARAGRAPH
        } else {
            SyntaxKind::PLAIN
        };

        builder.start_node(block_kind.into());

        // Get text and parse inline elements
        let text = self.get_text_for_parsing();

        // Handle task checkbox specially
        if let Some(ListItemContent::TaskCheckbox { checked }) = self.segments.first() {
            // Emit checkbox as a token
            let checkbox_text = if *checked { "[x]" } else { "[ ]" };
            builder.token(SyntaxKind::TASK_CHECKBOX.into(), checkbox_text);

            // Parse remaining text
            let remaining_text = text.strip_prefix(checkbox_text).unwrap_or(&text);
            if !remaining_text.is_empty() {
                inline_emission::emit_inlines(builder, remaining_text, config);
            }
        } else {
            // No checkbox, just parse all text
            if !text.is_empty() {
                inline_emission::emit_inlines(builder, &text, config);
            }
        }

        builder.finish_node(); // Close PLAIN or PARAGRAPH
    }

    /// Clear the buffer for reuse.
    #[allow(dead_code)]
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
    fn test_blank_line_not_between_content() {
        let mut buffer = ListItemBuffer::new();
        buffer.push_text("Content\n");
        buffer.push_blank_line();
        // Blank line at end doesn't count as "between content"
        assert!(!buffer.has_blank_lines_between_content());
    }

    #[test]
    fn test_blank_line_between_content() {
        let mut buffer = ListItemBuffer::new();
        buffer.push_text("First paragraph\n");
        buffer.push_blank_line();
        buffer.push_text("Second paragraph\n");
        // Blank line followed by content counts as "between content"
        assert!(buffer.has_blank_lines_between_content());
    }

    #[test]
    fn test_multiple_blank_lines() {
        let mut buffer = ListItemBuffer::new();
        buffer.push_text("First\n");
        buffer.push_blank_line();
        buffer.push_blank_line();
        buffer.push_text("Second\n");
        assert!(buffer.has_blank_lines_between_content());
    }

    #[test]
    fn test_task_checkbox_unchecked() {
        let mut buffer = ListItemBuffer::new();
        buffer.push_task_checkbox(false);
        buffer.push_text(" Task description");
        assert_eq!(buffer.get_text_for_parsing(), "[ ] Task description");
    }

    #[test]
    fn test_task_checkbox_checked() {
        let mut buffer = ListItemBuffer::new();
        buffer.push_task_checkbox(true);
        buffer.push_text(" Task description");
        assert_eq!(buffer.get_text_for_parsing(), "[x] Task description");
    }

    #[test]
    fn test_clear_buffer() {
        let mut buffer = ListItemBuffer::new();
        buffer.push_text("Some text");
        buffer.push_blank_line();
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
