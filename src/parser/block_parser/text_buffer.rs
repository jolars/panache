//! Text buffer for accumulating multi-line block content.
//!
//! Used during paragraph and plain text parsing to collect lines before
//! emitting them with inline parsing applied.

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
