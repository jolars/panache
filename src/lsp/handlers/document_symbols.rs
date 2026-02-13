use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::Client;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use crate::lsp::conversions::offset_to_position;
use crate::lsp::helpers::get_document_and_config;
use crate::syntax::{SyntaxKind, SyntaxNode};

pub async fn document_symbol(
    client: &Client,
    document_map: Arc<Mutex<HashMap<String, String>>>,
    workspace_root: Arc<Mutex<Option<PathBuf>>>,
    params: DocumentSymbolParams,
) -> Result<Option<DocumentSymbolResponse>> {
    let uri = params.text_document.uri;
    log::debug!("document_symbol request for: {}", *uri);

    // Use helper to get document and config
    let (content, config) =
        match get_document_and_config(client, &document_map, &workspace_root, &uri).await {
            Some(result) => result,
            None => {
                log::warn!("Document not found in document_map: {}", *uri);
                return Ok(None);
            }
        };
    log::debug!("Document content length: {} bytes", content.len());

    // Parse and build symbols synchronously (SyntaxNode is not Send)
    let syntax_tree = crate::parser::parse(&content, Some(config));
    let symbols = build_document_symbols(&syntax_tree, &content);

    log::debug!("Found {} top-level symbols", symbols.len());
    if symbols.is_empty() {
        Ok(None)
    } else {
        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }
}

fn build_document_symbols(root: &SyntaxNode, content: &str) -> Vec<DocumentSymbol> {
    let mut symbols = Vec::new();
    let mut heading_stack: Vec<(usize, DocumentSymbol)> = Vec::new();

    log::debug!("build_document_symbols: root kind = {:?}", root.kind());

    // Find DOCUMENT node
    let document = root.children().find(|n| n.kind() == SyntaxKind::DOCUMENT);
    let document = match document {
        Some(d) => {
            log::debug!("Found DOCUMENT node with {} children", d.children().count());
            d
        }
        None => {
            log::warn!("No DOCUMENT node found in syntax tree");
            return symbols;
        }
    };

    for node in document.children() {
        match node.kind() {
            SyntaxKind::Heading => {
                if let Some(symbol) = extract_heading_symbol(&node, content) {
                    let level = get_heading_level(&node);

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
            SyntaxKind::SimpleTable
            | SyntaxKind::PipeTable
            | SyntaxKind::GridTable
            | SyntaxKind::MultilineTable => {
                if let Some(symbol) = extract_table_symbol(&node, content) {
                    // Add to current heading section or root
                    if let Some((_, heading)) = heading_stack.last_mut() {
                        heading.children.get_or_insert_with(Vec::new).push(symbol);
                    } else {
                        symbols.push(symbol);
                    }
                }
            }
            SyntaxKind::PARAGRAPH => {
                // Check if paragraph contains an ImageLink (figure)
                for child in node.children() {
                    if child.kind() == SyntaxKind::ImageLink
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

fn get_heading_level(node: &SyntaxNode) -> usize {
    // Count ATX markers (#)
    for child in node.children() {
        if child.kind() == SyntaxKind::AtxHeadingMarker {
            let text = child.text().to_string();
            return text.chars().filter(|&c| c == '#').count();
        }
    }
    1 // Default to H1
}

fn extract_heading_symbol(node: &SyntaxNode, content: &str) -> Option<DocumentSymbol> {
    let heading_text = extract_heading_text(node)?;
    let range = node_to_range(node, content)?;
    let selection_range = node_to_range(node, content)?; // For now, same as range

    #[allow(deprecated)]
    Some(DocumentSymbol {
        name: heading_text,
        detail: None,
        kind: SymbolKind::STRING,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: Some(Vec::new()),
    })
}

fn extract_heading_text(node: &SyntaxNode) -> Option<String> {
    for child in node.children() {
        if child.kind() == SyntaxKind::HeadingContent {
            let text = child.text().to_string().trim().to_string();
            return if text.is_empty() {
                Some("(empty)".to_string())
            } else {
                Some(text)
            };
        }
    }
    Some("(empty)".to_string())
}

fn extract_table_symbol(node: &SyntaxNode, content: &str) -> Option<DocumentSymbol> {
    let caption = extract_table_caption(node);
    let name = if let Some(cap) = caption {
        format!("Table: {}", cap)
    } else {
        "Table".to_string()
    };

    let range = node_to_range(node, content)?;
    let selection_range = node_to_range(node, content)?;

    #[allow(deprecated)]
    Some(DocumentSymbol {
        name,
        detail: None,
        kind: SymbolKind::ARRAY,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: None,
    })
}

fn extract_table_caption(node: &SyntaxNode) -> Option<String> {
    for child in node.children() {
        if child.kind() == SyntaxKind::TableCaption {
            let text = child.text().to_string().trim().to_string();
            return if text.is_empty() { None } else { Some(text) };
        }
    }
    None
}

fn extract_figure_symbol(node: &SyntaxNode, content: &str) -> Option<DocumentSymbol> {
    let alt_text = extract_image_alt(node);
    let name = if let Some(alt) = alt_text {
        format!("Figure: {}", alt)
    } else {
        "Figure".to_string()
    };

    let range = node_to_range(node, content)?;
    let selection_range = node_to_range(node, content)?;

    #[allow(deprecated)]
    Some(DocumentSymbol {
        name,
        detail: None,
        kind: SymbolKind::OBJECT,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: None,
    })
}

fn extract_image_alt(node: &SyntaxNode) -> Option<String> {
    for child in node.children() {
        if child.kind() == SyntaxKind::ImageAlt {
            let text = child.text().to_string().trim().to_string();
            return if text.is_empty() { None } else { Some(text) };
        }
    }
    None
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
        let symbols = build_document_symbols(&tree, content);

        assert_eq!(symbols.len(), 2); // Two H1 headings

        let h1_first = &symbols[0];
        assert_eq!(h1_first.name, "H1");
        assert_eq!(h1_first.kind, SymbolKind::STRING);
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
        let symbols = build_document_symbols(&tree, content);

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
        let symbols = build_document_symbols(&tree, content);

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
        let symbols = build_document_symbols(&tree, content);

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
        let symbols = build_document_symbols(&tree, content);

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
        let symbols = build_document_symbols(&tree, content);

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
        let symbols = build_document_symbols(&tree, content);

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
        let symbols = build_document_symbols(&tree, content);

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
}
