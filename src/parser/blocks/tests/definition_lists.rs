use super::helpers::{assert_block_kinds, find_all, find_first, parse_blocks};
use crate::syntax::SyntaxKind;

#[test]
fn definition_list_allows_nested_list_after_blank_line() {
    let input = "Term\n\n:  Definition\n\n    - Bullet\n";
    let tree = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::DEFINITION_LIST]);
    assert!(
        find_first(&tree, SyntaxKind::LIST).is_some(),
        "Expected list to be nested inside definition"
    );
}

#[test]
fn definition_list_plain_does_not_start_list_without_blank_line() {
    let input = "A definition list with nested items\n:   Here comes a list (or wait, is it?)\n    - A\n    - B\n";
    let tree = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::DEFINITION_LIST]);

    assert!(
        find_first(&tree, SyntaxKind::LIST).is_none(),
        "Expected no list without blank line in definition"
    );
}

#[test]
fn definition_marker_without_content_preserves_newline_losslessly() {
    let input = "Input\n:   \n\n````markdown\n";
    let tree = parse_blocks(input);

    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn definition_content_can_start_with_atx_heading() {
    let input = "Term\n: # Header\n";
    let tree = parse_blocks(input);

    let definition = find_first(&tree, SyntaxKind::DEFINITION).expect("should find definition");

    assert!(
        find_first(&definition, SyntaxKind::HEADING).is_some(),
        "definition should contain HEADING"
    );
    assert!(
        find_first(&definition, SyntaxKind::PLAIN).is_none(),
        "heading-only definition should not be parsed as PLAIN"
    );
}

#[test]
fn definition_list_continues_across_blank_lines_with_additional_definitions() {
    let input = "Term\n: Def\n\n: Def\n";
    let tree = parse_blocks(input);

    let definition_lists = find_all(&tree, SyntaxKind::DEFINITION_LIST);
    assert_eq!(
        definition_lists.len(),
        1,
        "should remain one definition list"
    );

    let definition_items = find_all(&tree, SyntaxKind::DEFINITION_ITEM);
    assert_eq!(
        definition_items.len(),
        1,
        "should remain one definition item"
    );

    let definitions = find_all(&tree, SyntaxKind::DEFINITION);
    assert_eq!(
        definitions.len(),
        2,
        "should have two definitions for one term"
    );
}

#[test]
fn definition_marker_after_blank_line_does_not_create_orphan_item() {
    let input = "Term\n: Def\n\n: Def\n";
    let tree = parse_blocks(input);

    let definition_item = find_first(&tree, SyntaxKind::DEFINITION_ITEM).expect("definition item");
    let term_count = definition_item
        .children()
        .filter(|child| child.kind() == SyntaxKind::TERM)
        .count();
    assert_eq!(
        term_count, 1,
        "definition item should keep exactly one term"
    );
}
