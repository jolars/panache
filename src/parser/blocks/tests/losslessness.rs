use crate::config::Config;
use crate::parser::Parser;

#[test]
fn test_losslessness_basic() {
    let input = "# H1\n\n### H3\n";
    let config = Config::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
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
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_multiple_blank_lines() {
    let input = "\n\n\n";
    let config = Config::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_paragraph() {
    let input = "First line\nSecond line\n";
    let config = Config::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_indented_code_blank_line_with_spaces() {
    let input = "    A\n        \n    B\n";
    let config = Config::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_fenced_div_open_with_trailing_space() {
    let input = "::: {.panel-tabset group=\"language\"} \n\n## R\n";
    let config = Config::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_blockquote_list_continuation_lines() {
    let input = "> practical skills in:\n> \n> - Developing and integrating custom formats\n>   while reducing repetition across projects.\n> - Implementing filters to automate and streamline content\n>   transformation.\n";
    let config = Config::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_fenced_code_closing_fence_trailing_spaces() {
    let input = "````{.python}\ncity = \"Corvallis\"\n````    \n";
    let config = Config::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_definition_first_line_trailing_spaces() {
    let input = "`repo`\n\n:   Add a link to repo:  \n";
    let config = Config::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_grid_table_cell_with_leading_pipe_text() {
    let input = "+--------------------------+--------------------------+\n| ``` markdown             | | Line Block             |\n| | Line Block             | |    Spaces and newlines |\n+--------------------------+--------------------------+\n";
    let config = Config::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_grid_table_cell_with_nbsp() {
    let input = "+--------------------------------------------+----------------+\n| `QUARTO_FIG_WIDTH` and `QUARTO_FIG_HEIGHT` | Value          |\n+--------------------------------------------+----------------+\n";
    let config = Config::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}
