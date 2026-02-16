//! Shared utilities for block parsing.

use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

/// Helper to emit a line's text and newline tokens separately.
/// Lines from split_lines_inclusive contain trailing newlines (LF or CRLF) that must be separated.
pub(crate) fn emit_line_tokens(builder: &mut GreenNodeBuilder<'static>, line: &str) {
    // Handle both CRLF and LF line endings
    if let Some(text) = line.strip_suffix("\r\n") {
        builder.token(SyntaxKind::TEXT.into(), text);
        builder.token(SyntaxKind::NEWLINE.into(), "\r\n");
    } else if let Some(text) = line.strip_suffix('\n') {
        builder.token(SyntaxKind::TEXT.into(), text);
        builder.token(SyntaxKind::NEWLINE.into(), "\n");
    } else {
        // No trailing newline (last line of input)
        builder.token(SyntaxKind::TEXT.into(), line);
    }
}

/// Strip up to N leading spaces from a line.
/// This is the generalized version of the previous strip_leading_spaces (which stripped up to 3).
pub(crate) fn strip_leading_spaces_n(line: &str, max_spaces: usize) -> &str {
    let spaces_to_strip = line
        .chars()
        .take(max_spaces)
        .take_while(|&c| c == ' ')
        .count();
    &line[spaces_to_strip..]
}

/// Strip up to 3 leading spaces from a line.
/// This is a convenience wrapper for the common case in Markdown parsing.
pub(crate) fn strip_leading_spaces(line: &str) -> &str {
    strip_leading_spaces_n(line, 3)
}

/// Count leading spaces in a line, up to a maximum.
/// Returns the actual count (may be less than max).
#[allow(dead_code)] // Utility function for future use
pub(crate) fn count_leading_spaces(line: &str, max: usize) -> usize {
    line.chars().take(max).take_while(|&c| c == ' ').count()
}

/// Skip leading whitespace (spaces and tabs) from a string.
/// Returns the string with leading whitespace removed.
#[allow(dead_code)] // Utility function for future use
pub(crate) fn skip_whitespace(s: &str) -> &str {
    s.trim_start_matches([' ', '\t'])
}

/// Strip trailing newline (LF or CRLF) from a line, returning the content and the newline string.
/// Returns (content_without_newline, newline_str).
pub(crate) fn strip_newline(line: &str) -> (&str, &str) {
    if let Some(content) = line.strip_suffix("\r\n") {
        (content, "\r\n")
    } else if let Some(content) = line.strip_suffix('\n') {
        (content, "\n")
    } else {
        (line, "")
    }
}

/// Split input into lines while preserving line endings (LF or CRLF).
/// This is like split_inclusive but handles both \n and \r\n.
pub(crate) fn split_lines_inclusive(input: &str) -> Vec<&str> {
    if input.is_empty() {
        return vec![];
    }

    let mut lines = Vec::new();
    let mut start = 0;
    let bytes = input.as_bytes();
    let len = bytes.len();

    let mut i = 0;
    while i < len {
        if bytes[i] == b'\n' {
            // Found LF, include it in the line
            lines.push(&input[start..=i]);
            start = i + 1;
            i += 1;
        } else if bytes[i] == b'\r' && i + 1 < len && bytes[i + 1] == b'\n' {
            // Found CRLF, include both in the line
            lines.push(&input[start..=i + 1]);
            start = i + 2;
            i += 2;
        } else {
            i += 1;
        }
    }

    // Add remaining text if any (last line without newline)
    if start < len {
        lines.push(&input[start..]);
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_leading_spaces_n() {
        assert_eq!(strip_leading_spaces_n("   text", 3), "text");
        assert_eq!(strip_leading_spaces_n("  text", 3), "text");
        assert_eq!(strip_leading_spaces_n(" text", 3), "text");
        assert_eq!(strip_leading_spaces_n("text", 3), "text");
        assert_eq!(strip_leading_spaces_n("    text", 3), " text");
    }

    #[test]
    fn test_count_leading_spaces() {
        assert_eq!(count_leading_spaces("   text", 10), 3);
        assert_eq!(count_leading_spaces("  text", 10), 2);
        assert_eq!(count_leading_spaces("text", 10), 0);
        assert_eq!(count_leading_spaces("     text", 3), 3);
    }

    #[test]
    fn test_skip_whitespace() {
        assert_eq!(skip_whitespace("  \t text"), "text");
        assert_eq!(skip_whitespace("\t\ttext"), "text");
        assert_eq!(skip_whitespace("text"), "text");
        assert_eq!(skip_whitespace("   "), "");
    }

    #[test]
    fn test_strip_newline() {
        assert_eq!(strip_newline("text\n"), ("text", "\n"));
        assert_eq!(strip_newline("text\r\n"), ("text", "\r\n"));
        assert_eq!(strip_newline("text"), ("text", ""));
    }
}
