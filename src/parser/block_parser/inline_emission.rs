//! Inline element emission during block parsing.
//!
//! This module provides utilities for emitting inline structure directly during
//! block parsing, as part of the migration from the two-pass (block → inline)
//! architecture to Pandoc's single-pass approach.
//!
//! **Key invariant**: "Detect first, emit once"
//! Because GreenNodeBuilder cannot backtrack, we must determine what to emit
//! before calling builder methods. The inline parser already follows this pattern
//! (it detects delimiters/patterns before emitting nodes).

use crate::config::Config;
use crate::parser::block_parser::text_buffer::TextBuffer;
use crate::parser::inline_parser::core;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

/// Emit inline elements from text content directly into the builder.
///
/// This helper calls the existing recursive inline parser, allowing block-level
/// parsers to emit inline structure without code duplication.
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
///
/// # Note
/// This uses the same recursive parsing algorithm as the second-pass inline parser,
/// ensuring identical output. The difference is *when* it's called:
/// - Old: Block parser emits TEXT → InlineParser traverses tree and rebuilds
/// - New: Block parser calls this directly → emits inline nodes immediately
#[allow(dead_code)] // Will be used in Phase 2 when migrating blocks
pub fn emit_inlines(builder: &mut GreenNodeBuilder, text: &str, config: &Config) {
    log::trace!(
        "emit_inlines: {:?} ({} bytes)",
        &text[..text.len().min(40)],
        text.len()
    );

    // Call the existing recursive inline parser
    // This preserves all behavior from the second-pass approach
    core::parse_inline_text_recursive(builder, text, config);
}

/// Emit PLAIN block content with inline parsing applied.
///
/// This is a specialized version for PLAIN blocks that handles:
/// 1. Multi-line content buffering (via TextBuffer)
/// 2. Newline joining between lines
/// 3. Integrated inline parsing when flag is enabled
///
/// # Arguments
/// * `builder` - The GreenNodeBuilder to emit nodes into
/// * `buffer` - The accumulated PLAIN text lines (without trailing newlines)
/// * `config` - Configuration controlling parser behavior
///
/// # Behavior
/// - If `use_integrated_inline_parsing` is enabled: emits inline-parsed content
/// - Otherwise: emits legacy TEXT tokens with NEWLINE tokens between lines
///
/// # Note
/// The buffer should contain lines without trailing newlines. This function
/// will insert NEWLINE tokens between lines to preserve losslessness.
#[allow(dead_code)] // Used in Subtask 4
pub fn emit_plain_with_inlines(
    builder: &mut GreenNodeBuilder,
    buffer: &TextBuffer,
    config: &Config,
) {
    if buffer.is_empty() {
        return;
    }

    if config.parser.use_integrated_inline_parsing {
        // New path: emit inline-parsed content
        let text = buffer.get_accumulated_text();
        core::parse_inline_text_recursive(builder, &text, config);
    } else {
        // Legacy path: emit TEXT tokens with NEWLINE between lines
        // This preserves the exact behavior of the current parser
        for (i, line) in buffer.lines().enumerate() {
            if !line.is_empty() {
                builder.token(SyntaxKind::TEXT.into(), line);
            }
            // Add NEWLINE between lines (not after the last line)
            if i < buffer.len() - 1 {
                builder.token(SyntaxKind::NEWLINE.into(), "\n");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::syntax::{SyntaxKind, SyntaxNode};
    use rowan::GreenNodeBuilder;

    /// Test that emit_inlines produces identical output to the standalone inline parser.
    /// This is critical to ensure the migration doesn't change behavior.
    #[test]
    fn test_emit_inlines_matches_inline_parser() {
        let config = Config::default();
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
            // Build using emit_inlines (new approach)
            // Need to wrap in a node since builder requires balanced start/finish
            let mut builder_new = GreenNodeBuilder::new();
            builder_new.start_node(SyntaxKind::HEADING_CONTENT.into()); // Use arbitrary container
            emit_inlines(&mut builder_new, text, &config);
            builder_new.finish_node();
            let green_new = builder_new.finish();
            let tree_new = SyntaxNode::new_root(green_new);

            // Build using inline parser directly (old approach via second pass)
            let mut builder_old = GreenNodeBuilder::new();
            builder_old.start_node(SyntaxKind::HEADING_CONTENT.into());
            core::parse_inline_text_recursive(&mut builder_old, text, &config);
            builder_old.finish_node();
            let green_old = builder_old.finish();
            let tree_old = SyntaxNode::new_root(green_old);

            // Trees should be structurally identical
            assert_eq!(
                format!("{:?}", tree_new),
                format!("{:?}", tree_old),
                "Mismatch for text: {:?}",
                text
            );
        }
    }

    /// Test that emit_inlines handles empty text correctly.
    #[test]
    fn test_emit_inlines_empty() {
        let config = Config::default();
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
        let config = Config::default();
        let text = "  leading and trailing  ";

        let mut builder = GreenNodeBuilder::new();
        builder.start_node(SyntaxKind::HEADING_CONTENT.into());
        emit_inlines(&mut builder, text, &config);
        builder.finish_node();
        let green = builder.finish();
        let tree = SyntaxNode::new_root(green);

        // Should preserve all whitespace
        // Get the HEADING_CONTENT node's text
        assert_eq!(tree.text().to_string(), text);
    }
}
