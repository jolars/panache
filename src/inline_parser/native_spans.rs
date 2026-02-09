//! Native span parsing for Pandoc's `native_spans` extension.
//!
//! Syntax: `<span class="foo">content</span>`
//!
//! When the `native_spans` extension is enabled, HTML `<span>` tags are
//! treated as native Pandoc Span elements instead of raw HTML.

use crate::config::Config;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use super::parse_inline_text;

/// Try to parse a native HTML span starting at the current position.
/// Returns Some((length, content, attributes)) if successful.
///
/// Native spans have the form: <span attrs...>content</span>
/// The content can contain markdown that will be parsed recursively.
pub(crate) fn try_parse_native_span(text: &str) -> Option<(usize, &str, String)> {
    let bytes = text.as_bytes();

    // Must start with <span
    if !text.starts_with("<span") {
        return None;
    }

    let mut pos = 5; // After "<span"

    // Next char must be space, >, or end of tag
    if pos >= text.len() {
        return None;
    }

    let next_char = bytes[pos] as char;
    if !matches!(next_char, ' ' | '\t' | '\n' | '\r' | '>') {
        // Could be <spanx> or something else, not a span tag
        return None;
    }

    // Parse attributes until we find >
    let attr_start = pos;
    while pos < text.len() && bytes[pos] != b'>' {
        // Handle quoted attributes
        if bytes[pos] == b'"' || bytes[pos] == b'\'' {
            let quote = bytes[pos];
            pos += 1;
            // Skip until closing quote
            while pos < text.len() && bytes[pos] != quote {
                if bytes[pos] == b'\\' {
                    pos += 2; // Skip escaped character
                } else {
                    pos += 1;
                }
            }
            if pos < text.len() {
                pos += 1; // Skip closing quote
            }
        } else {
            pos += 1;
        }
    }

    if pos >= text.len() {
        // No closing > found
        return None;
    }

    // Extract attributes
    let attributes = text[attr_start..pos].trim().to_string();

    // Skip the >
    pos += 1;

    // Now find the closing </span>
    let content_start = pos;
    let mut depth = 1;

    while pos < text.len() && depth > 0 {
        // Check for nested <span>
        if text[pos..].starts_with("<span") {
            // Make sure it's actually a span tag (space or > follows)
            let check_pos = pos + 5;
            if check_pos < text.len() {
                let ch = bytes[check_pos] as char;
                if matches!(ch, ' ' | '\t' | '\n' | '\r' | '>') {
                    depth += 1;
                    pos += 5;
                    continue;
                }
            }
        }

        // Check for closing </span>
        if text[pos..].starts_with("</span>") {
            depth -= 1;
            if depth == 0 {
                // Found the matching closing tag
                let content = &text[content_start..pos];
                let total_len = pos + 7; // Include </span>
                return Some((total_len, content, attributes));
            }
            pos += 7;
            continue;
        }

        pos += 1;
    }

    // No matching closing tag found
    None
}

/// Parse HTML attributes and convert to a format suitable for storage.
/// For now, we just store the raw HTML attributes as a string.
///
/// HTML attributes like: class="foo bar" id="baz" data-x="value"
/// Could be converted to Pandoc format: #baz .foo .bar data-x="value"
/// But for simplicity, we'll keep them as HTML attributes for now.
#[allow(dead_code)] // TODO: Use for attribute conversion
pub(crate) fn parse_span_attributes(html_attrs: &str) -> String {
    // For now, just return the HTML attributes as-is
    // Future enhancement: parse and convert to Pandoc attribute format
    html_attrs.to_string()
}

/// Emit a native span node to the builder.
pub(crate) fn emit_native_span(
    builder: &mut GreenNodeBuilder,
    content: &str,
    attributes: &str,
    config: &Config,
) {
    builder.start_node(SyntaxKind::BracketedSpan.into());

    // Opening tag
    builder.token(SyntaxKind::SpanBracketOpen.into(), "<span");
    if !attributes.is_empty() {
        // Add space before attributes
        builder.token(SyntaxKind::WHITESPACE.into(), " ");
        builder.token(SyntaxKind::SpanAttributes.into(), attributes);
    }
    builder.token(SyntaxKind::SpanBracketOpen.into(), ">");

    // Parse the content recursively for inline markdown
    builder.start_node(SyntaxKind::SpanContent.into());
    parse_inline_text(builder, content, config, None);
    builder.finish_node();

    // Closing tag
    builder.token(SyntaxKind::SpanBracketClose.into(), "</span>");

    builder.finish_node();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_span() {
        let result = try_parse_native_span("<span>text</span>");
        assert_eq!(result, Some((17, "text", String::new())));
    }

    #[test]
    fn test_parse_span_with_class() {
        let result = try_parse_native_span(r#"<span class="foo">text</span>"#);
        assert_eq!(result, Some((29, "text", r#"class="foo""#.to_string())));
    }

    #[test]
    fn test_parse_span_with_id() {
        let result = try_parse_native_span(r#"<span id="bar">text</span>"#);
        assert_eq!(result, Some((26, "text", r#"id="bar""#.to_string())));
    }

    #[test]
    fn test_parse_span_with_multiple_attrs() {
        let result = try_parse_native_span(r#"<span id="x" class="y z">text</span>"#);
        assert_eq!(
            result,
            Some((36, "text", r#"id="x" class="y z""#.to_string()))
        );
    }

    #[test]
    fn test_parse_span_with_markdown() {
        let result = try_parse_native_span("<span>*emphasis* and `code`</span>");
        assert_eq!(result, Some((34, "*emphasis* and `code`", String::new())));
    }

    #[test]
    fn test_parse_nested_spans() {
        let result = try_parse_native_span("<span>outer <span>inner</span> text</span>");
        assert_eq!(
            result,
            Some((42, "outer <span>inner</span> text", String::new()))
        );
    }

    #[test]
    fn test_parse_span_with_newlines_in_content() {
        let result = try_parse_native_span("<span>line 1\nline 2</span>");
        assert_eq!(result, Some((26, "line 1\nline 2", String::new())));
    }

    #[test]
    fn test_not_span_no_closing_tag() {
        let result = try_parse_native_span("<span>text");
        assert_eq!(result, None);
    }

    #[test]
    fn test_not_span_wrong_tag() {
        let result = try_parse_native_span("<spanx>text</spanx>");
        assert_eq!(result, None);
    }

    #[test]
    fn test_not_span_no_space_after() {
        // <spanner> should not be parsed as <span>
        let result = try_parse_native_span("<spanner>text</spanner>");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_span_with_quoted_attrs_containing_gt() {
        let result = try_parse_native_span(r#"<span title="a > b">text</span>"#);
        assert_eq!(result, Some((31, "text", r#"title="a > b""#.to_string())));
    }

    #[test]
    fn test_parse_empty_span() {
        let result = try_parse_native_span("<span></span>");
        assert_eq!(result, Some((13, "", String::new())));
    }

    #[test]
    fn test_parse_span_trailing_text() {
        let result = try_parse_native_span("<span>text</span> more");
        assert_eq!(result, Some((17, "text", String::new())));
    }
}
