use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::ls_types::Uri;

use crate::Config;
use crate::lsp::DocumentState;
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::{NodeOrToken, TextRange, TextSize};

use super::config::load_config;

/// Helper to get document content from the document map
pub(crate) async fn get_document_content(
    document_map: &Arc<Mutex<HashMap<String, DocumentState>>>,
    uri: &Uri,
) -> Option<String> {
    let doc_map = document_map.lock().await;
    doc_map
        .get(&uri.to_string())
        .map(|state| state.text.clone())
}

/// Helper to load config with URI-based flavor detection
pub(crate) async fn get_config(
    client: &tower_lsp_server::Client,
    workspace_root: &Arc<Mutex<Option<PathBuf>>>,
    uri: &Uri,
) -> Config {
    let workspace_root = workspace_root.lock().await.clone();
    load_config(client, &workspace_root, Some(uri)).await
}

/// Combined helper: get document and config in one call
pub(crate) async fn get_document_and_config(
    client: &tower_lsp_server::Client,
    document_map: &Arc<Mutex<HashMap<String, DocumentState>>>,
    workspace_root: &Arc<Mutex<Option<PathBuf>>>,
    uri: &Uri,
) -> Option<(String, Config)> {
    let content = get_document_content(document_map, uri).await?;
    let config = get_config(client, workspace_root, uri).await;
    Some((content, config))
}

/// Find the syntax node at the given byte offset
pub(crate) fn find_node_at_offset(root: &SyntaxNode, offset: usize) -> Option<SyntaxNode> {
    let text_size = TextSize::from(offset as u32);
    let range = TextRange::new(text_size, text_size);
    match root.covering_element(range) {
        NodeOrToken::Node(node) => Some(node),
        NodeOrToken::Token(token) => token.parent(),
    }
}

/// Normalize a label for case-insensitive matching (collapses whitespace, lowercases)
fn normalize_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Extract the reference label from a LinkRef or FootnoteReference node
pub(crate) fn extract_reference_label(node: &SyntaxNode) -> Option<(String, bool)> {
    match node.kind() {
        SyntaxKind::LinkRef => {
            // LinkRef contains TEXT child with the label
            let text = node
                .children_with_tokens()
                .filter_map(|child| child.into_token())
                .filter(|token| token.kind() == SyntaxKind::TEXT)
                .map(|token| token.text().to_string())
                .collect::<String>();
            Some((normalize_label(&text), false))
        }
        SyntaxKind::FootnoteReference => {
            // FootnoteReference has TEXT children: "[^", "id", "]"
            // Extract the middle TEXT token (the ID)
            let tokens: Vec<_> = node
                .children_with_tokens()
                .filter_map(|child| child.into_token())
                .filter(|token| token.kind() == SyntaxKind::TEXT)
                .map(|token| token.text().to_string())
                .collect();

            if tokens.len() >= 2 && tokens[0] == "[^" {
                // The ID is in the second token
                let id = &tokens[1];
                Some((normalize_label(id), true))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Extract the label from a definition node (ReferenceDefinition or FootnoteDefinition)
fn extract_definition_label(node: &SyntaxNode) -> Option<String> {
    match node.kind() {
        SyntaxKind::ReferenceDefinition => {
            // ReferenceDefinition has a Link child with LinkText containing the label
            node.children()
                .find(|child| child.kind() == SyntaxKind::Link)
                .and_then(|link| {
                    link.children()
                        .find(|child| child.kind() == SyntaxKind::LinkText)
                })
                .map(|link_text| {
                    let text = link_text
                        .children_with_tokens()
                        .filter_map(|child| child.into_token())
                        .filter(|token| token.kind() == SyntaxKind::TEXT)
                        .map(|token| token.text().to_string())
                        .collect::<String>();
                    normalize_label(&text)
                })
        }
        SyntaxKind::FootnoteDefinition => {
            // FootnoteDefinition has a FootnoteReference token with text like "[^1]: "
            node.children_with_tokens()
                .filter_map(|child| child.into_token())
                .find(|token| token.kind() == SyntaxKind::FootnoteReference)
                .and_then(|token| {
                    let text = token.text();
                    // Extract ID from "[^id]: " format
                    if text.starts_with("[^") && text.contains("]:") {
                        let id = text.trim_start_matches("[^").split(']').next()?;
                        Some(normalize_label(id))
                    } else {
                        None
                    }
                })
        }
        _ => None,
    }
}

/// Find a definition node matching the given label
pub(crate) fn find_definition_node(
    root: &SyntaxNode,
    label: &str,
    is_footnote: bool,
) -> Option<SyntaxNode> {
    let target_kind = if is_footnote {
        SyntaxKind::FootnoteDefinition
    } else {
        SyntaxKind::ReferenceDefinition
    };

    root.descendants().find(|node| {
        node.kind() == target_kind && extract_definition_label(node).as_deref() == Some(label)
    })
}

/// Find the definition for a reference at the given offset
/// Returns the TextRange of the definition if found
pub(crate) fn find_definition_at_offset(root: &SyntaxNode, offset: usize) -> Option<TextRange> {
    // Find the node at this offset
    let mut node = find_node_at_offset(root, offset)?;

    // Walk up the tree to find a reference node
    loop {
        if let Some((label, is_footnote)) = extract_reference_label(&node) {
            // Found a reference - now find its definition
            let definition = find_definition_node(root, &label, is_footnote)?;
            return Some(definition.text_range());
        }

        // Check if this is a Link that might contain a LinkRef
        if node.kind() == SyntaxKind::Link
            && let Some(link_ref) = node
                .children()
                .find(|child| child.kind() == SyntaxKind::LinkRef)
            && let Some((label, is_footnote)) = extract_reference_label(&link_ref)
        {
            let definition = find_definition_node(root, &label, is_footnote)?;
            return Some(definition.text_range());
        }

        // Check if this is an ImageLink that might contain a LinkRef
        if node.kind() == SyntaxKind::ImageLink
            && let Some(link_ref) = node
                .children()
                .find(|child| child.kind() == SyntaxKind::LinkRef)
            && let Some((label, is_footnote)) = extract_reference_label(&link_ref)
        {
            let definition = find_definition_node(root, &label, is_footnote)?;
            return Some(definition.text_range());
        }

        // Move up to parent
        node = node.parent()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to parse a document for testing
    fn parse(input: &str) -> SyntaxNode {
        crate::parse(input, None)
    }

    #[test]
    fn test_find_node_at_offset() {
        let root = parse("[text][ref]");

        // Offset 0: at "["
        let node = find_node_at_offset(&root, 0);
        assert!(node.is_some());

        // Offset 7: at "r" in "ref"
        let node = find_node_at_offset(&root, 7);
        assert!(node.is_some());
    }

    #[test]
    fn test_normalize_label() {
        assert_eq!(normalize_label("Foo"), "foo");
        assert_eq!(normalize_label("foo bar"), "foo bar");
        assert_eq!(normalize_label("foo  bar"), "foo bar");
        assert_eq!(normalize_label(" foo bar "), "foo bar");
    }

    #[test]
    fn test_extract_reference_label_from_link_ref() {
        let root = parse("[text][ref]");
        let link_ref = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::LinkRef)
            .expect("Should find LinkRef");

        let (label, is_footnote) =
            extract_reference_label(&link_ref).expect("Should extract label");
        assert_eq!(label, "ref");
        assert!(!is_footnote);
    }

    #[test]
    fn test_extract_reference_label_from_footnote() {
        let root = parse("[^1]");
        let footnote_ref = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::FootnoteReference)
            .expect("Should find FootnoteReference");

        let (label, is_footnote) =
            extract_reference_label(&footnote_ref).expect("Should extract label");
        assert_eq!(label, "1");
        assert!(is_footnote);
    }

    #[test]
    fn test_extract_definition_label_from_reference() {
        let root = parse("[ref]: /url");
        let def = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::ReferenceDefinition)
            .expect("Should find ReferenceDefinition");

        let label = extract_definition_label(&def).expect("Should extract label");
        assert_eq!(label, "ref");
    }

    #[test]
    fn test_extract_definition_label_from_footnote() {
        let root = parse("[^1]: content");
        let def = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::FootnoteDefinition)
            .expect("Should find FootnoteDefinition");

        let label = extract_definition_label(&def).expect("Should extract label");
        assert_eq!(label, "1");
    }

    #[test]
    fn test_find_definition_node_reference() {
        let root = parse("[text][ref]\n\n[ref]: /url");
        let def = find_definition_node(&root, "ref", false);
        assert!(def.is_some());
        assert_eq!(def.unwrap().kind(), SyntaxKind::ReferenceDefinition);
    }

    #[test]
    fn test_find_definition_node_case_insensitive() {
        let root = parse("[text][REF]\n\n[ref]: /url");
        let def = find_definition_node(&root, "ref", false);
        assert!(def.is_some());
    }

    #[test]
    fn test_find_definition_node_footnote() {
        let root = parse("Text[^1]\n\n[^1]: content");
        let def = find_definition_node(&root, "1", true);
        assert!(def.is_some());
        assert_eq!(def.unwrap().kind(), SyntaxKind::FootnoteDefinition);
    }

    #[test]
    fn test_find_definition_node_not_found() {
        let root = parse("[text][ref]");
        let def = find_definition_node(&root, "ref", false);
        assert!(def.is_none());
    }

    #[test]
    fn test_find_definition_at_offset_reference_link() {
        let input = "[text][ref]\n\n[ref]: /url";
        let root = parse(input);

        // Offset 7: at "r" in [ref]
        let range = find_definition_at_offset(&root, 7);
        assert!(range.is_some());

        let range = range.unwrap();
        let def_text = &input[range.start().into()..range.end().into()];
        assert!(def_text.contains("[ref]: /url"));
    }

    #[test]
    fn test_find_definition_at_offset_footnote() {
        let input = "Text[^1]\n\n[^1]: content";
        let root = parse(input);

        // Offset 5: at "[^1]"
        let range = find_definition_at_offset(&root, 5);
        assert!(range.is_some());

        let range = range.unwrap();
        let def_text = &input[range.start().into()..range.end().into()];
        assert!(def_text.contains("[^1]:"));
    }

    #[test]
    fn test_find_definition_at_offset_not_on_reference() {
        let root = parse("Just some text");
        let range = find_definition_at_offset(&root, 0);
        assert!(range.is_none());
    }

    #[test]
    fn test_find_definition_at_offset_reference_not_found() {
        let root = parse("[text][ref]");
        // Even though we're on a reference, there's no definition
        let range = find_definition_at_offset(&root, 7);
        assert!(range.is_none());
    }

    #[test]
    fn test_find_definition_whitespace_normalization() {
        let input = "[text][foo  bar]\n\n[foo bar]: /url";
        let root = parse(input);

        // Offset 7: at "foo  bar" reference
        let range = find_definition_at_offset(&root, 7);
        assert!(range.is_some());
    }
}
