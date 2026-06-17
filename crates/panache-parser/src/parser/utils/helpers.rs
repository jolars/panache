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

/// Emit a table separator line as distinct marker tokens instead of one
/// coalesced `TEXT`. Splits the column delimiters (`|` / `+`), dash runs,
/// equals runs (grid `===` dividers), colons, and interior whitespace into
/// separate CST tokens so downstream alignment/width derivations read
/// structure rather than re-scanning a string. Any unexpected bytes fall back
/// to a `TEXT` token so the emission stays lossless. The concatenation of all
/// emitted token texts byte-equals `line`.
///
/// The caller has already emitted any container prefix (indentation,
/// blockquote markers) as separate tokens; `line` is the separator tail.
pub(crate) fn emit_separator_tokens(builder: &mut GreenNodeBuilder<'static>, line: &str) {
    let (content, newline) = strip_newline(line);
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'|' | b'+' => {
                builder.token(SyntaxKind::TABLE_SEP_DELIM.into(), &content[i..i + 1]);
                i += 1;
            }
            b':' => {
                builder.token(SyntaxKind::TABLE_SEP_COLON.into(), &content[i..i + 1]);
                i += 1;
            }
            b'-' => {
                let start = i;
                while i < bytes.len() && bytes[i] == b'-' {
                    i += 1;
                }
                builder.token(SyntaxKind::TABLE_SEP_DASHES.into(), &content[start..i]);
            }
            b'=' => {
                let start = i;
                while i < bytes.len() && bytes[i] == b'=' {
                    i += 1;
                }
                builder.token(SyntaxKind::TABLE_SEP_EQUALS.into(), &content[start..i]);
            }
            b' ' | b'\t' => {
                let start = i;
                while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
                    i += 1;
                }
                builder.token(SyntaxKind::TABLE_SEP_WHITESPACE.into(), &content[start..i]);
            }
            _ => {
                // Unexpected byte (the block detector validated this is a
                // separator, but stay lossless and total): accumulate the run
                // of unrecognized bytes and emit as TEXT. Advance by whole
                // chars so we never split a multibyte sequence.
                let start = i;
                while i < bytes.len()
                    && !matches!(bytes[i], b'|' | b'+' | b':' | b'-' | b'=' | b' ' | b'\t')
                {
                    i += 1;
                }
                builder.token(SyntaxKind::TEXT.into(), &content[start..i]);
            }
        }
    }
    if !newline.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline);
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

    fn separator_tokens(line: &str) -> Vec<(SyntaxKind, String)> {
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(SyntaxKind::TABLE_SEPARATOR.into());
        emit_separator_tokens(&mut builder, line);
        builder.finish_node();
        let node = crate::syntax::SyntaxNode::new_root(builder.finish());
        node.children_with_tokens()
            .filter_map(|el| el.into_token())
            .map(|t| (t.kind(), t.text().to_string()))
            .collect()
    }

    #[test]
    fn test_emit_separator_tokens_reconstruction() {
        // Concatenation of token texts must byte-equal the input.
        for line in [
            "|:--|--:|:-:|\n",
            "+------+:----:+------+\n",
            "+======+======+\r\n",
            "------- ------ ----------\n",
            ":--:",               // no bounding delims, no newline
            "|:--|--:|?weird|\n", // unexpected byte falls back to TEXT
        ] {
            let reconstructed: String = separator_tokens(line)
                .iter()
                .map(|(_, t)| t.as_str())
                .collect();
            assert_eq!(reconstructed, line, "round-trip failed for {line:?}");
        }
    }

    #[test]
    fn test_emit_separator_tokens_kinds() {
        use SyntaxKind::*;
        assert_eq!(
            separator_tokens("|:--|--:|\n"),
            vec![
                (TABLE_SEP_DELIM, "|".to_string()),
                (TABLE_SEP_COLON, ":".to_string()),
                (TABLE_SEP_DASHES, "--".to_string()),
                (TABLE_SEP_DELIM, "|".to_string()),
                (TABLE_SEP_DASHES, "--".to_string()),
                (TABLE_SEP_COLON, ":".to_string()),
                (TABLE_SEP_DELIM, "|".to_string()),
                (NEWLINE, "\n".to_string()),
            ],
        );
        // Grid `===` divider and interior whitespace in a simple separator.
        assert_eq!(
            separator_tokens("--- ---\n"),
            vec![
                (TABLE_SEP_DASHES, "---".to_string()),
                (TABLE_SEP_WHITESPACE, " ".to_string()),
                (TABLE_SEP_DASHES, "---".to_string()),
                (NEWLINE, "\n".to_string()),
            ],
        );
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
