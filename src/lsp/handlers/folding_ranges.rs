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

pub async fn folding_range(
    client: &Client,
    document_map: Arc<Mutex<HashMap<String, String>>>,
    workspace_root: Arc<Mutex<Option<PathBuf>>>,
    params: FoldingRangeParams,
) -> Result<Option<Vec<FoldingRange>>> {
    let uri = params.text_document.uri;

    // Use helper to get document and config
    let (content, config) =
        match get_document_and_config(client, &document_map, &workspace_root, &uri).await {
            Some(result) => result,
            None => return Ok(None),
        };

    // Parse and build folding ranges synchronously (SyntaxNode is not Send)
    let syntax_tree = crate::parser::parse(&content, Some(config));
    let ranges = build_folding_ranges(&syntax_tree, &content);

    if ranges.is_empty() {
        Ok(None)
    } else {
        Ok(Some(ranges))
    }
}

fn build_folding_ranges(root: &SyntaxNode, content: &str) -> Vec<FoldingRange> {
    let mut ranges = Vec::new();

    // Find DOCUMENT node
    let document = root.children().find(|n| n.kind() == SyntaxKind::DOCUMENT);
    let document = match document {
        Some(d) => d,
        None => return ranges,
    };

    // Track heading positions for folding sections
    let mut heading_positions: Vec<(usize, usize)> = Vec::new();

    for node in document.children() {
        match node.kind() {
            SyntaxKind::Heading => {
                let level = get_heading_level(&node);
                let start_offset = node.text_range().start().into();
                heading_positions.push((level, start_offset));
            }
            SyntaxKind::CodeBlock => {
                if let Some(range) = extract_code_block_range(&node, content) {
                    ranges.push(range);
                }
            }
            SyntaxKind::FencedDiv => {
                if let Some(range) = extract_fenced_div_range(&node, content) {
                    ranges.push(range);
                }
            }
            SyntaxKind::YamlMetadata => {
                if let Some(range) = extract_yaml_metadata_range(&node, content) {
                    ranges.push(range);
                }
            }
            _ => {}
        }
    }

    // Process heading sections - fold from heading to next same/higher level heading
    for (i, &(level, start_offset)) in heading_positions.iter().enumerate() {
        // Find next heading of same or higher level
        let end_offset = if let Some(&(_, next_offset)) = heading_positions
            .iter()
            .skip(i + 1)
            .find(|(next_level, _)| *next_level <= level)
        {
            next_offset
        } else {
            // No next heading at same/higher level, fold to end of document
            content.len()
        };

        // Only create fold if there's content after the heading
        if end_offset > start_offset {
            let start_pos = offset_to_position(content, start_offset);
            let end_pos = offset_to_position(content, end_offset.saturating_sub(1));

            // Fold from the line after the heading to the last line before next heading
            if start_pos.line < end_pos.line {
                ranges.push(FoldingRange {
                    start_line: start_pos.line,
                    start_character: None,
                    end_line: end_pos.line,
                    end_character: None,
                    kind: Some(FoldingRangeKind::Region),
                    collapsed_text: None,
                });
            }
        }
    }

    ranges
}

fn get_heading_level(heading: &SyntaxNode) -> usize {
    // Count # markers or determine from setext underline
    for child in heading.children() {
        if child.kind() == SyntaxKind::AtxHeadingMarker {
            let text = child.text().to_string();
            return text.chars().filter(|&c| c == '#').count();
        }
        if child.kind() == SyntaxKind::SetextHeadingUnderline {
            let text = child.text().to_string();
            return if text.contains('=') { 1 } else { 2 };
        }
    }
    1 // Default to level 1 if no marker found
}

fn extract_code_block_range(node: &SyntaxNode, content: &str) -> Option<FoldingRange> {
    let start_offset: usize = node.text_range().start().into();
    let end_offset: usize = node.text_range().end().into();

    let start_pos = offset_to_position(content, start_offset);
    let end_pos = offset_to_position(content, end_offset.saturating_sub(1));

    // Only fold if block spans multiple lines
    if start_pos.line < end_pos.line {
        Some(FoldingRange {
            start_line: start_pos.line,
            start_character: None,
            end_line: end_pos.line,
            end_character: None,
            kind: Some(FoldingRangeKind::Region),
            collapsed_text: None,
        })
    } else {
        None
    }
}

fn extract_fenced_div_range(node: &SyntaxNode, content: &str) -> Option<FoldingRange> {
    let start_offset: usize = node.text_range().start().into();
    let end_offset: usize = node.text_range().end().into();

    let start_pos = offset_to_position(content, start_offset);
    let end_pos = offset_to_position(content, end_offset.saturating_sub(1));

    // Only fold if div spans multiple lines
    if start_pos.line < end_pos.line {
        Some(FoldingRange {
            start_line: start_pos.line,
            start_character: None,
            end_line: end_pos.line,
            end_character: None,
            kind: Some(FoldingRangeKind::Region),
            collapsed_text: None,
        })
    } else {
        None
    }
}

fn extract_yaml_metadata_range(node: &SyntaxNode, content: &str) -> Option<FoldingRange> {
    let start_offset: usize = node.text_range().start().into();
    let end_offset: usize = node.text_range().end().into();

    let start_pos = offset_to_position(content, start_offset);
    let end_pos = offset_to_position(content, end_offset.saturating_sub(1));

    // Only fold if metadata spans multiple lines
    if start_pos.line < end_pos.line {
        Some(FoldingRange {
            start_line: start_pos.line,
            start_character: None,
            end_line: end_pos.line,
            end_character: None,
            kind: Some(FoldingRangeKind::Region),
            collapsed_text: None,
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heading_hierarchy_folding() {
        let content = r#"# Heading 1

Some content under h1.

## Heading 2

Content under h2.

### Heading 3

More content.

## Another H2

Final content.
"#;
        let config = crate::config::Config::default();
        let tree = crate::parser::parse(content, Some(config));
        let ranges = build_folding_ranges(&tree, content);

        // Should have 4 folding ranges: h1, h2, h3, h2
        assert!(!ranges.is_empty(), "Should have folding ranges");

        // Verify we have heading folds (at least)
        let heading_folds: Vec<_> = ranges
            .iter()
            .filter(|r| r.kind == Some(FoldingRangeKind::Region))
            .collect();
        assert!(!heading_folds.is_empty(), "Should have heading folds");
    }

    #[test]
    fn test_code_block_folding() {
        let content = r#"# Test

```python
def hello():
    print("Hello, world!")
    return True
```

More text.
"#;
        let config = crate::config::Config::default();
        let tree = crate::parser::parse(content, Some(config));
        let ranges = build_folding_ranges(&tree, content);

        // Should have at least the code block fold
        let code_folds: Vec<_> = ranges
            .iter()
            .filter(|r| r.kind == Some(FoldingRangeKind::Region))
            .collect();
        assert!(!code_folds.is_empty(), "Should have code block fold");
    }

    #[test]
    fn test_fenced_div_folding() {
        let content = r#"# Test

::: {.callout-note}
This is a note.
It has multiple lines.
:::

Text after.
"#;
        let config = crate::config::Config::default();
        let tree = crate::parser::parse(content, Some(config));
        let ranges = build_folding_ranges(&tree, content);

        // Should have at least the fenced div fold
        assert!(!ranges.is_empty(), "Should have folding ranges");
    }

    #[test]
    fn test_yaml_frontmatter_folding() {
        let content = r#"---
title: "My Document"
author: "Test Author"
date: 2024-01-01
---

# Heading

Content here.
"#;
        let config = crate::config::Config::default();
        let tree = crate::parser::parse(content, Some(config));
        let ranges = build_folding_ranges(&tree, content);

        // Should have frontmatter fold + heading fold
        assert!(
            ranges.len() >= 2,
            "Should have at least 2 folds (frontmatter + heading)"
        );
    }

    #[test]
    fn test_nested_structures() {
        let content = r#"# Main Heading

Some intro text.

```rust
fn main() {
    println!("nested");
}
```

## Subheading

More content.
"#;
        let config = crate::config::Config::default();
        let tree = crate::parser::parse(content, Some(config));
        let ranges = build_folding_ranges(&tree, content);

        // Should have: h1 fold, code block fold, h2 fold
        assert!(ranges.len() >= 3, "Should have at least 3 folds");
    }

    #[test]
    fn test_empty_document() {
        let content = "";
        let config = crate::config::Config::default();
        let tree = crate::parser::parse(content, Some(config));
        let ranges = build_folding_ranges(&tree, content);

        assert!(ranges.is_empty(), "Empty document should have no folds");
    }

    #[test]
    fn test_single_heading_no_content() {
        let content = "# Heading\n";
        let config = crate::config::Config::default();
        let tree = crate::parser::parse(content, Some(config));
        let ranges = build_folding_ranges(&tree, content);

        // Single heading with no content should not create fold
        assert!(
            ranges.is_empty(),
            "Single heading with no content should have no folds"
        );
    }

    #[test]
    fn test_no_foldable_content() {
        let content = r#"Just a paragraph.

Another paragraph.

And one more.
"#;
        let config = crate::config::Config::default();
        let tree = crate::parser::parse(content, Some(config));
        let ranges = build_folding_ranges(&tree, content);

        assert!(ranges.is_empty(), "Plain paragraphs should have no folds");
    }
}
