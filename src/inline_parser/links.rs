//! Parsing for links, images, and automatic links.
//!
//! Implements:
//! - Automatic links: `<http://example.com>` and `<user@example.com>`
//! - Inline links: `[text](url)` and `[text](url "title")`
//! - Inline images: `![alt](url)` and `![alt](url "title")`
//! - Image attributes: `![alt](url){#id .class key=value}`

use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

// Import attribute parsing
use crate::block_parser::attributes::{
    AttributeBlock, emit_attributes, try_parse_trailing_attributes,
};

/// Try to parse an inline image starting at the current position.
///
/// Inline images have the form `![alt](url)` or `![alt](url "title")`.
/// Can also have trailing attributes: `![alt](url){#id .class}`.
/// Returns Some((length, alt_text, dest_content, attributes)) if a valid image is found.
pub fn try_parse_inline_image(text: &str) -> Option<(usize, &str, &str, Option<AttributeBlock>)> {
    if !text.starts_with("![") {
        return None;
    }

    // Find the closing ]
    let mut bracket_depth = 0;
    let mut escape_next = false;
    let mut close_bracket_pos = None;

    for (i, ch) in text[2..].char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }

        match ch {
            '\\' => escape_next = true,
            '[' => bracket_depth += 1,
            ']' => {
                if bracket_depth == 0 {
                    close_bracket_pos = Some(i + 2);
                    break;
                }
                bracket_depth -= 1;
            }
            _ => {}
        }
    }

    let close_bracket = close_bracket_pos?;
    let alt_text = &text[2..close_bracket];

    // Check for immediate ( after ]
    let after_bracket = close_bracket + 1;
    if text.len() <= after_bracket || !text[after_bracket..].starts_with('(') {
        return None;
    }

    // Find closing ) for destination (reuse same logic as links)
    let dest_start = after_bracket + 1;
    let remaining = &text[dest_start..];

    let mut paren_depth = 0;
    let mut escape_next = false;
    let mut in_quotes = false;
    let mut close_paren_pos = None;

    for (i, ch) in remaining.char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }

        match ch {
            '\\' => escape_next = true,
            '"' => in_quotes = !in_quotes,
            '(' if !in_quotes => paren_depth += 1,
            ')' if !in_quotes => {
                if paren_depth == 0 {
                    close_paren_pos = Some(i);
                    break;
                }
                paren_depth -= 1;
            }
            _ => {}
        }
    }

    let close_paren = close_paren_pos?;
    let dest_content = &remaining[..close_paren];

    // Check for trailing attributes {#id .class key=value}
    let after_paren = dest_start + close_paren + 1;
    let after_close = &text[after_paren..];

    if let Some((attrs, _)) = try_parse_trailing_attributes(after_close) {
        // Attributes must start immediately after closing paren (no space)
        if after_close.starts_with('{') {
            // Calculate total length including attributes
            let attr_len = after_close.find('}').map(|i| i + 1).unwrap_or(0);
            let total_len = after_paren + attr_len;
            return Some((total_len, alt_text, dest_content, Some(attrs)));
        }
    }

    // No attributes, just return the image
    let total_len = after_paren;
    Some((total_len, alt_text, dest_content, None))
}

/// Emit an inline image node to the builder.
/// Note: alt_text may contain inline elements and should be parsed recursively.
pub fn emit_inline_image(
    builder: &mut GreenNodeBuilder,
    _text: &str,
    alt_text: &str,
    dest: &str,
    attributes: Option<AttributeBlock>,
) {
    builder.start_node(SyntaxKind::ImageLink.into());

    // Opening ![
    builder.start_node(SyntaxKind::ImageLinkStart.into());
    builder.token(SyntaxKind::ImageLinkStart.into(), "![");
    builder.finish_node();

    // Alt text (recursively parse inline elements)
    builder.start_node(SyntaxKind::ImageAlt.into());
    // Use the standalone parse_inline_text function for recursive parsing
    crate::inline_parser::parse_inline_text(builder, alt_text);
    builder.finish_node();

    // Closing ] and opening (
    builder.token(SyntaxKind::TEXT.into(), "](");

    // Destination
    builder.start_node(SyntaxKind::LinkDest.into());
    builder.token(SyntaxKind::TEXT.into(), dest);
    builder.finish_node();

    // Closing )
    builder.token(SyntaxKind::TEXT.into(), ")");

    // Emit attributes if present
    if let Some(attrs) = attributes {
        emit_attributes(builder, &attrs);
    }

    builder.finish_node();
}

/// Try to parse an automatic link starting at the current position.
///
/// Automatic links have the form `<url>` or `<email@example.com>`.
/// Returns Some((length, url_content)) if a valid automatic link is found.
pub fn try_parse_autolink(text: &str) -> Option<(usize, &str)> {
    if !text.starts_with('<') {
        return None;
    }

    // Find the closing >
    let close_pos = text[1..].find('>')?;
    let content = &text[1..1 + close_pos];

    // Automatic links cannot contain spaces or newlines
    if content.contains(|c: char| c.is_whitespace()) {
        return None;
    }

    // Must contain at least one character
    if content.is_empty() {
        return None;
    }

    // Basic validation: should look like a URL or email
    // URL: contains :// or starts with scheme:
    // Email: contains @
    let is_url = content.contains("://") || content.contains(':');
    let is_email = content.contains('@');

    if !is_url && !is_email {
        return None;
    }

    // Total length includes < and >
    Some((close_pos + 2, content))
}

/// Emit an automatic link node to the builder.
pub fn emit_autolink(builder: &mut GreenNodeBuilder, _text: &str, url: &str) {
    builder.start_node(SyntaxKind::AutoLink.into());

    // Opening <
    builder.start_node(SyntaxKind::AutoLinkMarker.into());
    builder.token(SyntaxKind::AutoLinkMarker.into(), "<");
    builder.finish_node();

    // URL content
    builder.token(SyntaxKind::TEXT.into(), url);

    // Closing >
    builder.start_node(SyntaxKind::AutoLinkMarker.into());
    builder.token(SyntaxKind::AutoLinkMarker.into(), ">");
    builder.finish_node();

    builder.finish_node();
}

/// Try to parse an inline link starting at the current position.
///
/// Inline links have the form `[text](url)` or `[text](url "title")`.
/// Returns Some((length, text_content, dest_content)) if a valid link is found.
pub fn try_parse_inline_link(text: &str) -> Option<(usize, &str, &str)> {
    if !text.starts_with('[') {
        return None;
    }

    // Find the closing ]
    let mut bracket_depth = 0;
    let mut escape_next = false;
    let mut close_bracket_pos = None;

    for (i, ch) in text[1..].char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }

        match ch {
            '\\' => escape_next = true,
            '[' => bracket_depth += 1,
            ']' => {
                if bracket_depth == 0 {
                    close_bracket_pos = Some(i + 1);
                    break;
                }
                bracket_depth -= 1;
            }
            _ => {}
        }
    }

    let close_bracket = close_bracket_pos?;
    let link_text = &text[1..close_bracket];

    // Check for immediate ( after ]
    let after_bracket = close_bracket + 1;
    if text.len() <= after_bracket || !text[after_bracket..].starts_with('(') {
        return None;
    }

    // Find closing ) for destination
    let dest_start = after_bracket + 1;
    let remaining = &text[dest_start..];

    let mut paren_depth = 0;
    let mut escape_next = false;
    let mut in_quotes = false;
    let mut close_paren_pos = None;

    for (i, ch) in remaining.char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }

        match ch {
            '\\' => escape_next = true,
            '"' => in_quotes = !in_quotes,
            '(' if !in_quotes => paren_depth += 1,
            ')' if !in_quotes => {
                if paren_depth == 0 {
                    close_paren_pos = Some(i);
                    break;
                }
                paren_depth -= 1;
            }
            _ => {}
        }
    }

    let close_paren = close_paren_pos?;
    let dest_content = &remaining[..close_paren];

    // Total length: [ + text + ] + ( + dest + )
    let total_len = dest_start + close_paren + 1;

    Some((total_len, link_text, dest_content))
}

/// Emit an inline link node to the builder.
/// Note: link_text may contain inline elements and should be parsed recursively.
pub fn emit_inline_link(builder: &mut GreenNodeBuilder, _text: &str, link_text: &str, dest: &str) {
    builder.start_node(SyntaxKind::Link.into());

    // Opening [
    builder.start_node(SyntaxKind::LinkStart.into());
    builder.token(SyntaxKind::LinkStart.into(), "[");
    builder.finish_node();

    // Link text (recursively parse inline elements)
    builder.start_node(SyntaxKind::LinkText.into());
    // Use the standalone parse_inline_text function for recursive parsing
    crate::inline_parser::parse_inline_text(builder, link_text);
    builder.finish_node();

    // Closing ] and opening (
    builder.token(SyntaxKind::TEXT.into(), "](");

    // Destination
    builder.start_node(SyntaxKind::LinkDest.into());
    builder.token(SyntaxKind::TEXT.into(), dest);
    builder.finish_node();

    // Closing )
    builder.token(SyntaxKind::TEXT.into(), ")");

    builder.finish_node();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_autolink_url() {
        let input = "<https://example.com>";
        let result = try_parse_autolink(input);
        assert_eq!(result, Some((21, "https://example.com")));
    }

    #[test]
    fn test_parse_autolink_email() {
        let input = "<user@example.com>";
        let result = try_parse_autolink(input);
        assert_eq!(result, Some((18, "user@example.com")));
    }

    #[test]
    fn test_parse_autolink_no_close() {
        let input = "<https://example.com";
        let result = try_parse_autolink(input);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_autolink_with_space() {
        let input = "<https://example.com >";
        let result = try_parse_autolink(input);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_autolink_not_url_or_email() {
        let input = "<notaurl>";
        let result = try_parse_autolink(input);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_inline_link_simple() {
        let input = "[text](url)";
        let result = try_parse_inline_link(input);
        assert_eq!(result, Some((11, "text", "url")));
    }

    #[test]
    fn test_parse_inline_link_with_title() {
        let input = r#"[text](url "title")"#;
        let result = try_parse_inline_link(input);
        assert_eq!(result, Some((19, "text", r#"url "title""#)));
    }

    #[test]
    fn test_parse_inline_link_with_nested_brackets() {
        let input = "[outer [inner] text](url)";
        let result = try_parse_inline_link(input);
        assert_eq!(result, Some((25, "outer [inner] text", "url")));
    }

    #[test]
    fn test_parse_inline_link_no_space_between_brackets_and_parens() {
        let input = "[text] (url)";
        let result = try_parse_inline_link(input);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_inline_link_no_closing_bracket() {
        let input = "[text(url)";
        let result = try_parse_inline_link(input);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_inline_link_no_closing_paren() {
        let input = "[text](url";
        let result = try_parse_inline_link(input);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_inline_link_escaped_bracket() {
        let input = r"[text\]more](url)";
        let result = try_parse_inline_link(input);
        assert_eq!(result, Some((17, r"text\]more", "url")));
    }

    #[test]
    fn test_parse_inline_link_parens_in_url() {
        let input = "[text](url(with)parens)";
        let result = try_parse_inline_link(input);
        assert_eq!(result, Some((23, "text", "url(with)parens")));
    }

    #[test]
    fn test_parse_inline_image_simple() {
        let input = "![alt](image.jpg)";
        let result = try_parse_inline_image(input);
        assert_eq!(result, Some((17, "alt", "image.jpg", None)));
    }

    #[test]
    fn test_parse_inline_image_with_title() {
        let input = r#"![alt](image.jpg "A title")"#;
        let result = try_parse_inline_image(input);
        assert_eq!(result, Some((27, "alt", r#"image.jpg "A title""#, None)));
    }

    #[test]
    fn test_parse_inline_image_with_nested_brackets() {
        let input = "![outer [inner] alt](image.jpg)";
        let result = try_parse_inline_image(input);
        assert_eq!(result, Some((31, "outer [inner] alt", "image.jpg", None)));
    }

    #[test]
    fn test_parse_inline_image_no_space_between_brackets_and_parens() {
        let input = "![alt] (image.jpg)";
        let result = try_parse_inline_image(input);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_inline_image_no_closing_bracket() {
        let input = "![alt(image.jpg)";
        let result = try_parse_inline_image(input);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_inline_image_no_closing_paren() {
        let input = "![alt](image.jpg";
        let result = try_parse_inline_image(input);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_inline_image_with_simple_class() {
        let input = "![alt](img.png){.large}";
        let result = try_parse_inline_image(input);
        let (len, alt, dest, attrs) = result.unwrap();
        assert_eq!(len, 23);
        assert_eq!(alt, "alt");
        assert_eq!(dest, "img.png");
        assert!(attrs.is_some());
        let attrs = attrs.unwrap();
        assert_eq!(attrs.classes, vec!["large"]);
    }

    #[test]
    fn test_parse_inline_image_with_id() {
        let input = "![Figure 1](fig1.png){#fig-1}";
        let result = try_parse_inline_image(input);
        let (len, alt, dest, attrs) = result.unwrap();
        assert_eq!(len, 29);
        assert_eq!(alt, "Figure 1");
        assert_eq!(dest, "fig1.png");
        assert!(attrs.is_some());
        let attrs = attrs.unwrap();
        assert_eq!(attrs.identifier, Some("fig-1".to_string()));
    }

    #[test]
    fn test_parse_inline_image_with_full_attributes() {
        let input = "![alt](img.png){#fig .large width=\"80%\"}";
        let result = try_parse_inline_image(input);
        let (len, alt, dest, attrs) = result.unwrap();
        assert_eq!(len, 40);
        assert_eq!(alt, "alt");
        assert_eq!(dest, "img.png");
        assert!(attrs.is_some());
        let attrs = attrs.unwrap();
        assert_eq!(attrs.identifier, Some("fig".to_string()));
        assert_eq!(attrs.classes, vec!["large"]);
        assert_eq!(attrs.key_values.len(), 1);
        assert_eq!(attrs.key_values[0].0, "width");
    }

    #[test]
    fn test_parse_inline_image_attributes_must_be_adjacent() {
        // Space between ) and { should not parse as attributes
        let input = "![alt](img.png) {.large}";
        let result = try_parse_inline_image(input);
        assert_eq!(result, Some((15, "alt", "img.png", None)));
    }
}
