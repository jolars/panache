//! Figure parsing for standalone images.
//!
//! In Pandoc, a paragraph containing only an image (and optional attributes)
//! is treated as a Figure block element rather than a paragraph with inline image.

use crate::config::Config;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use crate::parser::utils::helpers;
use crate::parser::utils::inline_emission::emit_inlines;

/// Parse a figure block (standalone image).
///
/// Emits inline-parsed structure directly during block parsing.
pub(in crate::parser) fn parse_figure(
    builder: &mut GreenNodeBuilder<'static>,
    line: &str,
    config: &Config,
) {
    builder.start_node(SyntaxKind::FIGURE.into());

    // Split off trailing newline
    let (text_without_newline, newline_str) = helpers::strip_newline(line);

    // Parse inline content (IMAGE_LINK) directly
    if !text_without_newline.is_empty() {
        emit_inlines(builder, text_without_newline, config);
    }

    if !newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
    }

    builder.finish_node(); // Close Figure
}
