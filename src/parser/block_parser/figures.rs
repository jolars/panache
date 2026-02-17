//! Figure parsing for standalone images.
//!
//! In Pandoc, a paragraph containing only an image (and optional attributes)
//! is treated as a Figure block element rather than a paragraph with inline image.

use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use super::utils;
use crate::parser::inline_parser::links::try_parse_inline_image;

/// Try to parse a line as a standalone figure (image).
///
/// Uses the existing inline image parser to validate the syntax properly.
/// Returns true if the line contains only a valid image (possibly with attributes).
pub(super) fn try_parse_figure(line: &str) -> bool {
    let trimmed = line.trim();

    // Must start with ![
    if !trimmed.starts_with("![") {
        return false;
    }

    // Use the inline parser's image validation to check if this is a valid image
    // This handles all the bracket/paren matching, escapes, etc.
    if let Some((len, _alt, _dest, _attrs)) = try_parse_inline_image(trimmed) {
        // Check that the image spans the entire line (except trailing whitespace)
        // After the image, only whitespace should remain
        let after_image = &trimmed[len..];
        after_image.trim().is_empty()
    } else {
        false
    }
}

/// Parse a figure block (standalone image).
///
/// The line is preserved exactly for lossless parsing, and the inline parser
/// will later process the image syntax.
pub(super) fn parse_figure(builder: &mut GreenNodeBuilder<'static>, line: &str) {
    builder.start_node(SyntaxKind::FIGURE.into());

    // Split off trailing newline
    let (text_without_newline, newline_str) = utils::strip_newline(line);

    if !text_without_newline.is_empty() {
        builder.token(SyntaxKind::TEXT.into(), text_without_newline);
    }

    if !newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
    }

    builder.finish_node(); // Close Figure
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_try_parse_figure_starts_with_image() {
        assert!(try_parse_figure("![alt text](image.png)"));
        assert!(try_parse_figure("  ![alt text](image.png)  "));
        assert!(try_parse_figure("![alt text](image.png)\n"));
        assert!(try_parse_figure("![](image.png)"));
        assert!(try_parse_figure("![alt text](image.png \"Title\")"));
    }

    #[test]
    fn test_try_parse_figure_not_a_figure() {
        // Has text before the image
        assert!(!try_parse_figure("Text before ![alt](img.png)"));

        // Not an image (regular link)
        assert!(!try_parse_figure("[text](url)"));

        // Empty or other content
        assert!(!try_parse_figure(""));
        assert!(!try_parse_figure("# Heading"));
    }
}
