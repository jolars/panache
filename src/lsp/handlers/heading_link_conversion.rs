//! Heading link conversion utilities for code actions.

use crate::config::{Extensions, Flavor};
use crate::syntax::{AstNode, Heading, Link, SyntaxNode, UnresolvedReference};
use crate::utils::{implicit_heading_ids, normalize_label};
use lsp_types::{Range, TextEdit};

use super::super::conversions::offset_to_position;

/// Extract the shortcut label of a `[label]`-shape link node, or
/// `None` if the node isn't a shortcut form. Handles both `Link`
/// (when a refdef happened to resolve the shortcut) and
/// `UnresolvedReference` (when no refdef matches).
fn shortcut_label(node: &SyntaxNode) -> Option<String> {
    if let Some(link) = Link::cast(node.clone()) {
        if link.dest().is_some() || link.reference().is_some() {
            return None;
        }
        let text = link.text()?;
        let label = normalize_label(&text.raw_label());
        if label.is_empty() { None } else { Some(label) }
    } else if let Some(unresolved) = UnresolvedReference::cast(node.clone()) {
        if unresolved.is_image() || unresolved.label().is_some() {
            return None;
        }
        let label = normalize_label(&unresolved.raw_label());
        if label.is_empty() { None } else { Some(label) }
    } else {
        None
    }
}

/// Find an implicit heading shortcut link at the given position.
pub fn find_implicit_heading_link_at_position(
    tree: &SyntaxNode,
    offset: usize,
) -> Option<SyntaxNode> {
    let text_size = rowan::TextSize::from(offset as u32);
    let token = tree.token_at_offset(text_size).right_biased()?;
    token
        .parent_ancestors()
        .find(|node| shortcut_label(node).is_some())
}

/// Convert an implicit heading shortcut link (`[label]`) to explicit hash link (`[label](#slug)`).
pub fn convert_to_explicit_heading_link(
    link_node: &SyntaxNode,
    tree: &SyntaxNode,
    text: &str,
    extensions: &Extensions,
    flavor: Flavor,
) -> Vec<TextEdit> {
    let Some(normalized_label) = shortcut_label(link_node) else {
        return vec![];
    };

    let Some(entry) = implicit_heading_ids(tree, extensions, flavor)
        .into_iter()
        .find(|entry| {
            Heading::cast(entry.heading.clone())
                .map(|heading| normalize_label(&heading.raw_label()) == normalized_label)
                .unwrap_or(false)
        })
    else {
        return vec![];
    };

    let link_raw = link_node.text().to_string();
    if !link_raw.starts_with('[') || !link_raw.ends_with(']') {
        return vec![];
    }

    let replacement = format!("{}(#{})", link_raw, entry.id);
    let index = crate::lsp::line_index::LineIndex::new(text);
    let start = offset_to_position(&index, link_node.text_range().start().into());
    let end = offset_to_position(&index, link_node.text_range().end().into());
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

        let edits = convert_to_explicit_heading_link(
            &link_node,
            &tree,
            input,
            &crate::config::Extensions::default(),
            Flavor::Pandoc,
        );
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "[unordered lists](#unordered-lists)");
    }

    #[test]
    fn convert_raw_heading_reference_preserves_markup_and_visible_slug() {
        let input = "# *Title*\n\n[*Title*]\n";
        let tree = crate::parse(input, None);
        let offset = input.find("[*Title*]").unwrap() + 2;
        let node = find_implicit_heading_link_at_position(&tree, offset).unwrap();
        let edits = convert_to_explicit_heading_link(
            &node,
            &tree,
            input,
            &crate::config::Extensions::default(),
            Flavor::Pandoc,
        );
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "[*Title*](#title)");
    }

    #[test]
    fn preserves_raw_shortcut_label_identity_when_matching_headings() {
        let input = "# Title\n\n[<span>Title</span>]\n";
        let tree = crate::parse(input, None);
        let offset = input.find("<span>").unwrap();
        let node = find_implicit_heading_link_at_position(&tree, offset).unwrap();
        assert!(
            convert_to_explicit_heading_link(
                &node,
                &tree,
                input,
                &crate::config::Extensions::default(),
                Flavor::Pandoc,
            )
            .is_empty()
        );
    }
}
