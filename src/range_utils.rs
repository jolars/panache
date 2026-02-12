use crate::syntax::{SyntaxKind, SyntaxNode};
use crate::utils::is_block_element;

/// Convert 1-indexed line range to byte offsets
pub fn line_range_to_byte_offsets(
    text: &str,
    start_line: usize,
    end_line: usize,
) -> Option<(usize, usize)> {
    if start_line == 0 || end_line == 0 || start_line > end_line {
        return None;
    }

    let mut current_line = 1;
    let mut start_offset = None;
    let mut byte_offset = 0;

    for line in text.split_inclusive('\n') {
        if current_line == start_line {
            start_offset = Some(byte_offset);
        }

        if current_line == end_line {
            // End offset is at the end of the end_line (inclusive)
            let end_offset = byte_offset + line.len();
            return start_offset.map(|start| (start, end_offset));
        }

        byte_offset += line.len();
        current_line += 1;
    }

    // If we reached end of document
    if current_line == end_line + 1 && start_offset.is_some() {
        // end_line was the last line
        return start_offset.map(|start| (start, byte_offset));
    }

    // end_line is beyond document
    None
}

/// Find the smallest block-level node containing the given offset
fn find_enclosing_block(node: &SyntaxNode, offset: usize) -> Option<SyntaxNode> {
    let text_offset = rowan::TextSize::try_from(offset).ok()?;

    // Start with the node at this offset
    let token = node.token_at_offset(text_offset).right_biased()?;
    let mut current = token.parent()?;

    // Walk up the tree to find the smallest block element
    loop {
        if is_block_element(current.kind()) {
            return Some(current);
        }

        current = current.parent()?;
    }
}

/// Check if a node or any of its ancestors is a container that should be expanded as a unit
fn find_expandable_container(node: &SyntaxNode) -> Option<SyntaxNode> {
    let mut current = node.clone();

    loop {
        match current.kind() {
            // Lists should be formatted as a whole unit
            SyntaxKind::List => return Some(current),
            // BlockQuotes should be formatted as a whole unit
            SyntaxKind::BlockQuote => return Some(current),
            // FencedDivs should be formatted as a whole unit
            SyntaxKind::FencedDiv => return Some(current),
            // Definition lists should be formatted as a whole unit
            SyntaxKind::DefinitionList => return Some(current),
            // Line blocks should be formatted as a whole unit
            SyntaxKind::LineBlock => return Some(current),
            _ => {}
        }

        current = current.parent()?;
    }
}

/// Expand a byte range to encompass complete block-level elements (internal helper).
///
/// This ensures that formatting doesn't split blocks mid-content and that
/// context-dependent formatting (lists, blockquotes) works correctly.
///
/// # Arguments
/// * `tree` - The syntax tree root
/// * `start` - Starting byte offset (inclusive)
/// * `end` - Ending byte offset (exclusive)
///
/// # Returns
/// Expanded byte range `(start, end)` that covers complete blocks
fn expand_byte_range_to_blocks(tree: &SyntaxNode, start: usize, end: usize) -> (usize, usize) {
    // Handle empty or invalid ranges
    if start >= end {
        // Treat as cursor position - find enclosing block
        if let Some(block) = find_enclosing_block(tree, start) {
            let range = block.text_range();
            return (range.start().into(), range.end().into());
        }
        return (start, start);
    }

    // Find blocks at start and end positions
    let start_block = find_enclosing_block(tree, start);
    let end_block = find_enclosing_block(tree, end.saturating_sub(1)); // end is exclusive

    let (mut expanded_start, mut expanded_end) = match (start_block, end_block) {
        (Some(start_node), Some(end_node)) => {
            let start_range = start_node.text_range();
            let end_range = end_node.text_range();
            (start_range.start().into(), end_range.end().into())
        }
        (Some(start_node), None) => {
            // Only start is in a block
            let range = start_node.text_range();
            (range.start().into(), end)
        }
        (None, Some(end_node)) => {
            // Only end is in a block
            let range = end_node.text_range();
            (start, range.end().into())
        }
        (None, None) => {
            // Neither position is in a block (shouldn't normally happen)
            return (start, end);
        }
    };

    // Check if we need to expand to encompass parent containers
    // This handles cases where the range touches list items, blockquotes, etc.
    if let Some(start_node) = find_enclosing_block(tree, expanded_start)
        && let Some(container) = find_expandable_container(&start_node)
    {
        let container_range = container.text_range();
        expanded_start = expanded_start.min(container_range.start().into());
        expanded_end = expanded_end.max(container_range.end().into());
    }

    if let Some(end_node) = find_enclosing_block(tree, expanded_end.saturating_sub(1))
        && let Some(container) = find_expandable_container(&end_node)
    {
        let container_range = container.text_range();
        expanded_start = expanded_start.min(container_range.start().into());
        expanded_end = expanded_end.max(container_range.end().into());
    }

    (expanded_start, expanded_end)
}

/// Expand a 1-indexed line range to encompass complete block-level elements.
///
/// This is the public API for range formatting. It converts line numbers to byte offsets,
/// expands to block boundaries, and returns the expanded byte range.
///
/// # Arguments
/// * `tree` - The syntax tree root
/// * `text` - The original document text
/// * `start_line` - Starting line number (1-indexed, inclusive)
/// * `end_line` - Ending line number (1-indexed, inclusive)
///
/// # Returns
/// Expanded byte range `(start, end)` that covers complete blocks, or None if range is invalid
pub fn expand_line_range_to_blocks(
    tree: &SyntaxNode,
    text: &str,
    start_line: usize,
    end_line: usize,
) -> Option<(usize, usize)> {
    let (start, end) = line_range_to_byte_offsets(text, start_line, end_line)?;
    Some(expand_byte_range_to_blocks(tree, start, end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn parse_test_doc(input: &str) -> SyntaxNode {
        crate::parse(input, Some(Config::default()))
    }

    #[test]
    fn test_line_range_to_byte_offsets() {
        let doc = "Line 1\nLine 2\nLine 3\n";

        // Line 1 (1-indexed)
        let (start, end) = line_range_to_byte_offsets(doc, 1, 1).unwrap();
        assert_eq!(&doc[start..end], "Line 1\n");

        // Line 2
        let (start, end) = line_range_to_byte_offsets(doc, 2, 2).unwrap();
        assert_eq!(&doc[start..end], "Line 2\n");

        // Lines 1-2
        let (start, end) = line_range_to_byte_offsets(doc, 1, 2).unwrap();
        assert_eq!(&doc[start..end], "Line 1\nLine 2\n");

        // Invalid ranges
        assert!(line_range_to_byte_offsets(doc, 0, 1).is_none()); // 0-indexed not allowed
        assert!(line_range_to_byte_offsets(doc, 2, 1).is_none()); // start > end
        assert!(line_range_to_byte_offsets(doc, 1, 10).is_none()); // beyond document
    }

    #[test]
    fn test_expand_single_paragraph() {
        let doc = "Para 1\n\nPara 2\n\nPara 3\n";
        let tree = parse_test_doc(doc);

        // Select line 3 (Para 2)
        let (start, end) = expand_line_range_to_blocks(&tree, doc, 3, 3).unwrap();

        let selected = &doc[start..end];
        assert!(selected.contains("Para 2"), "Range should include Para 2");
        assert!(
            !selected.contains("Para 1"),
            "Range should not include Para 1"
        );
        assert!(
            !selected.contains("Para 3"),
            "Range should not include Para 3"
        );
    }

    #[test]
    fn test_expand_code_block() {
        let doc = "Text before\n\n```rust\nfn main() {}\n```\n\nText after\n";
        let tree = parse_test_doc(doc);

        // Line 3 is "```rust", line 4 is "fn main() {}", line 5 is "```"
        // Select line 4 (inside code block)
        let (start, end) = expand_line_range_to_blocks(&tree, doc, 4, 4).unwrap();

        // Should expand to entire code block
        let selected = &doc[start..end];
        assert!(
            selected.contains("```rust"),
            "Range should include opening fence"
        );
        assert!(
            selected.contains("fn main() {}"),
            "Range should include code"
        );
        assert!(
            selected.contains("```"),
            "Range should include closing fence"
        );
        assert!(
            !selected.contains("Text before"),
            "Range should not include text before"
        );
        assert!(
            !selected.contains("Text after"),
            "Range should not include text after"
        );
    }

    #[test]
    fn test_expand_list_item_to_full_list() {
        let doc = "Before\n\n- Item 1\n- Item 2\n- Item 3\n\nAfter\n";
        let tree = parse_test_doc(doc);

        // Line 4 is "- Item 2"
        let (start, end) = expand_line_range_to_blocks(&tree, doc, 4, 4).unwrap();

        // Should expand to entire list (all items)
        let selected = &doc[start..end];
        assert!(selected.contains("Item 1"), "Range should include Item 1");
        assert!(selected.contains("Item 2"), "Range should include Item 2");
        assert!(selected.contains("Item 3"), "Range should include Item 3");
        assert!(
            !selected.contains("Before"),
            "Range should not include Before"
        );
        assert!(
            !selected.contains("After"),
            "Range should not include After"
        );
    }

    #[test]
    fn test_single_line_expands_to_block() {
        let doc = "# Heading\n\nParagraph text here.\n";
        let tree = parse_test_doc(doc);

        // Line 3 is "Paragraph text here."
        let (start, end) = expand_line_range_to_blocks(&tree, doc, 3, 3).unwrap();

        // Should expand to entire paragraph
        let selected = &doc[start..end];
        assert!(
            selected.contains("Paragraph text here."),
            "Range should include paragraph"
        );
        assert!(
            !selected.contains("Heading"),
            "Range should not include heading"
        );
    }

    #[test]
    #[ignore]
    fn test_expand_blockquote() {
        let doc = "Before\n\n> Line 1\n> Line 2\n> Line 3\n\nAfter\n";
        let tree = parse_test_doc(doc);

        // Line 4 is "> Line 2"
        let result = expand_line_range_to_blocks(&tree, doc, 4, 4);
        assert!(result.is_some(), "Failed to expand range for line 4");
        let (start, end) = result.unwrap();

        // Should expand to entire blockquote (note: parser strips "> " markers)
        // So the range will be "Line 1\nLine 2\nLine 3\n" without markers
        let selected = &doc[start..end];

        // The range should include all three lines
        assert!(selected.contains("Line 1"), "Range should include Line 1");
        assert!(selected.contains("Line 2"), "Range should include Line 2");
        assert!(selected.contains("Line 3"), "Range should include Line 3");
        assert!(
            !selected.contains("Before"),
            "Range should not include Before"
        );
        assert!(
            !selected.contains("After"),
            "Range should not include After"
        );

        // Verify it's the BlockQuote range (8-35 in parsed tree, includes markers now)
        assert_eq!(start, 8, "Should start at BlockQuote");
        assert_eq!(end, 35, "Should end at BlockQuote");
    }
}
