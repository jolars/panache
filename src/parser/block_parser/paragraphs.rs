//! Paragraph handling utilities.
//!
//! Note: Most paragraph logic is in the main BlockParser since paragraphs
//! are tightly integrated with container handling.

use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use super::container_stack::{Container, ContainerStack};
use super::utils;

/// Start a paragraph if not already in one.
pub(super) fn start_paragraph_if_needed(
    containers: &mut ContainerStack,
    builder: &mut GreenNodeBuilder<'static>,
) {
    if !matches!(containers.last(), Some(Container::Paragraph { .. })) {
        builder.start_node(SyntaxKind::PARAGRAPH.into());
        containers.push(Container::Paragraph {});
    }
}

/// Append a line to the current paragraph (preserving losslessness).
pub(super) fn append_paragraph_line(builder: &mut GreenNodeBuilder<'static>, line: &str) {
    // For lossless parsing, preserve the line exactly as-is
    // Don't strip to content column in the parser - that's the formatter's job

    // Split off trailing newline (LF or CRLF) if present
    let (text_without_newline, newline_str) = utils::strip_newline(line);

    if !text_without_newline.is_empty() {
        builder.token(SyntaxKind::TEXT.into(), text_without_newline);
    }

    if !newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
    }
}

/// Get the current content column from the container stack.
pub(super) fn current_content_col(containers: &ContainerStack) -> usize {
    containers
        .stack
        .iter()
        .rev()
        .find_map(|c| match c {
            Container::ListItem { content_col } => Some(*content_col),
            Container::FootnoteDefinition { content_col, .. } => Some(*content_col),
            _ => None,
        })
        .unwrap_or(0)
}
