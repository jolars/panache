use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::Client;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use crate::lsp::DocumentState;
use crate::lsp::conversions::offset_to_position;
use crate::lsp::helpers::get_document_content_and_tree;
use crate::syntax::{
    AstNode, Document, GridTable, Heading, ImageLink, MultilineTable, Paragraph, PipeTable,
    SimpleTable, SyntaxKind, SyntaxNode,
};

pub async fn document_symbol(
    _client: &Client,
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    salsa_db: Arc<Mutex<crate::salsa::SalsaDb>>,
    _workspace_root: Arc<Mutex<Option<PathBuf>>>,
    params: DocumentSymbolParams,
) -> Result<Option<DocumentSymbolResponse>> {
    let uri = params.text_document.uri;
    log::debug!("document_symbol request for: {}", *uri);
    let parsed_yaml_regions = {
        let map = document_map.lock().await;
        map.get(&uri.to_string())
            .map(|state| state.parsed_yaml_regions.clone())
            .unwrap_or_default()
    };
    // Use helper to get document content and tree
    let (content, syntax_tree) =
        match get_document_content_and_tree(&document_map, &salsa_db, &uri).await {
            Some(result) => result,
            None => {
                log::warn!("Document not found in document_map: {}", *uri);
                return Ok(None);
            }
        };
    log::debug!("Document content length: {} bytes", content.len());
    let yaml_frontmatter_region = parsed_yaml_regions
        .iter()
        .find(|region| region.is_frontmatter());

    // Build symbols synchronously (SyntaxNode is not Send)
    let symbols = build_document_symbols(&syntax_tree, &content, yaml_frontmatter_region);

    log::debug!("Found {} top-level symbols", symbols.len());
    if symbols.is_empty() {
        Ok(None)
    } else {
        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }
}

fn build_document_symbols(
    root: &SyntaxNode,
    content: &str,
    yaml_frontmatter_region: Option<&crate::syntax::ParsedYamlRegionSnapshot>,
) -> Vec<DocumentSymbol> {
    let mut symbols = Vec::new();
    let mut heading_stack: Vec<(usize, DocumentSymbol)> = Vec::new();
    let db = crate::salsa::SalsaDb::default();
    let symbol_index = crate::salsa::symbol_usage_index_from_tree(&db, root);
    let heading_levels: std::collections::HashMap<rowan::TextRange, usize> =
        symbol_index.heading_sequence().iter().copied().collect();
    log::debug!("build_document_symbols: root kind = {:?}", root.kind());

    // Root is now DOCUMENT node directly
    let Some(document) = Document::cast(root.clone()) else {
        log::warn!("Root is not a DOCUMENT node: {:?}", root.kind());
        return symbols;
    };
    symbols.extend(
        yaml_frontmatter_region.and_then(|region| extract_yaml_region_symbol(region, content)),
    );

    for node in document.blocks() {
        match node.kind() {
            SyntaxKind::HEADING => {
                if let Some(symbol) = extract_heading_symbol(&node, content) {
                    let level = heading_levels.get(&node.text_range()).copied().unwrap_or(1);

                    // Pop stack until we find a parent with lower level
                    while let Some((stack_level, _)) = heading_stack.last() {
                        if *stack_level < level {
                            break;
                        }
                        let (_, completed) = heading_stack.pop().unwrap();

                        // Add to parent or root
                        if let Some((_, parent)) = heading_stack.last_mut() {
                            parent.children.get_or_insert_with(Vec::new).push(completed);
                        } else {
                            symbols.push(completed);
                        }
                    }

                    heading_stack.push((level, symbol));
                }
            }
            SyntaxKind::SIMPLE_TABLE
            | SyntaxKind::PIPE_TABLE
            | SyntaxKind::GRID_TABLE
            | SyntaxKind::MULTILINE_TABLE => {
                if let Some(symbol) = extract_table_symbol(&node, content) {
                    // Add to current heading section or root
                    if let Some((_, heading)) = heading_stack.last_mut() {
                        heading.children.get_or_insert_with(Vec::new).push(symbol);
                    } else {
                        symbols.push(symbol);
                    }
                }
            }
            SyntaxKind::FIGURE => {
                // Figure is a standalone image block
                // Look for ImageLink child
                for child in node.children() {
                    if child.kind() == SyntaxKind::IMAGE_LINK
                        && let Some(symbol) = extract_figure_symbol(&child, content)
                    {
                        // Add to current heading section or root
                        if let Some((_, heading)) = heading_stack.last_mut() {
                            heading.children.get_or_insert_with(Vec::new).push(symbol);
                        } else {
                            symbols.push(symbol);
                        }
                    }
                }
            }
            SyntaxKind::PARAGRAPH => {
                // Check if paragraph contains an ImageLink (figure)
                // This handles the old case where images were in paragraphs
                if let Some(paragraph) = Paragraph::cast(node.clone()) {
                    for image in paragraph.image_links() {
                        if let Some(symbol) = extract_figure_symbol(image.syntax(), content) {
                            // Add to current heading section or root
                            if let Some((_, heading)) = heading_stack.last_mut() {
                                heading.children.get_or_insert_with(Vec::new).push(symbol);
                            } else {
                                symbols.push(symbol);
                            }
                        }
                    }
                } else {
                    for child in node.children() {
                        if child.kind() == SyntaxKind::IMAGE_LINK
                            && let Some(symbol) = extract_figure_symbol(&child, content)
                        {
                            // Add to current heading section or root
                            if let Some((_, heading)) = heading_stack.last_mut() {
                                heading.children.get_or_insert_with(Vec::new).push(symbol);
                            } else {
                                symbols.push(symbol);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Flush remaining headings from stack
    while let Some((_, completed)) = heading_stack.pop() {
        if let Some((_, parent)) = heading_stack.last_mut() {
            parent.children.get_or_insert_with(Vec::new).push(completed);
        } else {
            symbols.push(completed);
        }
    }

    symbols
}

fn extract_yaml_region_symbol(
    region: &crate::syntax::ParsedYamlRegionSnapshot,
    content: &str,
) -> Option<DocumentSymbol> {
    let host_range = region.host_range();
    let range = Range {
        start: offset_to_position(content, host_range.start),
        end: offset_to_position(content, host_range.end),
    };
    Some(make_document_symbol(
        "YAML Frontmatter".to_string(),
        Some(match region.document_shape_summary() {
            Some(summary) => format!("{} ({})", region.id(), summary),
            None => format!("{} (invalid YAML)", region.id()),
        }),
        SymbolKind::NAMESPACE,
        range,
        range,
        None,
    ))
}

fn extract_heading_symbol(node: &SyntaxNode, content: &str) -> Option<DocumentSymbol> {
    // Use typed wrapper
    let heading = Heading::cast(node.clone())?;
    let text = heading.text();

    let range = node_to_range(node, content)?;

    Some(make_document_symbol(
        if text.is_empty() {
            "(empty)".to_string()
        } else {
            text
        },
        None,
        SymbolKind::NAMESPACE,
        range,
        range,
        Some(Vec::new()),
    ))
}

fn extract_table_symbol(node: &SyntaxNode, content: &str) -> Option<DocumentSymbol> {
    // Use typed wrappers to extract caption
    let caption = PipeTable::cast(node.clone())
        .and_then(|t| t.caption())
        .or_else(|| GridTable::cast(node.clone()).and_then(|t| t.caption()))
        .or_else(|| SimpleTable::cast(node.clone()).and_then(|t| t.caption()))
        .or_else(|| MultilineTable::cast(node.clone()).and_then(|t| t.caption()))
        .map(|c| c.text());

    let name = if let Some(cap) = caption {
        format!("Table: {}", cap)
    } else {
        "Table".to_string()
    };

    let range = node_to_range(node, content)?;
    let selection_range = node_to_range(node, content)?;

    Some(make_document_symbol(
        name,
        None,
        SymbolKind::ARRAY,
        range,
        selection_range,
        None,
    ))
}

fn extract_figure_symbol(node: &SyntaxNode, content: &str) -> Option<DocumentSymbol> {
    // Use typed wrapper for cleaner access
    let alt_text = ImageLink::cast(node.clone())
        .and_then(|img| img.alt())
        .map(|alt| alt.text())
        .filter(|text| !text.is_empty());

    let name = if let Some(alt) = alt_text {
        format!("Figure: {}", alt)
    } else {
        "Figure".to_string()
    };

    let range = node_to_range(node, content)?;
    let selection_range = node_to_range(node, content)?;

    Some(make_document_symbol(
        name,
        None,
        SymbolKind::OBJECT,
        range,
        selection_range,
        None,
    ))
}

fn make_document_symbol(
    name: String,
    detail: Option<String>,
    kind: SymbolKind,
    range: Range,
    selection_range: Range,
    children: Option<Vec<DocumentSymbol>>,
) -> DocumentSymbol {
    serde_json::from_value(json!({
        "name": name,
        "detail": detail,
        "kind": kind,
        "tags": null,
        "range": range,
        "selectionRange": selection_range,
        "children": children,
    }))
    .expect("failed to build DocumentSymbol")
}

fn node_to_range(node: &SyntaxNode, content: &str) -> Option<Range> {
    let range = node.text_range();
    let start_pos = offset_to_position(content, range.start().into());
    let end_pos = offset_to_position(content, range.end().into());

    Some(Range {
        start: start_pos,
        end: end_pos,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn test_heading_hierarchy() {
        let content = "# H1\n\n## H2\n\n### H3\n\n## H2 Again\n\n# H1 Again";
        let config = Config::default();
        let tree = crate::parser::parse(content, Some(config));
        let symbols = build_document_symbols(&tree, content, None);

        assert_eq!(symbols.len(), 2); // Two H1 headings

        let h1_first = &symbols[0];
        assert_eq!(h1_first.name, "H1");
        assert_eq!(h1_first.kind, SymbolKind::NAMESPACE);
        assert_eq!(h1_first.children.as_ref().unwrap().len(), 2); // Two H2 children

        let h2_first = &h1_first.children.as_ref().unwrap()[0];
        assert_eq!(h2_first.name, "H2");
        assert_eq!(h2_first.children.as_ref().unwrap().len(), 1); // One H3 child

        let h3 = &h2_first.children.as_ref().unwrap()[0];
        assert_eq!(h3.name, "H3");

        let h2_second = &h1_first.children.as_ref().unwrap()[1];
        assert_eq!(h2_second.name, "H2 Again");

        let h1_second = &symbols[1];
        assert_eq!(h1_second.name, "H1 Again");
    }

    #[test]
    fn test_table_under_heading() {
        let content = "# Heading\n\n| col1 | col2 |\n|------|------|\n| a    | b    |\n";
        let config = Config::default();
        let tree = crate::parser::parse(content, Some(config));
        let symbols = build_document_symbols(&tree, content, None);

        assert_eq!(symbols.len(), 1);
        let heading = &symbols[0];
        assert_eq!(heading.name, "Heading");

        let children = heading.children.as_ref().unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].name, "Table");
        assert_eq!(children[0].kind, SymbolKind::ARRAY);
    }

    #[test]
    fn test_table_with_caption() {
        let content = "# Heading\n\n| col1 | col2 |\n|------|------|\n| a    | b    |\n: Results\n";
        let config = Config::default();
        let tree = crate::parser::parse(content, Some(config));
        let symbols = build_document_symbols(&tree, content, None);

        assert_eq!(symbols.len(), 1);
        let children = symbols[0].children.as_ref().unwrap();
        assert_eq!(children.len(), 1);
        assert!(children[0].name.starts_with("Table:"));
    }

    #[test]
    fn test_figure() {
        let content = "# Heading\n\n![Figure caption](image.png)\n";
        let config = Config::default();
        let tree = crate::parser::parse(content, Some(config));
        let symbols = build_document_symbols(&tree, content, None);

        assert_eq!(symbols.len(), 1);
        let children = symbols[0].children.as_ref().unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].name, "Figure: Figure caption");
        assert_eq!(children[0].kind, SymbolKind::OBJECT);
    }

    #[test]
    fn test_figure_without_alt() {
        let content = "# Heading\n\n![](image.png)\n";
        let config = Config::default();
        let tree = crate::parser::parse(content, Some(config));
        let symbols = build_document_symbols(&tree, content, None);

        assert_eq!(symbols.len(), 1);
        let children = symbols[0].children.as_ref().unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].name, "Figure");
    }

    #[test]
    fn test_empty_heading() {
        let content = "# \n\n## Subtitle";
        let config = Config::default();
        let tree = crate::parser::parse(content, Some(config));
        let symbols = build_document_symbols(&tree, content, None);

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "(empty)");

        let children = symbols[0].children.as_ref().unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].name, "Subtitle");
    }

    #[test]
    fn test_no_headings() {
        let content = "| col1 | col2 |\n|------|------|\n| a    | b    |\n\n![Figure](image.png)";
        let config = Config::default();
        let tree = crate::parser::parse(content, Some(config));
        let symbols = build_document_symbols(&tree, content, None);

        // Tables and figures at root level when no headings
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].name, "Table");
        assert_eq!(symbols[1].name, "Figure: Figure");
    }

    #[test]
    fn test_mixed_document() {
        let content = r#"# Introduction

Some text here.

| col1 | col2 |
|------|------|
| a    | b    |

## Methods

![Method diagram](method.png)

### Subsection

Another table:

| x | y |
|---|---|
| 1 | 2 |
: Data
"#;
        let config = Config::default();
        let tree = crate::parser::parse(content, Some(config));
        let symbols = build_document_symbols(&tree, content, None);

        assert_eq!(symbols.len(), 1); // One H1
        let h1 = &symbols[0];
        assert_eq!(h1.name, "Introduction");

        let h1_children = h1.children.as_ref().unwrap();
        // Should have: table + h2
        assert!(h1_children.len() >= 2);

        // Find the H2
        let h2 = h1_children.iter().find(|s| s.name == "Methods").unwrap();
        let h2_children = h2.children.as_ref().unwrap();

        // H2 should have figure + h3
        assert!(h2_children.iter().any(|s| s.name.starts_with("Figure:")));
        assert!(h2_children.iter().any(|s| s.name == "Subsection"));
    }

    #[test]
    fn test_yaml_frontmatter_symbol_uses_parsed_summary_detail() {
        let content = "---\ntitle: Test\n---\n\n# H1\n";
        let config = Config::default();
        let tree = crate::parser::parse(content, Some(config));
        let parsed = crate::syntax::collect_parsed_yaml_region_snapshots(&tree);
        let yaml_frontmatter_region = parsed.iter().find(|region| region.is_frontmatter());
        let symbols = build_document_symbols(&tree, content, yaml_frontmatter_region);
        let yaml_symbol = symbols
            .iter()
            .find(|symbol| symbol.name == "YAML Frontmatter")
            .expect("yaml frontmatter symbol");
        let detail = yaml_symbol.detail.as_ref().expect("yaml symbol detail");
        assert!(detail.contains("Root"));
        assert!(detail.contains("BlockMap"));
    }

    #[test]
    fn test_yaml_frontmatter_symbol_shows_invalid_yaml_detail() {
        let content = "---\ntitle: [\n---\n\n# H1\n";
        let config = Config::default();
        let tree = crate::parser::parse(content, Some(config));
        let parsed = crate::syntax::collect_parsed_yaml_region_snapshots(&tree);
        let yaml_frontmatter_region = parsed.iter().find(|region| region.is_frontmatter());
        let symbols = build_document_symbols(&tree, content, yaml_frontmatter_region);
        let yaml_symbol = symbols
            .iter()
            .find(|symbol| symbol.name == "YAML Frontmatter")
            .expect("yaml frontmatter symbol");
        let detail = yaml_symbol.detail.as_ref().expect("yaml symbol detail");
        assert!(detail.contains("invalid YAML"));
    }

    #[test]
    fn test_container_headings_are_not_section_symbols() {
        let content = "# Top\n\n- # Item Heading\n\nTerm\n: # Definition Heading\n\n> # Quote Heading\n\n## Child\n";
        let config = Config::default();
        let tree = crate::parser::parse(content, Some(config));
        let symbols = build_document_symbols(&tree, content, None);

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Top");
        let children = symbols[0].children.as_ref().expect("top-level children");
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].name, "Child");
    }
}
