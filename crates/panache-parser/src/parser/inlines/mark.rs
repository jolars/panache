//! Parsing for mark/highlight (==text==).
//!
//! This is a Pandoc non-default extension.
//! Syntax: ==text== produces highlighted text.
//!
//! Rules (Pandoc parity):
//! - Must start with exactly two `=` delimiters
//! - Content cannot be empty or all whitespace
//! - Content cannot start or end with whitespace
//! - Closers are matched greedily at the first valid `==`

use super::core::parse_inline_text;
use crate::options::ParserOptions;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

/// Try to parse mark/highlight (==text==).
/// Returns: (total_len, inner_content)
pub fn try_parse_mark(text: &str) -> Option<(usize, &str)> {
    let bytes = text.as_bytes();

    // Must start with ==
    if bytes.len() < 4 || bytes[0] != b'=' || bytes[1] != b'=' {
        return None;
    }

    // Find the closing ==
    let mut pos = 2;
    let mut found_close = false;
    while pos + 1 < bytes.len() {
        if bytes[pos] == b'=' && bytes[pos + 1] == b'=' {
            found_close = true;
            break;
        }
        pos += 1;
    }

    if !found_close {
        return None;
    }

    let content = &text[2..pos];

    // Content cannot be empty or only whitespace
    if content.trim().is_empty() {
        return None;
    }

    // Pandoc parity: no whitespace immediately inside delimiters.
    if content.starts_with(char::is_whitespace) || content.ends_with(char::is_whitespace) {
        return None;
    }

    let total_len = pos + 2; // include closing ==
    Some((total_len, content))
}

/// Emit a mark node with its content.
pub fn emit_mark(builder: &mut GreenNodeBuilder, inner_text: &str, config: &ParserOptions) {
    builder.start_node(SyntaxKind::MARK.into());

    builder.start_node(SyntaxKind::MARK_MARKER.into());
    builder.token(SyntaxKind::MARK_MARKER.into(), "==");
    builder.finish_node();

    parse_inline_text(builder, inner_text, config, false);

    builder.start_node(SyntaxKind::MARK_MARKER.into());
    builder.token(SyntaxKind::MARK_MARKER.into(), "==");
    builder.finish_node();

    builder.finish_node();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_mark() {
        assert_eq!(try_parse_mark("==hi=="), Some((6, "hi")));
    }

    #[test]
    fn test_mark_with_spaces_inside_content() {
        assert_eq!(try_parse_mark("==hello world=="), Some((15, "hello world")));
    }

    #[test]
    fn test_mark_requires_non_whitespace_content() {
        assert_eq!(try_parse_mark("===="), None);
        assert_eq!(try_parse_mark("==  =="), None);
    }

    #[test]
    fn test_mark_disallows_whitespace_just_inside_delimiters() {
        assert_eq!(try_parse_mark("== hi=="), None);
        assert_eq!(try_parse_mark("==hi =="), None);
        assert_eq!(try_parse_mark("== hi =="), None);
    }

    #[test]
    fn test_mark_allows_neighboring_extra_equals_like_pandoc() {
        assert_eq!(try_parse_mark("===a==="), Some((6, "=a")));
        assert_eq!(try_parse_mark("==a==="), Some((5, "a")));
        assert_eq!(try_parse_mark("====a=="), None);
        assert_eq!(try_parse_mark("==a===="), Some((5, "a")));
    }
}
