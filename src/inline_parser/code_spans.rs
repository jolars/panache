/// Parsing for inline code spans (`code`)
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

/// Try to parse a code span starting at the current position.
/// Returns the number of characters consumed if successful, or None if not a code span.
pub fn try_parse_code_span(text: &str) -> Option<(usize, &str, usize)> {
    // Count opening backticks
    let opening_backticks = text.bytes().take_while(|&b| b == b'`').count();
    if opening_backticks == 0 {
        return None;
    }

    let rest = &text[opening_backticks..];

    // Look for matching closing backticks
    let mut pos = 0;
    while pos < rest.len() {
        if rest[pos..].starts_with('`') {
            let closing_backticks = rest[pos..].bytes().take_while(|&b| b == b'`').count();

            if closing_backticks == opening_backticks {
                // Found matching close
                let code_content = &rest[..pos];
                let total_len = opening_backticks + pos + closing_backticks;
                return Some((total_len, code_content, opening_backticks));
            }
            // Skip these backticks and continue searching
            pos += closing_backticks;
        } else {
            // Move to next character (handle UTF-8 properly)
            pos += rest[pos..].chars().next()?.len_utf8();
        }
    }

    // No matching close found
    None
}

/// Emit a code span node to the builder.
pub fn emit_code_span(builder: &mut GreenNodeBuilder, content: &str, backtick_count: usize) {
    builder.start_node(SyntaxKind::CodeSpan.into());

    // Opening backticks
    builder.token(
        SyntaxKind::CodeSpanMarker.into(),
        &"`".repeat(backtick_count),
    );

    // Code content
    builder.token(SyntaxKind::TEXT.into(), content);

    // Closing backticks
    builder.token(
        SyntaxKind::CodeSpanMarker.into(),
        &"`".repeat(backtick_count),
    );

    builder.finish_node();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_code_span() {
        let result = try_parse_code_span("`code`");
        assert_eq!(result, Some((6, "code", 1)));
    }

    #[test]
    fn test_parse_code_span_with_backticks() {
        let result = try_parse_code_span("`` `backtick` ``");
        assert_eq!(result, Some((16, " `backtick` ", 2)));
    }

    #[test]
    fn test_parse_code_span_triple_backticks() {
        let result = try_parse_code_span("``` `` ```");
        assert_eq!(result, Some((10, " `` ", 3)));
    }

    #[test]
    fn test_parse_code_span_no_close() {
        let result = try_parse_code_span("`no close");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_code_span_mismatched_close() {
        let result = try_parse_code_span("`single``");
        assert_eq!(result, None);
    }

    #[test]
    fn test_not_code_span() {
        let result = try_parse_code_span("no backticks");
        assert_eq!(result, None);
    }

    #[test]
    fn test_code_span_with_trailing_text() {
        let result = try_parse_code_span("`code` and more");
        assert_eq!(result, Some((6, "code", 1)));
    }
}
