use crate::block_parser::tests::helpers::{
    assert_block_kinds, count_children, find_all, find_first, get_text, parse_blocks,
};
use crate::syntax::SyntaxKind;

#[test]
fn simple_bullet_list() {
    let input = "* one\n* two\n* three\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 3);
}

#[test]
fn bullet_list_requires_space_after_marker() {
    let input = "*one\n*two\n";
    let tree = parse_blocks(input);
    // Should not parse as list
    assert!(find_first(&tree, SyntaxKind::List).is_none());
}

#[test]
fn bullet_list_with_different_markers() {
    let input = "* item\n+ item\n- item\n";
    let tree = parse_blocks(input);
    // Should create three separate lists (different markers)
    let lists = find_all(&tree, SyntaxKind::List);
    assert_eq!(lists.len(), 3);
}

#[test]
fn bullet_list_indented_1_to_3_spaces() {
    let input = " * one space\n  * two spaces\n   * three spaces\n";
    let tree = parse_blocks(input);
    // All should be valid list items
    let list_items = find_all(&tree, SyntaxKind::ListItem);
    assert_eq!(list_items.len(), 3);
}

#[test]
fn bullet_list_indented_4_spaces_is_code() {
    let input = "    * not a list\n";
    let tree = parse_blocks(input);
    // Should be code block, not list
    assert!(find_first(&tree, SyntaxKind::List).is_none());
}

#[test]
fn bullet_list_with_continuation() {
    let input = "* here is my first\n  list item.\n* and my second.\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 2);
}

#[test]
fn bullet_list_lazy_continuation() {
    let input = "* here is my first\nlist item.\n* and my second.\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 2);
}

#[test]
fn nested_bullet_lists() {
    let input = "* fruits\n  + apples\n  + pears\n* vegetables\n";
    let tree = parse_blocks(input);
    let outer_list = find_first(&tree, SyntaxKind::List).expect("should find outer list");
    assert_eq!(count_children(&outer_list, SyntaxKind::ListItem), 2);

    // Should have nested list inside first item
    let nested_lists = find_all(&tree, SyntaxKind::List);
    assert!(
        nested_lists.len() >= 2,
        "should have at least 2 lists (outer + nested)"
    );
}

#[test]
fn loose_list_with_blank_lines() {
    let input = "* one\n\n* two\n\n* three\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 3);
}

#[test]
fn simple_ordered_list() {
    let input = "1. one\n2. two\n3. three\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 3);
}

#[test]
fn ordered_list_numbers_ignored() {
    let input = "5. one\n7. two\n1. three\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 3);
}

#[test]
fn ordered_list_with_hash_marker() {
    let input = "#. one\n#. two\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 2);
}

#[test]
fn ordered_list_requires_space_after_marker() {
    let input = "1.one\n2.two\n";
    let tree = parse_blocks(input);
    // Should not parse as list
    assert!(find_first(&tree, SyntaxKind::List).is_none());
}

#[test]
fn mixed_markers_create_separate_lists() {
    let input = "(2) Two\n(5) Three\n1. Four\n* Five\n";
    let tree = parse_blocks(input);
    // Should create separate lists for each marker type
    let lists = find_all(&tree, SyntaxKind::List);
    assert!(lists.len() >= 3, "should have at least 3 separate lists");
}

#[test]
fn task_list_unchecked() {
    let input = "- [ ] unchecked task\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 1);
}

#[test]
fn task_list_checked() {
    let input = "- [x] checked task\n- [X] also checked\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 2);
}

#[test]
fn list_with_multiple_paragraphs() {
    let input = "* First paragraph.\n\n  Continued.\n\n* Second item.\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 2);
}

#[test]
fn list_after_blank_line() {
    let input = "\n* item\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::List).expect("should find list after blank");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 1);
}

#[test]
fn list_after_paragraph() {
    let input = "Not a list.\n\n* Now a list\n";
    let tree = parse_blocks(input);
    assert_block_kinds(
        input,
        &[
            SyntaxKind::PARAGRAPH,
            SyntaxKind::BlankLine,
            SyntaxKind::List,
        ],
    );
}
