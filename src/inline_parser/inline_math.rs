/// Parsing for inline math ($...$) and display math ($$...$$)
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
            if let Some(next_ch) = rest[pos + 1..].chars().next()
                && next_ch.is_ascii_digit()
            {
                // Continue searching - this $ doesn't close the math
                pos += 1;
                continue;
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

/// Try to parse single backslash inline math: \(...\)
/// Extension: tex_math_single_backslash
pub fn try_parse_single_backslash_inline_math(text: &str) -> Option<(usize, &str)> {
    if !text.starts_with(r"\(") {
        return None;
    }

    let rest = &text[2..]; // Skip \(

    // Look for closing \)
    let mut pos = 0;
    while pos < rest.len() {
        let ch = rest[pos..].chars().next()?;

        if ch == '\\' && rest[pos..].starts_with(r"\)") {
            // Found closing \)
            let math_content = &rest[..pos];
            let total_len = 2 + pos + 2; // \( + content + \)
            return Some((total_len, math_content));
        }

        // Can't span multiple lines
        if ch == '\n' {
            return None;
        }

        pos += ch.len_utf8();
    }

    None
}

/// Try to parse double backslash inline math: \\(...\\)
/// Extension: tex_math_double_backslash
pub fn try_parse_double_backslash_inline_math(text: &str) -> Option<(usize, &str)> {
    if !text.starts_with(r"\\(") {
        return None;
    }

    let rest = &text[3..]; // Skip \\(

    // Look for closing \\)
    let mut pos = 0;
    while pos < rest.len() {
        let ch = rest[pos..].chars().next()?;

        if ch == '\\' && rest[pos..].starts_with(r"\\)") {
            // Found closing \\)
            let math_content = &rest[..pos];
            let total_len = 3 + pos + 3; // \\( + content + \\)
            return Some((total_len, math_content));
        }

        // Can't span multiple lines
        if ch == '\n' {
            return None;
        }

        pos += ch.len_utf8();
    }

    None
}

/// Try to parse display math ($$...$$) starting at the current position.
/// Returns the number of characters consumed and the math content if successful.
/// Display math can span multiple lines in inline contexts.
pub fn try_parse_display_math(text: &str) -> Option<(usize, &str)> {
    // Must start with at least $$
    if !text.starts_with("$$") {
        return None;
    }

    // Count opening dollar signs
    let opening_count = text.chars().take_while(|&c| c == '$').count();
    if opening_count < 2 {
        return None;
    }

    let rest = &text[opening_count..];

    // Look for matching closing delimiter
    let mut pos = 0;
    while pos < rest.len() {
        let ch = rest[pos..].chars().next()?;

        if ch == '$' {
            // Check if it's escaped
            if pos > 0 && rest.as_bytes()[pos - 1] == b'\\' {
                // Escaped dollar, continue searching
                pos += ch.len_utf8();
                continue;
            }

            // Count closing dollar signs
            let closing_count = rest[pos..].chars().take_while(|&c| c == '$').count();

            // Must have at least as many closing dollars as opening
            if closing_count >= opening_count {
                let math_content = &rest[..pos];
                let total_len = opening_count + pos + closing_count;
                return Some((total_len, math_content));
            }

            // Not enough dollars, skip this run and continue
            pos += closing_count;
            continue;
        }

        pos += ch.len_utf8();
    }

    // No matching close found
    None
}

/// Try to parse single backslash display math: \[...\]
/// Extension: tex_math_single_backslash
pub fn try_parse_single_backslash_display_math(text: &str) -> Option<(usize, &str)> {
    if !text.starts_with(r"\[") {
        return None;
    }

    let rest = &text[2..]; // Skip \[

    // Look for closing \]
    let mut pos = 0;
    while pos < rest.len() {
        let ch = rest[pos..].chars().next()?;

        if ch == '\\' && rest[pos..].starts_with(r"\]") {
            // Found closing \]
            let math_content = &rest[..pos];
            let total_len = 2 + pos + 2; // \[ + content + \]
            return Some((total_len, math_content));
        }

        pos += ch.len_utf8();
    }

    None
}

/// Try to parse double backslash display math: \\[...\\]
/// Extension: tex_math_double_backslash
pub fn try_parse_double_backslash_display_math(text: &str) -> Option<(usize, &str)> {
    if !text.starts_with(r"\\[") {
        return None;
    }

    let rest = &text[3..]; // Skip \\[

    // Look for closing \\]
    let mut pos = 0;
    while pos < rest.len() {
        let ch = rest[pos..].chars().next()?;

        if ch == '\\' && rest[pos..].starts_with(r"\\]") {
            // Found closing \\]
            let math_content = &rest[..pos];
            let total_len = 3 + pos + 3; // \\[ + content + \\]
            return Some((total_len, math_content));
        }

        pos += ch.len_utf8();
    }

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

/// Emit a single backslash inline math node: \(...\)
pub fn emit_single_backslash_inline_math(builder: &mut GreenNodeBuilder, content: &str) {
    builder.start_node(SyntaxKind::InlineMath.into());

    builder.token(SyntaxKind::InlineMathMarker.into(), r"\(");
    builder.token(SyntaxKind::TEXT.into(), content);
    builder.token(SyntaxKind::InlineMathMarker.into(), r"\)");

    builder.finish_node();
}

/// Emit a double backslash inline math node: \\(...\\)
pub fn emit_double_backslash_inline_math(builder: &mut GreenNodeBuilder, content: &str) {
    builder.start_node(SyntaxKind::InlineMath.into());

    builder.token(SyntaxKind::InlineMathMarker.into(), r"\\(");
    builder.token(SyntaxKind::TEXT.into(), content);
    builder.token(SyntaxKind::InlineMathMarker.into(), r"\\)");

    builder.finish_node();
}

/// Emit a display math node to the builder (when occurring inline in paragraph).
pub fn emit_display_math(builder: &mut GreenNodeBuilder, content: &str, dollar_count: usize) {
    builder.start_node(SyntaxKind::InlineMath.into()); // Note: Using InlineMath for now

    // Opening $$
    let marker = "$".repeat(dollar_count);
    builder.token(SyntaxKind::BlockMathMarker.into(), &marker);

    // Math content
    builder.token(SyntaxKind::TEXT.into(), content);

    // Closing $$
    builder.token(SyntaxKind::BlockMathMarker.into(), &marker);

    builder.finish_node();
}

/// Emit a single backslash display math node: \[...\]
pub fn emit_single_backslash_display_math(builder: &mut GreenNodeBuilder, content: &str) {
    builder.start_node(SyntaxKind::InlineMath.into());

    builder.token(SyntaxKind::BlockMathMarker.into(), r"\[");
    builder.token(SyntaxKind::TEXT.into(), content);
    builder.token(SyntaxKind::BlockMathMarker.into(), r"\]");

    builder.finish_node();
}

/// Emit a double backslash display math node: \\[...\\]
pub fn emit_double_backslash_display_math(builder: &mut GreenNodeBuilder, content: &str) {
    builder.start_node(SyntaxKind::InlineMath.into());

    builder.token(SyntaxKind::BlockMathMarker.into(), r"\\[");
    builder.token(SyntaxKind::TEXT.into(), content);
    builder.token(SyntaxKind::BlockMathMarker.into(), r"\\]");

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

    // Display math tests
    #[test]
    fn test_parse_display_math_simple() {
        let result = try_parse_display_math("$$x = y$$");
        assert_eq!(result, Some((9, "x = y")));
    }

    #[test]
    fn test_parse_display_math_multiline() {
        let result = try_parse_display_math("$$\nx = y\n$$");
        assert_eq!(result, Some((11, "\nx = y\n")));
    }

    #[test]
    fn test_parse_display_math_triple_dollars() {
        let result = try_parse_display_math("$$$x = y$$$");
        assert_eq!(result, Some((11, "x = y")));
    }

    #[test]
    fn test_parse_display_math_no_close() {
        let result = try_parse_display_math("$$no close");
        assert_eq!(result, None);
    }

    #[test]
    fn test_not_display_math() {
        let result = try_parse_display_math("$single dollar");
        assert_eq!(result, None);
    }

    #[test]
    fn test_display_math_with_trailing_text() {
        let result = try_parse_display_math("$$x = y$$ and more");
        assert_eq!(result, Some((9, "x = y")));
    }

    // Single backslash math tests
    #[test]
    fn test_single_backslash_inline_math() {
        let result = try_parse_single_backslash_inline_math(r"\(x^2\)");
        assert_eq!(result, Some((7, "x^2")));
    }

    #[test]
    fn test_single_backslash_inline_math_complex() {
        let result = try_parse_single_backslash_inline_math(r"\(\frac{a}{b}\)");
        assert_eq!(result, Some((15, r"\frac{a}{b}")));
    }

    #[test]
    fn test_single_backslash_inline_math_no_close() {
        let result = try_parse_single_backslash_inline_math(r"\(no close");
        assert_eq!(result, None);
    }

    #[test]
    fn test_single_backslash_inline_math_no_multiline() {
        let result = try_parse_single_backslash_inline_math("\\(x =\ny\\)");
        assert_eq!(result, None);
    }

    #[test]
    fn test_single_backslash_display_math() {
        let result = try_parse_single_backslash_display_math(r"\[E = mc^2\]");
        assert_eq!(result, Some((12, "E = mc^2")));
    }

    #[test]
    fn test_single_backslash_display_math_multiline() {
        let result = try_parse_single_backslash_display_math("\\[\nx = y\n\\]");
        assert_eq!(result, Some((11, "\nx = y\n")));
    }

    #[test]
    fn test_single_backslash_display_math_no_close() {
        let result = try_parse_single_backslash_display_math(r"\[no close");
        assert_eq!(result, None);
    }

    // Double backslash math tests
    #[test]
    fn test_double_backslash_inline_math() {
        let result = try_parse_double_backslash_inline_math(r"\\(x^2\\)");
        assert_eq!(result, Some((9, "x^2")));
    }

    #[test]
    fn test_double_backslash_inline_math_complex() {
        let result = try_parse_double_backslash_inline_math(r"\\(\alpha + \beta\\)");
        assert_eq!(result, Some((20, r"\alpha + \beta")));
    }

    #[test]
    fn test_double_backslash_inline_math_no_close() {
        let result = try_parse_double_backslash_inline_math(r"\\(no close");
        assert_eq!(result, None);
    }

    #[test]
    fn test_double_backslash_inline_math_no_multiline() {
        let result = try_parse_double_backslash_inline_math("\\\\(x =\ny\\\\)");
        assert_eq!(result, None);
    }

    #[test]
    fn test_double_backslash_display_math() {
        let result = try_parse_double_backslash_display_math(r"\\[E = mc^2\\]");
        assert_eq!(result, Some((14, "E = mc^2")));
    }

    #[test]
    fn test_double_backslash_display_math_multiline() {
        let result = try_parse_double_backslash_display_math("\\\\[\nx = y\n\\\\]");
        assert_eq!(result, Some((13, "\nx = y\n")));
    }

    #[test]
    fn test_double_backslash_display_math_no_close() {
        let result = try_parse_double_backslash_display_math(r"\\[no close");
        assert_eq!(result, None);
    }
}
