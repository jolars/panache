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
fn test_losslessness_fenced_code_opening_fence_trailing_spaces() {
    let input = "```{r em-alg} \nem <- 1\n```\n";
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

#[test]
fn test_losslessness_colon_definition_before_grid_table() {
    let input = "Misaligned separators in grid table \\`\\`\\`\n\n% pandoc -f markdown -t html\n:   Grid Table\n\n+-----------+---------------------------------+\n| Some text | [text]{.class1 .class2 .class3} |\n+===========+:===============================:+\n| Some text | [text]{.class1 .class2 .class3} |\n+-----------+---------------------------------+\n| Some text | [text]{.class1 .class2 .class3} |\n+-----------+---------------------------------+\n^D\n<table style=\"width:69%;\">\n<caption>Grid Table</caption>\n<colgroup>\n<col style=\"width: 25%\" />\n<col style=\"width: 44%\" />\n</colgroup>\n<tbody>\n<tr>\n<td>Some text</td>\n<td><span class=\"class1 class2 class3\">text</span></td>\n</tr>\n</tbody>\n</table> \\`\\`\\`\n";
    let config = Config::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_fenced_code_open_leading_space() {
    let input = " ```\n x\n ```\n";
    let config = Config::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_grid_table_spanning_style_row() {
    let input = "+-----------------------------------------+-----------------------------------------+\n| Student ID                              | Name                                    |\n+:========================================+:========================================+\n| Computer Science                                                                  |\n+-----------------------------------------+-----------------------------------------+\n";
    let config = Config::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_blockquote_fenced_code_lines() {
    let input = "> ~~~ {.xml}\n> <ruby>text</ruby>\n> ~~~\n";
    let config = Config::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_line_block_empty_marker_line() {
    let input = "| Hello\n|\n| Goodbye\n";
    let config = Config::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_horizontal_rule_with_leading_spaces() {
    let input = "before\n\n  ----\n\nafter\n";
    let config = Config::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_blockquote_atx_heading_with_attributes() {
    let input = "> ## Header attributes inside block quote {#foobar .baz key=\"val\"}\n";
    let config = Config::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_blockquote_tex_command_attribution_line() {
    let input = "> quote line\n>\n> \\medskip\n> \\hfill---Joe Armstrong\n";
    let config = Config::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_grid_table_wide_and_zero_width_chars() {
    let input = "+--+----+\n|魚|fish|\n+--+----+\n\n+-------+-------+\n|German |English|\n+-------+-------+\n|Auf‌lage|edition|\n+-------+-------+\n\n+-------+---------+\n|می‌خواهم|I want to|\n+-------+---------+\n";
    let config = Config::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_adjacent_tables_with_caption_between_and_following_heading() {
    let input = "| H1 | H2 |\n|----|----|\n| a  | b  |\nTable: first\n\n| J1 | J2 |\n|----|----|\n| c  | d  |\nTable: second\n\n### Exercises\n";
    let config = Config::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_triple_underscore_emphasis_preserves_delimiters() {
    let input = "a. ___License grant.___\n";
    let config = Config::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_blockquote_line_with_pipe_does_not_hang() {
    // Regression: this shape previously triggered a non-progress loop by
    // misdetecting a line block from blockquote-stripped content.
    let input = "> | When dollars appear it's a sign\n";
    let config = Config::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_blockquote_list_fenced_code_indentation() {
    let input = "> - One bullet.\n> \n>   ````\n>   ```{r, eval=TRUE}`r ''`\n>   ````\n>   ```r\n>   2 + 2\n>   ```\n>   ```\n>   ## [1] 4\n>   ```\n>   ````\n>   ```\n>   ````\n";
    let config = Config::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}
