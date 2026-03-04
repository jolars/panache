//! Unit tests for syntax AST node wrappers.
//!
//! These tests verify that the typed AST wrappers in `src/syntax/` correctly
//! expose accessors for their underlying CST nodes. We test links, images,
//! tables, headings, and other AST constructs.

use panache::{parse, syntax::SyntaxKind};

// Import AST wrappers
use panache::syntax::{
    AstNode, Figure, GridTable, Heading, ImageAlt, ImageLink, Link, LinkDest, LinkRef, LinkText,
    MultilineTable, PipeTable, SimpleTable, TableCaption, TableCell, TableRow,
};

// =============================================================================
// Link Tests
// =============================================================================

#[test]
fn link_cast_and_accessors() {
    let input = "[text](url)\n";
    let tree = parse(input, None);

    // Find the LINK node
    let link_node = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::LINK)
        .expect("Should find LINK node");

    // Cast to Link
    let link = Link::cast(link_node).expect("Should cast to Link");

    // Test text() accessor
    let link_text = link.text().expect("Link should have text");
    let text_content = link_text.text_content();
    assert!(
        text_content.contains("text"),
        "Link text should contain 'text'"
    );

    // Test dest() accessor
    let link_dest = link.dest().expect("Link should have destination");
    let url = link_dest.url_content();
    assert_eq!(url, "url", "Link destination should be 'url'");

    // reference() should be None for inline links
    assert!(
        link.reference().is_none(),
        "Inline link should not have reference"
    );
}

#[test]
fn link_reference_style() {
    let input = "[text][ref]\n\n[ref]: url\n";
    let tree = parse(input, None);

    // Find the LINK node
    let link_node = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::LINK)
        .expect("Should find LINK node");

    let link = Link::cast(link_node).expect("Should cast to Link");

    // Should have text
    assert!(link.text().is_some(), "Reference link should have text");

    // Should have reference
    let link_ref = link
        .reference()
        .expect("Reference link should have reference");
    let label = link_ref.label();
    assert!(
        label.contains("ref"),
        "Reference label should contain 'ref'"
    );
}

#[test]
fn link_text_content() {
    let input = "[hello world](url)\n";
    let tree = parse(input, None);

    let link_text_node = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::LINK_TEXT)
        .expect("Should find LINK_TEXT node");

    let link_text = LinkText::cast(link_text_node).expect("Should cast to LinkText");
    let content = link_text.text_content();

    assert!(
        content.contains("hello") && content.contains("world"),
        "Link text should contain 'hello world'"
    );
}

#[test]
fn link_dest_url_with_parentheses() {
    let input = "[text](https://example.com)\n";
    let tree = parse(input, None);

    let dest_node = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::LINK_DEST)
        .expect("Should find LINK_DEST node");

    let dest = LinkDest::cast(dest_node).expect("Should cast to LinkDest");

    // url() includes parentheses from CST
    let url_with_parens = dest.url();
    assert!(
        url_with_parens.contains("https://example.com"),
        "URL should contain the full URL"
    );

    // url_content() strips parentheses
    let url_content = dest.url_content();
    assert_eq!(
        url_content, "https://example.com",
        "URL content should not include parentheses"
    );
}

#[test]
fn link_can_cast_rejects_wrong_kind() {
    assert!(
        !Link::can_cast(SyntaxKind::IMAGE_LINK),
        "Link should not accept IMAGE_LINK kind"
    );
    assert!(
        !Link::can_cast(SyntaxKind::PARAGRAPH),
        "Link should not accept PARAGRAPH kind"
    );
}

// =============================================================================
// Image Tests
// =============================================================================

#[test]
fn image_link_cast_and_accessors() {
    let input = "![alt text](image.png)\n";
    let tree = parse(input, None);

    let image_node = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::IMAGE_LINK)
        .expect("Should find IMAGE_LINK node");

    let image = ImageLink::cast(image_node).expect("Should cast to ImageLink");

    // Test alt() accessor
    let alt = image.alt().expect("Image should have alt text");
    let alt_text = alt.text();
    assert!(
        alt_text.contains("alt text"),
        "Image alt should contain 'alt text'"
    );

    // Test dest() accessor
    let dest = image.dest().expect("Image should have destination");
    let url = dest.url_content();
    assert_eq!(url, "image.png", "Image destination should be 'image.png'");
}

#[test]
fn image_alt_text_extraction() {
    let input = "![hello world](img.png)\n";
    let tree = parse(input, None);

    let alt_node = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::IMAGE_ALT)
        .expect("Should find IMAGE_ALT node");

    let alt = ImageAlt::cast(alt_node).expect("Should cast to ImageAlt");
    let text = alt.text();

    assert!(
        text.contains("hello") && text.contains("world"),
        "Alt text should contain 'hello world'"
    );
}

#[test]
fn figure_with_image() {
    let input = "![Caption](image.png)\n\n";
    let tree = parse(input, None);

    // Look for FIGURE node (if parser creates them for captioned images)
    if let Some(figure_node) = tree.descendants().find(|n| n.kind() == SyntaxKind::FIGURE) {
        let figure = Figure::cast(figure_node).expect("Should cast to Figure");
        let image = figure.image().expect("Figure should contain image");
        assert!(
            image.syntax().kind() == SyntaxKind::IMAGE_LINK,
            "Figure should contain IMAGE_LINK"
        );
    }
}

// =============================================================================
// Table Tests - Pipe Tables
// =============================================================================

#[test]
fn pipe_table_cast_and_rows() {
    let input = "| A | B |\n|---|---|\n| 1 | 2 |\n";
    let tree = parse(input, None);

    let table_node = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::PIPE_TABLE)
        .expect("Should find PIPE_TABLE node");

    let table = PipeTable::cast(table_node).expect("Should cast to PipeTable");

    // Check rows
    let rows: Vec<_> = table.rows().collect();
    assert!(!rows.is_empty(), "Table should have at least 1 row");
}

#[test]
fn pipe_table_with_caption() {
    let input = "| A | B |\n|---|---|\n| 1 | 2 |\n\n: Table caption\n";
    let tree = parse(input, None);

    if let Some(table_node) = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::PIPE_TABLE)
    {
        let table = PipeTable::cast(table_node).expect("Should cast to PipeTable");

        // Check for caption
        if let Some(caption) = table.caption() {
            let text = caption.text();
            assert!(
                text.contains("Table caption") || text.contains("caption"),
                "Caption should contain text"
            );
        }
    }
}

#[test]
fn table_row_and_cells() {
    let input = "| A | B | C |\n|---|---|---|\n| 1 | 2 | 3 |\n";
    let tree = parse(input, None);

    let row_node = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::TABLE_ROW)
        .expect("Should find TABLE_ROW node");

    let row = TableRow::cast(row_node).expect("Should cast to TableRow");

    // Check cells
    let cells: Vec<_> = row.cells().collect();
    assert!(cells.len() >= 2, "Row should have at least 2 cells");
}

#[test]
fn table_cell_cast() {
    let input = "| A | B |\n|---|---|\n| 1 | 2 |\n";
    let tree = parse(input, None);

    let cell_node = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::TABLE_CELL)
        .expect("Should find TABLE_CELL node");

    let _cell = TableCell::cast(cell_node).expect("Should cast to TableCell");
    // Just verify casting works
}

// =============================================================================
// Table Tests - Grid Tables
// =============================================================================

#[test]
fn grid_table_cast() {
    let input = "+---+---+\n| A | B |\n+===+===+\n| 1 | 2 |\n+---+---+\n";
    let tree = parse(input, None);

    if let Some(table_node) = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::GRID_TABLE)
    {
        let table = GridTable::cast(table_node).expect("Should cast to GridTable");

        // Check rows accessor works
        let rows: Vec<_> = table.rows().collect();
        assert!(!rows.is_empty(), "Grid table should have rows");

        // Check caption accessor (should be None without caption)
        let _ = table.caption();
    }
}

// =============================================================================
// Table Tests - Simple Tables
// =============================================================================

#[test]
fn simple_table_cast() {
    let input = "  A   B\n  --- ---\n  1   2\n";
    let tree = parse(input, None);

    if let Some(table_node) = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::SIMPLE_TABLE)
    {
        let table = SimpleTable::cast(table_node).expect("Should cast to SimpleTable");

        // Check rows accessor works
        let rows: Vec<_> = table.rows().collect();
        assert!(!rows.is_empty(), "Simple table should have rows");

        // Check caption accessor
        let _ = table.caption();
    }
}

// =============================================================================
// Table Tests - Multiline Tables
// =============================================================================

#[test]
fn multiline_table_cast() {
    let input = "-----\nA   B\n--- ---\n1   2\n-----\n";
    let tree = parse(input, None);

    if let Some(table_node) = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::MULTILINE_TABLE)
    {
        let table = MultilineTable::cast(table_node).expect("Should cast to MultilineTable");

        // Check rows accessor works
        let rows: Vec<_> = table.rows().collect();
        assert!(!rows.is_empty(), "Multiline table should have rows");

        // Check caption accessor
        let _ = table.caption();
    }
}

#[test]
fn table_caption_text() {
    // Create any table with a caption
    let input = "| A |\n|---|\n| 1 |\n\n: My Caption\n";
    let tree = parse(input, None);

    if let Some(caption_node) = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::TABLE_CAPTION)
    {
        let caption = TableCaption::cast(caption_node).expect("Should cast to TableCaption");
        let text = caption.text();
        assert!(
            !text.is_empty(),
            "Caption should have text, got: '{}'",
            text
        );
    }
}

// =============================================================================
// Heading Tests
// =============================================================================

#[test]
fn heading_cast_and_level() {
    let input = "# Level 1\n";
    let tree = parse(input, None);

    let heading_node = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::HEADING)
        .expect("Should find HEADING node");

    let heading = Heading::cast(heading_node).expect("Should cast to Heading");

    // Test level() accessor
    let level = heading.level();
    assert_eq!(level, 1, "Heading level should be 1");

    // Test text() accessor
    let text = heading.text();
    assert!(
        text.contains("Level 1"),
        "Heading text should contain 'Level 1'"
    );
}

#[test]
fn heading_multiple_levels() {
    for (input, expected_level) in [
        ("# H1\n", 1),
        ("## H2\n", 2),
        ("### H3\n", 3),
        ("#### H4\n", 4),
        ("##### H5\n", 5),
        ("###### H6\n", 6),
    ] {
        let tree = parse(input, None);

        let heading_node = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::HEADING)
            .expect("Should find HEADING node");

        let heading = Heading::cast(heading_node).expect("Should cast to Heading");
        assert_eq!(
            heading.level(),
            expected_level,
            "Heading level should be {} for input '{}'",
            expected_level,
            input.trim()
        );
    }
}

#[test]
fn heading_with_inline_formatting() {
    let input = "# Heading with *emphasis* and `code`\n";
    let tree = parse(input, None);

    let heading_node = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::HEADING)
        .expect("Should find HEADING node");

    let heading = Heading::cast(heading_node).expect("Should cast to Heading");

    // The text() method extracts only TEXT tokens from HEADING_CONTENT
    // (not from nested inline elements like EMPHASIS or CODE_SPAN)
    let text = heading.text();
    assert!(
        text.contains("Heading with") && text.contains(" and "),
        "Heading text should include direct text content (got: '{}')",
        text
    );

    // But the full heading should have inline children with their content
    let full_text = heading.syntax().text().to_string();
    assert!(
        full_text.contains("*emphasis*") && full_text.contains("`code`"),
        "Full heading should preserve formatting with content"
    );

    // Verify inline elements are present as children
    let has_emphasis = heading
        .syntax()
        .descendants()
        .any(|n| n.kind() == SyntaxKind::EMPHASIS);
    let has_code = heading
        .syntax()
        .descendants()
        .any(|n| n.kind() == SyntaxKind::CODE_SPAN);

    assert!(has_emphasis, "Heading should contain EMPHASIS node");
    assert!(has_code, "Heading should contain CODE_SPAN node");
}

#[test]
fn heading_can_cast_rejects_wrong_kind() {
    assert!(
        !Heading::can_cast(SyntaxKind::PARAGRAPH),
        "Heading should not accept PARAGRAPH kind"
    );
    assert!(
        !Heading::can_cast(SyntaxKind::CODE_BLOCK),
        "Heading should not accept CODE_BLOCK kind"
    );
}

#[test]
fn astnode_can_cast_accepts_matching_kind() {
    assert!(Link::can_cast(SyntaxKind::LINK));
    assert!(ImageLink::can_cast(SyntaxKind::IMAGE_LINK));
    assert!(PipeTable::can_cast(SyntaxKind::PIPE_TABLE));
    assert!(Heading::can_cast(SyntaxKind::HEADING));
}

#[test]
fn astnode_cast_returns_none_for_wrong_node() {
    let input = "# Heading\n";
    let tree = parse(input, None);

    let heading_node = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::HEADING)
        .expect("Should find HEADING node");

    // Try to cast heading to Link - should return None
    assert!(
        Link::cast(heading_node.clone()).is_none(),
        "Should not cast Heading node to Link"
    );

    // Try to cast heading to PipeTable - should return None
    assert!(
        PipeTable::cast(heading_node).is_none(),
        "Should not cast Heading node to PipeTable"
    );
}

#[test]
fn astnode_syntax_returns_underlying_node() {
    let input = "[link](url)\n";
    let tree = parse(input, None);

    let link_node = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::LINK)
        .expect("Should find LINK node");

    let link = Link::cast(link_node.clone()).expect("Should cast to Link");

    // syntax() should return reference to the same node
    assert_eq!(
        link.syntax().kind(),
        SyntaxKind::LINK,
        "syntax() should return the underlying LINK node"
    );
    assert_eq!(
        link.syntax().text().to_string(),
        link_node.text().to_string(),
        "syntax() should preserve node text"
    );
}

// =============================================================================
// Support Module Tests (indirect)
// =============================================================================

#[test]
fn support_child_finds_correct_child() {
    let input = "[text](url)\n";
    let tree = parse(input, None);

    let link_node = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::LINK)
        .expect("Should find LINK node");

    let link = Link::cast(link_node).expect("Should cast to Link");

    // support::child should find LINK_TEXT
    assert!(link.text().is_some(), "Should find LINK_TEXT child");

    // support::child should find LINK_DEST
    assert!(link.dest().is_some(), "Should find LINK_DEST child");
}

#[test]
fn support_children_iterator() {
    let input = "| A | B | C |\n|---|---|---|\n| 1 | 2 | 3 |\n";
    let tree = parse(input, None);

    if let Some(table_node) = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::PIPE_TABLE)
    {
        let table = PipeTable::cast(table_node).expect("Should cast to PipeTable");

        // support::children should iterate all TABLE_ROW children
        let rows: Vec<_> = table.rows().collect();
        assert!(!rows.is_empty(), "Should find TABLE_ROW children");

        // Check first row has cells
        if let Some(first_row) = rows.first() {
            let cells: Vec<_> = first_row.cells().collect();
            assert!(!cells.is_empty(), "Row should have TABLE_CELL children");
        }
    }
}

// =============================================================================
// Text Extraction from Nested Inline Elements
// =============================================================================
// These tests verify that text extraction methods correctly extract text
// from nested inline elements (emphasis, code, etc.), not just direct children.

#[test]
fn heading_text_with_nested_emphasis() {
    let input = "# Heading with *emphasis* and `code`\n";
    let tree = parse(input, None);

    let heading_node = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::HEADING)
        .expect("Should find HEADING node");

    let heading = Heading::cast(heading_node).expect("Should cast to Heading");

    // Should extract text from nested inline elements
    let text = heading.text();
    assert_eq!(
        text, "Heading with emphasis and code",
        "Heading text should extract text from nested inline elements"
    );
}

#[test]
fn link_text_with_nested_emphasis() {
    let input = "[text with *emphasis*](url)\n";
    let tree = parse(input, None);

    let link_text_node = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::LINK_TEXT)
        .expect("Should find LINK_TEXT node");

    let link_text = LinkText::cast(link_text_node).expect("Should cast to LinkText");

    // Should extract text from nested inline elements
    let content = link_text.text_content();
    assert_eq!(
        content, "text with emphasis",
        "Link text should extract text from nested inline elements"
    );
}

#[test]
fn image_alt_with_nested_emphasis() {
    let input = "![alt with *emphasis*](img.png)\n";
    let tree = parse(input, None);

    let alt_node = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::IMAGE_ALT)
        .expect("Should find IMAGE_ALT node");

    let alt = ImageAlt::cast(alt_node).expect("Should cast to ImageAlt");

    // Should extract text from nested inline elements
    let text = alt.text();
    assert_eq!(
        text, "alt with emphasis",
        "Image alt should extract text from nested inline elements"
    );
}

#[test]
fn table_caption_with_nested_emphasis() {
    let input = "| A |\n|---|\n| 1 |\n\n: Caption with *emphasis*\n";
    let tree = parse(input, None);

    if let Some(caption_node) = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::TABLE_CAPTION)
    {
        let caption = TableCaption::cast(caption_node).expect("Should cast to TableCaption");

        // Should extract text from nested inline elements
        let text = caption.text();
        assert_eq!(
            text, "Caption with emphasis",
            "Table caption should extract text from nested inline elements"
        );
    }
}

#[test]
fn link_ref_label_extraction() {
    let input = "[text][*emphasis* ref]\n\n[*emphasis* ref]: url\n";
    let tree = parse(input, None);

    if let Some(link_ref_node) = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::LINK_REF)
    {
        let link_ref = LinkRef::cast(link_ref_node).expect("Should cast to LinkRef");

        // NOTE: In this case, the parser doesn't create nested EMPHASIS nodes inside LINK_REF,
        // so the text contains the raw "*emphasis* ref" string.
        // This test documents the current behavior - need to verify if this is correct.
        let label = link_ref.label();

        // Currently returns "*emphasis* ref" (with asterisks, no nested emphasis parsing)
        // This may be correct Pandoc behavior - reference labels might not parse inline formatting
        println!("Link ref label: '{}'", label);
        assert!(
            label.contains("emphasis") || label.contains("*emphasis*"),
            "Link reference label should contain the reference text, got: '{}'",
            label
        );
    }
}

// =============================================================================
// Edge Cases
// =============================================================================

#[test]
fn empty_link_text() {
    let input = "[](url)\n";
    let tree = parse(input, None);

    if let Some(link_node) = tree.descendants().find(|n| n.kind() == SyntaxKind::LINK) {
        let link = Link::cast(link_node).expect("Should cast to Link");

        // Should still have text node, even if empty
        if let Some(text) = link.text() {
            let content = text.text_content();
            // Empty or whitespace only
            assert!(
                content.is_empty() || content.trim().is_empty(),
                "Empty link should have empty text content"
            );
        }
    }
}

#[test]
fn heading_without_text() {
    let input = "#\n";
    let tree = parse(input, None);

    if let Some(heading_node) = tree.descendants().find(|n| n.kind() == SyntaxKind::HEADING) {
        let heading = Heading::cast(heading_node).expect("Should cast to Heading");
        assert_eq!(heading.level(), 1, "Should still recognize level");
        // Text may be empty
        let _ = heading.text();
    }
}

#[test]
fn table_without_caption() {
    let input = "| A | B |\n|---|---|\n| 1 | 2 |\n";
    let tree = parse(input, None);

    if let Some(table_node) = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::PIPE_TABLE)
    {
        let table = PipeTable::cast(table_node).expect("Should cast to PipeTable");

        // caption() should return None without caption
        assert!(
            table.caption().is_none(),
            "Table without caption should return None"
        );
    }
}
