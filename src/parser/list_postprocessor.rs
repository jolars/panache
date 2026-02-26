//! Post-processor to wrap list item content in Plain or PARAGRAPH blocks.
//!
//! This module traverses the syntax tree after initial parsing to identify list items and wrap
//! their content in Plain (for tight lists) or PARAGRAPH (for loose lists) nodes. It also applies
//! inline parsing to the wrapped content to ensure that inline elements are correctly recognized
//! within list items.

use crate::config::Config;
use crate::parser::inlines::core::parse_inline_text_recursive;
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::{GreenNode, GreenNodeBuilder};

/// Post-processes a syntax tree to wrap list item content in Plain/PARAGRAPH blocks.
///
/// Traverses the tree looking for List nodes, determines if they're tight or loose,
/// and wraps bare TEXT/NEWLINE tokens in list items appropriately. Also applies inline
/// parsing to the wrapped content.
pub fn wrap_list_item_content(root: SyntaxNode, config: &Config) -> GreenNode {
    let mut builder = GreenNodeBuilder::new();
    process_node(&root, &mut builder, config);
    builder.finish()
}

fn process_node(node: &SyntaxNode, builder: &mut GreenNodeBuilder, config: &Config) {
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
                        process_list_item(child_node, is_loose, builder, config);
                    } else {
                        // Other children (BlankLine, etc.) - recurse normally
                        process_node_or_token(child_node, builder, config);
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
                    process_node(child_node, builder, config);
                } else {
                    let token = child.as_token().unwrap();
                    builder.token(token.kind().into(), token.text());
                }
            }
            builder.finish_node();
        }
    }
}

fn process_node_or_token(node: &SyntaxNode, builder: &mut GreenNodeBuilder, config: &Config) {
    builder.start_node(node.kind().into());
    for child in node.children_with_tokens() {
        if let Some(child_node) = child.as_node() {
            process_node(child_node, builder, config);
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
/// Now also applies inline parsing to the wrapped content.
fn process_list_item(
    item: &SyntaxNode,
    is_loose: bool,
    builder: &mut GreenNodeBuilder,
    config: &Config,
) {
    builder.start_node(SyntaxKind::LIST_ITEM.into());

    let wrapper_kind = if is_loose {
        SyntaxKind::PARAGRAPH
    } else {
        SyntaxKind::PLAIN
    };

    // Check if this is an empty list item (only marker + whitespace/newline, no text content)
    let is_empty_item = !item.children_with_tokens().any(|child| {
        match child {
            rowan::NodeOrToken::Token(token) => {
                matches!(token.kind(), SyntaxKind::TEXT | SyntaxKind::ESCAPED_CHAR)
            }
            rowan::NodeOrToken::Node(node) => {
                // Has actual content nodes (not just nested lists)
                !is_block_node(node.kind())
            }
        }
    });

    let mut in_content_wrapper = false;
    let mut after_marker = false;
    let mut accumulated_text = String::new();

    for child in item.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(token) => {
                let kind = token.kind();

                // ListMarker and first WHITESPACE after it are not wrapped
                if kind == SyntaxKind::LIST_MARKER {
                    // Close wrapper if open (emit accumulated text with inline parsing)
                    if in_content_wrapper {
                        emit_wrapper_with_inline_parsing(
                            builder,
                            wrapper_kind,
                            &accumulated_text,
                            config,
                        );
                        accumulated_text.clear();
                        in_content_wrapper = false;
                    }
                    builder.token(kind.into(), token.text());
                    after_marker = true;
                } else if kind == SyntaxKind::WHITESPACE && after_marker {
                    // First whitespace after marker is not wrapped
                    // Close wrapper if open
                    if in_content_wrapper {
                        emit_wrapper_with_inline_parsing(
                            builder,
                            wrapper_kind,
                            &accumulated_text,
                            config,
                        );
                        accumulated_text.clear();
                        in_content_wrapper = false;
                    }
                    builder.token(kind.into(), token.text());
                    after_marker = false;
                } else if kind == SyntaxKind::TEXT
                    || kind == SyntaxKind::NEWLINE
                    || kind == SyntaxKind::ESCAPED_CHAR
                {
                    // For empty items, don't wrap the trailing newline
                    if is_empty_item && kind == SyntaxKind::NEWLINE {
                        if in_content_wrapper {
                            emit_wrapper_with_inline_parsing(
                                builder,
                                wrapper_kind,
                                &accumulated_text,
                                config,
                            );
                            accumulated_text.clear();
                            in_content_wrapper = false;
                        }
                        builder.token(kind.into(), token.text());
                    } else {
                        // Accumulate text for inline parsing
                        if !in_content_wrapper {
                            in_content_wrapper = true;
                        }
                        accumulated_text.push_str(token.text());
                    }
                } else if kind == SyntaxKind::WHITESPACE {
                    // WHITESPACE handling:
                    // - After marker whitespace (after_marker=true) -> already handled above
                    // - All other whitespace should be wrapped (it's either leading indent
                    //   for nested lists, or inline whitespace)
                    if !in_content_wrapper {
                        in_content_wrapper = true;
                    }
                    accumulated_text.push_str(token.text());
                } else {
                    // Other tokens (like HardLineBreak, etc.)
                    // These might need wrapping or not depending on context
                    // For now, don't wrap them
                    if in_content_wrapper {
                        emit_wrapper_with_inline_parsing(
                            builder,
                            wrapper_kind,
                            &accumulated_text,
                            config,
                        );
                        accumulated_text.clear();
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
                        emit_wrapper_with_inline_parsing(
                            builder,
                            wrapper_kind,
                            &accumulated_text,
                            config,
                        );
                        accumulated_text.clear();
                        in_content_wrapper = false;
                    }
                    // Emit the block node as-is (recurse for nested lists)
                    process_node(&node, builder, config);
                } else {
                    // Inline nodes (Strong, Emphasis, Code, Link, etc.) - should not happen
                    // with current architecture, but handle just in case
                    if !in_content_wrapper {
                        in_content_wrapper = true;
                    }
                    // Copy this node as text to be re-parsed
                    // This shouldn't happen in practice since list items have raw TEXT tokens
                    accumulated_text.push_str(&node.text().to_string());
                }
            }
        }
    }

    // Close wrapper if still open (emit accumulated text with inline parsing)
    if in_content_wrapper && !accumulated_text.is_empty() {
        emit_wrapper_with_inline_parsing(builder, wrapper_kind, &accumulated_text, config);
    }

    builder.finish_node();
}

/// Emit a PLAIN or PARAGRAPH wrapper with inline-parsed content.
fn emit_wrapper_with_inline_parsing(
    builder: &mut GreenNodeBuilder,
    wrapper_kind: SyntaxKind,
    text: &str,
    config: &Config,
) {
    builder.start_node(wrapper_kind.into());

    // Special case: if the content is ONLY whitespace, emit it as WHITESPACE token
    // (not as inline-parsed TEXT). This preserves the token type for leading indents.
    if text.chars().all(|c| c.is_whitespace()) {
        builder.token(SyntaxKind::WHITESPACE.into(), text);
    } else {
        parse_inline_text_recursive(builder, text, config);
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
