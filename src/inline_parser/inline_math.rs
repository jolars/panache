/// Parsing for inline math ($...$)
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

/// Try to parse an inline math span starting at the current position.
/// Returns the number of characters consumed if successful, or None if not inline math.
///
/// Per Pandoc spec (tex_math_dollars extension):
/// - Opening $ must have non-space character immediately to its right
/// - Closing $ must have non-space character immediately to its left
/// - Closing $ must not be followed immediately by a digit
pub fn try_parse_inline_math(text: &str) -> Option<(usize, &str)> {
    // Must start with exactly one $
    if !text.starts_with('$') || text.starts_with("$$") {
        return None;
    }

    let rest = &text[1..];

    // Opening $ must have non-space character immediately to its right
    if rest.is_empty() || rest.starts_with(char::is_whitespace) {
        return None;
    }

    // Look for closing $
    let mut pos = 0;
    while pos < rest.len() {
        let ch = rest[pos..].chars().next()?;

        if ch == '$' {
            // Check if it's escaped
            if pos > 0 && rest.as_bytes()[pos - 1] == b'\\' {
                // Escaped dollar, continue searching
                pos += 1;
                continue;
            }

            // Check if it's part of $$ (display math)
            if rest[pos..].starts_with("$$") {
                // This is display math start, not inline math end
                return None;
            }

            // Closing $ must have non-space character immediately to its left
            if pos == 0 || rest[..pos].ends_with(char::is_whitespace) {
                // Continue searching - this $ doesn't close the math
                pos += 1;
                continue;
            }

            // Closing $ must not be followed immediately by a digit
            if let Some(next_ch) = rest[pos + 1..].chars().next() {
                if next_ch.is_ascii_digit() {
                    // Continue searching - this $ doesn't close the math
                    pos += 1;
                    continue;
                }
            }

            // Found valid closing $
            let math_content = &rest[..pos];
            let total_len = 1 + pos + 1; // opening $ + content + closing $
            return Some((total_len, math_content));
        }

        // Dollar signs can't span multiple lines
        if ch == '\n' {
            return None;
        }

        pos += ch.len_utf8();
    }

    // No matching close found
    None
}

/// Emit an inline math node to the builder.
pub fn emit_inline_math(builder: &mut GreenNodeBuilder, content: &str) {
    builder.start_node(SyntaxKind::InlineMath.into());

    // Opening $
    builder.token(SyntaxKind::InlineMathMarker.into(), "$");

    // Math content
    builder.token(SyntaxKind::TEXT.into(), content);

    // Closing $
    builder.token(SyntaxKind::InlineMathMarker.into(), "$");

    builder.finish_node();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_inline_math() {
        let result = try_parse_inline_math("$x = y$");
        assert_eq!(result, Some((7, "x = y")));
    }

    #[test]
    fn test_parse_inline_math_with_spaces_inside() {
        // Spaces inside math are OK, just not immediately after opening or before closing
        let result = try_parse_inline_math("$a + b$");
        assert_eq!(result, Some((7, "a + b")));
    }

    #[test]
    fn test_parse_inline_math_complex() {
        let result = try_parse_inline_math(r"$\frac{1}{2}$");
        assert_eq!(result, Some((13, r"\frac{1}{2}")));
    }

    #[test]
    fn test_not_inline_math_display() {
        // $$ is display math, not inline
        let result = try_parse_inline_math("$$x = y$$");
        assert_eq!(result, None);
    }

    #[test]
    fn test_inline_math_no_close() {
        let result = try_parse_inline_math("$no close");
        assert_eq!(result, None);
    }

    #[test]
    fn test_inline_math_no_multiline() {
        let result = try_parse_inline_math("$x =\ny$");
        assert_eq!(result, None);
    }

    #[test]
    fn test_not_inline_math() {
        let result = try_parse_inline_math("no dollar");
        assert_eq!(result, None);
    }

    #[test]
    fn test_inline_math_with_trailing_text() {
        let result = try_parse_inline_math("$x$ and more");
        assert_eq!(result, Some((3, "x")));
    }

    #[test]
    fn test_inline_math_escaped_dollar() {
        // Currently we don't handle escaped dollars - this is a TODO
        // This test documents current behavior
        let result = try_parse_inline_math(r"$a \$ b$");
        // This should find the first unescaped $, but our simple impl
        // will find the escaped one. We'll improve this later.
        assert!(result.is_some());
    }

    #[test]
    fn test_spec_opening_must_have_non_space_right() {
        // Per Pandoc spec: opening $ must have non-space immediately to right
        let result = try_parse_inline_math("$ x$");
        assert_eq!(result, None, "Opening $ with space should not parse");
    }

    #[test]
    fn test_spec_closing_must_have_non_space_left() {
        // Per Pandoc spec: closing $ must have non-space immediately to left
        let result = try_parse_inline_math("$x $");
        assert_eq!(result, None, "Closing $ with space should not parse");
    }

    #[test]
    fn test_spec_closing_not_followed_by_digit() {
        // Per Pandoc spec: closing $ must not be followed by digit
        let result = try_parse_inline_math("$x$5");
        assert_eq!(result, None, "Closing $ followed by digit should not parse");
    }

    #[test]
    fn test_spec_dollar_amounts() {
        // $20,000 should not parse as math
        let result = try_parse_inline_math("$20,000");
        assert_eq!(result, None, "Dollar amounts should not parse as math");
    }

    #[test]
    fn test_valid_math_after_spec_checks() {
        // $x$ alone should still parse
        let result = try_parse_inline_math("$x$");
        assert_eq!(result, Some((3, "x")), "Valid math should parse");
    }

    #[test]
    fn test_math_followed_by_non_digit() {
        // $x$a should parse (not followed by digit)
        let result = try_parse_inline_math("$x$a");
        assert_eq!(
            result,
            Some((3, "x")),
            "Math followed by non-digit should parse"
        );
    }
}
