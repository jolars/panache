//! Parsing for strikeout (~~strikethrough~~)
//!
//! This is a Pandoc extension that's also part of GitHub Flavored Markdown.
//! Syntax: ~~text~~ produces strikethrough text.
//!
//! Rules:
//! - Must have exactly 2 tildes on each side
//! - Content cannot be empty
//! - Tildes cannot have whitespace immediately inside

use crate::config::Config;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

/// Try to parse strikeout (~~text~~)
/// Returns: (total_len, inner_content)
pub fn try_parse_strikeout(text: &str) -> Option<(usize, &str)> {
    let bytes = text.as_bytes();

    // Must start with ~~
    if bytes.len() < 4 || bytes[0] != b'~' || bytes[1] != b'~' {
        return None;
    }

    // Check that it's not more than 2 tildes at the start (~~~ would be a code fence)
    if bytes.get(2) == Some(&b'~') {
        return None;
    }

    // Find the closing ~~
    let mut pos = 2;
    let mut found_close = false;

    while pos + 1 < bytes.len() {
        if bytes[pos] == b'~' && bytes[pos + 1] == b'~' {
            // Check that there's no third tilde (to avoid ~~text~~~)
            if pos + 2 < bytes.len() && bytes[pos + 2] == b'~' {
                pos += 1;
                continue;
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
    let content = &text[2..pos];

    // Content cannot be empty or only whitespace
    if content.trim().is_empty() {
        return None;
    }

    // Content cannot start or end with whitespace (CommonMark-style rule)
    if content.starts_with(char::is_whitespace) || content.ends_with(char::is_whitespace) {
        return None;
    }

    let total_len = pos + 2; // Include closing ~~
    Some((total_len, content))
}

/// Emit a strikeout node with its content
pub fn emit_strikeout(builder: &mut GreenNodeBuilder, inner_text: &str, config: &Config) {
    builder.start_node(SyntaxKind::STRIKEOUT.into());

    // Opening marker
    builder.start_node(SyntaxKind::STRIKEOUT_MARKER.into());
    builder.token(SyntaxKind::STRIKEOUT_MARKER.into(), "~~");
    builder.finish_node();

    // Parse inner content recursively for nested inline elements
    super::parse_inline_text(builder, inner_text, config, false);

    // Closing marker
    builder.start_node(SyntaxKind::STRIKEOUT_MARKER.into());
    builder.token(SyntaxKind::STRIKEOUT_MARKER.into(), "~~");
    builder.finish_node();

    builder.finish_node();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_strikeout() {
        assert_eq!(try_parse_strikeout("~~hello~~"), Some((9, "hello")));
    }

    #[test]
    fn test_strikeout_with_spaces() {
        assert_eq!(
            try_parse_strikeout("~~hello world~~"),
            Some((15, "hello world"))
        );
    }

    #[test]
    fn test_no_whitespace_inside_delimiters() {
        // Content cannot start with whitespace
        assert_eq!(try_parse_strikeout("~~ hello~~"), None);

        // Content cannot end with whitespace
        assert_eq!(try_parse_strikeout("~~hello ~~"), None);
    }

    #[test]
    fn test_empty_content() {
        assert_eq!(try_parse_strikeout("~~~~"), None);
        assert_eq!(try_parse_strikeout("~~ ~~"), None);
    }

    #[test]
    fn test_not_enough_tildes() {
        assert_eq!(try_parse_strikeout("~hello~"), None);
    }

    #[test]
    fn test_too_many_tildes() {
        // Three tildes would be code fence
        assert_eq!(try_parse_strikeout("~~~hello~~~"), None);
    }

    #[test]
    fn test_no_closing() {
        assert_eq!(try_parse_strikeout("~~hello"), None);
        assert_eq!(try_parse_strikeout("~~hello world"), None);
    }

    #[test]
    fn test_strikeout_with_other_content_after() {
        assert_eq!(try_parse_strikeout("~~hello~~ world"), Some((9, "hello")));
    }

    #[test]
    fn test_strikeout_in_middle() {
        assert_eq!(try_parse_strikeout("~~text~~"), Some((8, "text")));
    }
}
