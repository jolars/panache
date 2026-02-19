//! List conversion utilities for code actions.
//!
//! Provides functions to convert lists between loose and compact formatting styles.

use crate::syntax::{AstNode, List, SyntaxKind, SyntaxNode};
use tower_lsp_server::ls_types::{Position, Range, TextEdit};

/// Find the innermost LIST node at the given position.
pub fn find_list_at_position(tree: &SyntaxNode, offset: usize) -> Option<SyntaxNode> {
    // Find the deepest node at this offset
    let text_size = rowan::TextSize::from(offset as u32);
    let token = tree.token_at_offset(text_size).right_biased()?;

    // Walk up the tree to find the innermost LIST node
    token
        .parent_ancestors()
        .find(|node| node.kind() == SyntaxKind::LIST)
}

/// Convert a compact list to a loose list by inserting blank lines between items.
///
/// Returns a list of TextEdits that insert blank lines after each list item
/// (except the last one, to avoid adding trailing blank lines).
pub fn convert_to_loose(list_node: &SyntaxNode, text: &str) -> Vec<TextEdit> {
    let list = match List::cast(list_node.clone()) {
        Some(l) => l,
        None => return vec![],
    };

    if list.is_loose() {
        // Already loose, nothing to do
        return vec![];
    }

    let mut edits = Vec::new();
    let items: Vec<_> = list.items().collect();

    // Insert blank line after each item except the last
    for (idx, item) in items.iter().enumerate() {
        if idx == items.len() - 1 {
            // Don't add blank line after last item
            continue;
        }

        // Find the end of this item (right after its last newline)
        let item_end = item.text_range().end().into();

        // Insert a blank line
        let position = offset_to_position(text, item_end);
        edits.push(TextEdit {
            range: Range {
                start: position,
                end: position,
            },
            new_text: "\n".to_string(),
        });
    }

    edits
}

/// Convert a loose list to a compact list by removing blank lines between items.
///
/// Returns a list of TextEdits that remove blank lines between list items.
pub fn convert_to_compact(list_node: &SyntaxNode, text: &str) -> Vec<TextEdit> {
    let list = match List::cast(list_node.clone()) {
        Some(l) => l,
        None => return vec![],
    };

    if list.is_compact() {
        // Already compact, nothing to do
        return vec![];
    }

    let mut edits = Vec::new();

    // Find all BLANK_LINE nodes that are between LIST_ITEM nodes
    let children: Vec<_> = list_node.children_with_tokens().collect();
    let mut prev_was_item = false;

    for (idx, child) in children.iter().enumerate() {
        if let Some(node) = child.as_node() {
            if node.kind() == SyntaxKind::LIST_ITEM {
                prev_was_item = true;
            } else if node.kind() == SyntaxKind::BLANK_LINE && prev_was_item {
                // Check if next non-blank node is also a LIST_ITEM
                let has_next_item = children[idx + 1..]
                    .iter()
                    .find(|c| {
                        c.as_node()
                            .map(|n| n.kind() != SyntaxKind::BLANK_LINE)
                            .unwrap_or(false)
                    })
                    .and_then(|c| c.as_node())
                    .map(|n| n.kind() == SyntaxKind::LIST_ITEM)
                    .unwrap_or(false);

                if has_next_item {
                    // Remove this blank line
                    let start = offset_to_position(text, node.text_range().start().into());
                    let end = offset_to_position(text, node.text_range().end().into());
                    edits.push(TextEdit {
                        range: Range { start, end },
                        new_text: String::new(),
                    });
                }
            }
        } else if let Some(token) = child.as_token()
            && token.kind() == SyntaxKind::BLANK_LINE
            && prev_was_item
        {
            // Check if next non-blank element is also a LIST_ITEM
            let has_next_item = children[idx + 1..]
                .iter()
                .find(|c| {
                    if let Some(n) = c.as_node() {
                        n.kind() != SyntaxKind::BLANK_LINE
                    } else if let Some(t) = c.as_token() {
                        t.kind() != SyntaxKind::BLANK_LINE
                    } else {
                        false
                    }
                })
                .and_then(|c| c.as_node())
                .map(|n| n.kind() == SyntaxKind::LIST_ITEM)
                .unwrap_or(false);

            if has_next_item {
                // Remove this blank line token
                let start = offset_to_position(text, token.text_range().start().into());
                let end = offset_to_position(text, token.text_range().end().into());
                edits.push(TextEdit {
                    range: Range { start, end },
                    new_text: String::new(),
                });
            }
        }
    }

    edits
}

/// Convert UTF-8 byte offset to LSP Position.
fn offset_to_position(text: &str, offset: usize) -> Position {
    let mut line = 0;
    let mut col = 0;
    let mut current_offset = 0;

    for ch in text.chars() {
        if current_offset >= offset {
            break;
        }

        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += ch.len_utf16() as u32;
        }

        current_offset += ch.len_utf8();
    }

    Position {
        line: line as u32,
        character: col,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn find_list_at_position_finds_innermost() {
        let input = "- Outer\n  - Inner\n  - Inner2\n- Outer2\n";
        let tree = parse(input, None);

        // Offset inside "Inner" (should find inner list)
        let offset = input.find("Inner").unwrap();
        let list = find_list_at_position(&tree, offset).expect("Should find list");
        let wrapped = List::cast(list).expect("Should cast to List");

        // The inner list has 2 items
        assert_eq!(wrapped.items().count(), 2);
    }

    #[test]
    fn convert_compact_to_loose() {
        let input = "- First\n- Second\n- Third\n";
        let tree = parse(input, None);

        let list_node = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::LIST)
            .expect("Should find list");

        let edits = convert_to_loose(&list_node, input);

        // Should have 2 edits (after first and second items)
        assert_eq!(edits.len(), 2);

        // First edit should insert after "First\n"
        assert_eq!(edits[0].new_text, "\n");
        assert_eq!(edits[0].range.start.line, 1);

        // Second edit should insert after "Second\n"
        assert_eq!(edits[1].new_text, "\n");
        assert_eq!(edits[1].range.start.line, 2);
    }

    #[test]
    fn convert_loose_to_compact() {
        let input = "- First\n\n- Second\n\n- Third\n";
        let tree = parse(input, None);

        let list_node = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::LIST)
            .expect("Should find list");

        let edits = convert_to_compact(&list_node, input);

        // Should have 2 edits (remove 2 blank lines)
        assert_eq!(edits.len(), 2);

        // Each edit should remove one blank line
        for edit in &edits {
            assert_eq!(edit.new_text, "");
            assert!(edit.range.start != edit.range.end);
        }
    }

    #[test]
    fn already_loose_returns_empty() {
        let input = "- First\n\n- Second\n";
        let tree = parse(input, None);

        let list_node = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::LIST)
            .expect("Should find list");

        let edits = convert_to_loose(&list_node, input);
        assert_eq!(edits.len(), 0, "Should return no edits for already loose");
    }

    #[test]
    fn already_compact_returns_empty() {
        let input = "- First\n- Second\n";
        let tree = parse(input, None);

        let list_node = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::LIST)
            .expect("Should find list");

        let edits = convert_to_compact(&list_node, input);
        assert_eq!(edits.len(), 0, "Should return no edits for already compact");
    }
}
