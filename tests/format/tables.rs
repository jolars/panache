use panache::config::WrapMode;
use panache::{Config, format};

#[test]
fn test_basic_pipe_table() {
    let input = "| A | B |\n|---|---|\n| C | D |";
    let expected = "  | A   | B   |\n  | --- | --- |\n  | C   | D   |\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_pipe_table_with_alignments() {
    let input = "| Left | Right | Center |\n|:---|---:|:---:|\n| A | B | C |";
    let expected =
        "  | Left | Right | Center |\n  | :--- | ----: | :----: |\n  | A    |     B |   C    |\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_pipe_table_uneven_widths() {
    let input = "| Short | Very long content here |\n|---|---|\n| X | Y |";
    let expected = "  | Short | Very long content here |\n  | ----- | ---------------------- |\n  | X     | Y                      |\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_pipe_table_with_inline_elements() {
    let input = "| *emphasis* | `code` |\n|---|---|\n| X | Y |";
    let expected =
        "  | *emphasis* | `code` |\n  | ---------- | ------ |\n  | X          | Y      |\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_pipe_table_idempotency() {
    let input = "| A | B |\n|---|---|\n| C | D |";

    let first_format = format(input, None, None);
    let second_format = format(&first_format, None, None);

    assert_eq!(first_format, second_format);
}

#[test]
fn test_pipe_table_with_caption_after() {
    let input = "| A | B |\n|---|---|\n| C | D |\n\n: Caption text";
    let expected = "  | A   | B   |\n  | --- | --- |\n  | C   | D   |\n\n  : Caption text\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_pipe_table_with_caption_before() {
    let input = ": Caption text\n\n| A | B |\n|---|---|\n| C | D |";
    let expected = "  : Caption text\n\n  | A   | B   |\n  | --- | --- |\n  | C   | D   |\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_pipe_table_caption_reflow_wraps() {
    let input = "| A | B |\n|---|---|\n| C | D |\n\n: A long caption that should wrap over multiple lines when reflow mode is enabled for formatting.";
    let config = Config {
        wrap: Some(WrapMode::Reflow),
        line_width: 56,
        ..Default::default()
    };

    let result = format(input, Some(config), None);
    assert!(result.contains("  : A long caption that should wrap over multiple lines"));
    assert!(result.contains("\n    when reflow mode is enabled for formatting."));
}

#[test]
fn test_pipe_table_caption_sentence_wraps() {
    let input =
        ": First caption sentence. Second caption sentence.\n\n| A | B |\n|---|---|\n| C | D |";
    let config = Config {
        wrap: Some(WrapMode::Sentence),
        line_width: 100,
        ..Default::default()
    };

    let result = format(input, Some(config), None);
    assert!(result.contains("  : First caption sentence.\n    Second caption sentence."));
}

#[test]
fn test_pipe_table_empty_cells() {
    let input = "| A | |\n|---|---|\n| | D |";
    let expected = "  | A   |     |\n  | --- | --- |\n  |     | D   |\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_pipe_table_single_column() {
    let input = "| Header |\n|---|\n| Cell |";
    let expected = "  | Header |\n  | ------ |\n  | Cell   |\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_pipe_table_multiple_rows() {
    let input = "| A | B |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n| 5 | 6 |";
    let expected =
        "  | A   | B   |\n  | --- | --- |\n  | 1   | 2   |\n  | 3   | 4   |\n  | 5   | 6   |\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_pipe_table_right_alignment() {
    let input = "| Number |\n|---:|\n| 12 |\n| 345 |\n| 6 |";
    let expected = "  | Number |\n  | -----: |\n  |     12 |\n  |    345 |\n  |      6 |\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_pipe_table_center_alignment() {
    let input = "| Center |\n|:---:|\n| X |\n| YYY |";
    let expected = "  | Center |\n  | :----: |\n  |   X    |\n  |  YYY   |\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_pipe_table_without_edge_pipes() {
    let input = "A | B\n---|---\nC | D";
    let expected = "  | A   | B   |\n  | --- | --- |\n  | C   | D   |\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

// Grid table tests
// ============================================================================

#[test]
fn test_basic_grid_table() {
    let input = "+-------+--------+\n| Left  | Right  |\n+=======+========+\n| A     | B      |\n+-------+--------+\n| C     | D      |\n+-------+--------+";
    let expected = "  +------+-------+\n  | Left | Right |\n  +======+=======+\n  | A    | B     |\n  +------+-------+\n  | C    | D     |\n  +------+-------+\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_grid_table_with_alignments() {
    let input = "+:------+-------:+:------:+\n| Left  | Right  | Center |\n+=======+========+========+\n| A     | B      | C      |\n+-------+--------+--------+";
    let expected = "  +------+-------+--------+\n  | Left | Right | Center |\n  +:=====+======:+:======:+\n  | A    |     B |   C    |\n  +------+-------+--------+\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_grid_table_uneven_widths() {
    let input = "+-------+------------------------+\n| Short | Very long content here |\n+=======+========================+\n| X     | Y                      |\n+-------+------------------------+";
    let expected = "  +-------+------------------------+\n  | Short | Very long content here |\n  +=======+========================+\n  | X     | Y                      |\n  +-------+------------------------+\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_grid_table_with_inline_elements() {
    let input = "+------------+----------+\n| *emphasis* | `code`   |\n+============+==========+\n| X          | Y        |\n+------------+----------+";
    let expected = "  +------------+--------+\n  | *emphasis* | `code` |\n  +============+========+\n  | X          | Y      |\n  +------------+--------+\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_grid_table_idempotency() {
    let input = "+-------+--------+\n| A     | B      |\n+=======+========+\n| C     | D      |\n+-------+--------+";

    let first_format = format(input, None, None);
    let second_format = format(&first_format, None, None);

    assert_eq!(first_format, second_format);
}

#[test]
fn test_headless_grid_table_with_alignments_idempotency() {
    let input = "+-----------------:+:----------+:----------:+\n| r1 a             | b         | c          |\n| r1 bis           | b 2       | c 2        |\n+------------------+-----------+------------+\n| r2 d             | e         | f          |\n+------------------+-----------+------------+";

    let first_format = format(input, None, None);
    let second_format = format(&first_format, None, None);

    assert_eq!(first_format, second_format);
}

#[test]
fn test_grid_table_multiline_cell_idempotency() {
    let input = "+-------+----------------------+\n| Var   | Desc                 |\n+=======+======================+\n| `A`   | First line           |\n|       |                      |\n|       | ```                  |\n|       | CODE=1               |\n|       | ```                  |\n+-------+----------------------+\n";

    let first_format = format(input, None, None);
    let second_format = format(&first_format, None, None);

    assert_eq!(
        first_format, second_format,
        "Grid table with multiline cell content must be idempotent"
    );
}

#[test]
fn test_grid_table_adjacent_code_spans_with_escaped_separators_idempotency() {
    let input = "+----------+\n| Pref     |\n+==========+\n| `small`\\> `medium`\\>`large` |\n+----------+\n";

    let first_format = format(input, None, None);
    let second_format = format(&first_format, None, None);

    assert_eq!(first_format, second_format);
}

#[test]
fn test_grid_table_with_spanning_style_rows_stays_idempotent() {
    let input = "+---------------------+----------+\n| Property            | Earth    |\n+=============+=======+==========+\n|             | min   | -89.2 °C |\n| Temperature +-------+----------+\n| 1961-1990   | mean  | 14 °C    |\n|             +-------+----------+\n|             | min   | 56.7 °C  |\n+-------------+-------+----------+\n";
    let first = format(input, None, None);
    let second = format(&first, None, None);
    assert_eq!(first, second);
}

#[test]
fn test_grid_table_with_caption_after() {
    let input = "+-----+-----+\n| A   | B   |\n+=====+=====+\n| C   | D   |\n+-----+-----+\n\nTable: Caption text";
    let expected =
        "  +---+---+\n  | A | B |\n  +===+===+\n  | C | D |\n  +---+---+\n\n  : Caption text\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_grid_table_planets_regression_case() {
    let input = include_str!("../fixtures/cases/grid_table_planets/input.md");
    let expected = include_str!("../fixtures/cases/grid_table_planets/expected.md");
    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_grid_table_multiline_header_and_footer_sections() {
    let input = "+---------+--------+\n| Name    | Value  |\n|         | (2020) |\n+:=======:+:======:+\n| Denmark | 5.8    |\n+---------+--------+\n+=========+========+\n| Total   | 5.8    |\n+=========+========+";
    let expected = "  +---------+--------+\n  |  Name   | Value  |\n  |         | (2020) |\n  +:=======:+:======:+\n  | Denmark |  5.8   |\n  +=========+========+\n  |  Total  |  5.8   |\n  +=========+========+\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_grid_table_empty_cells() {
    let input = "+-----+-----+\n| A   |     |\n+=====+=====+\n|     | D   |\n+-----+-----+";
    let expected = "  +---+---+\n  | A |   |\n  +===+===+\n  |   | D |\n  +---+---+\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_grid_table_single_column() {
    let input = "+--------+\n| Header |\n+========+\n| Cell   |\n+--------+";
    let expected = "  +--------+\n  | Header |\n  +========+\n  | Cell   |\n  +--------+\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_grid_table_multiple_rows() {
    let input = "+---+---+\n| A | B |\n+===+===+\n| 1 | 2 |\n+---+---+\n| 3 | 4 |\n+---+---+\n| 5 | 6 |\n+---+---+";
    let expected = "  +---+---+\n  | A | B |\n  +===+===+\n  | 1 | 2 |\n  +---+---+\n  | 3 | 4 |\n  +---+---+\n  | 5 | 6 |\n  +---+---+\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_grid_table_right_alignment() {
    let input = "+--------+\n| Number |\n+========+\n| 12     |\n+--------+\n| 345    |\n+--------+\n| 6      |\n+--------+";
    let expected = "  +--------+\n  | Number |\n  +========+\n  | 12     |\n  +--------+\n  | 345    |\n  +--------+\n  | 6      |\n  +--------+\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_grid_table_center_alignment() {
    let input =
        "+--------+\n| Center |\n+========+\n| X      |\n+--------+\n| YYY    |\n+--------+";
    let expected = "  +--------+\n  | Center |\n  +========+\n  | X      |\n  +--------+\n  | YYY    |\n  +--------+\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_multiline_table_idempotency() {
    let input = r#"-------------------------------------------------------------
 Centered   Default           Right Left
  Header    Aligned         Aligned Aligned
----------- ------- --------------- -------------------------
   First    row                12.0 Example of a row that
                                    spans multiple lines.

  Second    row                 5.0 Here's another one. Note
                                    the blank line between
                                    rows.
-------------------------------------------------------------
"#;

    let first_format = format(input, None, None);
    let second_format = format(&first_format, None, None);

    assert_eq!(
        first_format, second_format,
        "Multiline table formatting must be idempotent"
    );
}

#[test]
fn test_multiline_table_with_wide_chars_stays_idempotent() {
    let input = "---- ----\n魚    fish\n---- ----\n";
    let first = format(input, None, None);
    let second = format(&first, None, None);
    assert_eq!(first, second);
}

#[test]
fn test_simple_table_compresses_oversized_separator_columns() {
    let input = "   Right     Left\n -------     --------------\n     12         12\n   123          123\n       1        1\n";
    let expected = "    Right     Left\n  -------     ----\n       12     12\n      123     123\n        1     1\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}
