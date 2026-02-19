//! Tests for navigation features (document symbols, folding, goto definition).

use super::helpers::*;
use tower_lsp_server::ls_types::*;

#[tokio::test]
async fn test_document_symbols_hierarchical() {
    let server = TestLspServer::new();

    // Open a document with nested headings
    let content = "# Top Level\n\n## Section 1\n\nContent.\n\n## Section 2\n\n### Subsection\n\nMore content.";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    // Request document symbols
    let symbols = server.get_symbols("file:///test.qmd").await;

    assert!(symbols.is_some());
    if let Some(DocumentSymbolResponse::Nested(syms)) = symbols {
        // Should have 1 top-level symbol (h1)
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].name, "Top Level");

        // H1 should have 2 children (the h2 sections)
        let children = syms[0].children.as_ref().unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].name, "Section 1");
        assert_eq!(children[1].name, "Section 2");

        // Second h2 should have 1 child (h3)
        let subsections = children[1].children.as_ref().unwrap();
        assert_eq!(subsections.len(), 1);
        assert_eq!(subsections[0].name, "Subsection");
    } else {
        panic!("Expected nested document symbols");
    }
}

#[tokio::test]
async fn test_document_symbols_with_table() {
    let server = TestLspServer::new();

    // Open a document with heading and table
    let content = "# Report\n\n| Col1 | Col2 |\n|------|------|\n| A    | B    |\n\nText.";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let symbols = server.get_symbols("file:///test.qmd").await;

    assert!(symbols.is_some());
    if let Some(DocumentSymbolResponse::Nested(syms)) = symbols {
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].name, "Report");

        // Table should be a child of the heading
        let children = syms[0].children.as_ref();
        assert!(children.is_some());
        let children = children.unwrap();

        // Find the table symbol
        let table_symbol = children.iter().find(|s| s.name.starts_with("Table"));
        assert!(table_symbol.is_some(), "Should have a table symbol");
    } else {
        panic!("Expected nested document symbols");
    }
}

#[tokio::test]
async fn test_folding_ranges_headings() {
    let server = TestLspServer::new();

    // Open a document with multiple headings
    let content = "# Heading 1\n\nContent 1.\n\n# Heading 2\n\nContent 2.";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let ranges = server.get_folding_ranges("file:///test.qmd").await;

    assert!(ranges.is_some());
    let ranges = ranges.unwrap();

    // Should have folding ranges for both heading sections
    assert!(ranges.len() >= 2, "Should have at least 2 folding ranges");

    // First fold should start at heading 1
    assert_eq!(ranges[0].start_line, 0);
}

#[tokio::test]
async fn test_folding_ranges_code_block() {
    let server = TestLspServer::new();

    // Open a document with a code block
    let content = "# Doc\n\n```python\nprint('hello')\nprint('world')\n```\n\nText.";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let ranges = server.get_folding_ranges("file:///test.qmd").await;

    assert!(ranges.is_some());
    let ranges = ranges.unwrap();

    // Should have fold for heading section and code block
    assert!(!ranges.is_empty());

    // Find the code block fold
    let code_fold = ranges.iter().find(|r| r.start_line == 2);
    assert!(code_fold.is_some(), "Should have fold for code block");
}
