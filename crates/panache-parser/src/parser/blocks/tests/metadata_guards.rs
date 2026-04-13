use super::helpers::{find_first, parse_blocks_with_config};
use crate::config::ParserOptions;
use crate::syntax::SyntaxKind;

#[test]
fn pandoc_title_block_disabled_does_not_parse_title_block() {
    let mut config = ParserOptions::default();
    config.extensions.pandoc_title_block = false;

    let tree = parse_blocks_with_config("% Title\n% Author\n\nBody\n", &config);
    assert!(
        find_first(&tree, SyntaxKind::PANDOC_TITLE_BLOCK).is_none(),
        "pandoc_title_block disabled should prevent title block parsing"
    );
}

#[test]
fn mmd_title_block_disabled_does_not_parse_title_block() {
    let mut config = ParserOptions::default();
    config.extensions.pandoc_title_block = false;
    config.extensions.mmd_title_block = false;

    let tree = parse_blocks_with_config("Title: My Title\nAuthor: Jane Doe\n\nBody\n", &config);
    assert!(
        find_first(&tree, SyntaxKind::MMD_TITLE_BLOCK).is_none(),
        "mmd_title_block disabled should prevent MMD title block parsing"
    );
}

#[test]
fn pandoc_title_block_takes_precedence_over_mmd_title_block() {
    let mut config = ParserOptions::default();
    config.extensions.pandoc_title_block = true;
    config.extensions.mmd_title_block = true;

    let tree = parse_blocks_with_config("% Title\n% Author\n\nBody\n", &config);
    assert!(find_first(&tree, SyntaxKind::PANDOC_TITLE_BLOCK).is_some());
    assert!(find_first(&tree, SyntaxKind::MMD_TITLE_BLOCK).is_none());
}

#[test]
fn yaml_metadata_block_disabled_does_not_parse_yaml_metadata() {
    let mut config = ParserOptions::default();
    config.extensions.yaml_metadata_block = false;

    let tree = parse_blocks_with_config("---\ntitle: Test\n---\nBody\n", &config);
    assert!(
        find_first(&tree, SyntaxKind::YAML_METADATA).is_none(),
        "yaml_metadata_block disabled should prevent YAML metadata parsing"
    );
}

#[test]
fn definition_lists_disabled_do_not_open_definition_list() {
    let mut config = ParserOptions::default();
    config.extensions.definition_lists = false;

    let tree = parse_blocks_with_config("Term\n: definition\n", &config);
    assert!(
        find_first(&tree, SyntaxKind::DEFINITION_LIST).is_none(),
        "definition_lists disabled should prevent definition list parsing"
    );
}

#[test]
fn fenced_divs_disabled_do_not_trigger_blank_before_header_logic() {
    let mut config = ParserOptions::default();
    config.extensions.fenced_divs = false;
    config.extensions.blank_before_header = true;

    let tree = parse_blocks_with_config("::: note\n# Heading\n", &config);
    assert!(
        find_first(&tree, SyntaxKind::HEADING).is_none(),
        "fenced_divs disabled should not treat ::: as opening fenced div for blank-before checks"
    );
}

#[test]
fn reference_links_disabled_does_not_parse_reference_definition() {
    let mut config = ParserOptions::default();
    config.extensions.reference_links = false;

    let tree = parse_blocks_with_config("[label]: https://example.com\n", &config);
    assert!(
        find_first(&tree, SyntaxKind::REFERENCE_DEFINITION).is_none(),
        "reference_links disabled should prevent reference definition parsing"
    );
}

#[test]
fn reference_links_enabled_parses_reference_definition() {
    let mut config = ParserOptions::default();
    config.extensions.reference_links = true;

    let tree = parse_blocks_with_config("[label]: https://example.com\n", &config);
    assert!(
        find_first(&tree, SyntaxKind::REFERENCE_DEFINITION).is_some(),
        "reference_links enabled should parse reference definitions"
    );
}

#[test]
fn mmd_link_attributes_disabled_does_not_consume_continuation_lines() {
    let mut config = ParserOptions::default();
    config.extensions.reference_links = true;
    config.extensions.mmd_link_attributes = false;

    let tree = parse_blocks_with_config(
        "[ref]: https://example.com \"Title\"\n    width=20px height=30px\n",
        &config,
    );

    let refdef = find_first(&tree, SyntaxKind::REFERENCE_DEFINITION).expect("reference definition");
    assert_eq!(
        refdef.text().to_string(),
        "[ref]: https://example.com \"Title\"\n"
    );
}

#[test]
fn mmd_link_attributes_enabled_consumes_continuation_lines() {
    let mut config = ParserOptions::default();
    config.extensions.reference_links = true;
    config.extensions.mmd_link_attributes = true;

    let tree = parse_blocks_with_config(
        "[ref]: https://example.com \"Title\"\n    width=20px height=30px\n\tid=myId class=\"myClass1 myClass2\"\n",
        &config,
    );

    let refdef = find_first(&tree, SyntaxKind::REFERENCE_DEFINITION).expect("reference definition");
    assert_eq!(
        refdef.text().to_string(),
        "[ref]: https://example.com \"Title\"\n    width=20px height=30px\n\tid=myId class=\"myClass1 myClass2\"\n"
    );
}
