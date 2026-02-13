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

/// Strip up to 3 leading spaces from a line.
pub(crate) fn strip_leading_spaces(line: &str) -> &str {
    line.strip_prefix("   ")
        .or_else(|| line.strip_prefix("  "))
        .or_else(|| line.strip_prefix(" "))
        .unwrap_or(line)
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
