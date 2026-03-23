//! Footnote conversion utilities for code actions.
//!
//! Provides functions to convert footnotes between inline and reference styles.

use crate::syntax::{AstNode, FootnoteDefinition, FootnoteReference, InlineFootnote, SyntaxNode};
use tower_lsp_server::ls_types::{Range, TextEdit};

use super::super::conversions::offset_to_position;

/// Find the innermost FOOTNOTE_REFERENCE node at the given position.
pub fn find_footnote_reference_at_position(tree: &SyntaxNode, offset: usize) -> Option<SyntaxNode> {
    find_ancestor_at_offset(tree, offset, FootnoteReference::cast)
        .map(|reference| reference.syntax().clone())
}

/// Find the innermost INLINE_FOOTNOTE node at the given position.
pub fn find_inline_footnote_at_position(tree: &SyntaxNode, offset: usize) -> Option<SyntaxNode> {
    find_ancestor_at_offset(tree, offset, InlineFootnote::cast)
        .map(|inline| inline.syntax().clone())
}

/// Check if a footnote reference can be converted to inline style.
/// Returns true if the corresponding definition exists and is simple.
pub fn can_convert_to_inline(reference_node: &SyntaxNode, tree: &SyntaxNode) -> bool {
    let reference = match FootnoteReference::cast(reference_node.clone()) {
        Some(r) => r,
        None => return false,
    };

    let id = reference.id();

    // Find the corresponding definition
    tree.descendants()
        .find_map(|node| FootnoteDefinition::cast(node).filter(|def| def.id() == id))
        .map(|def| def.is_simple())
        .unwrap_or(false)
}

/// Convert a reference footnote to inline style.
/// Returns TextEdits to:
/// 1. Replace [^id] with ^[content]
/// 2. Remove the footnote definition
pub fn convert_to_inline(
    reference_node: &SyntaxNode,
    tree: &SyntaxNode,
    text: &str,
) -> Vec<TextEdit> {
    let reference = match FootnoteReference::cast(reference_node.clone()) {
        Some(r) => r,
        None => return vec![],
    };

    let id = reference.id();

    // Find the corresponding definition
    let definition = match tree
        .descendants()
        .find_map(|node| FootnoteDefinition::cast(node).filter(|def| def.id() == id))
    {
        Some(def) => def,
        None => return vec![],
    };

    if !definition.is_simple() {
        return vec![];
    }

    let mut edits = Vec::new();

    // Edit 1: Replace [^id] with ^[content]
    let content = definition.content().trim().to_string();
    let ref_start = offset_to_position(text, reference_node.text_range().start().into());
    let ref_end = offset_to_position(text, reference_node.text_range().end().into());
    edits.push(TextEdit {
        range: Range {
            start: ref_start,
            end: ref_end,
        },
        new_text: format!("^[{}]", content),
    });

    // Edit 2: Remove the definition (including any trailing newlines)
    let def_node = definition.syntax();
    let def_start: usize = def_node.text_range().start().into();
    let def_end: usize = def_node.text_range().end().into();

    // Extend to include the newline after the definition
    let extended_end = if def_end < text.len() && text.as_bytes()[def_end] == b'\n' {
        def_end + 1
    } else {
        def_end
    };

    edits.push(TextEdit {
        range: Range {
            start: offset_to_position(text, def_start),
            end: offset_to_position(text, extended_end),
        },
        new_text: String::new(),
    });

    edits
}

/// Generate a new footnote ID by finding the next available number.
/// Scans existing FootnoteDefinition nodes and returns max + 1.
pub fn generate_footnote_id(tree: &SyntaxNode) -> String {
    let max_id = tree
        .descendants()
        .filter_map(FootnoteDefinition::cast)
        .filter_map(|def| def.id().parse::<u32>().ok())
        .max()
        .unwrap_or(0);

    (max_id + 1).to_string()
}

/// Convert an inline footnote to reference style.
/// Returns TextEdits to:
/// 1. Replace ^[content] with [^id]
/// 2. Insert definition at end of document
pub fn convert_to_reference(
    inline_node: &SyntaxNode,
    tree: &SyntaxNode,
    text: &str,
) -> Vec<TextEdit> {
    let inline = match InlineFootnote::cast(inline_node.clone()) {
        Some(i) => i,
        None => return vec![],
    };

    let content = inline.content();
    let id = generate_footnote_id(tree);

    let mut edits = Vec::new();

    // Edit 1: Replace ^[content] with [^id]
    let inline_start = offset_to_position(text, inline_node.text_range().start().into());
    let inline_end = offset_to_position(text, inline_node.text_range().end().into());
    edits.push(TextEdit {
        range: Range {
            start: inline_start,
            end: inline_end,
        },
        new_text: format!("[^{}]", id),
    });

    // Edit 2: Insert definition at end of document
    // Find the last FootnoteDefinition to insert after it, or insert at end
    let insert_position = tree
        .descendants()
        .filter_map(FootnoteDefinition::cast)
        .last()
        .map(|def| {
            let end: usize = def.syntax().text_range().end().into();
            offset_to_position(text, end)
        })
        .unwrap_or_else(|| {
            // No existing definitions, insert at end of document
            offset_to_position(text, text.len())
        });

    // Determine if we need leading newlines
    let prefix = if tree
        .descendants()
        .filter_map(FootnoteDefinition::cast)
        .next()
        .is_some()
    {
        // There are existing definitions, just add a newline before our definition
        "\n"
    } else {
        // No existing definitions, add two newlines to separate from content
        "\n\n"
    };

    edits.push(TextEdit {
        range: Range {
            start: insert_position,
            end: insert_position,
        },
        new_text: format!("{}[^{}]: {}\n", prefix, id, content),
    });

    edits
}

fn find_ancestor_at_offset<T: AstNode<Language = crate::syntax::PanacheLanguage>>(
    tree: &SyntaxNode,
    offset: usize,
    cast: fn(SyntaxNode) -> Option<T>,
) -> Option<T> {
    let text_size = rowan::TextSize::from(offset as u32);
    let token = tree.token_at_offset(text_size).right_biased()?;
    token.parent_ancestors().find_map(cast)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn find_footnote_reference_at_cursor() {
        let input = "Text [^1] more text.\n\n[^1]: Footnote content.";
        let tree = parse(input, None);

        // Position inside [^1]
        let offset = input.find("[^1]").unwrap() + 2;
        let node =
            find_footnote_reference_at_position(&tree, offset).expect("Should find reference");
        assert!(FootnoteReference::cast(node).is_some());
    }

    #[test]
    fn find_inline_footnote_at_cursor() {
        let input = "Text^[Inline note] more text.";
        let tree = parse(input, None);

        // Position inside inline footnote
        let offset = input.find("Inline").unwrap();
        let node = find_inline_footnote_at_position(&tree, offset).expect("Should find inline");
        assert!(InlineFootnote::cast(node).is_some());
    }

    #[test]
    fn can_convert_simple_footnote() {
        let input = "Text [^1] more.\n\n[^1]: Simple footnote.";
        let tree = parse(input, None);

        let ref_node = tree
            .descendants()
            .find_map(FootnoteReference::cast)
            .map(|reference| reference.syntax().clone())
            .unwrap();

        assert!(can_convert_to_inline(&ref_node, &tree));
    }

    #[test]
    fn cannot_convert_complex_footnote() {
        let input = "Text [^1] more.\n\n[^1]: First para.\n\n    Second para.";
        let tree = parse(input, None);

        let ref_node = tree
            .descendants()
            .find_map(FootnoteReference::cast)
            .map(|reference| reference.syntax().clone())
            .unwrap();

        assert!(!can_convert_to_inline(&ref_node, &tree));
    }

    #[test]
    fn test_convert_reference_to_inline() {
        let input = "Text [^1] more.\n\n[^1]: Simple note.";
        let tree = parse(input, None);

        let ref_node = tree
            .descendants()
            .find_map(FootnoteReference::cast)
            .map(|reference| reference.syntax().clone())
            .unwrap();

        let edits = convert_to_inline(&ref_node, &tree, input);

        // Should have 2 edits: replace reference, remove definition
        assert_eq!(edits.len(), 2);
        assert!(edits[0].new_text.contains("^[Simple note."));
        assert_eq!(edits[1].new_text, "");
    }

    #[test]
    fn test_generate_footnote_id() {
        let input = "[^1]: First.\n[^2]: Second.\n[^5]: Fifth.";
        let tree = parse(input, None);

        let id = generate_footnote_id(&tree);
        assert_eq!(id, "6"); // Next after max (5)
    }

    #[test]
    fn test_generate_footnote_id_no_existing() {
        let input = "Just text.";
        let tree = parse(input, None);

        let id = generate_footnote_id(&tree);
        assert_eq!(id, "1"); // First ID
    }

    #[test]
    fn test_convert_inline_to_reference() {
        let input = "Text^[Inline note] more.";
        let tree = parse(input, None);

        let inline_node = tree
            .descendants()
            .find_map(InlineFootnote::cast)
            .map(|inline| inline.syntax().clone())
            .unwrap();

        let edits = convert_to_reference(&inline_node, &tree, input);

        // Should have 2 edits: replace inline, insert definition
        assert_eq!(edits.len(), 2);
        assert!(edits[0].new_text.contains("[^1]"));
        assert!(edits[1].new_text.contains("[^1]: Inline note"));
    }

    #[test]
    fn test_convert_inline_to_reference_with_existing() {
        let input = "Text^[New note] more.\n\n[^1]: Existing.";
        let tree = parse(input, None);

        let inline_node = tree
            .descendants()
            .find_map(InlineFootnote::cast)
            .map(|inline| inline.syntax().clone())
            .unwrap();

        let edits = convert_to_reference(&inline_node, &tree, input);

        // Should generate ID 2 (next after 1)
        assert_eq!(edits.len(), 2);
        assert!(edits[0].new_text.contains("[^2]"));
        assert!(edits[1].new_text.contains("[^2]: New note"));
    }
}
