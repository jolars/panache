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
use crate::metadata::inline_reference_contains;
use crate::syntax::{AstNode, FootnoteDefinition};

use super::super::{conversions, helpers};

/// Handle textDocument/hover request
pub(crate) async fn hover(
    _client: &tower_lsp_server::Client,
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    salsa_db: Arc<Mutex<crate::salsa::SalsaDb>>,
    _workspace_root: Arc<Mutex<Option<PathBuf>>>,
    params: HoverParams,
) -> Result<Option<Hover>> {
    let uri = &params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;

    let metadata = {
        let map = document_map.lock().await;
        map.get(&uri.to_string())
            .and_then(|state| state.metadata.clone())
    };

    let pending_footnote = {
        let Some((content, root)) =
            helpers::get_document_content_and_tree(&document_map, &salsa_db, uri).await
        else {
            return Ok(None);
        };

        // Convert LSP position to byte offset
        let Some(offset) = conversions::position_to_offset(&content, position) else {
            return Ok(None);
        };

        // Find the node at this offset
        let Some(mut node) = helpers::find_node_at_offset(&root, offset) else {
            return Ok(None);
        };

        // Walk up the tree to find a footnote reference or citation
        loop {
            if let Some((label, is_footnote)) = helpers::extract_reference_label(&node) {
                // Only handle footnotes (not regular references)
                if is_footnote {
                    // First try local definition (no cross-document index needed)
                    if let Some(footnote_def) = helpers::find_definition_node(&root, &label, true)
                        .and_then(FootnoteDefinition::cast)
                    {
                        let content = footnote_def.content();
                        let trimmed = content.trim();
                        return Ok(Some(Hover {
                            contents: HoverContents::Markup(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: trimmed.to_string(),
                            }),
                            range: None,
                        }));
                    }

                    break Some(label);
                }
            }

            if let Some(key) = helpers::extract_citation_key(&node)
                && let Some(metadata) = metadata.clone()
            {
                if let Some(parse) = metadata.bibliography_parse
                    && let Some(entry) = parse.index.get(&key)
                {
                    let summary = format_bibliography_entry(entry);
                    if !summary.is_empty() {
                        return Ok(Some(Hover {
                            contents: HoverContents::Markup(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: summary,
                            }),
                            range: None,
                        }));
                    }
                }

                if inline_reference_contains(&metadata.inline_references, &key) {
                    return Ok(Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: "Inline YAML reference".to_string(),
                        }),
                        range: None,
                    }));
                }
            }

            // Move up to parent, or return None if at root
            match node.parent() {
                Some(parent) => node = parent,
                None => break None,
            }
        }
    };

    let Some(label) = pending_footnote else {
        return Ok(None);
    };

    // Cross-document footnote lookup (done after CST traversal to avoid holding non-Send nodes
    // across await points).
    let definition_index =
        helpers::get_definition_index_with_includes(&document_map, &salsa_db, uri).await;
    let Some(location) = definition_index.find_footnote(&label) else {
        return Ok(None);
    };

    let text = {
        let db = salsa_db.lock().await;
        db.file_text_if_cached(location.path())
            .map(|file| file.text(&*db).clone())
            .or_else(|| std::fs::read_to_string(location.path()).ok())
    };

    let Some(text) = text else {
        return Ok(None);
    };

    let tree = crate::parse(&text, None);
    let Some(footnote_def) = tree
        .descendants()
        .filter_map(FootnoteDefinition::cast)
        .find(|def| def.id() == label)
    else {
        return Ok(None);
    };

    let trimmed = footnote_def.content().trim().to_string();
    Ok(Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: trimmed,
        }),
        range: None,
    }))
}

/// Format a bibliography entry for hover display.
///
/// Works with any bibliography format (BibTeX, CSL-JSON, CSL-YAML, RIS).
fn format_bibliography_entry(entry: &crate::bib::BibEntry) -> String {
    let author = entry
        .fields
        .get("author")
        .or_else(|| entry.fields.get("editor"))
        .map(|s| s.as_str())
        .unwrap_or_default();

    let year = entry
        .fields
        .get("year")
        .or_else(|| entry.fields.get("date"))
        .or_else(|| entry.fields.get("issued"))
        .map(|s| s.as_str())
        .unwrap_or_default();

    let title = entry
        .fields
        .get("title")
        .or_else(|| entry.fields.get("booktitle"))
        .map(|s| s.as_str())
        .unwrap_or_default();

    let container = entry
        .fields
        .get("journal")
        .or_else(|| entry.fields.get("journaltitle"))
        .or_else(|| entry.fields.get("container-title"))
        .or_else(|| entry.fields.get("publisher"))
        .map(|s| s.as_str())
        .unwrap_or_default();

    let locator = build_locator_unified(entry);

    let mut summary = String::new();
    if !author.is_empty() {
        summary.push_str(author);
    }
    if !year.is_empty() {
        if !summary.is_empty() {
            summary.push_str(" (");
            summary.push_str(year);
            summary.push(')');
        } else {
            summary.push_str(year);
        }
    }
    if !title.is_empty() {
        if !summary.is_empty() {
            summary.push_str(". ");
        }
        summary.push_str(&format!("*{}*", title));
    }
    if !container.is_empty() {
        summary.push_str(". ");
        summary.push_str(container);
    }
    if !locator.is_empty() {
        summary.push_str(", ");
        summary.push_str(&locator);
    }

    summary.trim().to_string()
}

fn build_locator_unified(entry: &crate::bib::BibEntry) -> String {
    let volume = entry
        .fields
        .get("volume")
        .map(|s| s.as_str())
        .unwrap_or_default();
    let number = entry
        .fields
        .get("number")
        .or_else(|| entry.fields.get("issue"))
        .map(|s| s.as_str())
        .unwrap_or_default();
    let pages = entry
        .fields
        .get("pages")
        .or_else(|| entry.fields.get("page"))
        .map(|s| s.as_str())
        .unwrap_or_default();

    let mut parts = Vec::new();
    if !volume.is_empty() {
        if !number.is_empty() {
            parts.push(format!("{}({})", volume, number));
        } else {
            parts.push(volume.to_string());
        }
    } else if !number.is_empty() {
        parts.push(number.to_string());
    }
    if !pages.is_empty() {
        parts.push(pages.to_string());
    }
    parts.join(", ")
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
