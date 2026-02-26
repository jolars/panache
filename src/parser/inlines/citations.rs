//! Citation parsing for Pandoc's citations extension.
//!
//! Syntax:
//! - Bracketed: `[@doe99]`, `[@doe99; @smith2000]`
//! - With locator: `[see @doe99, pp. 33-35]`
//! - Suppress author: `[-@doe99]`
//! - Author-in-text: `@doe99` (bare, without brackets)

use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

/// Try to parse a bracketed citation starting at the current position.
/// Returns Some((length, content)) if successful, None otherwise.
///
/// Bracketed citations have the syntax: [@key], [@key1; @key2], [see @key, pp. 1-10]
pub(crate) fn try_parse_bracketed_citation(text: &str) -> Option<(usize, &str)> {
    let bytes = text.as_bytes();

    // Must start with [
    if bytes.is_empty() || bytes[0] != b'[' {
        return None;
    }

    // Look ahead to see if this contains a citation marker (@)
    // We need to distinguish from regular links
    let mut has_citation = false;
    let mut pos = 1;
    let mut bracket_depth = 0;

    while pos < bytes.len() {
        match bytes[pos] {
            b'\\' => {
                // Skip escaped character
                pos += 2;
                continue;
            }
            b'[' => {
                bracket_depth += 1;
                pos += 1;
            }
            b']' => {
                if bracket_depth == 0 {
                    // Closing bracket of main citation - stop looking
                    break;
                }
                bracket_depth -= 1;
                pos += 1;
            }
            b'@' => {
                // Found a citation marker - this is likely a citation
                has_citation = true;
                break;
            }
            b'(' if bracket_depth == 0 => {
                // Opening paren at top level suggests this might be a link [text](url)
                // Not a citation
                break;
            }
            _ => {
                pos += 1;
            }
        }
    }

    if !has_citation {
        return None;
    }

    // Now find the closing bracket
    pos = 1;
    bracket_depth = 1;

    while pos < bytes.len() {
        match bytes[pos] {
            b'\\' => {
                // Skip escaped character
                pos += 2;
                continue;
            }
            b'[' => {
                bracket_depth += 1;
                pos += 1;
            }
            b']' => {
                bracket_depth -= 1;
                if bracket_depth == 0 {
                    // Found the closing bracket
                    let content = &text[1..pos];
                    return Some((pos + 1, content));
                }
                pos += 1;
            }
            _ => {
                pos += 1;
            }
        }
    }

    // No closing bracket found
    None
}

/// Try to parse a bare citation (author-in-text) starting at the current position.
/// Returns Some((length, key, has_suppress)) if successful, None otherwise.
///
/// Bare citations have the syntax: @key or -@key
pub(crate) fn try_parse_bare_citation(text: &str) -> Option<(usize, &str, bool)> {
    let bytes = text.as_bytes();

    if bytes.is_empty() {
        return None;
    }

    let mut pos = 0;
    let has_suppress = bytes[pos] == b'-';

    if has_suppress {
        pos += 1;
        if pos >= bytes.len() {
            return None;
        }
    }

    // Must have @ next
    if bytes[pos] != b'@' {
        return None;
    }
    pos += 1;

    if pos >= bytes.len() {
        return None;
    }

    // Parse the citation key
    let key_start = pos;
    let key_len = parse_citation_key(&text[pos..])?;

    if key_len == 0 {
        return None;
    }

    let total_len = pos + key_len;
    let key = &text[key_start..total_len];

    Some((total_len, key, has_suppress))
}

/// Parse a citation key following Pandoc's rules.
/// Returns the length of the key, or None if invalid.
///
/// Citation keys:
/// - Must start with letter, digit, or _
/// - Can contain alphanumerics and single internal punctuation: :.#$%&-+?<>~/
/// - Keys in braces @{...} can contain anything
/// - Double internal punctuation terminates key
/// - Trailing punctuation not included
fn parse_citation_key(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();

    if bytes.is_empty() {
        return None;
    }

    // Check for braced key: @{...}
    if bytes[0] == b'{' {
        // Find matching closing brace
        let mut pos = 1;
        let mut escape_next = false;

        while pos < bytes.len() {
            if escape_next {
                escape_next = false;
                pos += 1;
                continue;
            }

            match bytes[pos] {
                b'\\' => {
                    escape_next = true;
                    pos += 1;
                }
                b'}' => {
                    // Include the closing brace
                    return Some(pos + 1);
                }
                _ => {
                    pos += 1;
                }
            }
        }

        // No closing brace found
        return None;
    }

    // Regular key: must start with letter, digit, or _
    let first_char = bytes[0] as char;
    if !first_char.is_alphanumeric() && first_char != '_' {
        return None;
    }

    let mut pos = 1;
    let mut prev_was_punct = false;

    while pos < bytes.len() {
        let ch = bytes[pos] as char;

        if ch.is_alphanumeric() || ch == '_' {
            prev_was_punct = false;
            pos += 1;
        } else if is_internal_punctuation(ch) {
            // Check if previous was also punctuation (double punct terminates)
            if prev_was_punct {
                // Double punctuation - terminate key before this character
                return Some(pos - 1);
            }
            prev_was_punct = true;
            pos += 1;
        } else {
            // Not a valid key character - terminate here
            break;
        }
    }

    // Remove trailing punctuation
    while pos > 0 && is_internal_punctuation(bytes[pos - 1] as char) {
        pos -= 1;
    }

    if pos == 0 { None } else { Some(pos) }
}

/// Check if a character is valid internal punctuation in citation keys.
fn is_internal_punctuation(ch: char) -> bool {
    matches!(
        ch,
        ':' | '.' | '#' | '$' | '%' | '&' | '-' | '+' | '?' | '<' | '>' | '~' | '/'
    )
}

/// Emit a bracketed citation node to the builder.
pub(crate) fn emit_bracketed_citation(builder: &mut GreenNodeBuilder, content: &str) {
    builder.start_node(SyntaxKind::CITATION.into());

    // Opening bracket
    builder.token(SyntaxKind::LINK_START.into(), "[");

    // The content contains the citation(s) and any prefix/suffix text
    // We emit it as raw text for now - the formatter can handle the structure
    builder.token(SyntaxKind::CITATION_CONTENT.into(), content);

    // Closing bracket
    builder.token(SyntaxKind::LINK_DEST.into(), "]");

    builder.finish_node();
}

/// Emit a bare citation node to the builder.
pub(crate) fn emit_bare_citation(builder: &mut GreenNodeBuilder, key: &str, has_suppress: bool) {
    builder.start_node(SyntaxKind::CITATION.into());

    // Emit marker (@ or -@)
    if has_suppress {
        builder.token(SyntaxKind::CITATION_MARKER.into(), "-@");
    } else {
        builder.token(SyntaxKind::CITATION_MARKER.into(), "@");
    }

    // Check if key is braced
    if key.starts_with('{') && key.ends_with('}') {
        builder.token(SyntaxKind::CITATION_BRACE_OPEN.into(), "{");
        builder.token(SyntaxKind::CITATION_KEY.into(), &key[1..key.len() - 1]);
        builder.token(SyntaxKind::CITATION_BRACE_CLOSE.into(), "}");
    } else {
        builder.token(SyntaxKind::CITATION_KEY.into(), key);
    }

    builder.finish_node();
}

#[cfg(test)]
mod tests {
    use super::*;

    // Citation key parsing tests
    #[test]
    fn test_parse_simple_citation_key() {
        assert_eq!(parse_citation_key("doe99"), Some(5));
        assert_eq!(parse_citation_key("smith2000"), Some(9));
    }

    #[test]
    fn test_parse_citation_key_with_internal_punct() {
        assert_eq!(parse_citation_key("Foo_bar.baz"), Some(11));
        assert_eq!(parse_citation_key("author:2020"), Some(11));
    }

    #[test]
    fn test_parse_citation_key_trailing_punct() {
        // Trailing punctuation should be excluded
        assert_eq!(parse_citation_key("Foo_bar.baz."), Some(11));
        assert_eq!(parse_citation_key("key:value:"), Some(9));
    }

    #[test]
    fn test_parse_citation_key_double_punct() {
        // Double punctuation terminates key
        assert_eq!(parse_citation_key("Foo_bar--baz"), Some(7)); // key is "Foo_bar"
    }

    #[test]
    fn test_parse_citation_key_with_braces() {
        assert_eq!(parse_citation_key("{https://example.com}"), Some(21));
        assert_eq!(parse_citation_key("{Foo_bar.baz.}"), Some(14));
    }

    #[test]
    fn test_parse_citation_key_invalid_start() {
        assert_eq!(parse_citation_key(".invalid"), None);
        assert_eq!(parse_citation_key(":invalid"), None);
    }

    #[test]
    fn test_parse_citation_key_stops_at_space() {
        assert_eq!(parse_citation_key("key rest"), Some(3));
    }

    // Bare citation parsing tests
    #[test]
    fn test_parse_bare_citation_simple() {
        let result = try_parse_bare_citation("@doe99");
        assert_eq!(result, Some((6, "doe99", false)));
    }

    #[test]
    fn test_parse_bare_citation_with_suppress() {
        let result = try_parse_bare_citation("-@smith04");
        assert_eq!(result, Some((9, "smith04", true)));
    }

    #[test]
    fn test_parse_bare_citation_with_trailing_text() {
        let result = try_parse_bare_citation("@doe99 says");
        assert_eq!(result, Some((6, "doe99", false)));
    }

    #[test]
    fn test_parse_bare_citation_braced_key() {
        let result = try_parse_bare_citation("@{https://example.com}");
        assert_eq!(result, Some((22, "{https://example.com}", false)));
    }

    #[test]
    fn test_parse_bare_citation_not_citation() {
        assert_eq!(try_parse_bare_citation("not a citation"), None);
        assert_eq!(try_parse_bare_citation("@"), None);
    }

    // Bracketed citation parsing tests
    #[test]
    fn test_parse_bracketed_citation_simple() {
        let result = try_parse_bracketed_citation("[@doe99]");
        assert_eq!(result, Some((8, "@doe99")));
    }

    #[test]
    fn test_parse_bracketed_citation_multiple() {
        let result = try_parse_bracketed_citation("[@doe99; @smith2000]");
        assert_eq!(result, Some((20, "@doe99; @smith2000")));
    }

    #[test]
    fn test_parse_bracketed_citation_with_prefix() {
        let result = try_parse_bracketed_citation("[see @doe99]");
        assert_eq!(result, Some((12, "see @doe99")));
    }

    #[test]
    fn test_parse_bracketed_citation_with_locator() {
        let result = try_parse_bracketed_citation("[@doe99, pp. 33-35]");
        assert_eq!(result, Some((19, "@doe99, pp. 33-35")));
    }

    #[test]
    fn test_parse_bracketed_citation_complex() {
        let result = try_parse_bracketed_citation("[see @doe99, pp. 33-35 and *passim*]");
        assert_eq!(result, Some((36, "see @doe99, pp. 33-35 and *passim*")));
    }

    #[test]
    fn test_parse_bracketed_citation_with_suppress() {
        let result = try_parse_bracketed_citation("[-@doe99]");
        assert_eq!(result, Some((9, "-@doe99")));
    }

    #[test]
    fn test_parse_bracketed_citation_not_citation() {
        // Regular link should not be parsed as citation
        assert_eq!(try_parse_bracketed_citation("[text](url)"), None);
        assert_eq!(try_parse_bracketed_citation("[just text]"), None);
    }

    #[test]
    fn test_parse_bracketed_citation_nested_brackets() {
        let result = try_parse_bracketed_citation("[see [nested] @doe99]");
        assert_eq!(result, Some((21, "see [nested] @doe99")));
    }

    #[test]
    fn test_parse_bracketed_citation_escaped_bracket() {
        let result = try_parse_bracketed_citation(r"[@doe99 with \] escaped]");
        assert_eq!(result, Some((24, r"@doe99 with \] escaped")));
    }
}
