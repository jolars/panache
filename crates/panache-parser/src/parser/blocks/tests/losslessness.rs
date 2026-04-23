use crate::options::ParserOptions;
use crate::parser::Parser;

#[test]
fn test_losslessness_basic() {
    let input = "# H1\n\n### H3\n";
    let config = ParserOptions::default();
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
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_multiple_blank_lines() {
    let input = "\n\n\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_paragraph() {
    let input = "First line\nSecond line\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_indented_code_blank_line_with_spaces() {
    let input = "    A\n        \n    B\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_fenced_div_open_with_trailing_space() {
    let input = "::: {.panel-tabset group=\"language\"} \n\n## R\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_blockquote_list_continuation_lines() {
    let input = "> practical skills in:\n> \n> - Developing and integrating custom formats\n>   while reducing repetition across projects.\n> - Implementing filters to automate and streamline content\n>   transformation.\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_fenced_code_closing_fence_trailing_spaces() {
    let input = "````{.python}\ncity = \"Corvallis\"\n````    \n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_fenced_code_opening_fence_trailing_spaces() {
    let input = "```{r em-alg} \nem <- 1\n```\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_definition_first_line_trailing_spaces() {
    let input = "`repo`\n\n:   Add a link to repo:  \n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_grid_table_cell_with_leading_pipe_text() {
    let input = "+--------------------------+--------------------------+\n| ``` markdown             | | Line Block             |\n| | Line Block             | |    Spaces and newlines |\n+--------------------------+--------------------------+\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_grid_table_cell_with_nbsp() {
    let input = "+--------------------------------------------+----------------+\n| `QUARTO_FIG_WIDTH` and `QUARTO_FIG_HEIGHT` | Value          |\n+--------------------------------------------+----------------+\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_colon_definition_before_grid_table() {
    let input = "Misaligned separators in grid table \\`\\`\\`\n\n% pandoc -f markdown -t html\n:   Grid Table\n\n+-----------+---------------------------------+\n| Some text | [text]{.class1 .class2 .class3} |\n+===========+:===============================:+\n| Some text | [text]{.class1 .class2 .class3} |\n+-----------+---------------------------------+\n| Some text | [text]{.class1 .class2 .class3} |\n+-----------+---------------------------------+\n^D\n<table style=\"width:69%;\">\n<caption>Grid Table</caption>\n<colgroup>\n<col style=\"width: 25%\" />\n<col style=\"width: 44%\" />\n</colgroup>\n<tbody>\n<tr>\n<td>Some text</td>\n<td><span class=\"class1 class2 class3\">text</span></td>\n</tr>\n</tbody>\n</table> \\`\\`\\`\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_fenced_code_open_leading_space() {
    let input = " ```\n x\n ```\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_grid_table_spanning_style_row() {
    let input = "+-----------------------------------------+-----------------------------------------+\n| Student ID                              | Name                                    |\n+:========================================+:========================================+\n| Computer Science                                                                  |\n+-----------------------------------------+-----------------------------------------+\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_grid_table_three_col_row_with_asymmetric_padding() {
    let input = "+-------------------------+---------------------------+-----------------------+\n| `scale_fill_grey()`     | `scale_colour_grey()`     | Greyscale palette     |\n+-------------------------+---------------------------+-----------------------+\n| `scale_fill_viridis_d()`| `scale_colour_viridis_d()` |  Viridis palettes    |\n+-------------------------+---------------------------+-----------------------+\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_blockquote_fenced_code_lines() {
    let input = "> ~~~ {.xml}\n> <ruby>text</ruby>\n> ~~~\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_line_block_empty_marker_line() {
    let input = "| Hello\n|\n| Goodbye\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_horizontal_rule_with_leading_spaces() {
    let input = "before\n\n  ----\n\nafter\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_blockquote_atx_heading_with_attributes() {
    let input = "> ## Header attributes inside block quote {#foobar .baz key=\"val\"}\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_blockquote_tex_command_attribution_line() {
    let input = "> quote line\n>\n> \\medskip\n> \\hfill---Joe Armstrong\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_grid_table_wide_and_zero_width_chars() {
    let input = "+--+----+\n|魚|fish|\n+--+----+\n\n+-------+-------+\n|German |English|\n+-------+-------+\n|Auf‌lage|edition|\n+-------+-------+\n\n+-------+---------+\n|می‌خواهم|I want to|\n+-------+---------+\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_adjacent_tables_with_caption_between_and_following_heading() {
    let input = "| H1 | H2 |\n|----|----|\n| a  | b  |\nTable: first\n\n| J1 | J2 |\n|----|----|\n| c  | d  |\nTable: second\n\n### Exercises\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_triple_underscore_emphasis_preserves_delimiters() {
    let input = "a. ___License grant.___\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_blockquote_line_with_pipe_does_not_hang() {
    // Regression: this shape previously triggered a non-progress loop by
    // misdetecting a line block from blockquote-stripped content.
    let input = "> | When dollars appear it's a sign\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_blockquote_list_fenced_code_indentation() {
    let input = "> - One bullet.\n> \n>   ````\n>   ```{r, eval=TRUE}`r ''`\n>   ````\n>   ```r\n>   2 + 2\n>   ```\n>   ```\n>   ## [1] 4\n>   ```\n>   ````\n>   ```\n>   ````\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_hashpipe_block_scalar_in_list_fenced_chunk() {
    // Regression (#140): continuation metadata lines like `#| fig-alt: |`
    // must keep their original indentation in indented list contexts.
    let input = "- item\n\n    ```{r}\n    #| fig-cap: |\n    #|   A visual representation.\n    #| fig-alt: |\n    #|   Alt text.\n    plot(1:3)\n    ```\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn test_losslessness_multiline_table_blank_rows_and_following_captioned_simple_table() {
    let input = "Table: (\\#tab:basic-data-types) Types of variables encountered in typical data visualization scenarios.\n\n---------------------------------------------------------------------------------------------------------------------\nType of variable         Examples              Appropriate scale       Description\n------------------------ --------------------- ----------------------- ----------------------------------------------\nquantitative/numerical   1.3, 5.7, 83,         continuous              Arbitrary numerical values. These can be\ncontinuous               1.5x10^-2^                                    integers, rational numbers, or real numbers.\n \nquantitative/numerical   1, 2, 3, 4            discrete                Numbers in discrete units. These are most\ndiscrete                                                               commonly but not necessarily integers.\n                                                                       For example, the numbers 0.5, 1.0, 1.5 could\n                                                                       also be treated as discrete if intermediate\n                                                                       values cannot exist in the given dataset.\n                                                                       \nqualitative/categorical  good, fair, poor      discrete                Categories with order. These are discrete\nordered                                                                and unique categories with an order. For\n                                                                       example, \"fair\" always lies between \"good\"\n                                                                       and \"poor\". These variables are\n                                                                       also called *ordered factors*.\n\ndate or time             Jan. 5 2018, 8:03am   continuous or discrete  Specific days and/or times. Also\n                                                                       generic dates, such as July 4 or Dec. 25\n                                                                       (without year).\n\ntext                     The quick brown fox   none, or discrete       Free-form text. Can be treated\n                         jumps over the lazy                           as categorical if needed.\n                         dog.\n---------------------------------------------------------------------------------------------------------------------\n\nTable: (\\#tab:data-example) First 12 rows of a dataset listing daily temperature normals for four weather stations. Data source: NOAA.\n\n Month   Day  Location      Station ID   Temperature\n------- ----- ------------ ------------ -------------\n  Jan     1   Chicago      USW00014819        25.6\n";
    let config = ParserOptions::default();
    let parser = Parser::new(input, &config);
    let tree = parser.parse();
    assert_eq!(tree.text().to_string(), input);
}
