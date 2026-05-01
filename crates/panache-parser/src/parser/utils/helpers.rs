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

/// Strip trailing `\n` and `\r` bytes. ASCII byte-level equivalent of
/// `s.trim_end_matches(['\r', '\n'])` — avoids the slice-pattern
/// `MultiCharEqSearcher` codepath that goes through `char_indices` and
/// shows up as a measurable hot frame in per-line block detect work.
#[inline]
pub(crate) fn trim_end_newlines(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut end = bytes.len();
    while end > 0 {
        let b = bytes[end - 1];
        if b == b'\n' || b == b'\r' {
            end -= 1;
        } else {
            break;
        }
    }
    // SAFETY: we only stripped ASCII `\n` / `\r` bytes from the end, so the
    // remaining prefix is still valid UTF-8 ending on a char boundary.
    unsafe { std::str::from_utf8_unchecked(&bytes[..end]) }
}

/// Strip leading ASCII space and tab bytes. Equivalent to
/// `s.trim_start_matches([' ', '\t'])` but byte-level.
#[inline]
pub(crate) fn trim_start_spaces_tabs(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut start = 0;
    while start < bytes.len() {
        let b = bytes[start];
        if b == b' ' || b == b'\t' {
            start += 1;
        } else {
            break;
        }
    }
    // SAFETY: only ASCII bytes stripped from the start.
    unsafe { std::str::from_utf8_unchecked(&bytes[start..]) }
}

/// Test whether `s` is a blank line: empty or composed only of ASCII
/// whitespace (`' '`, `'\t'`, `'\n'`, `'\r'`). Equivalent to
/// `s.trim_end_matches('\n').trim().is_empty()` for ASCII-whitespace
/// inputs but bypasses the unicode-whitespace iteration in `str::trim`.
#[inline]
pub(crate) fn is_blank_line(s: &str) -> bool {
    s.as_bytes()
        .iter()
        .all(|&b| b == b' ' || b == b'\t' || b == b'\n' || b == b'\r')
}

/// Strip trailing ASCII space and tab bytes. Equivalent to
/// `s.trim_end_matches([' ', '\t'])` but byte-level.
#[inline]
pub(crate) fn trim_end_spaces_tabs(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut end = bytes.len();
    while end > 0 {
        let b = bytes[end - 1];
        if b == b' ' || b == b'\t' {
            end -= 1;
        } else {
            break;
        }
    }
    // SAFETY: only ASCII bytes stripped from the end.
    unsafe { std::str::from_utf8_unchecked(&bytes[..end]) }
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
    fn test_strip_newline() {
        assert_eq!(strip_newline("text\n"), ("text", "\n"));
        assert_eq!(strip_newline("text\r\n"), ("text", "\r\n"));
        assert_eq!(strip_newline("text"), ("text", ""));
    }

    #[test]
    fn test_trim_end_newlines() {
        assert_eq!(trim_end_newlines("foo\n"), "foo");
        assert_eq!(trim_end_newlines("foo\r\n"), "foo");
        assert_eq!(trim_end_newlines("foo\n\n"), "foo");
        assert_eq!(trim_end_newlines("foo"), "foo");
        assert_eq!(trim_end_newlines(""), "");
        assert_eq!(trim_end_newlines("\n"), "");
        // Non-ASCII byte sequences stay intact.
        assert_eq!(trim_end_newlines("föö\n"), "föö");
    }

    #[test]
    fn test_trim_spaces_tabs() {
        assert_eq!(trim_start_spaces_tabs("  \tfoo"), "foo");
        assert_eq!(trim_start_spaces_tabs("foo"), "foo");
        assert_eq!(trim_start_spaces_tabs(""), "");
        assert_eq!(trim_end_spaces_tabs("foo  \t"), "foo");
        assert_eq!(trim_end_spaces_tabs("foo"), "foo");
        assert_eq!(trim_end_spaces_tabs(""), "");
        assert_eq!(trim_end_spaces_tabs("föö  "), "föö");
    }
}
