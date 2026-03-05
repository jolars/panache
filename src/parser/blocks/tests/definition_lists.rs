use super::helpers::{assert_block_kinds, find_first, parse_blocks};
use crate::syntax::SyntaxKind;

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
