use panache::format;

#[test]
fn test_basic_pipe_table() {
    let input = "| A | B |\n|---|---|\n| C | D |";
    let expected = "| A   | B   |\n| --- | --- |\n| C   | D   |\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_pipe_table_with_alignments() {
    let input = "| Left | Right | Center |\n|:---|---:|:---:|\n| A | B | C |";
    let expected =
        "| Left | Right | Center |\n| :--- | ----: | :----: |\n| A    |     B |   C    |\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_pipe_table_uneven_widths() {
    let input = "| Short | Very long content here |\n|---|---|\n| X | Y |";
    let expected = "| Short | Very long content here |\n| ----- | ---------------------- |\n| X     | Y                      |\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_pipe_table_with_inline_elements() {
    let input = "| *emphasis* | `code` |\n|---|---|\n| X | Y |";
    let expected = "| *emphasis* | `code` |\n| ---------- | ------ |\n| X          | Y      |\n";

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
    let expected = "| A   | B   |\n| --- | --- |\n| C   | D   |\n\nTable: Caption text\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_pipe_table_with_caption_before() {
    let input = ": Caption text\n\n| A | B |\n|---|---|\n| C | D |";
    let expected = "Table: Caption text\n\n| A   | B   |\n| --- | --- |\n| C   | D   |\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_pipe_table_empty_cells() {
    let input = "| A | |\n|---|---|\n| | D |";
    let expected = "| A   |     |\n| --- | --- |\n|     | D   |\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_pipe_table_single_column() {
    let input = "| Header |\n|---|\n| Cell |";
    let expected = "| Header |\n| ------ |\n| Cell   |\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_pipe_table_multiple_rows() {
    let input = "| A | B |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n| 5 | 6 |";
    let expected = "| A   | B   |\n| --- | --- |\n| 1   | 2   |\n| 3   | 4   |\n| 5   | 6   |\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_pipe_table_right_alignment() {
    let input = "| Number |\n|---:|\n| 12 |\n| 345 |\n| 6 |";
    let expected = "| Number |\n| -----: |\n|     12 |\n|    345 |\n|      6 |\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_pipe_table_center_alignment() {
    let input = "| Center |\n|:---:|\n| X |\n| YYY |";
    let expected = "| Center |\n| :----: |\n|   X    |\n|  YYY   |\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_pipe_table_without_edge_pipes() {
    let input = "A | B\n---|---\nC | D";
    let expected = "| A   | B   |\n| --- | --- |\n| C   | D   |\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

// Grid table tests
// ============================================================================

#[test]
fn test_basic_grid_table() {
    let input = "+-------+--------+\n| Left  | Right  |\n+=======+========+\n| A     | B      |\n+-------+--------+\n| C     | D      |\n+-------+--------+";
    let expected = "+------+-------+\n| Left | Right |\n+======+=======+\n| A    | B     |\n+------+-------+\n| C    | D     |\n+------+-------+\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_grid_table_with_alignments() {
    let input = "+:------+-------:+:------:+\n| Left  | Right  | Center |\n+=======+========+========+\n| A     | B      | C      |\n+-------+--------+--------+";
    let expected = "+------+-------+--------+\n| Left | Right | Center |\n+:=====+======:+:======:+\n| A    |     B |   C    |\n+------+-------+--------+\n";

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
    let expected = "+------------+--------+\n| *emphasis* | `code` |\n+============+========+\n| X          | Y      |\n+------------+--------+\n";

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
fn test_grid_table_with_caption_after() {
    let input = "+-----+-----+\n| A   | B   |\n+=====+=====+\n| C   | D   |\n+-----+-----+\n\nTable: Caption text";
    let expected = "+---+---+\n| A | B |\n+===+===+\n| C | D |\n+---+---+\n\nTable: Caption text\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_grid_table_empty_cells() {
    let input = "+-----+-----+\n| A   |     |\n+=====+=====+\n|     | D   |\n+-----+-----+";
    let expected = "+---+---+\n| A |   |\n+===+===+\n|   | D |\n+---+---+\n";

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
