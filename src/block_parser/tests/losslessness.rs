use crate::block_parser::BlockParser;
use crate::config::Config;

#[test]
fn test_losslessness_basic() {
    let input = "# H1\n\n### H3\n";
    let config = Config::default();
    let parser = BlockParser::new(input, &config);
    let (tree, _) = parser.parse();
    assert_eq!(
        tree.text().to_string(),
        input,
        "AST must preserve exact input (lossless CST)"
    );
}

#[test]
fn test_losslessness_no_trailing_newline() {
    let input = "# Heading";
    let config = Config::default();
    let parser = BlockParser::new(input, &config);
    let (tree, _) = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_multiple_blank_lines() {
    let input = "\n\n\n";
    let config = Config::default();
    let parser = BlockParser::new(input, &config);
    let (tree, _) = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_paragraph() {
    let input = "First line\nSecond line\n";
    let config = Config::default();
    let parser = BlockParser::new(input, &config);
    let (tree, _) = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}
