//! ATX heading parsing utilities.

use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use super::attributes::{emit_attributes, try_parse_trailing_attributes};

/// Try to parse an ATX heading from content, returns heading level (1-6) if found.
pub(crate) fn try_parse_atx_heading(content: &str) -> Option<usize> {
    let trimmed = content.trim_start();

    // Must start with 1-6 # characters
    let hash_count = trimmed.chars().take_while(|&c| c == '#').count();
    if hash_count == 0 || hash_count > 6 {
        return None;
    }

    // After hashes, must be end of line, space, or tab
    let after_hashes = &trimmed[hash_count..];
    if !after_hashes.is_empty() && !after_hashes.starts_with(' ') && !after_hashes.starts_with('\t')
    {
        return None;
    }

    // Check leading spaces (max 3)
    let leading_spaces = content.len() - trimmed.len();
    if leading_spaces > 3 {
        return None;
    }

    Some(hash_count)
}

/// Emit an ATX heading node to the builder.
pub(crate) fn emit_atx_heading(
    builder: &mut GreenNodeBuilder<'static>,
    content: &str,
    level: usize,
) {
    builder.start_node(SyntaxKind::Heading.into());

    let trimmed = content.trim_start();

    // Marker node for the hashes (must be a node containing a token, not just a token)
    builder.start_node(SyntaxKind::AtxHeadingMarker.into());
    builder.token(SyntaxKind::AtxHeadingMarker.into(), &trimmed[..level]);
    builder.finish_node();

    // Get content after marker
    let after_marker = &trimmed[level..];
    let content_start = after_marker
        .find(|c: char| !c.is_whitespace())
        .unwrap_or(after_marker.len());

    // Strip trailing hashes
    let heading_content = after_marker[content_start..].trim_end();
    let heading_content = heading_content.trim_end_matches(|c: char| c == '#' || c.is_whitespace());

    // Try to parse trailing attributes
    let (text_content, attributes) =
        if let Some((attrs, text_before)) = try_parse_trailing_attributes(heading_content) {
            (text_before, Some(attrs))
        } else {
            (heading_content, None)
        };

    // Heading content node
    builder.start_node(SyntaxKind::HeadingContent.into());
    if !text_content.is_empty() {
        builder.token(SyntaxKind::TEXT.into(), text_content);
    }
    builder.finish_node();

    // Emit attributes if present
    if let Some(attrs) = attributes {
        emit_attributes(builder, &attrs);
    }

    builder.finish_node(); // Heading
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_heading() {
        assert_eq!(try_parse_atx_heading("# Heading"), Some(1));
    }

    #[test]
    fn test_level_3_heading() {
        assert_eq!(try_parse_atx_heading("### Level 3"), Some(3));
    }

    #[test]
    fn test_heading_with_leading_spaces() {
        assert_eq!(try_parse_atx_heading("   # Heading"), Some(1));
    }

    #[test]
    fn test_four_spaces_not_heading() {
        assert_eq!(try_parse_atx_heading("    # Not heading"), None);
    }

    #[test]
    fn test_no_space_after_hash() {
        assert_eq!(try_parse_atx_heading("#NoSpace"), None);
    }

    #[test]
    fn test_empty_heading() {
        assert_eq!(try_parse_atx_heading("# "), Some(1));
    }

    #[test]
    fn test_level_7_invalid() {
        assert_eq!(try_parse_atx_heading("####### Too many"), None);
    }
}
