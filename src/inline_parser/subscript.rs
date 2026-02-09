//! Parsing for subscript (~text~)
//!
//! This is a Pandoc extension.
//! Syntax: ~text~ produces subscript text.
//!
//! Rules:
//! - Must have exactly 1 tilde on each side
//! - Content cannot be empty
//! - Tildes cannot have whitespace immediately inside
//! - Must not be confused with ~~ (strikeout)

use crate::config::Config;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

/// Try to parse subscript (~text~)
/// Returns: (total_len, inner_content)
pub fn try_parse_subscript(text: &str) -> Option<(usize, &str)> {
    let bytes = text.as_bytes();

    // Must start with ~
    if bytes.is_empty() || bytes[0] != b'~' {
        return None;
    }

    // Check that it's not ~~ (strikeout)
    if bytes.len() > 1 && bytes[1] == b'~' {
        return None;
    }

    // Content cannot start with whitespace
    if bytes.len() > 1 && bytes[1].is_ascii_whitespace() {
        return None;
    }

    // Find the closing ~
    let mut pos = 1;
    let mut found_close = false;

    while pos < bytes.len() {
        if bytes[pos] == b'~' {
            // Make sure it's not part of ~~
            if pos + 1 < bytes.len() && bytes[pos + 1] == b'~' {
                return None;
            }
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

    let total_len = pos + 1; // Include closing ~
    Some((total_len, content))
}

/// Emit a subscript node with its content
pub fn emit_subscript(builder: &mut GreenNodeBuilder, inner_text: &str, config: &Config) {
    builder.start_node(SyntaxKind::Subscript.into());

    // Opening marker
    builder.start_node(SyntaxKind::SubscriptMarker.into());
    builder.token(SyntaxKind::SubscriptMarker.into(), "~");
    builder.finish_node();

    // Parse inner content recursively for nested inline elements
    super::parse_inline_text(builder, inner_text, config);

    // Closing marker
    builder.start_node(SyntaxKind::SubscriptMarker.into());
    builder.token(SyntaxKind::SubscriptMarker.into(), "~");
    builder.finish_node();

    builder.finish_node();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_subscript() {
        assert_eq!(try_parse_subscript("~2~"), Some((3, "2")));
        assert_eq!(try_parse_subscript("~n~"), Some((3, "n")));
    }

    #[test]
    fn test_subscript_with_multiple_chars() {
        assert_eq!(try_parse_subscript("~text~"), Some((6, "text")));
        assert_eq!(try_parse_subscript("~i+1~"), Some((5, "i+1")));
    }

    #[test]
    fn test_no_whitespace_inside_delimiters() {
        // Content cannot start with whitespace
        assert_eq!(try_parse_subscript("~ text~"), None);

        // Content cannot end with whitespace
        assert_eq!(try_parse_subscript("~text ~"), None);
    }

    #[test]
    fn test_empty_content() {
        assert_eq!(try_parse_subscript("~~"), None);
        assert_eq!(try_parse_subscript("~ ~"), None);
    }

    #[test]
    fn test_no_closing() {
        assert_eq!(try_parse_subscript("~text"), None);
        assert_eq!(try_parse_subscript("~hello world"), None);
    }

    #[test]
    fn test_not_confused_with_strikeout() {
        // ~~ should not be parsed as subscript
        assert_eq!(try_parse_subscript("~~text~~"), None);
    }

    #[test]
    fn test_subscript_with_other_content_after() {
        assert_eq!(try_parse_subscript("~2~ text"), Some((3, "2")));
        assert_eq!(try_parse_subscript("~n~ of sequence"), Some((3, "n")));
    }

    #[test]
    fn test_spaces_inside_are_ok() {
        assert_eq!(try_parse_subscript("~some text~"), Some((11, "some text")));
    }

    #[test]
    fn test_single_char() {
        assert_eq!(try_parse_subscript("~a~"), Some((3, "a")));
    }

    #[test]
    fn test_subscript_before_strikeout_marker() {
        // If there's a subscript followed by another ~, it should work
        assert_eq!(try_parse_subscript("~x~ ~"), Some((3, "x")));
    }
}
