//! Inline element emission during block parsing.
//!
//! This module provides utilities for emitting inline structure directly during
//! block parsing, using Pandoc's single-pass architecture.
//!
//! **Key invariant**: "Detect first, emit once"
//! Because GreenNodeBuilder cannot backtrack, we must determine what to emit
//! before calling builder methods. The inline parser already follows this pattern
//! (it detects delimiters/patterns before emitting nodes).

use crate::options::ParserOptions;
use crate::parser::inlines::core;
use rowan::GreenNodeBuilder;

/// Emit inline elements from text content directly into the builder.
///
/// This helper calls the recursive inline parser, allowing block-level
/// parsers to emit inline structure during parsing.
///
/// # Arguments
/// * `builder` - The GreenNodeBuilder to emit nodes into
/// * `text` - The text content to parse for inline elements
/// * `config` - Configuration controlling which extensions are enabled
///
/// # Example
/// ```ignore
/// // In a block parser (e.g., headings):
/// builder.start_node(SyntaxKind::HEADING_CONTENT.into());
/// emit_inlines(builder, heading_text, config);
/// builder.finish_node();
/// ```
pub fn emit_inlines(builder: &mut GreenNodeBuilder, text: &str, config: &ParserOptions) {
    log::trace!(
        "emit_inlines: {:?} ({} bytes)",
        &text[..text.len().min(40)],
        text.len()
    );

    // Call the recursive inline parser
    core::parse_inline_text_recursive(builder, text, config);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::ParserOptions;
    use crate::syntax::{SyntaxKind, SyntaxNode};
    use rowan::GreenNodeBuilder;

    /// Test that emit_inlines produces correct inline structure.
    #[test]
    fn test_emit_inlines_basic() {
        let config = ParserOptions::default();
        let test_cases = vec![
            "plain text",
            "text with *emphasis*",
            "text with **strong**",
            "text with `code`",
            "text with [link](url)",
            "mixed *emph* and **strong** and `code`",
            "nested *emphasis with `code` inside*",
            "multiple *a* and *b* emphasis",
        ];

        for text in test_cases {
            // Build using emit_inlines
            let mut builder_new = GreenNodeBuilder::new();
            builder_new.start_node(SyntaxKind::HEADING_CONTENT.into());
            emit_inlines(&mut builder_new, text, &config);
            builder_new.finish_node();
            let green_new = builder_new.finish();
            let tree_new = SyntaxNode::new_root(green_new);

            // Verify losslessness
            assert_eq!(
                tree_new.text().to_string(),
                text,
                "Losslessness check failed for: {:?}",
                text
            );
        }
    }

    /// Test that emit_inlines handles empty text correctly.
    #[test]
    fn test_emit_inlines_empty() {
        let config = ParserOptions::default();
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(SyntaxKind::HEADING_CONTENT.into());
        emit_inlines(&mut builder, "", &config);
        builder.finish_node();
        let green = builder.finish();
        let tree = SyntaxNode::new_root(green);

        // Should produce a container with no inline content
        assert_eq!(tree.kind(), SyntaxKind::HEADING_CONTENT);
        assert_eq!(tree.children_with_tokens().count(), 0);
    }

    /// Test that emit_inlines preserves whitespace.
    #[test]
    fn test_emit_inlines_preserves_whitespace() {
        let config = ParserOptions::default();
        let text = "  leading and trailing  ";

        let mut builder = GreenNodeBuilder::new();
        builder.start_node(SyntaxKind::HEADING_CONTENT.into());
        emit_inlines(&mut builder, text, &config);
        builder.finish_node();
        let green = builder.finish();
        let tree = SyntaxNode::new_root(green);

        // Should preserve all whitespace
        assert_eq!(tree.text().to_string(), text);
    }
}
