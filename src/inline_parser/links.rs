//! Parsing for links, images, and automatic links.
//!
//! Implements:
//! - Automatic links: `<http://example.com>` and `<user@example.com>`
//! - Inline links: `[text](url)` and `[text](url "title")`
//! - Inline images: `![alt](url)` and `![alt](url "title")`
//! - Image attributes: `![alt](url){#id .class key=value}`
//! - Reference links: `[text][ref]`, `[text][]`, `[text]`
//! - Reference images: `![alt][ref]`, `![alt][]`, `![alt]`

use crate::block_parser::ReferenceRegistry;
use crate::inline_parser::parse_inline_text;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

// Import attribute parsing
use crate::block_parser::attributes::{
    AttributeBlock, emit_attributes, try_parse_trailing_attributes,
};

/// Helper to normalize a reference label (lowercase, collapse whitespace)
#[allow(dead_code)] // TODO: Used for reference link resolution (not yet fully implemented)
fn normalize_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

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

/// Try to parse a reference link starting at the current position.
///
/// Reference links have three forms:
/// - Explicit: `[text][label]`
/// - Implicit: `[text][]` (label = text)
/// - Shortcut: `[text]` (if shortcut_reference_links enabled)
///
/// Returns Some((length, text_content, label, is_shortcut)) if a valid reference link is found.
/// The label is what should be looked up in the registry.
#[allow(dead_code)] // TODO: Will be integrated into main parsing loop
pub fn try_parse_reference_link(
    text: &str,
    allow_shortcut: bool,
) -> Option<(usize, &str, String, bool)> {
    if !text.starts_with('[') {
        return None;
    }

    // Find the closing ] for the text
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

    // Check what follows the ]
    let after_bracket = close_bracket + 1;

    // Check if followed by ( - if so, this is an inline link, not a reference link
    if after_bracket < text.len() && text[after_bracket..].starts_with('(') {
        return None;
    }

    // Check for explicit reference [text][label] or implicit [text][]
    if after_bracket < text.len() && text[after_bracket..].starts_with('[') {
        // Find the closing ] for the label
        let label_start = after_bracket + 1;
        let mut label_end = None;

        for (i, ch) in text[label_start..].char_indices() {
            if ch == ']' {
                label_end = Some(i + label_start);
                break;
            }
            // Labels can't contain newlines
            if ch == '\n' {
                return None;
            }
        }

        let label_end = label_end?;
        let label = &text[label_start..label_end];

        // Total length includes both bracket pairs
        let total_len = label_end + 1;

        // Implicit reference: empty label means use text as label
        if label.is_empty() {
            return Some((total_len, link_text, link_text.to_string(), false));
        }

        // Explicit reference: use the provided label
        Some((total_len, link_text, label.to_string(), false))
    } else if allow_shortcut {
        // Shortcut reference: [text] with no second bracket pair
        // The text is both the display text and the label
        Some((after_bracket, link_text, link_text.to_string(), true))
    } else {
        // No second bracket pair and shortcut not allowed - not a reference link
        None
    }
}

/// Emit a reference link node to the builder.
/// If the reference is found in the registry, use the URL from the definition.
/// Otherwise, emit as unresolved reference.
#[allow(dead_code)] // TODO: Will be integrated into main parsing loop
pub fn emit_reference_link(
    builder: &mut GreenNodeBuilder,
    link_text: &str,
    label: &str,
    registry: &ReferenceRegistry,
) {
    builder.start_node(SyntaxKind::Link.into());

    // Opening [
    builder.start_node(SyntaxKind::LinkStart.into());
    builder.token(SyntaxKind::LinkStart.into(), "[");
    builder.finish_node();

    // Link text (recursively parse inline elements)
    builder.start_node(SyntaxKind::LinkText.into());
    crate::inline_parser::parse_inline_text(builder, link_text);
    builder.finish_node();

    // Closing ]
    builder.token(SyntaxKind::TEXT.into(), "]");

    // Try to resolve the reference
    if let Some(def) = registry.get(label) {
        // Found definition - emit as resolved link
        // Format: [text](resolved_url)
        builder.token(SyntaxKind::TEXT.into(), "(");

        builder.start_node(SyntaxKind::LinkDest.into());
        builder.token(SyntaxKind::TEXT.into(), &def.url);
        if let Some(title) = &def.title {
            builder.token(SyntaxKind::TEXT.into(), " \"");
            builder.token(SyntaxKind::TEXT.into(), title);
            builder.token(SyntaxKind::TEXT.into(), "\"");
        }
        builder.finish_node();

        builder.token(SyntaxKind::TEXT.into(), ")");
    } else {
        // Unresolved reference - emit the reference label
        builder.token(SyntaxKind::TEXT.into(), "[");
        builder.start_node(SyntaxKind::LinkRef.into());
        builder.token(SyntaxKind::TEXT.into(), label);
        builder.finish_node();
        builder.token(SyntaxKind::TEXT.into(), "]");
    }

    builder.finish_node();
}

/// Try to parse a reference-style image: `![alt][ref]`, `![alt][]`, or `![alt]`
/// Returns (total_len, alt_text, label, is_shortcut) if successful.
#[allow(dead_code)] // TODO: Will be integrated into main parsing loop
pub fn try_parse_reference_image(
    text: &str,
    allow_shortcut: bool,
) -> Option<(usize, &str, String, bool)> {
    let bytes = text.as_bytes();
    if bytes.len() < 4 || bytes[0] != b'!' || bytes[1] != b'[' {
        return None;
    }

    let mut pos = 2;
    let mut bracket_depth = 1;
    let alt_start = pos;

    // Find the end of the alt text (allowing nested brackets)
    while pos < bytes.len() && bracket_depth > 0 {
        match bytes[pos] {
            b'[' => bracket_depth += 1,
            b']' => bracket_depth -= 1,
            b'\\' if pos + 1 < bytes.len() => pos += 1, // skip escaped char
            _ => {}
        }
        pos += 1;
    }

    if bracket_depth > 0 {
        return None; // Unclosed brackets
    }

    let alt_text = &text[alt_start..pos - 1];

    // Now check for the label part
    if pos >= bytes.len() {
        return None;
    }

    // Explicit reference: `![alt][label]`
    if bytes[pos] == b'[' {
        pos += 1;
        let label_start = pos;

        // Find the end of the label (no nested brackets, no newlines)
        while pos < bytes.len() && bytes[pos] != b']' && bytes[pos] != b'\n' {
            pos += 1;
        }

        if pos >= bytes.len() || bytes[pos] != b']' {
            return None;
        }

        let label_text = &text[label_start..pos];
        pos += 1;

        // Empty label means implicit reference
        let label = if label_text.is_empty() {
            normalize_label(alt_text)
        } else {
            normalize_label(label_text)
        };

        return Some((pos, alt_text, label, false));
    }

    // Shortcut reference: `![alt]` (only if enabled)
    // BUT not if followed by (url) - that's an inline image
    if allow_shortcut {
        // Check if next char is ( - if so, not a reference
        if pos < bytes.len() && bytes[pos] == b'(' {
            return None;
        }

        let label = normalize_label(alt_text);
        return Some((pos, alt_text, label, true));
    }

    None
}

/// Emit a reference image node with registry lookup.
#[allow(dead_code)] // TODO: Will be integrated into main parsing loop
pub fn emit_reference_image(
    builder: &mut GreenNodeBuilder,
    alt_text: &str,
    label: &str,
    registry: &crate::block_parser::ReferenceRegistry,
) {
    builder.start_node(SyntaxKind::ImageLink.into());

    // Look up the reference in the registry
    if let Some(def) = registry.get(label) {
        log::debug!("Resolved reference image: label={}, url={}", label, def.url);

        // Emit as a resolved image
        builder.start_node(SyntaxKind::ImageLinkStart.into());
        builder.token(SyntaxKind::ImageLinkStart.into(), "![");
        builder.finish_node();

        // Alt text (recursively parse inline elements)
        builder.start_node(SyntaxKind::ImageAlt.into());
        parse_inline_text(builder, alt_text);
        builder.finish_node();

        // Closing ] and opening (
        builder.token(SyntaxKind::TEXT.into(), "](");

        // Destination
        builder.start_node(SyntaxKind::LinkDest.into());
        builder.token(SyntaxKind::TEXT.into(), &def.url);
        builder.finish_node();

        // Title if present
        if let Some(title) = &def.title {
            builder.token(SyntaxKind::TEXT.into(), " \"");
            builder.token(SyntaxKind::TEXT.into(), title);
            builder.token(SyntaxKind::TEXT.into(), "\"");
        }

        // Closing )
        builder.token(SyntaxKind::TEXT.into(), ")");
    } else {
        log::debug!("Unresolved reference image: label={}", label);

        // Emit as unresolved (keep original syntax)
        builder.start_node(SyntaxKind::ImageLinkStart.into());
        builder.token(SyntaxKind::ImageLinkStart.into(), "![");
        builder.finish_node();

        builder.start_node(SyntaxKind::ImageAlt.into());
        parse_inline_text(builder, alt_text);
        builder.finish_node();

        builder.token(SyntaxKind::TEXT.into(), "][");
        builder.token(SyntaxKind::TEXT.into(), label);
        builder.token(SyntaxKind::TEXT.into(), "]");
    }

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

    // Reference link tests
    #[test]
    fn test_parse_reference_link_explicit() {
        let input = "[link text][label]";
        let result = try_parse_reference_link(input, false);
        assert_eq!(result, Some((18, "link text", "label".to_string(), false)));
    }

    #[test]
    fn test_parse_reference_link_implicit() {
        let input = "[link text][]";
        let result = try_parse_reference_link(input, false);
        assert_eq!(
            result,
            Some((13, "link text", "link text".to_string(), false))
        );
    }

    #[test]
    fn test_parse_reference_link_shortcut() {
        let input = "[link text] rest";
        let result = try_parse_reference_link(input, true);
        assert_eq!(
            result,
            Some((11, "link text", "link text".to_string(), true))
        );
    }

    #[test]
    fn test_parse_reference_link_shortcut_disabled() {
        let input = "[link text] rest";
        let result = try_parse_reference_link(input, false);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_reference_link_not_inline_link() {
        // Should not match inline links with (url)
        let input = "[text](url)";
        let result = try_parse_reference_link(input, true);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_reference_link_with_nested_brackets() {
        let input = "[outer [inner] text][ref]";
        let result = try_parse_reference_link(input, false);
        assert_eq!(
            result,
            Some((25, "outer [inner] text", "ref".to_string(), false))
        );
    }

    #[test]
    fn test_parse_reference_link_label_no_newline() {
        let input = "[text][label\nmore]";
        let result = try_parse_reference_link(input, false);
        assert_eq!(result, None);
    }

    // Reference image tests
    #[test]
    fn test_parse_reference_image_explicit() {
        let input = "![alt text][label]";
        let result = try_parse_reference_image(input, false);
        assert_eq!(result, Some((18, "alt text", "label".to_string(), false)));
    }

    #[test]
    fn test_parse_reference_image_implicit() {
        let input = "![alt text][]";
        let result = try_parse_reference_image(input, false);
        assert_eq!(
            result,
            Some((13, "alt text", "alt text".to_string(), false))
        );
    }

    #[test]
    fn test_parse_reference_image_shortcut() {
        let input = "![alt text] rest";
        let result = try_parse_reference_image(input, true);
        assert_eq!(result, Some((11, "alt text", "alt text".to_string(), true)));
    }

    #[test]
    fn test_parse_reference_image_shortcut_disabled() {
        let input = "![alt text] rest";
        let result = try_parse_reference_image(input, false);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_reference_image_not_inline() {
        // Should not match inline images with (url)
        let input = "![alt](url)";
        let result = try_parse_reference_image(input, true);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_reference_image_with_nested_brackets() {
        let input = "![alt [nested] text][ref]";
        let result = try_parse_reference_image(input, false);
        assert_eq!(
            result,
            Some((25, "alt [nested] text", "ref".to_string(), false))
        );
    }
}
