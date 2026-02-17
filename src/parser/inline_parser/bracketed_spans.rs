//! Bracketed span parsing for Pandoc's `bracketed_spans` extension.
//!
//! Syntax: `[inline content]{.class key="val"}`

use crate::config::Config;
use crate::parser::inline_parser::parse_inline_text;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

/// Try to parse a bracketed span starting from the current position.
/// Returns (total_length, content, attributes) if successful.
///
/// A bracketed span is: [content]{attributes}
/// - Must have matching brackets
/// - Must be immediately followed by attributes in braces
/// - Can contain nested inline elements
pub(crate) fn try_parse_bracketed_span(text: &str) -> Option<(usize, String, String)> {
    let bytes = text.as_bytes();

    if bytes.first() != Some(&b'[') {
        return None;
    }

    // Find the closing bracket, tracking nesting
    let mut pos = 1;
    let mut depth = 1;
    let mut escaped = false;

    while pos < text.len() {
        if escaped {
            escaped = false;
            pos += 1;
            continue;
        }

        match bytes[pos] {
            b'\\' => escaped = true,
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    // Found closing bracket, now check for attributes
                    let content = &text[1..pos];

                    // Must be immediately followed by {attributes}
                    if pos + 1 >= text.len() || bytes[pos + 1] != b'{' {
                        return None;
                    }

                    // Find the closing brace for attributes
                    let attr_start = pos + 2;
                    let mut attr_pos = attr_start;
                    let mut attr_escaped = false;

                    while attr_pos < text.len() {
                        if attr_escaped {
                            attr_escaped = false;
                            attr_pos += 1;
                            continue;
                        }

                        match bytes[attr_pos] {
                            b'\\' => attr_escaped = true,
                            b'}' => {
                                // Found closing brace
                                let attributes = &text[attr_start..attr_pos];
                                let total_len = attr_pos + 1;
                                return Some((
                                    total_len,
                                    content.to_string(),
                                    attributes.to_string(),
                                ));
                            }
                            _ => {}
                        }
                        attr_pos += 1;
                    }

                    // No closing brace found
                    return None;
                }
            }
            _ => {}
        }
        pos += 1;
    }

    None
}

/// Emit a bracketed span node
pub(crate) fn emit_bracketed_span(
    builder: &mut GreenNodeBuilder,
    content: &str,
    attributes: &str,
    config: &Config,
) {
    builder.start_node(SyntaxKind::BRACKETED_SPAN.into());

    // Opening bracket
    builder.token(SyntaxKind::SPAN_BRACKET_OPEN.into(), "[");

    // Content (with recursive inline parsing)
    builder.start_node(SyntaxKind::SPAN_CONTENT.into());
    parse_inline_text(builder, content, config, None);
    builder.finish_node(); // SpanContent

    // Closing bracket
    builder.token(SyntaxKind::SPAN_BRACKET_CLOSE.into(), "]");

    // Attributes (preserve all whitespace - formatter will normalize)
    builder.start_node(SyntaxKind::SPAN_ATTRIBUTES.into());
    builder.token(SyntaxKind::TEXT.into(), "{");

    // Parse attributes byte-by-byte to preserve whitespace
    let mut pos = 0;
    let bytes = attributes.as_bytes();
    while pos < bytes.len() {
        if bytes[pos].is_ascii_whitespace() {
            // Emit whitespace run
            let start = pos;
            while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            builder.token(SyntaxKind::WHITESPACE.into(), &attributes[start..pos]);
        } else {
            // Emit non-whitespace run
            let start = pos;
            while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            builder.token(SyntaxKind::TEXT.into(), &attributes[start..pos]);
        }
    }

    builder.token(SyntaxKind::TEXT.into(), "}");
    builder.finish_node(); // SpanAttributes

    builder.finish_node(); // BracketedSpan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_span() {
        let result = try_parse_bracketed_span("[text]{.class}");
        assert!(result.is_some());
        let (len, content, attrs) = result.unwrap();
        assert_eq!(len, 14);
        assert_eq!(content, "text");
        assert_eq!(attrs, ".class");
    }

    #[test]
    fn parses_span_with_multiple_attributes() {
        let result = try_parse_bracketed_span("[text]{.class key=\"val\"}");
        assert!(result.is_some());
        let (len, content, attrs) = result.unwrap();
        assert_eq!(len, 24);
        assert_eq!(content, "text");
        assert_eq!(attrs, ".class key=\"val\"");
    }

    #[test]
    fn parses_span_with_emphasis() {
        let result = try_parse_bracketed_span("[**bold** text]{.highlight}");
        assert!(result.is_some());
        let (len, content, attrs) = result.unwrap();
        assert_eq!(len, 27);
        assert_eq!(content, "**bold** text");
        assert_eq!(attrs, ".highlight");
    }

    #[test]
    fn handles_nested_brackets() {
        let result = try_parse_bracketed_span("[[nested]]{.class}");
        assert!(result.is_some());
        let (len, content, attrs) = result.unwrap();
        assert_eq!(len, 18);
        assert_eq!(content, "[nested]");
        assert_eq!(attrs, ".class");
    }

    #[test]
    fn requires_attributes() {
        // Without attributes, should not parse
        let result = try_parse_bracketed_span("[text]");
        assert!(result.is_none());
    }

    #[test]
    fn requires_immediate_attributes() {
        // Space between ] and { should not parse
        let result = try_parse_bracketed_span("[text] {.class}");
        assert!(result.is_none());
    }

    #[test]
    fn handles_escaped_brackets() {
        let result = try_parse_bracketed_span(r"[text \] more]{.class}");
        assert!(result.is_some());
        let (len, content, _) = result.unwrap();
        assert_eq!(len, 22);
        assert_eq!(content, r"text \] more");
    }

    #[test]
    fn handles_escaped_braces_in_attributes() {
        let result = try_parse_bracketed_span(r"[text]{key=\}}");
        assert!(result.is_some());
        let (len, _, attrs) = result.unwrap();
        assert_eq!(len, 14);
        assert_eq!(attrs, r"key=\}");
    }
}
