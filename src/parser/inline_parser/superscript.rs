//! Parsing for superscript (^text^)
//!
//! This is a Pandoc extension.
//! Syntax: ^text^ produces superscript text.
//!
//! Rules:
//! - Must have exactly 1 caret on each side
//! - Content cannot be empty
//! - Carets cannot have whitespace immediately inside
//! - Must not be confused with ^[...] (inline footnotes)

use crate::config::Config;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

/// Try to parse superscript (^text^)
/// Returns: (total_len, inner_content)
pub fn try_parse_superscript(text: &str) -> Option<(usize, &str)> {
    let bytes = text.as_bytes();

    // Must start with ^
    if bytes.is_empty() || bytes[0] != b'^' {
        return None;
    }

    // Check that it's not ^[ (inline footnote)
    if bytes.len() > 1 && bytes[1] == b'[' {
        return None;
    }

    // Content cannot start with whitespace
    if bytes.len() > 1 && bytes[1].is_ascii_whitespace() {
        return None;
    }

    // Find the closing ^
    let mut pos = 1;
    let mut found_close = false;

    while pos < bytes.len() {
        if bytes[pos] == b'^' {
            found_close = true;
            break;
        }
        pos += 1;
    }

    if !found_close {
        return None;
    }

    // Extract content between the delimiters
    let content = &text[1..pos];

    // Content cannot be empty or only whitespace
    if content.trim().is_empty() {
        return None;
    }

    // Content cannot end with whitespace
    if content.ends_with(char::is_whitespace) {
        return None;
    }

    let total_len = pos + 1; // Include closing ^
    Some((total_len, content))
}

/// Emit a superscript node with its content
pub fn emit_superscript(builder: &mut GreenNodeBuilder, inner_text: &str, config: &Config) {
    builder.start_node(SyntaxKind::SUPERSCRIPT.into());

    // Opening marker
    builder.start_node(SyntaxKind::SUPERSCRIPT_MARKER.into());
    builder.token(SyntaxKind::SUPERSCRIPT_MARKER.into(), "^");
    builder.finish_node();

    // Parse inner content recursively for nested inline elements
    super::parse_inline_text(builder, inner_text, config, None);

    // Closing marker
    builder.start_node(SyntaxKind::SUPERSCRIPT_MARKER.into());
    builder.token(SyntaxKind::SUPERSCRIPT_MARKER.into(), "^");
    builder.finish_node();

    builder.finish_node();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_superscript() {
        assert_eq!(try_parse_superscript("^2^"), Some((3, "2")));
        assert_eq!(try_parse_superscript("^nd^"), Some((4, "nd")));
    }

    #[test]
    fn test_superscript_with_multiple_chars() {
        assert_eq!(try_parse_superscript("^(tm)^"), Some((6, "(tm)")));
        assert_eq!(try_parse_superscript("^text^"), Some((6, "text")));
    }

    #[test]
    fn test_no_whitespace_inside_delimiters() {
        // Content cannot start with whitespace
        assert_eq!(try_parse_superscript("^ text^"), None);

        // Content cannot end with whitespace
        assert_eq!(try_parse_superscript("^text ^"), None);
    }

    #[test]
    fn test_empty_content() {
        assert_eq!(try_parse_superscript("^^"), None);
        assert_eq!(try_parse_superscript("^ ^"), None);
    }

    #[test]
    fn test_no_closing() {
        assert_eq!(try_parse_superscript("^text"), None);
        assert_eq!(try_parse_superscript("^hello world"), None);
    }

    #[test]
    fn test_not_confused_with_inline_footnote() {
        // ^[ should not be parsed as superscript
        assert_eq!(try_parse_superscript("^[footnote]"), None);
    }

    #[test]
    fn test_superscript_with_other_content_after() {
        assert_eq!(try_parse_superscript("^2^ text"), Some((3, "2")));
        assert_eq!(try_parse_superscript("^nd^ of the month"), Some((4, "nd")));
    }

    #[test]
    fn test_spaces_inside_are_ok() {
        assert_eq!(
            try_parse_superscript("^some text^"),
            Some((11, "some text"))
        );
    }

    #[test]
    fn test_single_char() {
        assert_eq!(try_parse_superscript("^a^"), Some((3, "a")));
    }
}
