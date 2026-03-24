//! Heading link conversion utilities for code actions.

use crate::syntax::{AstNode, Heading, Link, SyntaxNode};
use crate::utils::{implicit_heading_ids, normalize_label};
use tower_lsp_server::ls_types::{Range, TextEdit};

use super::super::conversions::offset_to_position;

/// Find an implicit heading shortcut link at the given position.
pub fn find_implicit_heading_link_at_position(
    tree: &SyntaxNode,
    offset: usize,
) -> Option<SyntaxNode> {
    let text_size = rowan::TextSize::from(offset as u32);
    let token = tree.token_at_offset(text_size).right_biased()?;
    token.parent_ancestors().find_map(|node| {
        let link = Link::cast(node)?;
        if link.dest().is_some() || link.reference().is_some() {
            return None;
        }
        let text = link.text()?;
        if normalize_label(&text.text_content()).is_empty() {
            return None;
        }
        Some(link.syntax().clone())
    })
}

/// Convert an implicit heading shortcut link (`[label]`) to explicit hash link (`[label](#slug)`).
pub fn convert_to_explicit_heading_link(
    link_node: &SyntaxNode,
    tree: &SyntaxNode,
    text: &str,
) -> Vec<TextEdit> {
    let Some(link) = Link::cast(link_node.clone()) else {
        return vec![];
    };
    if link.dest().is_some() || link.reference().is_some() {
        return vec![];
    }

    let Some(link_text) = link.text() else {
        return vec![];
    };
    let normalized_label = normalize_label(&link_text.text_content());
    if normalized_label.is_empty() {
        return vec![];
    }

    let Some(entry) = implicit_heading_ids(tree).into_iter().find(|entry| {
        Heading::cast(entry.heading.clone())
            .map(|heading| normalize_label(&heading.text()) == normalized_label)
            .unwrap_or(false)
    }) else {
        return vec![];
    };

    let link_raw = link_node.text().to_string();
    if !link_raw.starts_with('[') || !link_raw.ends_with(']') {
        return vec![];
    }

    let replacement = format!("{}(#{})", link_raw, entry.id);
    let start = offset_to_position(text, link_node.text_range().start().into());
    let end = offset_to_position(text, link_node.text_range().end().into());
    vec![TextEdit {
        range: Range { start, end },
        new_text: replacement,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_implicit_heading_link_at_cursor() {
        let input = "# Heading\n\nSee [heading].\n";
        let tree = crate::parse(input, None);
        let offset = input.find("heading]").expect("link label") + 1;
        let node = find_implicit_heading_link_at_position(&tree, offset);
        assert!(node.is_some());
    }

    #[test]
    fn convert_to_explicit_heading_link_uses_pandoc_slug() {
        let input = "# Unordered Lists\n\n[unordered lists]\n";
        let tree = crate::parse(input, None);
        let offset = input.find("unordered").expect("link label");
        let link_node = find_implicit_heading_link_at_position(&tree, offset).expect("link node");

        let edits = convert_to_explicit_heading_link(&link_node, &tree, input);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "[unordered lists](#unordered-lists)");
    }
}
