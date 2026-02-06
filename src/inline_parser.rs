use crate::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use rowan::{GreenNode, GreenNodeBuilder};

mod code_spans;
mod inline_math;
mod tests;

use code_spans::{emit_code_span, try_parse_code_span};
use inline_math::{emit_inline_math, try_parse_inline_math};

/// The InlineParser takes a block-level CST and processes inline elements within text content.
/// It traverses the tree, finds TEXT tokens that need inline parsing, and replaces them
/// with properly parsed inline elements (emphasis, links, math, etc.).
pub struct InlineParser {
    root: SyntaxNode,
}

impl InlineParser {
    pub fn new(root: SyntaxNode) -> Self {
        Self { root }
    }

    /// Parse inline elements within the block-level CST.
    /// Traverses the tree and replaces TEXT tokens with parsed inline elements.
    pub fn parse(self) -> SyntaxNode {
        let green = self.parse_node(&self.root);
        SyntaxNode::new_root(green)
    }

    /// Recursively parse a node, replacing TEXT tokens with inline elements.
    fn parse_node(&self, node: &SyntaxNode) -> GreenNode {
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(node.kind().into());

        for child in node.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Node(n) => {
                    // Recursively parse child nodes
                    let green = self.parse_node(&n);
                    // Unwrap the green node and add it as a child by starting/finishing
                    // Actually, we need to just call the builder methods to add the subtree
                    // The rowan API doesn't have add_child - we need to checkpoint/restore
                    // Let me rebuild this differently
                    self.copy_node_to_builder(&mut builder, &n);
                }
                rowan::NodeOrToken::Token(t) => {
                    // Check if this is a TEXT token that needs inline parsing
                    if self.should_parse_inline(&t) {
                        self.parse_inline_text(&mut builder, t.text());
                    } else {
                        // Pass through other tokens unchanged
                        builder.token(t.kind().into(), t.text());
                    }
                }
            }
        }

        builder.finish_node();
        builder.finish()
    }

    /// Copy a node and its children to the builder, recursively parsing inline elements.
    fn copy_node_to_builder(&self, builder: &mut GreenNodeBuilder, node: &SyntaxNode) {
        builder.start_node(node.kind().into());

        for child in node.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Node(n) => {
                    self.copy_node_to_builder(builder, &n);
                }
                rowan::NodeOrToken::Token(t) => {
                    if self.should_parse_inline(&t) {
                        self.parse_inline_text(builder, t.text());
                    } else {
                        builder.token(t.kind().into(), t.text());
                    }
                }
            }
        }

        builder.finish_node();
    }

    /// Check if a token should be parsed for inline elements.
    fn should_parse_inline(&self, token: &SyntaxToken) -> bool {
        // For now, only parse TEXT tokens
        // Later we might exclude TEXT in certain contexts (e.g., inside CodeBlock)
        token.kind() == SyntaxKind::TEXT
    }

    /// Parse inline elements from text content.
    /// This is where the actual inline parsing happens.
    fn parse_inline_text(&self, builder: &mut GreenNodeBuilder, text: &str) {
        let mut pos = 0;
        let bytes = text.as_bytes();

        while pos < text.len() {
            // Try to parse code span
            if bytes[pos] == b'`' {
                if let Some((len, content, backtick_count)) = try_parse_code_span(&text[pos..]) {
                    emit_code_span(builder, content, backtick_count);
                    pos += len;
                    continue;
                }
            }

            // Try to parse inline math
            if bytes[pos] == b'$' {
                if let Some((len, content)) = try_parse_inline_math(&text[pos..]) {
                    emit_inline_math(builder, content);
                    pos += len;
                    continue;
                }
            }

            // TODO: Try other inline elements (emphasis, links, etc.)

            // No inline element matched - emit as plain text
            let next_pos = self.find_next_inline_start(&text[pos..]);
            let text_chunk = if next_pos > 0 {
                &text[pos..pos + next_pos]
            } else {
                &text[pos..]
            };

            if !text_chunk.is_empty() {
                builder.token(SyntaxKind::TEXT.into(), text_chunk);
            }

            if next_pos > 0 {
                pos += next_pos;
            } else {
                break;
            }
        }
    }

    /// Find the next position where an inline element might start.
    /// Returns the number of bytes to skip, or 0 if at end.
    fn find_next_inline_start(&self, text: &str) -> usize {
        for (i, ch) in text.char_indices() {
            match ch {
                '`' | '*' | '_' | '[' | '!' | '<' | '$' | '\\' => return i.max(1),
                _ => {}
            }
        }
        text.len()
    }
}

#[cfg(test)]
mod inline_tests {
    use super::*;
    use crate::block_parser::BlockParser;

    #[test]
    fn test_inline_parser_preserves_text() {
        let input = "This is plain text.";
        let block_tree = BlockParser::new(input).parse();
        let inline_tree = InlineParser::new(block_tree).parse();

        // Should preserve the text unchanged for now
        let text = inline_tree.to_string();
        assert!(text.contains("This is plain text."));
    }
}
