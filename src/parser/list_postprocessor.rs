//! Post-processor to wrap list item content in Plain or PARAGRAPH blocks.
//!
//! This module transforms list items from having bare TEXT/NEWLINE tokens as direct
//! children to wrapping them in Plain (tight lists) or PARAGRAPH (loose lists) blocks,
//! matching Pandoc's AST structure.

use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::{GreenNode, GreenNodeBuilder};

/// Post-processes a syntax tree to wrap list item content in Plain/PARAGRAPH blocks.
///
/// Traverses the tree looking for List nodes, determines if they're tight or loose,
/// and wraps bare TEXT/NEWLINE tokens in list items appropriately.
pub fn wrap_list_item_content(root: SyntaxNode) -> GreenNode {
    let mut builder = GreenNodeBuilder::new();
    process_node(&root, &mut builder);
    builder.finish()
}

fn process_node(node: &SyntaxNode, builder: &mut GreenNodeBuilder) {
    match node.kind() {
        SyntaxKind::LIST => {
            // Start the List node
            builder.start_node(SyntaxKind::LIST.into());

            // Determine if this list is tight or loose
            let is_loose = has_blank_line_between_items(node);

            // Process each child
            for child in node.children_with_tokens() {
                if let Some(child_node) = child.as_node() {
                    if child_node.kind() == SyntaxKind::LIST_ITEM {
                        process_list_item(child_node, is_loose, builder);
                    } else {
                        // Other children (BlankLine, etc.) - recurse normally
                        process_node_or_token(child_node, builder);
                    }
                } else {
                    // Token - add as-is
                    let token = child.as_token().unwrap();
                    builder.token(token.kind().into(), token.text());
                }
            }

            builder.finish_node();
        }
        _ => {
            // For non-List nodes, recurse normally
            builder.start_node(node.kind().into());
            for child in node.children_with_tokens() {
                if let Some(child_node) = child.as_node() {
                    process_node(child_node, builder);
                } else {
                    let token = child.as_token().unwrap();
                    builder.token(token.kind().into(), token.text());
                }
            }
            builder.finish_node();
        }
    }
}

fn process_node_or_token(node: &SyntaxNode, builder: &mut GreenNodeBuilder) {
    builder.start_node(node.kind().into());
    for child in node.children_with_tokens() {
        if let Some(child_node) = child.as_node() {
            process_node(child_node, builder);
        } else {
            let token = child.as_token().unwrap();
            builder.token(token.kind().into(), token.text());
        }
    }
    builder.finish_node();
}

/// Determines if a list is loose (has blank lines between items).
fn has_blank_line_between_items(list_node: &SyntaxNode) -> bool {
    let mut prev_was_item = false;

    for child in list_node.children() {
        match child.kind() {
            SyntaxKind::LIST_ITEM => {
                prev_was_item = true;
            }
            SyntaxKind::BLANK_LINE => {
                if prev_was_item {
                    // Found a blank line after an item - check if there's another item after
                    if let Some(next_sibling) = child.next_sibling()
                        && next_sibling.kind() == SyntaxKind::LIST_ITEM
                    {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }

    false
}

/// Processes a single list item, wrapping bare tokens in Plain/PARAGRAPH.
fn process_list_item(item: &SyntaxNode, is_loose: bool, builder: &mut GreenNodeBuilder) {
    builder.start_node(SyntaxKind::LIST_ITEM.into());

    let wrapper_kind = if is_loose {
        SyntaxKind::PARAGRAPH
    } else {
        SyntaxKind::PLAIN
    };

    let mut in_content_wrapper = false;
    let mut after_marker = false;

    for child in item.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(token) => {
                let kind = token.kind();

                // ListMarker and first WHITESPACE after it are not wrapped
                if kind == SyntaxKind::LIST_MARKER {
                    // Close wrapper if open
                    if in_content_wrapper {
                        builder.finish_node();
                        in_content_wrapper = false;
                    }
                    builder.token(kind.into(), token.text());
                    after_marker = true;
                } else if kind == SyntaxKind::WHITESPACE && after_marker {
                    // First whitespace after marker is not wrapped
                    // Close wrapper if open
                    if in_content_wrapper {
                        builder.finish_node();
                        in_content_wrapper = false;
                    }
                    builder.token(kind.into(), token.text());
                    after_marker = false;
                } else if kind == SyntaxKind::TEXT
                    || kind == SyntaxKind::NEWLINE
                    || kind == SyntaxKind::WHITESPACE
                {
                    // Start wrapper if not started
                    if !in_content_wrapper {
                        builder.start_node(wrapper_kind.into());
                        in_content_wrapper = true;
                    }
                    builder.token(kind.into(), token.text());
                } else {
                    // Other tokens (like HardLineBreak, etc.)
                    // These might need wrapping or not depending on context
                    // For now, don't wrap them
                    if in_content_wrapper {
                        builder.finish_node();
                        in_content_wrapper = false;
                    }
                    builder.token(kind.into(), token.text());
                }
            }
            rowan::NodeOrToken::Node(node) => {
                let kind = node.kind();

                // Check if this is a block-level node
                if is_block_node(kind) {
                    // Close wrapper before block
                    if in_content_wrapper {
                        builder.finish_node();
                        in_content_wrapper = false;
                    }
                    // Emit the block node as-is (recurse for nested lists)
                    process_node(&node, builder);
                } else {
                    // Inline nodes (Strong, Emphasis, Code, Link, etc.)
                    // These should be wrapped INSIDE Plain/PARAGRAPH
                    if !in_content_wrapper {
                        builder.start_node(wrapper_kind.into());
                        in_content_wrapper = true;
                    }
                    // Emit the inline node inside the wrapper
                    process_node_or_token(&node, builder);
                }
            }
        }
    }

    // Close wrapper if still open
    if in_content_wrapper {
        builder.finish_node();
    }

    builder.finish_node();
}

/// Checks if a node is block-level (should be a sibling, not wrapped in Plain/PARAGRAPH).
fn is_block_node(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::CODE_BLOCK
            | SyntaxKind::LIST
            | SyntaxKind::BLOCKQUOTE
            | SyntaxKind::SIMPLE_TABLE
            | SyntaxKind::PIPE_TABLE
            | SyntaxKind::GRID_TABLE
            | SyntaxKind::BLANK_LINE
            | SyntaxKind::HORIZONTAL_RULE
            | SyntaxKind::HTML_BLOCK
            | SyntaxKind::FENCED_DIV
            | SyntaxKind::PARAGRAPH // Existing PARAGRAPH nodes from parser should be preserved
    )
}
