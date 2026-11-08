use panache_formatter::config::WrapMode;
use panache_formatter::{Config, ConfigBuilder, SmartPunctuation, TabStopMode, format};

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
fn test_pipe_table_discards_excess_source_padding() {
    use unicode_width::UnicodeWidthStr;

    for padding in [96, 300] {
        let input = format!(
            "| A | B |\n| --- | --------- |\n| 界{} | b |\n",
            " ".repeat(padding)
        );
        let output = format(&input, None, None);
        for line in output.lines() {
            assert_eq!(line.trim_start().width(), 73, "{output}");
        }
        assert_eq!(format(&output, None, None), output);
    }
}

#[test]
fn test_pipe_table_escaped_pipe_in_code_span() {
    let input = "| cmd | run |\n| --- | --- |\n| go | `curl x \\| sh` |";
    let expected =
        "  | cmd | run            |\n  | --- | -------------- |\n  | go  | `curl x \\| sh` |\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
    assert_eq!(format(&result, None, None), result);
}

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
    let expected = "+-------+--------+--------+\n| Left  | Right  | Center |\n+=======+========+========+\n| A     | B      | C      |\n+-------+--------+--------+\n";

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
    let expected = "+-------------+-------+----------+\n|             | min   | -89.2 °C |\n| Temperature +-------+----------+\n| 1961-1990   | mean  | 14 °C    |\n|             +-------+----------+\n|             | min   | 56.7 °C  |\n+-------------+-------+----------+\n\n: My caption\n";
    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_grid_table_rowspan_reads_alignment_from_colons() {
    let input = "+--------+--------+\n| Name   | Value  |\n+:======:+=======:+\n| group  | 1.5    |\n| spans  +--------+\n| rows   | 22.0   |\n+--------+--------+\n";
    let result = format(input, None, None);
    assert_eq!(result, input);
    assert_eq!(format(&result, None, None), result, "must be idempotent");
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
    let expected = format!("{input}\n");

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
fn test_multiline_table_code_tabs_use_source_columns() {
    let input = concat!(
        "-------------------------------\n",
        "h1         `h\ti`\n",
        "---------- --------------------\n",
        "a          `x\ty`\n",
        "           *`u\tv`*\n",
        "-------------------------------\n",
    );
    for prefix in ["", "> "] {
        let input: String = input
            .lines()
            .map(|line| format!("{prefix}{line}\n"))
            .collect();
        for mode in [
            SmartPunctuation::Preserve,
            SmartPunctuation::Ascii,
            SmartPunctuation::Unicode,
        ] {
            for wrap in [None, Some(WrapMode::Preserve), Some(WrapMode::Reflow)] {
                for tab_width in [2, 4] {
                    let config = Config {
                        smart_punctuation: mode,
                        wrap: wrap.clone(),
                        tab_width,
                        table_indent: 0,
                        ..Config::default()
                    };
                    let output = format(&input, Some(config.clone()), None);
                    let spaces = " ".repeat(tab_width - ((prefix.len() + 13) % tab_width));
                    assert!(output.contains(&format!("`h{spaces}i`")), "{output:?}");
                    assert!(output.contains(&format!("`x{spaces}y`")), "{output:?}");
                    let spaces = " ".repeat(tab_width - ((prefix.len() + 14) % tab_width));
                    assert!(output.contains(&format!("*`u{spaces}v`*")), "{output:?}");
                    assert_eq!(format(&output, Some(config), None), output, "idempotency");
                }
            }
        }
    }
}

#[test]
fn test_multiline_table_slices_columns_after_tabs_in_earlier_cells() {
    for prefix in ["", "> "] {
        for tab_width in [2, 4] {
            let left_spaces = " ".repeat(tab_width - ((prefix.len() + 2) % tab_width));
            let left = format!("`a{left_spaces}b`c");
            let gutter = " ".repeat(11 - left.len());
            let right_spaces = " ".repeat(tab_width - ((prefix.len() + 13) % tab_width));
            let input = format!(
                "--------------------------\nh1         h2\n---------- ---------------\n`a\tb`c{gutter}`x\ty`\n--------------------------\n"
            );
            let expected = input
                .replace("`a\tb`c", &left)
                .replace("`x\ty`", &format!("`x{right_spaces}y`"));
            let quoted = |text: &str| -> String {
                text.lines()
                    .map(|line| format!("{prefix}{line}\n"))
                    .collect()
            };
            for wrap in [WrapMode::Preserve, WrapMode::Reflow] {
                let config = Config {
                    wrap: Some(wrap),
                    tab_width,
                    table_indent: 0,
                    ..Config::default()
                };
                let output = format(&quoted(&input), Some(config.clone()), None);
                assert_eq!(output, quoted(&expected));
                assert_eq!(format(&output, Some(config), None), output, "idempotency");
            }
        }
    }
}

#[test]
fn test_multiline_table_preserve_removes_lines_vacated_by_code_spans() {
    for (body, expected_body) in [
        ("a          `x\n           y`\n", "a          `x y`\n"),
        ("a          `x\nb          y`\n", "a          `x y`\nb\n"),
        (
            "a          `x\n           y\nb          z`\n",
            "a          `x y z`\nb\n",
        ),
    ] {
        let input = format!(
            "--------------------------\nh1         h2\n---------- ---------------\n{body}--------------------------\n"
        );
        let expected = format!(
            "--------------------------\nh1         h2\n---------- ---------------\n{expected_body}--------------------------\n"
        );
        let config = Config {
            wrap: Some(WrapMode::Preserve),
            table_indent: 0,
            ..Config::default()
        };
        let output = format(&input, Some(config.clone()), None);
        assert_eq!(output, expected);
        assert_eq!(format(&output, Some(config), None), output, "idempotency");
    }
}

#[test]
fn test_multiline_table_preserves_literal_code_tabs() {
    let input = "---------------------\nh1         h2\n---------- ----------\na          `x\ty`\n---------------------\n";
    let config = Config {
        tab_stops: TabStopMode::Preserve,
        table_indent: 0,
        ..Config::default()
    };
    let output = format(input, Some(config.clone()), None);
    assert!(output.contains("`x\ty`"), "{output:?}");
    assert_eq!(format(&output, Some(config), None), output);
}

#[test]
fn test_multiline_table_continued_code_tabs_use_source_columns() {
    let input = "-------------------------------\nh1         h2\n---------- --------------------\na          `x\n           y\tz`\n-------------------------------\n";
    let config = Config {
        table_indent: 0,
        ..Config::default()
    };
    let output = format(input, Some(config.clone()), None);
    assert!(output.contains("`x y    z`"), "{output:?}");
    assert_eq!(format(&output, Some(config), None), output);
}

#[test]
fn test_multiline_table_source_context_stays_outside_inline_content() {
    for (first, second) in [("[x]{.foo", "key=v}"), ("\\foo{a", "b}"), ("^[one", "two]")] {
        let input = format!(
            "{border}\nh1         h2\n---------- {column}\nalpha      {first}\nbeta       {second}\n{border}\n",
            border = "-".repeat(72),
            column = "-".repeat(61),
        );
        let output = format(&input, None, None);
        assert_eq!(output.matches("alpha").count(), 1, "{output:?}");
        assert_eq!(output.matches("beta").count(), 1, "{output:?}");
        assert_eq!(format(&output, None, None), output);
    }
}

#[test]
fn test_multiline_table_preserves_hard_breaks() {
    for (first, second) in [("one\\", "two"), ("*one\\", "two*")] {
        let input = format!(
            "---------------------\nh1         h2\n---------- ----------\na          {first}\n           {second}\n---------------------\n"
        );
        for wrap in [None, Some(WrapMode::Preserve), Some(WrapMode::Reflow)] {
            let config = Config {
                wrap,
                table_indent: 0,
                ..Config::default()
            };
            let output = format(&input, Some(config.clone()), None);
            assert_eq!(output, input, "{:?}", config.wrap);
            assert_eq!(format(&output, Some(config), None), output, "idempotency");
        }
    }
}

#[test]
fn test_multiline_table_with_wide_chars_stays_idempotent() {
    let input = "---- ----\n魚    fish\n---- ----\n";
    let first = format(input, None, None);
    let second = format(&first, None, None);
    assert_eq!(first, second);
}

#[test]
fn test_simple_table_normalizes_oversized_separator_columns() {
    let input = "   Right     Left\n -------     --------------\n     12         12\n   123          123\n       1        1\n";
    let expected = "    Right Left\n  ------- ------\n       12 12\n      123 123\n        1 1\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_pipe_table_indent_zero_is_flush() {
    let input = "| A | B |\n|---|---|\n| C | D |";
    let expected = "| A   | B   |\n| --- | --- |\n| C   | D   |\n";

    let config = ConfigBuilder::default().table_indent(0).build();
    let result = format(input, Some(config.clone()), None);
    assert_eq!(result, expected);
    assert_eq!(format(&result, Some(config), None), result);
}

#[test]
fn test_pipe_table_default_indent_is_two() {
    let input = "| A | B |\n|---|---|\n| C | D |";
    let expected = "  | A   | B   |\n  | --- | --- |\n  | C   | D   |\n";

    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn test_pipe_table_indent_three() {
    let input = "| A | B |\n|---|---|\n| C | D |";
    let expected = "   | A   | B   |\n   | --- | --- |\n   | C   | D   |\n";

    let config = ConfigBuilder::default().table_indent(3).build();
    let result = format(input, Some(config), None);
    assert_eq!(result, expected);
}

#[test]
fn test_simple_table_honors_table_indent() {
    let input = "   Right     Left\n -------     --------------\n     12         12\n   123          123\n       1        1\n";
    let expected = "  Right Left\n------- ------\n     12 12\n    123 123\n      1 1\n";

    let config = ConfigBuilder::default().table_indent(0).build();
    let result = format(input, Some(config), None);
    assert_eq!(result, expected);
}

#[test]
fn test_grid_colspan_preserves_all_cells_and_reflows() {
    let input = "+---------+\n|a        |\n+:=:+:===:+\n| aa|  ab |\n+---+-----+\n";
    let expected = "+----------+\n| a        |\n+:==:+:===:+\n| aa |  ab |\n+----+-----+\n";
    assert_eq!(format(input, None, None), expected);
}

#[test]
fn test_grid_colspan_headerless_span_top_separator_alignment() {
    let input =
        "+:-----------:+\n|           a |\n+--+----------+\n|aa|ab        |\n+--+----------+\n";
    let expected = "+:-------------:+\n|           a   |\n+----+----------+\n| aa | ab       |\n+----+----------+\n";
    assert_eq!(format(input, None, None), expected);
}

#[test]
fn test_grid_colspan_does_not_drop_spanned_columns() {
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
    let input = "+---+---+\n| a | b |\n+---+---+\n| a very wide spanning header |\n+---+---+\n| c | d |\n+---+---+\n";
    let result = format(input, None, None);
    assert!(result.contains("| a very wide spanning header |"));
    assert_eq!(format(&result, None, None), result, "must be idempotent");
}

#[test]
fn test_simple_table_cell_wider_than_dash_run_is_not_truncated() {
    let input = "Fruit            Price\n-----            -----\nboysenberries    9\n";
    let result = format(input, None, None);
    assert!(
        result.contains("boysenberries"),
        "wide cell must be preserved, got:\n{result}"
    );
    assert_eq!(format(&result, None, None), result, "must be idempotent");
}

#[test]
fn test_simple_table_spacing_is_normalized_independent_of_source() {
    let a = "Fruit            Price\n-------------    -----\nboysenberries    9\n";
    let b = "Fruit           Price\n--------------- -------\nboysenberries   9\n";
    let expected = "  Fruit           Price\n  --------------- -------\n  boysenberries   9\n";

    assert_eq!(format(a, None, None), expected);
    assert_eq!(format(b, None, None), expected);
}

#[test]
fn test_headerless_simple_table_alignment_from_first_data_row() {
    let config = ConfigBuilder::default().table_indent(0).build();
    let input = concat!(
        "-------     ------ ----------   -------\n",
        "     12     12        12             12\n",
        "    123     123       123           123\n",
        "      1     1          1              1\n",
        "-------     ------ ----------   -------\n",
    );
    let expected = concat!(
        "----- ----- ----- -----\n",
        "   12 12     12      12\n",
        "  123 123    123    123\n",
        "    1 1       1       1\n",
        "----- ----- ----- -----\n",
    );
    let result = format(input, Some(config.clone()), None);
    assert_eq!(result, expected, "got:\n{result}");
    assert_eq!(format(&result, Some(config), None), result, "idempotent");
}

#[test]
fn test_simple_table_dash_run_is_content_width_plus_two() {
    let config = ConfigBuilder::default().table_indent(0).build();
    let input = "Fruit           Price\n--------------- -------\nboysenberries   9\n";
    let result = format(input, Some(config), None);
    let sep = result.lines().nth(1).unwrap();
    assert_eq!(sep, "--------------- -------", "got:\n{result}");
}

/// A row short of the delimiter's count is padded out, so the delimiter row
/// keeps all three of its columns (`pandoc -f markdown` reads three columns,
/// the header's third one empty).
#[test]
fn short_rows_are_padded_to_the_delimiter_count() {
    let config = ConfigBuilder::default().table_indent(0).build();
    let input = "a | b\n---|---|---\n1 | 2 | 3\n";
    let expected = "| a   | b   |     |\n| --- | --- | --- |\n| 1   | 2   | 3   |\n";

    let result = format(input, Some(config.clone()), None);
    assert_eq!(result, expected, "got:\n{result}");
    assert_eq!(format(&result, Some(config), None), result, "idempotent");
}

/// A row carrying more cells than the delimiter row allows is left exactly as
/// written. Pandoc drops those cells on render, so reformatting the table
/// would either delete the author's text or widen the delimiter row and
/// change the rendered column count.
#[test]
fn surplus_cells_leave_the_table_verbatim() {
    let config = ConfigBuilder::default().table_indent(0).build();
    let input = "a | b | c\n---|---\n1 | 2 | 3\n";

    let result = format(input, Some(config.clone()), None);
    assert_eq!(result, input, "got:\n{result}");
    assert_eq!(format(&result, Some(config), None), result, "idempotent");
}

/// A surplus-cell table on a list marker line stays verbatim too. The splice
/// that puts the table's first line on the marker line used to slice a fixed
/// number of bytes off it, which ate the leading `| ` of an unindented
/// verbatim table and deleted a cell on every further pass.
#[test]
fn surplus_cells_in_a_list_item_leave_the_table_verbatim() {
    let config = ConfigBuilder::default().table_indent(0).build();
    let input = "- | a | b |\n  - |\n";

    let result = format(input, Some(config.clone()), None);
    assert_eq!(result, input, "got:\n{result}");
    assert_eq!(format(&result, Some(config), None), result, "idempotent");
}

/// Pandoc reads a simple table's alignment off the header's position against
/// the dash runs, and the emitted table is *wider* than the source whenever a
/// body cell is the widest in its column. Bounding the header slice at the
/// header line's length made the verdict fall back to `AlignDefault` on that
/// second pass, so a centered column wobbled between two layouts forever.
#[test]
fn test_simple_table_alignment_survives_a_widened_column() {
    let config = ConfigBuilder::default().table_indent(0).build();
    let input = "bb  x  y\n----- ---\nx  y  zz   w\n";
    let expected = "bb x     y\n------ ------\nx y     zz w\n";

    let result = format(input, Some(config.clone()), None);
    assert_eq!(result, expected, "got:\n{result}");
    assert_eq!(format(&result, Some(config), None), result, "idempotent");
}
