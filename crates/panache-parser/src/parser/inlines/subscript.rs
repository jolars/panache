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

use super::core::parse_inline_text;
use crate::options::ParserOptions;
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

    // Pandoc fallback: when strikeout (`~~text~~`) doesn't match, `~~` is
    // consumed as an empty Subscript (`Subscript []`), with the second `~`
    // closing the first. Probed against `pandoc -f markdown` for
    // `~~unclosed`, `*x ~~y*`, `a ~~b`, `~~ a ~~`. Dispatch order in
    // `inlines/core.rs` runs strikeout before subscript so a real
    // strikeout (`~~hello~~`) is not misinterpreted as two empty
    // subscripts.
    if bytes.len() > 1 && bytes[1] == b'~' {
        return Some((2, ""));
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

    // Pandoc rule: subscripted text cannot contain unescaped whitespace.
    // To include a space, source must escape it as `\ `. Verified against
    // `pandoc -f markdown` for `~x y~` → not a subscript, `~x\ y~` →
    // Subscript with NBSP-joined content.
    if contains_unescaped_whitespace(content) {
        return None;
    }

    let total_len = pos + 1; // Include closing ~
    Some((total_len, content))
}

fn contains_unescaped_whitespace(content: &str) -> bool {
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' && i + 1 < bytes.len() {
            i += 2;
            continue;
        }
        if (b as char).is_whitespace() {
            return true;
        }
        i += 1;
    }
    false
}

/// Emit a subscript node with its content
pub fn emit_subscript(builder: &mut GreenNodeBuilder, inner_text: &str, config: &ParserOptions) {
    builder.start_node(SyntaxKind::SUBSCRIPT.into());

    // Opening marker
    builder.start_node(SyntaxKind::SUBSCRIPT_MARKER.into());
    builder.token(SyntaxKind::SUBSCRIPT_MARKER.into(), "~");
    builder.finish_node();

    // Parse inner content recursively for nested inline elements
    parse_inline_text(builder, inner_text, config, false);

    // Closing marker
    builder.start_node(SyntaxKind::SUBSCRIPT_MARKER.into());
    builder.token(SyntaxKind::SUBSCRIPT_MARKER.into(), "~");
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
        // `~~` is consumed as an empty Subscript (pandoc strikeout-fallback);
        // a single space between tildes is still rejected as a degenerate
        // form (pandoc: `~ ~` → plain text).
        assert_eq!(try_parse_subscript("~~"), Some((2, "")));
        assert_eq!(try_parse_subscript("~ ~"), None);
    }

    #[test]
    fn test_no_closing() {
        assert_eq!(try_parse_subscript("~text"), None);
        assert_eq!(try_parse_subscript("~hello world"), None);
    }

    #[test]
    fn test_double_tilde_unclosed_is_empty_subscript() {
        // Pandoc strikeout-fallback: when `~~text~~` would otherwise match
        // strikeout, the dispatch order in `inlines/core.rs` ensures
        // strikeout fires first. When strikeout would not match (no closing
        // `~~`), `~~` is consumed as an empty Subscript, leaving the rest
        // of the input for downstream parsing. Probed against pandoc:
        // `~~unclosed` → `Subscript [] , Str "unclosed"`. The standalone
        // `try_parse_subscript("~~text~~")` now returns the empty form;
        // real strikeout matching is the dispatcher's responsibility.
        assert_eq!(try_parse_subscript("~~text~~"), Some((2, "")));
        assert_eq!(try_parse_subscript("~~unclosed"), Some((2, "")));
    }

    #[test]
    fn test_subscript_with_other_content_after() {
        assert_eq!(try_parse_subscript("~2~ text"), Some((3, "2")));
        assert_eq!(try_parse_subscript("~n~ of sequence"), Some((3, "n")));
    }

    #[test]
    fn test_internal_whitespace_rejected() {
        // Pandoc rejects unescaped internal whitespace in subscripts;
        // backslash-escaped spaces are accepted.
        assert_eq!(try_parse_subscript("~some text~"), None);
        assert_eq!(
            try_parse_subscript("~some\\ text~"),
            Some((12, "some\\ text"))
        );
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
