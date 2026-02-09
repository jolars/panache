//! Inline footnote parsing for Pandoc's inline_notes extension.
//!
//! Syntax: `^[footnote text]`

use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use super::parse_inline_text;
use crate::config::Config;

/// Try to parse an inline footnote starting at the current position.
/// Returns Some((length, content)) if successful, None otherwise.
///
/// Inline footnotes have the syntax: ^[text]
/// The text can contain most inline elements but not nested footnotes.
pub(crate) fn try_parse_inline_footnote(text: &str) -> Option<(usize, &str)> {
    let bytes = text.as_bytes();

    // Must start with ^[
    if bytes.len() < 3 || bytes[0] != b'^' || bytes[1] != b'[' {
        return None;
    }

    // Find the closing ]
    let mut pos = 2;
    let mut bracket_depth = 1; // Already opened one bracket

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
                    let content = &text[2..pos];
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

/// Emit an inline footnote node to the builder.
pub(crate) fn emit_inline_footnote(builder: &mut GreenNodeBuilder, content: &str, config: &Config) {
    builder.start_node(SyntaxKind::InlineFootnote.into());

    // Opening marker
    builder.token(SyntaxKind::InlineFootnoteStart.into(), "^[");

    // Parse the content recursively for nested inline elements
    parse_inline_text(builder, content, config, None);

    // Closing marker
    builder.token(SyntaxKind::InlineFootnoteEnd.into(), "]");

    builder.finish_node();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_inline_footnote() {
        let result = try_parse_inline_footnote("^[This is a note]");
        assert_eq!(result, Some((17, "This is a note")));
    }

    #[test]
    fn test_parse_inline_footnote_with_trailing_text() {
        let result = try_parse_inline_footnote("^[Note text] and more");
        assert_eq!(result, Some((12, "Note text")));
    }

    #[test]
    fn test_parse_inline_footnote_with_brackets_inside() {
        let result = try_parse_inline_footnote("^[Text with [nested] brackets]");
        assert_eq!(result, Some((30, "Text with [nested] brackets")));
    }

    #[test]
    fn test_parse_inline_footnote_with_escaped_bracket() {
        let result = try_parse_inline_footnote("^[Text with \\] escaped]");
        assert_eq!(result, Some((23, "Text with \\] escaped")));
    }

    #[test]
    fn test_not_inline_footnote_no_opening() {
        let result = try_parse_inline_footnote("[Not a footnote]");
        assert_eq!(result, None);
    }

    #[test]
    fn test_not_inline_footnote_no_closing() {
        let result = try_parse_inline_footnote("^[No closing bracket");
        assert_eq!(result, None);
    }

    #[test]
    fn test_not_inline_footnote_just_caret() {
        let result = try_parse_inline_footnote("^Not a footnote");
        assert_eq!(result, None);
    }

    #[test]
    fn test_empty_inline_footnote() {
        let result = try_parse_inline_footnote("^[]");
        assert_eq!(result, Some((3, "")));
    }

    #[test]
    fn test_inline_footnote_multiline() {
        // Inline footnotes can span multiple lines in the source
        let result = try_parse_inline_footnote("^[This is\na multiline\nnote]");
        assert_eq!(result, Some((27, "This is\na multiline\nnote")));
    }

    #[test]
    fn test_inline_footnote_with_code() {
        let result = try_parse_inline_footnote("^[Contains `code` inside]");
        assert_eq!(result, Some((25, "Contains `code` inside")));
    }
}
