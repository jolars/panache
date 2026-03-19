use super::helpers::{assert_block_kinds, find_first, parse_blocks};
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
