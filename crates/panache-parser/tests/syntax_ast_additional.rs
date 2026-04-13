//! Additional tests for syntax AST wrappers to improve coverage.
//!
//! These tests target specific uncovered code paths in syntax modules.

use panache_parser::parse;
use panache_parser::syntax::{
    AstNode, Citation, FootnoteDefinition, GridTable, List, MultilineTable, ReferenceDefinition,
    SimpleTable, TableCaption,
};

// ============================================================================
// Tests for syntax/tables.rs - additional table type methods
// ============================================================================

#[test]
fn grid_table_caption_and_rows() {
    let input = r#"+---------+---------+
| Header1 | Header2 |
+=========+=========+
| Cell1   | Cell2   |
+---------+---------+
| Cell3   | Cell4   |
+---------+---------+

Table: Grid table caption"#;
    let tree = parse(input, None);

    let grid = tree
        .descendants()
        .find_map(GridTable::cast)
        .expect("Should find GridTable");

    // Test caption extraction
    let caption = grid.caption().expect("Should have caption");
    let caption_text = caption.text();
    assert!(caption_text.contains("Grid table caption"));

    // Test rows
    assert_eq!(grid.rows().count(), 2, "Should have 2 data rows");
}

#[test]
fn simple_table_caption_and_rows() {
    let input = r#"  Header1   Header2
  --------  --------
  Cell1     Cell2
  Cell3     Cell4

Table: Simple table caption"#;
    let tree = parse(input, None);

    let simple = tree
        .descendants()
        .find_map(SimpleTable::cast)
        .expect("Should find SimpleTable");

    // Test caption
    let caption = simple.caption();
    assert!(caption.is_some(), "Should have caption");

    // Test rows
    assert!(simple.rows().next().is_some(), "Should have rows");
}

#[test]
fn multiline_table_caption_and_rows() {
    let input = r#"-------------------
Header1   Header2
--------  --------
Cell1     Cell2

Cell3     Cell4
-------------------

Table: Multiline table caption"#;
    let tree = parse(input, None);

    let multiline = tree
        .descendants()
        .find_map(MultilineTable::cast)
        .expect("Should find MultilineTable");

    // Test caption
    let caption = multiline.caption();
    assert!(caption.is_some(), "Should have caption");

    // Test rows
    assert!(multiline.rows().next().is_some(), "Should have rows");
}

#[test]
fn table_caption_without_nested_emphasis() {
    let input = r#"| A | B |
|---|---|
| 1 | 2 |

Table: Plain caption text"#;
    let tree = parse(input, None);

    let caption = tree
        .descendants()
        .find_map(TableCaption::cast)
        .expect("Should find TableCaption");

    let text = caption.text();
    assert_eq!(text, "Plain caption text");
}

// ============================================================================
// Tests for syntax/lists.rs - List and ListItem wrappers
// ============================================================================

#[test]
fn list_item_is_loose() {
    let input = "- First item\n\n- Second item\n";
    let tree = parse(input, None);

    let list = tree
        .descendants()
        .find_map(List::cast)
        .expect("Should find List");

    assert!(list.is_loose(), "List should be loose");
    assert!(!list.is_compact(), "List should not be compact");

    // Check list items - note: loose lists still contain PLAIN nodes in items
    // The "looseness" is determined by BLANK_LINE nodes between items
    assert_eq!(list.items().count(), 2);
}

#[test]
fn list_item_is_compact() {
    let input = "- First\n- Second\n- Third\n";
    let tree = parse(input, None);

    let list = tree
        .descendants()
        .find_map(List::cast)
        .expect("Should find List");

    assert!(list.is_compact(), "List should be compact");

    let item = list.items().next().expect("Should have list item");
    assert!(item.is_compact(), "Item in compact list should be compact");
    assert!(!item.is_loose(), "Item in compact list should not be loose");
}

// ============================================================================
// Tests for syntax/references.rs - ReferenceDefinition wrapper
// ============================================================================

#[test]
fn reference_definition_label() {
    let input = "[ref]: https://example.com \"Title\"\n\nSee [reference][ref].";
    let tree = parse(input, None);

    let ref_def = tree
        .descendants()
        .find_map(ReferenceDefinition::cast)
        .expect("Should find ReferenceDefinition");

    let label = ref_def.label();
    assert_eq!(label, "ref");
}

#[test]
fn reference_definition_link() {
    let input = "[label]: https://example.com\n";
    let tree = parse(input, None);

    let ref_def = tree
        .descendants()
        .find_map(ReferenceDefinition::cast)
        .expect("Should find ReferenceDefinition");

    let link = ref_def.link();
    assert!(link.is_some(), "Should have link child");
}

// ============================================================================
// Tests for syntax/citations.rs - Citation wrapper
// ============================================================================

#[test]
fn citation_key_texts() {
    let input = "Some text [@doe2020; @smith2021] more text.";
    let tree = parse(input, None);

    let citation = tree
        .descendants()
        .find_map(Citation::cast)
        .expect("Should find Citation");

    let key_texts = citation.key_texts();
    assert_eq!(key_texts.len(), 2);
    // Note: CITATION_KEY tokens don't include the @ marker
    assert!(key_texts.contains(&"doe2020".to_string()));
    assert!(key_texts.contains(&"smith2021".to_string()));
}

#[test]
fn citation_keys() {
    let input = "Text with [@key1; @key2; @key3] citation.";
    let tree = parse(input, None);

    let citation = tree
        .descendants()
        .find_map(Citation::cast)
        .expect("Should find Citation");

    let keys = citation.keys();
    assert_eq!(keys.len(), 3);

    // Test CitationKey::text() method
    // Note: keys don't include @ marker (that's separate)
    let key_texts: Vec<_> = keys.iter().map(|k| k.text()).collect();
    assert!(key_texts.contains(&"key1".to_string()));
    assert!(key_texts.contains(&"key2".to_string()));
    assert!(key_texts.contains(&"key3".to_string()));
}

// ============================================================================
// Additional edge case tests for comprehensive coverage
// ============================================================================

#[test]
fn citation_with_single_key() {
    let input = "Text with [@singlekey] citation.";
    let tree = parse(input, None);

    let citation = tree
        .descendants()
        .find_map(Citation::cast)
        .expect("Should find Citation");

    let keys = citation.keys();
    assert_eq!(keys.len(), 1);
    // Citation keys don't include @ marker
    assert_eq!(keys[0].text(), "singlekey");
}

#[test]
fn empty_list_items() {
    let input = "- \n- \n";
    let tree = parse(input, None);

    let list = tree
        .descendants()
        .find_map(List::cast)
        .expect("Should find List");

    assert_eq!(list.items().count(), 2, "Should have 2 items even if empty");
}

// ============================================================================
// Tests for FootnoteDefinition edge cases
// ============================================================================

#[test]
fn footnote_definition_is_simple_with_continuation() {
    let input = "[^1]: First line of text\n    that continues here.";
    let tree = parse(input, None);

    let def = tree
        .descendants()
        .find_map(FootnoteDefinition::cast)
        .expect("Should find FootnoteDefinition");

    assert!(
        def.is_simple(),
        "Footnote with continuation should be simple"
    );
}

#[test]
fn footnote_definition_is_not_simple_with_blank_line() {
    let input = "[^1]: First paragraph.\n\n    Second paragraph.";
    let tree = parse(input, None);

    let def = tree
        .descendants()
        .find_map(FootnoteDefinition::cast)
        .expect("Should find FootnoteDefinition");

    assert!(
        !def.is_simple(),
        "Multi-paragraph footnote should not be simple"
    );
}
