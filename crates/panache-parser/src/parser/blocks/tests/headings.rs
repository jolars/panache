use super::helpers::{find_first, parse_blocks};
use crate::options::ParserOptions;
use crate::parser::Parser;
use crate::syntax::{SyntaxKind, SyntaxNode};

fn get_heading_content(node: &SyntaxNode) -> Option<String> {
    find_first(node, SyntaxKind::HEADING_CONTENT).map(|n| n.text().to_string())
}

#[test]
fn parses_simple_atx_heading() {
    let node = parse_blocks("# Heading\n");
    let content = get_heading_content(&node).unwrap();
    assert_eq!(content, "Heading");
}

#[test]
fn empty_atx_heading() {
    let node = parse_blocks("# \n");
    let content = get_heading_content(&node).unwrap();
    assert_eq!(content, "");
}

#[test]
fn parses_atx_heading_with_leading_spaces() {
    let node = parse_blocks("  # Leading spaces\n");
    let content = get_heading_content(&node).unwrap();
    assert_eq!(content, "Leading spaces");
}

#[test]
fn parses_atx_heading_with_multiple_hashes() {
    let node = parse_blocks("### Subheading\n");
    let content = get_heading_content(&node).unwrap();
    assert_eq!(content, "Subheading");
}

#[test]
fn parses_atx_heading_with_trailing_hashes() {
    let node = parse_blocks("### Foo Bar ###\n");
    let content = get_heading_content(&node).unwrap();
    assert_eq!(content, "Foo Bar");
}

#[test]
fn does_not_parse_with_four_leading_spaces() {
    let node = parse_blocks("    # Not a heading\n");
    assert!(find_first(&node, SyntaxKind::HEADING).is_none());
}

#[test]
fn requires_blank_line_before_heading() {
    let node = parse_blocks("text\n# Heading\n");
    assert!(find_first(&node, SyntaxKind::HEADING).is_none());
}

#[test]
fn parses_heading_after_horizontal_rule_without_blank_line() {
    let node = parse_blocks("---\n# Heading\n");
    let headings: Vec<_> = node
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::HEADING)
        .collect();
    assert_eq!(headings.len(), 1);
}

#[test]
fn parses_heading_after_code_block_without_blank_line() {
    let node = parse_blocks("```r\nx\n```\n# Heading\n");
    let headings: Vec<_> = node
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::HEADING)
        .collect();
    assert_eq!(headings.len(), 1);
}

#[test]
fn parses_heading_without_blank_line_when_extension_disabled() {
    let mut config = ParserOptions::default();
    config.extensions.blank_before_header = false;
    let node = Parser::new("text\n# Heading\n", &config).parse();
    let headings: Vec<_> = node
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::HEADING)
        .collect();
    assert_eq!(headings.len(), 1);
}

#[test]
fn parses_setext_heading_without_blank_line_when_extension_disabled() {
    let mut config = ParserOptions::default();
    config.extensions.blank_before_header = false;
    let node = Parser::new("text\nHeading\n---\n", &config).parse();
    let headings: Vec<_> = node
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::HEADING)
        .collect();
    assert_eq!(headings.len(), 1);
}

#[test]
fn parses_heading_at_start_of_document() {
    let node = parse_blocks("# Start\n");
    let content = get_heading_content(&node).unwrap();
    assert_eq!(content, "Start");
}

#[test]
fn parses_multiple_headings() {
    let node = parse_blocks("# First\n\n## Second\n");
    let mut headings = node
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::HEADING_CONTENT);
    assert_eq!(headings.next().unwrap().text(), "First");
    assert_eq!(headings.next().unwrap().text(), "Second");
}

#[test]
fn parses_mmd_header_identifier_in_atx_when_enabled() {
    let mut config = ParserOptions::default();
    config.extensions.mmd_header_identifiers = true;
    let node = Parser::new("# Heading [my id]\n", &config).parse();

    let heading = find_first(&node, SyntaxKind::HEADING).expect("heading");
    let attr = heading
        .children()
        .find(|n| n.kind() == SyntaxKind::ATTRIBUTE)
        .expect("attribute");
    assert_eq!(attr.text().to_string(), "[my id]");
}

#[test]
fn does_not_parse_mmd_header_identifier_in_atx_when_disabled() {
    let mut config = ParserOptions::default();
    config.extensions.mmd_header_identifiers = false;
    let node = Parser::new("# Heading [my id]\n", &config).parse();

    let heading = find_first(&node, SyntaxKind::HEADING).expect("heading");
    assert!(
        heading
            .children()
            .all(|n| n.kind() != SyntaxKind::ATTRIBUTE),
        "mmd_header_identifiers disabled should keep [my id] in heading content"
    );
}

#[test]
fn parses_mmd_header_identifier_in_setext_when_enabled() {
    let mut config = ParserOptions::default();
    config.extensions.mmd_header_identifiers = true;
    let node = Parser::new("Heading [setext id]\n---\n", &config).parse();

    let heading = find_first(&node, SyntaxKind::HEADING).expect("heading");
    let attr = heading
        .children()
        .find(|n| n.kind() == SyntaxKind::ATTRIBUTE)
        .expect("attribute");
    assert_eq!(attr.text().to_string(), "[setext id]");
}

#[test]
fn atx_heading_immediately_after_yaml_frontmatter() {
    // Pandoc allows a heading directly after YAML frontmatter without a blank line.
    let input = "---\ntitle: Test\n---\n# Heading\n";
    let node = Parser::new(input, &ParserOptions::default()).parse();
    assert!(
        find_first(&node, SyntaxKind::HEADING).is_some(),
        "heading directly after YAML frontmatter should be parsed as a heading"
    );
}

#[test]
fn atx_heading_with_id_immediately_after_yaml_frontmatter() {
    // Heading IDs must be extractable when heading follows YAML directly.
    let input = "---\ntitle: Test\n---\n# One {#one}\n";
    let node = Parser::new(input, &ParserOptions::default()).parse();
    assert!(
        find_first(&node, SyntaxKind::HEADING).is_some(),
        "heading with ID directly after YAML frontmatter should be parsed as a heading"
    );
}

#[test]
fn parses_mmd_header_identifier_before_atx_closing_hashes() {
    let mut config = ParserOptions::default();
    config.extensions.mmd_header_identifiers = true;
    let input = "## Title [my id] ###\n";
    let node = Parser::new(input, &config).parse();

    let heading = find_first(&node, SyntaxKind::HEADING).expect("heading");
    let attr = heading
        .children()
        .find(|n| n.kind() == SyntaxKind::ATTRIBUTE)
        .expect("attribute");
    assert_eq!(attr.text().to_string(), "[my id]");
    assert_eq!(node.text().to_string(), input);
}
