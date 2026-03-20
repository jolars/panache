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

    let Some(ctx) =
        crate::lsp::context::get_open_document_context(&document_map, &salsa_db, uri).await
    else {
        return Ok(None);
    };
    let salsa_file = ctx.salsa_file;
    let salsa_config = ctx.salsa_config;
    let doc_path = ctx.path.clone();
    let parsed_yaml_regions = ctx.parsed_yaml_regions.clone();

    let Some(doc_path) = doc_path else {
        return Ok(None);
    };
    let content_for_offset = ctx.content.clone();
    let Some(offset) = conversions::position_to_offset(&content_for_offset, position) else {
        return Ok(None);
    };
    let in_frontmatter_region =
        helpers::is_offset_in_yaml_frontmatter(&parsed_yaml_regions, offset);
    if in_frontmatter_region {
        return Ok(None);
    }
    let yaml_ok = helpers::is_yaml_frontmatter_valid(&parsed_yaml_regions);
    if !yaml_ok {
        return Ok(None);
    }

    let metadata = {
        let db = salsa_db.lock().await;
        crate::salsa::metadata(&*db, salsa_file, salsa_config, doc_path.clone()).clone()
    };

    let pending_footnote = {
        let root = ctx.syntax_root();

        // Find the node at this offset
        let Some(mut node) = helpers::find_node_at_offset(&root, offset) else {
            return Ok(None);
        };

        // Walk up the tree to find a footnote reference or citation
        loop {
            if let Some((label, is_footnote)) = helpers::extract_reference_label(&node) {
                // Only handle footnotes (not regular references)
                if is_footnote {
                    break Some(label);
                }
            }

            if let Some(key) = helpers::extract_citation_key(&node) {
                if let Some(ref parse) = metadata.bibliography_parse
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

    // Cross-document footnote lookup via symbol usage index.
    let doc_indices = crate::lsp::navigation::project_symbol_documents(
        &salsa_db,
        salsa_file,
        salsa_config,
        &doc_path,
        uri,
        &content_for_offset,
    )
    .await;

    for doc in doc_indices {
        let Some(ranges) = doc.symbol_index.footnote_definitions(&label) else {
            continue;
        };
        let Some(range) = ranges.first() else {
            continue;
        };

        let tree = crate::parse(&doc.text, None);
        let Some(footnote_def) = tree
            .descendants()
            .filter_map(FootnoteDefinition::cast)
            .find(|def| def.syntax().text_range() == *range)
        else {
            continue;
        };

        let trimmed = footnote_def.content().trim().to_string();
        return Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: trimmed,
            }),
            range: None,
        }));
    }

    Ok(None)
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

        let db = crate::salsa::SalsaDb::default();
        let index = crate::salsa::symbol_usage_index_from_tree(&db, &root);
        let range = index
            .footnote_definitions(&label)
            .and_then(|ranges| ranges.first())
            .copied()
            .expect("Should find definition range");
        let footnote_def = root
            .descendants()
            .filter_map(FootnoteDefinition::cast)
            .find(|def| def.syntax().text_range() == range)
            .expect("Should find footnote definition by range");

        // Extract content
        let content = footnote_def.content();
        assert!(content.contains("This is the footnote content"));
    }

    #[test]
    fn test_hover_multiline_footnote() {
        let input = "Text[^1]\n\n[^1]: First line\n    Second line";
        let root = parse(input, None);

        let db = crate::salsa::SalsaDb::default();
        let index = crate::salsa::symbol_usage_index_from_tree(&db, &root);
        let range = index
            .footnote_definitions("1")
            .and_then(|ranges| ranges.first())
            .copied()
            .expect("Should find definition range");
        let footnote_def = root
            .descendants()
            .filter_map(FootnoteDefinition::cast)
            .find(|def| def.syntax().text_range() == range)
            .expect("Should find footnote definition by range");

        let content = footnote_def.content();
        assert!(content.contains("First line"));
        assert!(content.contains("Second line"));
    }

    #[test]
    fn test_no_definition_found() {
        let input = "Text with footnote[^missing]";
        let root = parse(input, None);

        let db = crate::salsa::SalsaDb::default();
        let index = crate::salsa::symbol_usage_index_from_tree(&db, &root);
        assert!(index.footnote_definitions("missing").is_none());
    }

    #[test]
    fn test_footnote_with_formatting() {
        let input = "[^1]: Text with *emphasis* and `code`.";
        let root = parse(input, None);

        let db = crate::salsa::SalsaDb::default();
        let index = crate::salsa::symbol_usage_index_from_tree(&db, &root);
        let range = index
            .footnote_definitions("1")
            .and_then(|ranges| ranges.first())
            .copied()
            .expect("Should find definition range");
        let footnote_def = root
            .descendants()
            .filter_map(FootnoteDefinition::cast)
            .find(|def| def.syntax().text_range() == range)
            .expect("Should find footnote definition by range");

        let content = footnote_def.content();
        assert!(content.contains("*emphasis*"));
        assert!(content.contains("`code`"));
    }

    #[test]
    fn footnote_definition_index_contains_definition_range() {
        let db = crate::salsa::SalsaDb::default();
        let text = "Text[^a]\n\n[^a]: Hello world\n";
        let tree = parse(text, None);
        let index = crate::salsa::symbol_usage_index_from_tree(&db, &tree);
        assert_eq!(
            index.footnote_definitions("a").map(|ranges| ranges.len()),
            Some(1)
        );
    }
}
