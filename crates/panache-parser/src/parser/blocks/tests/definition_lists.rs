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
fn definition_list_content_starting_with_list_marker_parses_as_list() {
    let input = "Term\n:   - One\n    - Two\n";
    let tree = parse_blocks(input);

    let definition = find_first(&tree, SyntaxKind::DEFINITION).expect("should find definition");

    assert!(
        find_first(&definition, SyntaxKind::LIST).is_some(),
        "definition should contain LIST when content starts with list marker"
    );

    let has_direct_plain_child = definition
        .children()
        .any(|child| child.kind() == SyntaxKind::PLAIN);
    assert!(
        !has_direct_plain_child,
        "list-only definition should not have a direct PLAIN child"
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

#[test]
fn definition_marker_after_list_definition_closes_nested_list() {
    let input = "Orange\n:   - a\n    - b\n:   Also a color\n";
    let tree = parse_blocks(input);

    let definition_item = find_first(&tree, SyntaxKind::DEFINITION_ITEM).expect("definition item");
    let definitions = definition_item
        .children()
        .filter(|child| child.kind() == SyntaxKind::DEFINITION)
        .count();
    assert_eq!(
        definitions, 2,
        "marker after list definition should create a sibling definition"
    );

    let nested_definition_item = definition_item
        .descendants()
        .any(|node| node.kind() == SyntaxKind::DEFINITION_ITEM && node != definition_item);
    assert!(
        !nested_definition_item,
        "list content should not capture a nested DEFINITION_ITEM"
    );
}

#[test]
fn colon_table_caption_before_table_is_not_definition_list() {
    let input = "Here's a table with a reference:\n\n: (\\#tab:mytable) A table with a reference.\n\n| A   | B   | C   |\n| --- | --- | --- |\n| 1   | 2   | 3   |\n";
    let tree = parse_blocks(input);

    assert!(
        find_first(&tree, SyntaxKind::DEFINITION_LIST).is_none(),
        "colon table caption before a table should not be parsed as DEFINITION_LIST"
    );
    assert!(
        find_first(&tree, SyntaxKind::PIPE_TABLE).is_some(),
        "expected PIPE_TABLE to be parsed for colon caption + table"
    );
    assert!(
        find_first(&tree, SyntaxKind::TABLE_CAPTION).is_some(),
        "expected TABLE_CAPTION node for colon caption"
    );
}
