/// Parsing for inline code spans (`code`)
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

// Import the attribute parsing from utils
use crate::parser::utils::attributes::{
    AttributeBlock, emit_attributes, try_parse_trailing_attributes,
};

/// Try to parse a code span starting at the current position.
/// Returns (total_len, code_content, backtick_count, optional_attributes) if successful.
pub fn try_parse_code_span(text: &str) -> Option<(usize, &str, usize, Option<AttributeBlock>)> {
    // Count opening backticks
    let opening_backticks = text.bytes().take_while(|&b| b == b'`').count();
    if opening_backticks == 0 {
        return None;
    }

    let rest = &text[opening_backticks..];
    let rest_bytes = rest.as_bytes();

    // Look for matching closing backticks. Skip non-backtick bytes via
    // memchr (compiles to vectorized scan) instead of stepping one
    // UTF-8 char at a time — `try_parse_code_span` is called on every
    // `` ` `` byte the dispatcher encounters and scans to end of input
    // when no closer matches, so the inner skip dominates self-time.
    let mut pos = 0;
    while pos < rest_bytes.len() {
        let next_tick = match rest_bytes[pos..].iter().position(|&b| b == b'`') {
            Some(off) => pos + off,
            None => break,
        };
        // Count the run of consecutive backticks starting at `next_tick`.
        let mut closing_backticks = 0;
        while next_tick + closing_backticks < rest_bytes.len()
            && rest_bytes[next_tick + closing_backticks] == b'`'
        {
            closing_backticks += 1;
        }

        if closing_backticks == opening_backticks {
            // Found matching close
            let code_content = &rest[..next_tick];
            let after_close = opening_backticks + next_tick + closing_backticks;

            // Check for trailing attributes {#id .class key=value}
            let remaining = &text[after_close..];
            if remaining.starts_with('{') {
                // Find the closing brace
                if let Some(close_brace_pos) = remaining.find('}') {
                    let attr_text = &remaining[..=close_brace_pos];
                    // Try to parse as attributes
                    if let Some((attrs, _)) = try_parse_trailing_attributes(attr_text) {
                        let total_len = after_close + close_brace_pos + 1;
                        return Some((total_len, code_content, opening_backticks, Some(attrs)));
                    }
                }
            }

            // No attributes, just return the code span
            return Some((after_close, code_content, opening_backticks, None));
        }
        // Skip past this run of backticks and keep searching.
        pos = next_tick + closing_backticks;
    }

    // No matching close found
    None
}

/// Emit a code span node to the builder.
pub fn emit_code_span(
    builder: &mut GreenNodeBuilder,
    content: &str,
    backtick_count: usize,
    attributes: Option<AttributeBlock>,
) {
    builder.start_node(SyntaxKind::INLINE_CODE.into());

    // Opening backticks
    builder.token(
        SyntaxKind::INLINE_CODE_MARKER.into(),
        &"`".repeat(backtick_count),
    );

    // Code content
    builder.token(SyntaxKind::INLINE_CODE_CONTENT.into(), content);

    // Closing backticks
    builder.token(
        SyntaxKind::INLINE_CODE_MARKER.into(),
        &"`".repeat(backtick_count),
    );

    // Emit attributes if present
    if let Some(attrs) = attributes {
        emit_attributes(builder, &attrs);
    }

    builder.finish_node();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_code_span() {
        let result = try_parse_code_span("`code`");
        assert_eq!(result, Some((6, "code", 1, None)));
    }

    #[test]
    fn test_parse_code_span_with_backticks() {
        let result = try_parse_code_span("`` `backtick` ``");
        assert_eq!(result, Some((16, " `backtick` ", 2, None)));
    }

    #[test]
    fn test_parse_code_span_triple_backticks() {
        let result = try_parse_code_span("``` `` ```");
        assert_eq!(result, Some((10, " `` ", 3, None)));
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
        assert_eq!(result, Some((6, "code", 1, None)));
    }

    #[test]
    fn test_code_span_with_simple_class() {
        let result = try_parse_code_span("`code`{.python}");
        let (len, content, backticks, attrs) = result.unwrap();
        assert_eq!(len, 15);
        assert_eq!(content, "code");
        assert_eq!(backticks, 1);
        assert!(attrs.is_some());
        let attrs = attrs.unwrap();
        assert_eq!(attrs.classes, vec!["python"]);
    }

    #[test]
    fn test_code_span_with_id() {
        let result = try_parse_code_span("`code`{#mycode}");
        let (len, content, backticks, attrs) = result.unwrap();
        assert_eq!(len, 15);
        assert_eq!(content, "code");
        assert_eq!(backticks, 1);
        assert!(attrs.is_some());
        let attrs = attrs.unwrap();
        assert_eq!(attrs.identifier, Some("mycode".to_string()));
    }

    #[test]
    fn test_code_span_with_full_attributes() {
        let result = try_parse_code_span("`x + y`{#calc .haskell .eval}");
        let (len, content, backticks, attrs) = result.unwrap();
        assert_eq!(len, 29);
        assert_eq!(content, "x + y");
        assert_eq!(backticks, 1);
        assert!(attrs.is_some());
        let attrs = attrs.unwrap();
        assert_eq!(attrs.identifier, Some("calc".to_string()));
        assert_eq!(attrs.classes, vec!["haskell", "eval"]);
    }

    #[test]
    fn test_code_span_attributes_must_be_adjacent() {
        // Space between closing backtick and { should not parse attributes
        let result = try_parse_code_span("`code` {.python}");
        assert_eq!(result, Some((6, "code", 1, None)));
    }
}
