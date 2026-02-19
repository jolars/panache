//! Handler for textDocument/hover LSP requests.
//!
//! Provides hover information for:
//! - Footnote references: `[^id]` → shows footnote content from `[^id]: content`

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use crate::lsp::DocumentState;
use crate::parser::parse;
use crate::syntax::{AstNode, FootnoteDefinition};

use super::super::{conversions, helpers};

/// Handle textDocument/hover request
pub(crate) async fn hover(
    client: &tower_lsp_server::Client,
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    workspace_root: Arc<Mutex<Option<PathBuf>>>,
    params: HoverParams,
) -> Result<Option<Hover>> {
    let uri = &params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;

    // Get document content and config
    let Some((content, config)) =
        helpers::get_document_and_config(client, &document_map, &workspace_root, uri).await
    else {
        return Ok(None);
    };

    // Parse the document
    let root = parse(&content, Some(config));

    // Convert LSP position to byte offset
    let Some(offset) = conversions::position_to_offset(&content, position) else {
        return Ok(None);
    };

    // Find the node at this offset
    let Some(mut node) = helpers::find_node_at_offset(&root, offset) else {
        return Ok(None);
    };

    // Walk up the tree to find a footnote reference
    loop {
        if let Some((label, is_footnote)) = helpers::extract_reference_label(&node) {
            // Only handle footnotes (not regular references)
            if is_footnote {
                // Find the footnote definition
                let Some(definition) = helpers::find_definition_node(&root, &label, true) else {
                    return Ok(None);
                };

                // Cast to FootnoteDefinition wrapper
                let Some(footnote_def) = FootnoteDefinition::cast(definition) else {
                    return Ok(None);
                };

                // Extract content
                let content = footnote_def.content();
                let trimmed = content.trim();

                // Return hover with markdown content
                return Ok(Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: trimmed.to_string(),
                    }),
                    range: None,
                }));
            }
        }

        // Move up to parent, or return None if at root
        match node.parent() {
            Some(parent) => node = parent,
            None => return Ok(None),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;
    use crate::syntax::AstNode;

    #[test]
    fn test_hover_on_footnote_reference() {
        let input = "Text with footnote[^1]\n\n[^1]: This is the footnote content.";
        let root = parse(input, None);

        // Find the footnote reference node
        let footnote_ref = root
            .descendants()
            .find(|n| n.kind() == crate::syntax::SyntaxKind::FOOTNOTE_REFERENCE)
            .expect("Should find footnote reference");

        // Extract the label
        let (label, is_footnote) =
            helpers::extract_reference_label(&footnote_ref).expect("Should extract label");
        assert_eq!(label, "1");
        assert!(is_footnote);

        // Find the definition
        let definition =
            helpers::find_definition_node(&root, &label, true).expect("Should find definition");

        // Cast to FootnoteDefinition
        let footnote_def =
            FootnoteDefinition::cast(definition).expect("Should cast to FootnoteDefinition");

        // Extract content
        let content = footnote_def.content();
        assert!(content.contains("This is the footnote content"));
    }

    #[test]
    fn test_hover_multiline_footnote() {
        let input = "Text[^1]\n\n[^1]: First line\n    Second line";
        let root = parse(input, None);

        // Find the definition
        let definition =
            helpers::find_definition_node(&root, "1", true).expect("Should find definition");

        let footnote_def =
            FootnoteDefinition::cast(definition).expect("Should cast to FootnoteDefinition");

        let content = footnote_def.content();
        assert!(content.contains("First line"));
        assert!(content.contains("Second line"));
    }

    #[test]
    fn test_no_definition_found() {
        let input = "Text with footnote[^missing]";
        let root = parse(input, None);

        let definition = helpers::find_definition_node(&root, "missing", true);
        assert!(definition.is_none());
    }

    #[test]
    fn test_footnote_with_formatting() {
        let input = "[^1]: Text with *emphasis* and `code`.";
        let root = parse(input, None);

        let definition =
            helpers::find_definition_node(&root, "1", true).expect("Should find definition");

        let footnote_def =
            FootnoteDefinition::cast(definition).expect("Should cast to FootnoteDefinition");

        let content = footnote_def.content();
        assert!(content.contains("*emphasis*"));
        assert!(content.contains("`code`"));
    }
}
