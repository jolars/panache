use panache_formatter::config::WrapMode;
use panache_formatter::{Config, format};

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

// An escaped pipe `\|` inside a table-cell code span is a literal pipe, not a
// column delimiter. The cell must stay one intact code span and the table must
// keep its original column count (regression: the row was re-split on `|`,
// producing a phantom 3rd column and rewriting `\|` to `\ |`).
#[test]
fn test_pipe_table_escaped_pipe_in_code_span() {
    let input = "| cmd | run |\n| --- | --- |\n| go | `curl x \\| sh` |";
    let expected =
        "  | cmd | run            |\n  | --- | -------------- |\n  | go  | `curl x \\| sh` |\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
    assert_eq!(format(&result, None, None), result);
}

// Guard: a genuine 3-column table (unescaped pipes) still formats as 3 columns.
#[test]
fn test_pipe_table_genuine_three_columns() {
    let input = "| a | b | c |\n| --- | --- | --- |\n| 1 | 2 | 3 |";
    let expected = "  | a   | b   | c   |\n  | --- | --- | --- |\n  | 1   | 2   | 3   |\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
    assert_eq!(format(&result, None, None), result);
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
    let expected = "  | A   | B   |\n  | --- | --- |\n  | C   | D   |\n\n  : Caption text\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_pipe_table_with_multiple_captions_preserves_both() {
    let input = ": A\n\n| a | b |\n|---|---|\n| A | B |\n\nTable: B";
    let expected = "  | a   | b   |\n  | --- | --- |\n  | A   | B   |\n\n  : A\n\nTable: B\n";

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
    let expected = "+-------+--------+\n| Left  | Right  |\n+=======+========+\n| A     | B      |\n+-------+--------+\n| C     | D      |\n+-------+--------+\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_grid_table_with_alignments() {
    let input = "+:------+-------:+:------:+\n| Left  | Right  | Center |\n+=======+========+========+\n| A     | B      | C      |\n+-------+--------+--------+";
    let expected = "+-------+--------+--------+\n| Left  | Right  | Center |\n+:======+=======:+:======:+\n| A     |      B |   C    |\n+-------+--------+--------+\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_grid_table_uneven_widths() {
    let input = "+-------+------------------------+\n| Short | Very long content here |\n+=======+========================+\n| X     | Y                      |\n+-------+------------------------+";
    let expected = "+-------+------------------------+\n| Short | Very long content here |\n+=======+========================+\n| X     | Y                      |\n+-------+------------------------+\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_grid_table_with_inline_elements() {
    let input = "+------------+----------+\n| *emphasis* | `code`   |\n+============+==========+\n| X          | Y        |\n+------------+----------+";
    let expected = "+------------+----------+\n| *emphasis* | `code`   |\n+============+==========+\n| X          | Y        |\n+------------+----------+\n";

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
fn test_grid_table_with_spanning_style_caption_before_normalizes_after() {
    let input = ": My caption\n\n+-------------+-------+----------+\n|             | min   | -89.2 °C |\n| Temperature +-------+----------+\n| 1961-1990   | mean  | 14 °C    |\n|             +-------+----------+\n|             | min   | 56.7 °C  |\n+-------------+-------+----------+\n";
    let expected = "+-------------+-------+----------+\n|             | min   | -89.2 °C |\n| Temperature +-------+----------+\n|  1961-1990  | mean  | 14 °C    |\n|             +-------+----------+\n|             | min   | 56.7 °C  |\n+-------------+-------+----------+\n\n: My caption\n";
    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_grid_table_with_caption_after() {
    let input = "+-----+-----+\n| A   | B   |\n+=====+=====+\n| C   | D   |\n+-----+-----+\n\nTable: Caption text";
    let expected = "+-----+-----+\n| A   | B   |\n+=====+=====+\n| C   | D   |\n+-----+-----+\n\n: Caption text\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_grid_table_planets_regression_case() {
    let input = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/cases/grid_table_planets/input.md"
    ));
    let expected = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/cases/grid_table_planets/expected.md"
    ));
    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_grid_table_multiline_header_and_footer_sections() {
    let input = "+---------+--------+\n| Name    | Value  |\n|         | (2020) |\n+:=======:+:======:+\n| Denmark | 5.8    |\n+---------+--------+\n+=========+========+\n| Total   | 5.8    |\n+=========+========+";
    let expected = "+---------+--------+\n|  Name   | Value  |\n|         | (2020) |\n+:=======:+:======:+\n| Denmark |  5.8   |\n+=========+========+\n|  Total  |  5.8   |\n+=========+========+\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_grid_table_empty_cells() {
    let input = "+-----+-----+\n| A   |     |\n+=====+=====+\n|     | D   |\n+-----+-----+";
    let expected = "+-----+-----+\n| A   |     |\n+=====+=====+\n|     | D   |\n+-----+-----+\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_grid_table_single_column() {
    let input = "+--------+\n| Header |\n+========+\n| Cell   |\n+--------+";
    let expected = "+--------+\n| Header |\n+========+\n| Cell   |\n+--------+\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_grid_table_multiple_rows() {
    let input = "+---+---+\n| A | B |\n+===+===+\n| 1 | 2 |\n+---+---+\n| 3 | 4 |\n+---+---+\n| 5 | 6 |\n+---+---+";
    let expected = "+---+---+\n| A | B |\n+===+===+\n| 1 | 2 |\n+---+---+\n| 3 | 4 |\n+---+---+\n| 5 | 6 |\n+---+---+\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_grid_table_right_alignment() {
    let input = "+--------+\n| Number |\n+========+\n| 12     |\n+--------+\n| 345    |\n+--------+\n| 6      |\n+--------+";
    let expected = "+--------+\n| Number |\n+========+\n| 12     |\n+--------+\n| 345    |\n+--------+\n| 6      |\n+--------+\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_grid_table_center_alignment() {
    let input =
        "+--------+\n| Center |\n+========+\n| X      |\n+--------+\n| YYY    |\n+--------+";
    let expected =
        "+--------+\n| Center |\n+========+\n| X      |\n+--------+\n| YYY    |\n+--------+\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_grid_table_in_list_item_keeps_container_indent() {
    // Top-level grid tables sit at column 0, but a grid nested in a list item
    // must keep the container indent so it still parses as a table (pandoc
    // strips the list prefix before recognizing the `+---+` border). The
    // formatter threads the container indent instead of a hardcoded one.
    let input = "- An item:\n\n  +---+---+\n  | a | b |\n  +===+===+\n  | 1 | 2 |\n  +===+===+\n";
    let first = format(input, None, None);
    let second = format(&first, None, None);
    assert_eq!(first, second, "list-nested grid table must be idempotent");
    assert!(
        first.contains("\n  +---+---+"),
        "grid border must keep the list container indent, got:\n{first}"
    );
}

#[test]
fn test_grid_table_preserves_wide_source_columns() {
    // Grid column widths carry relative-width info pandoc propagates to HTML
    // (<col style="width:X%">); the formatter must NOT shrink a wide column
    // down to its short content. See issue #323.
    let input = "+----------------+-----+\n| A              | B   |\n\
                 +================+=====+\n| C              | D   |\n\
                 +----------------+-----+\n";
    let result = format(input, None, None);
    assert!(
        result.contains("+----------------+-----+"),
        "expected source column widths to be preserved, got:\n{result}"
    );
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

// --- Column-spanning grid tables (issue #359) -------------------------------

#[test]
fn test_grid_colspan_preserves_all_cells_and_reflows() {
    // A header cell spanning both body columns must keep its span and the body
    // cells must not be truncated/padded away. See #359.
    let input = "+---------+\n|a        |\n+:=:+:===:+\n| aa|  ab |\n+---+-----+\n";
    let expected = "+---------+\n|    a    |\n+:==:+:==:+\n| aa | ab |\n+----+----+\n";
    assert_eq!(format(input, None, None), expected);
}

#[test]
fn test_grid_colspan_headerless_span_top_separator_alignment() {
    let input =
        "+:-----------:+\n|           a |\n+--+----------+\n|aa|ab        |\n+--+----------+\n";
    let expected = "+:-------:+\n|    a    |\n+----+----+\n| aa | ab |\n+----+----+\n";
    assert_eq!(format(input, None, None), expected);
}

#[test]
fn test_grid_colspan_does_not_drop_spanned_columns() {
    // Five fine columns with a two-cell spanning header. The structured path
    // used to truncate every row to the first separator's 3 columns, dropping
    // the 4th/5th cells; the span-aware path keeps them.
    let input = concat!(
        "+-----+-------------------+-------------------+\n",
        "|     | $s = 3$           | $s = 2$           |\n",
        "+-----+---------+---------+---------+---------+\n",
        "| $i$ | $m_{i}$ | $r_{i}$ | $m_{i}$ | $r_{i}$ |\n",
        "+=====+=========+=========+=========+=========+\n",
        "| 0   | 8       | 1       | 6       | 2       |\n",
        "+-----+---------+---------+---------+---------+\n",
    );
    let result = format(input, None, None);
    assert!(result.contains("| 0   | 8       | 1       | 6       | 2       |"));
    assert!(result.contains("| $i$ | $m_{i}$ | $r_{i}$ | $m_{i}$ | $r_{i}$ |"));
}

#[test]
fn test_grid_colspan_is_idempotent() {
    let input = "+---------+\n|a        |\n+:=:+:===:+\n| aa|  ab |\n+---+-----+\n";
    let first = format(input, None, None);
    let second = format(&first, None, None);
    assert_eq!(first, second);
}

#[test]
fn test_grid_colspan_misaligned_pipe_is_preserved_losslessly() {
    // A content pipe that lands on no separator boundary can't be laid out on
    // the grid; the table is preserved verbatim rather than emitting borders
    // that don't line up.
    let input = "+---+---+\n| a | b |\n+---+---+\n| a very wide spanning header |\n+---+---+\n| c | d |\n+---+---+\n";
    let result = format(input, None, None);
    assert!(result.contains("| a very wide spanning header |"));
    assert_eq!(format(&result, None, None), result, "must be idempotent");
}
