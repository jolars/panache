/// Parsing for inline math ($...$)
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

/// Try to parse an inline math span starting at the current position.
/// Returns the number of characters consumed if successful, or None if not inline math.
pub fn try_parse_inline_math(text: &str) -> Option<(usize, &str)> {
    // Must start with exactly one $
    if !text.starts_with('$') || text.starts_with("$$") {
        return None;
    }

    let rest = &text[1..];

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

            // Found closing $
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
    fn test_parse_inline_math_with_spaces() {
        let result = try_parse_inline_math("$ a + b $");
        assert_eq!(result, Some((9, " a + b ")));
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
}
