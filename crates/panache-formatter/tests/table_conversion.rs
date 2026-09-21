use panache_formatter::syntax::{SyntaxKind, Table};
use panache_formatter::{Config, TableConversionError, TableStyle, convert_table, format};

fn convert(input: &str, target: TableStyle) -> Result<String, TableConversionError> {
    let config = Config::default();
    let tree = panache_formatter::parser::parse(input, Some(config.parser_options()));
    let table = tree
        .descendants()
        .find_map(Table::cast)
        .expect("source table");
    convert_table(&table, target, &config, 40)
}

fn assert_style(text: &str, kind: SyntaxKind) {
    let tree = panache_formatter::parser::parse(text, None);
    let table = tree.descendants().find_map(Table::cast).expect(text);
    assert_eq!(table.syntax().kind(), kind, "{text}");
}

#[test]
fn converts_grid_sources_without_losing_wrapped_cells() {
    let source = "+---------+-----+\n| A       | B   |\n+=========+=====+\n| one two | 界  |\n| three   |     |\n+---------+-----+\n\n: Caption {#tbl-id}\n";
    for target in [TableStyle::Pipe, TableStyle::Simple, TableStyle::Multiline] {
        let output = convert(source, target).unwrap();
        assert_style(&output, target.syntax_kind());
        assert!(output.contains("one two three"));
        assert!(output.contains("界"));
        assert!(output.contains(": Caption {#tbl-id}"));
    }
}

#[test]
fn grid_formatting_preserves_prose_in_aligned_columns() {
    let source = "+-----------+\n| Header    |\n+==========:+\n| x         |\n+-----------+\n";
    let output = format(source, None, None);
    assert!(output.contains("| x         |"), "{output}");
    assert_eq!(format(&output, None, None), output);
    if std::process::Command::new("pandoc")
        .arg("--version")
        .output()
        .is_ok()
    {
        assert_eq!(pandoc(&output), pandoc(source));
    }
}

#[test]
fn spanning_grid_formatting_preserves_indented_code_with_literal_pipes() {
    let source = "+------------+-----+\n| Header           |\n+============+=====+\n|     a|b    | x   |\n+------------+-----+\n";
    let output = format(source, None, None);
    assert!(output.contains("|     a|b    | x   |"), "{output}");
    assert_eq!(format(&output, None, None), output);
    if std::process::Command::new("pandoc")
        .arg("--version")
        .output()
        .is_ok()
    {
        assert_eq!(pandoc_document(&output), pandoc_document(source));
    }
}

#[test]
fn spanning_grid_formatting_preserves_prose_in_aligned_columns() {
    for separator in ["===========:", ":==========:"] {
        let source = format!(
            "+------------+-----+\n| Header           |\n+{separator}+=====+\n| a|b        | x   |\n+------------+-----+\n"
        );
        let output = format(&source, None, None);
        assert!(output.contains("| a|b        | x   |"), "{output}");
        assert_eq!(format(&output, None, None), output);
        if std::process::Command::new("pandoc")
            .arg("--version")
            .output()
            .is_ok()
        {
            assert_eq!(pandoc_document(&output), pandoc_document(&source));
        }
    }
}

#[test]
fn spanning_grid_formatting_preserves_punctuation_cells() {
    for content in ["-", "--", ":", "=", "- = :"] {
        let source = format!(
            "+--------+--------+\n| A + B           |\n+========+========+\n| {content:<6} | ok     |\n+--------+--------+\n"
        );
        let output = format(&source, None, None);
        assert_eq!(output, source);
        assert_eq!(format(&output, None, None), output);
        if std::process::Command::new("pandoc")
            .arg("--version")
            .output()
            .is_ok()
        {
            assert_eq!(pandoc_document(&output), pandoc_document(&source));
        }
    }
}

#[test]
fn spanning_grid_separator_fill_ignores_equals_in_cell_content() {
    let source = "+--------+--------+\n| A      | B      |\n+========+========+\n| text   | one    |\n| =      +--------+\n| more   | two    |\n+--------+--------+\n";
    assert_eq!(format(source, None, None), source);
}

#[test]
fn grid_conversion_preserves_unicode_sequences() {
    for content in [
        "👩‍💻",
        "❤️",
        "e\u{301}",
        "가",
        "한",
        "\u{1160}\u{11ff}",
        "\u{d7b0}\u{d7ff}",
        "👩‍💻 👩‍💻 👩‍💻 👩‍💻 👩‍💻 👩‍💻 👩‍💻 👩‍💻",
    ] {
        let source = format!("| A | B |\n|---|---|\n| {content} | ok |\n");
        let grid = convert(&source, TableStyle::Grid).unwrap();
        let output = format(&grid, None, None);
        assert_eq!(format(&output, None, None), output);
        assert_eq!(
            convert(&output, TableStyle::Pipe),
            convert(&source, TableStyle::Pipe)
        );
        if std::process::Command::new("pandoc")
            .arg("--version")
            .output()
            .is_ok()
        {
            assert_eq!(pandoc(&grid), pandoc(&source));
            assert_eq!(pandoc(&output), pandoc(&source));
        }
    }
}

#[test]
fn spanning_grid_formatting_preserves_unicode_sequences() {
    let source = "+--------+--------+\n| 👩‍💻 + B        |\n+========+========+\n| 👩‍💻   | ok     |\n+--------+--------+\n";
    assert_eq!(format(source, None, None), source);
}

#[test]
fn grid_conversion_rejects_unparsed_pandoc_list_markers() {
    for content in [
        "@. hi",
        "@) hi",
        "@label. hi",
        "@label) hi",
        "(#) hi",
        "#) hi",
    ] {
        let pipe = format!("| A |\n|---|\n| {content} |\n");
        assert_eq!(
            convert(&pipe, TableStyle::Grid),
            Err(TableConversionError::BlockContent),
            "{content}"
        );
        let grid = grid_cell_source(content);
        for target in [TableStyle::Pipe, TableStyle::Simple, TableStyle::Multiline] {
            assert_eq!(
                convert(&grid, target),
                Err(TableConversionError::BlockContent),
                "{target:?}: {content}"
            );
        }
    }
}

#[test]
fn grid_conversion_allows_literal_list_markers() {
    for content in [
        r"\@. hi", r"(\#) hi", "@.hi", "(#)hi", "`@. hi`", "`(#) hi`",
    ] {
        let source = format!("| A |\n|---|\n| {content} |\n");
        assert!(convert(&source, TableStyle::Grid).is_ok(), "{content}");
    }
    let mut config = Config::default();
    config.parser_extensions.example_lists = false;
    config.parser_extensions.fancy_lists = false;
    for content in ["@. hi", "(#) hi"] {
        let source = format!("| A |\n|---|\n| {content} |\n");
        let tree = panache_formatter::parser::parse(&source, Some(config.parser_options()));
        let table = tree.descendants().find_map(Table::cast).unwrap();
        assert!(convert_table(&table, TableStyle::Grid, &config, 40).is_ok());
    }
}

fn grid_cell_source(content: &str) -> String {
    use unicode_width::UnicodeWidthStr;
    let width = content.lines().map(str::width).max().unwrap_or(0).max(6);
    let border = format!("+{}+\n", "-".repeat(width + 2));
    let mut source = format!(
        "{border}| Header{} |\n+{}+\n",
        " ".repeat(width - 6),
        "=".repeat(width + 2)
    );
    for line in content.lines() {
        source.push_str(&format!("| {line}{} |\n", " ".repeat(width - line.width())));
    }
    source.push_str(&border);
    source
}

#[test]
fn rejects_grid_structures_that_destinations_cannot_represent() {
    for (source, reason) in [
        (
            "+---+---+\n| A | B |\n+===+===+\n| x | y |\n|   +---+\n| z | w |\n+---+---+\n",
            TableConversionError::SpanningCells,
        ),
        (
            "+---+\n| A |\n+---+\n| B |\n+===+\n| x |\n+---+\n",
            TableConversionError::MultipleHeaders,
        ),
        (
            "+---+\n| A |\n+===+\n| x |\n+===+\n| f |\n+---+\n",
            TableConversionError::Footer,
        ),
        (
            "+---+---+\n| A | B |\n+===+===+\n| x | y\n+---+---+\n",
            TableConversionError::MissingBody,
        ),
    ] {
        for target in [TableStyle::Pipe, TableStyle::Simple, TableStyle::Multiline] {
            assert_eq!(convert(source, target), Err(reason), "{source}");
        }
    }
    for content in [
        "- item",
        "# heading",
        "> quote",
        "    code",
        "one\n\ntwo",
        "```\ncode\n```",
        "[ref]: url",
    ] {
        for target in [TableStyle::Pipe, TableStyle::Simple, TableStyle::Multiline] {
            assert_eq!(
                convert(&grid_cell_source(content), target),
                Err(TableConversionError::BlockContent),
                "{content}"
            );
        }
    }
    assert_eq!(
        convert(&grid_cell_source("one\\\ntwo"), TableStyle::Pipe),
        Err(TableConversionError::HardLineBreak)
    );
    assert_eq!(
        convert(&grid_cell_source("`one\ntwo`"), TableStyle::Pipe),
        Err(TableConversionError::MultilineLiteral)
    );
}

#[test]
fn grid_conversion_rejects_block_reinterpretation() {
    for content in ["- item", "# heading", "> quote", "---", "[ref]: url"] {
        let source = format!("| A |\n|---|\n| {content} |\n");
        assert_eq!(
            convert(&source, TableStyle::Grid),
            Err(TableConversionError::BlockContent),
            "{content}"
        );
    }
}

#[test]
fn grid_conversion_rejects_html_block_interruptions() {
    for (content, block) in [
        ("foo <div>bar</div>", "Div"),
        ("foo <DIV class=\"note\">bar</DIV>", "Div"),
        ("foo <section>bar</section>", "RawBlock"),
        ("foo <p>bar</p>", "RawBlock"),
    ] {
        let source = grid_cell_source(content);
        if std::process::Command::new("pandoc")
            .arg("--version")
            .output()
            .is_ok()
        {
            let native = pandoc_output(&source, "native");
            assert!(native.contains(block), "{native}");
            assert!(native.contains("Str \"bar\""), "{native}");
        }
        for target in [TableStyle::Pipe, TableStyle::Simple, TableStyle::Multiline] {
            assert_eq!(
                convert(&source, target),
                Err(TableConversionError::BlockContent),
                "{target:?}: {content}"
            );
        }
        let pipe = format!("| Header |\n|---|\n| {content} |\n");
        assert_eq!(
            convert(&pipe, TableStyle::Grid),
            Err(TableConversionError::BlockContent),
            "{content}"
        );
    }
}

#[test]
fn grid_conversion_preserves_inline_html_and_literal_block_tags() {
    for content in [
        "foo <span>bar</span>",
        "foo `<div>bar</div>`",
        r"foo \<div>bar\</div>",
        "foo &lt;div>bar&lt;/div>",
    ] {
        let source = grid_cell_source(content);
        let pipe = convert(&source, TableStyle::Pipe).unwrap();
        if std::process::Command::new("pandoc")
            .arg("--version")
            .output()
            .is_ok()
        {
            assert_eq!(pandoc(&pipe), pandoc(&source));
        }
    }
}

#[test]
fn wrapping_preserves_literal_inline_delimiters() {
    let config = Config::default();
    for content in [
        "foo * bar * baz",
        "foo _ bar _ baz",
        "foo ** bar ** baz",
        "foo __ bar __ baz",
        "foo* bar* baz",
        "foo_ bar_ baz",
    ] {
        for (target, source) in [
            (
                TableStyle::Grid,
                format!("| Header |\n|---|\n| {content} |\n"),
            ),
            (
                TableStyle::Multiline,
                format!("| Header | B |\n|---|---|\n| {content} | ok |\n"),
            ),
        ] {
            let tree = panache_formatter::parser::parse(&source, Some(config.parser_options()));
            let table = tree.descendants().find_map(Table::cast).unwrap();
            let output = convert_table(&table, target, &config, 10)
                .unwrap_or_else(|error| panic!("{content} to {target:?}: {error}"));
            let formatted = format(&output, None, None);
            assert_eq!(format(&formatted, None, None), formatted);
            if std::process::Command::new("pandoc")
                .arg("--version")
                .output()
                .is_ok()
            {
                assert_eq!(pandoc(&output), pandoc(&source), "{output}");
                assert_eq!(pandoc(&formatted), pandoc(&source), "{formatted}");
            }
        }
    }
}

#[test]
fn grid_safety_fixture_preserves_pandoc_content_after_formatting() {
    let source =
        include_str!("../../../tests/fixtures/cases/grid_table_conversion_safety/input.md");
    let output = format(source, None, None);
    assert_eq!(format(&output, None, None), output);
    if std::process::Command::new("pandoc")
        .arg("--version")
        .output()
        .is_ok()
    {
        assert_eq!(pandoc(&output), pandoc(source));
    }
}

#[test]
fn grid_conversion_rejects_terminal_hard_breaks() {
    for content in ["path\\", "path\\\\\\", "one\npath\\"] {
        let source = grid_cell_source(content);
        for target in [TableStyle::Pipe, TableStyle::Simple, TableStyle::Multiline] {
            assert_eq!(
                convert(&source, target),
                Err(TableConversionError::HardLineBreak),
                "{target:?}: {source}"
            );
        }
    }
}

#[test]
fn grid_conversion_rejects_introducing_terminal_hard_breaks() {
    for content in ["path\\", "path\\\\\\"] {
        let source = format!("| A |\n|---|\n| {content} |\n");
        assert_eq!(
            convert(&source, TableStyle::Grid),
            Err(TableConversionError::HardLineBreak),
            "{source}"
        );
    }
}

#[test]
fn grid_conversion_preserves_terminal_literal_backslashes() {
    for content in [r"path\\", r"path\\\\", r"`path\`"] {
        let source = format!("| A |\n|---|\n| {content} |\n");
        let grid = convert(&source, TableStyle::Grid).unwrap();
        let output = convert(&grid, TableStyle::Pipe).unwrap();
        assert!(output.contains(content), "{output}");
        if std::process::Command::new("pandoc")
            .arg("--version")
            .output()
            .is_ok()
        {
            assert_eq!(pandoc(&grid), pandoc(&source));
            assert_eq!(pandoc(&output), pandoc(&source));
        }
    }
}

#[test]
fn grid_conversion_does_not_drop_text_outside_the_border() {
    let source = "+---+---+\n| A | B |\n+===+===+\n| x | y | extra |\n+---+---+\n";
    for target in [TableStyle::Pipe, TableStyle::Simple, TableStyle::Multiline] {
        assert!(convert(source, target).is_err());
    }
    assert!(format(source, None, None).contains("extra"));
}

#[test]
fn grid_conversion_preserves_inline_content_and_headerless_tables() {
    let sources = [
        "| A | B |\n|---|---|\n| one two three four five six seven eight nine | prefix `some long code span` suffix |\n",
        "| Wide header | B |\n|---:|:---:|\n| x | 界 café e\u{301} 😀 |\n",
        "| A | B |\n|---|---|\n| a\\|b **c\\|d** | `x|y` $x|y$ |\n",
        "| A | B |\n|---|---|\n| | x |\n| y | |\n",
        "----- -----\none   two\n----- -----\n",
        "---\none\n---\n",
        ": Caption *before* {#tbl-id}\n\n| A | B |\n|---|---|\n| x | y |\n",
    ];
    let oracle = std::process::Command::new("pandoc")
        .arg("--version")
        .output()
        .is_ok();
    for source in sources {
        let grid = convert(source, TableStyle::Grid).unwrap();
        let formatted = format(
            &grid,
            Some(Config {
                math: panache_formatter::MathMode::Verbatim,
                ..Config::default()
            }),
            None,
        );
        assert_eq!(
            format(
                &formatted,
                Some(Config {
                    math: panache_formatter::MathMode::Verbatim,
                    ..Config::default()
                }),
                None
            ),
            formatted
        );
        for target in [TableStyle::Pipe, TableStyle::Simple, TableStyle::Multiline] {
            let output =
                convert(&grid, target).unwrap_or_else(|err| panic!("{target:?}: {err}\n{grid}"));
            if oracle {
                assert_eq!(pandoc(&output), pandoc(source), "{output}");
            }
        }
        if oracle {
            assert_eq!(pandoc(&grid), pandoc(source), "{grid}");
            assert_eq!(pandoc(&formatted), pandoc(source), "{formatted}");
        }
    }
}

#[test]
fn grid_conversion_honors_disabled_extension() {
    let mut config = Config::default();
    config.parser_extensions.grid_tables = false;
    let tree =
        panache_formatter::parser::parse("| A |\n|---|\n| x |\n", Some(config.parser_options()));
    let table = tree.descendants().find_map(Table::cast).unwrap();
    assert_eq!(
        convert_table(&table, TableStyle::Grid, &config, 40),
        Err(TableConversionError::DisabledExtension)
    );
}

#[test]
fn converts_all_supported_pairs_and_survives_formatting() {
    let sources = [
        (TableStyle::Pipe, "| A | B |\n|---|---|\n| one | two |\n"),
        (TableStyle::Simple, "A     B\n----- -----\none   two\n"),
        (
            TableStyle::Multiline,
            "-------------\nA      B\n------ ------\none    two\n\n-------------\n",
        ),
        (
            TableStyle::Grid,
            "+-----+-----+\n| A   | B   |\n+=====+=====+\n| one | two |\n+-----+-----+\n",
        ),
    ];
    for (source_style, source) in sources {
        for (target, _) in sources {
            if source_style == target {
                continue;
            }
            let output = convert(source, target).unwrap();
            assert_style(&output, target.syntax_kind());
            let formatted = format(&output, None, None);
            assert_style(&formatted, target.syntax_kind());
            assert_eq!(format(&formatted, None, None), formatted);
            assert!(formatted.contains("one"));
            assert!(formatted.contains("two"));
        }
    }
}

fn simple_cell(cell: &str) -> String {
    format!("{:<80} B\n{} ---\n{cell:<80} y\n", "A", "-".repeat(80))
}

#[test]
fn pipe_conversion_escapes_prose_and_preserves_literals() {
    for (cell, expected) in [
        ("a|b", r"a\|b"),
        (r"a\|b", r"a\|b"),
        (r"a\\|b", r"a\\\|b"),
        ("**a|b** [c|d](url)", r"**a\|b** [c\|d](url)"),
        ("`a|b` $x|y$", "`a|b` $x|y$"),
        (r"`a\|b` $x\|y$", r"`a\|b` $x\|y$"),
        ("**`a|b`**", "**`a|b`**"),
        (r"`\verb|x|`", r"`\verb|x|`"),
        (r"\\verb|x|", r"\\verb\|x\|"),
    ] {
        let source = simple_cell(cell);
        for source in [
            source.clone(),
            convert(&source, TableStyle::Multiline).unwrap(),
        ] {
            let output = convert(&source, TableStyle::Pipe).unwrap();
            assert!(output.contains(expected), "{output}");
            assert_style(&output, SyntaxKind::PIPE_TABLE);
            let formatted = format(&output, None, None);
            assert_eq!(format(&formatted, None, None), formatted);
        }
    }
}

#[test]
fn pipe_conversion_preserves_headerless_tables_and_empty_cells() {
    for source in ["----- -----\none\n----- -----\n", "---\none\n---\n"] {
        for source in [
            source.to_string(),
            convert(source, TableStyle::Multiline).unwrap(),
        ] {
            let output = convert(&source, TableStyle::Pipe).unwrap();
            assert_style(&output, SyntaxKind::PIPE_TABLE);
            assert!(
                output
                    .lines()
                    .next()
                    .unwrap()
                    .chars()
                    .all(|ch| ch == '|' || ch == ' ')
            );
            let back = convert(&output, TableStyle::Simple).unwrap();
            let tree = panache_formatter::parser::parse(&back, None);
            assert!(
                tree.descendants()
                    .find_map(Table::cast)
                    .unwrap()
                    .rows()
                    .iter()
                    .all(|row| !row.is_header())
            );
        }
    }
}

#[test]
fn pipe_conversion_declines_unsupported_literal_contexts() {
    assert!(
        convert(
            &simple_cell("<span title=\"a|b\">text</span>"),
            TableStyle::Pipe
        )
        .is_err()
    );
}

#[test]
fn conversion_declines_raw_tex_verbatim() {
    for cell in [r"\verb|x|", r"\verb*|x y|", r"**\verb|x|**", r"\verb!a|b!"] {
        for target in [TableStyle::Pipe, TableStyle::Multiline] {
            assert_eq!(
                convert(&simple_cell(cell), target),
                Err(TableConversionError::InvalidOutput),
                "{cell} to {target:?}"
            );
        }
    }
}

#[test]
fn conversion_honors_disabled_code_attributes() {
    let mut config = Config::default();
    config.parser_extensions.inline_code_attributes = false;
    let input = "| A | B | C |\n|---|---|---|\n| `x`{title=\"a|b\"} | z |\n";
    let tree = panache_formatter::parser::parse(input, Some(config.parser_options()));
    let table = tree.descendants().find_map(Table::cast).unwrap();
    let output = convert_table(&table, TableStyle::Simple, &config, 80).unwrap();
    assert!(output.contains("z"));
}

#[test]
fn pipe_conversion_honors_disabled_extension() {
    let mut config = Config::default();
    config.parser_extensions.pipe_tables = false;
    let tree =
        panache_formatter::parser::parse(&simple_cell("text"), Some(config.parser_options()));
    let table = tree.descendants().find_map(Table::cast).unwrap();
    assert_eq!(
        convert_table(&table, TableStyle::Pipe, &config, 40),
        Err(TableConversionError::DisabledExtension)
    );
}

#[test]
fn preserves_headerless_single_row_and_empty_cells() {
    let input = "----- -----\none\n----- -----\n";
    let output = convert(input, TableStyle::Multiline).unwrap();
    assert_style(&output, SyntaxKind::MULTILINE_TABLE);
    let formatted = format(&output, None, None);
    assert_style(&formatted, SyntaxKind::MULTILINE_TABLE);
    assert_eq!(format(&formatted, None, None), formatted);
    let back = convert(&output, TableStyle::Simple).unwrap();
    let tree = panache_formatter::parser::parse(&back, None);
    let table = tree.descendants().find_map(Table::cast).unwrap();
    assert!(table.rows().iter().all(|row| !row.is_header()));
}

#[test]
fn preserves_inline_literals_and_caption_position() {
    let input = ": A *caption* {#tbl-demo}\n\n| A | B |\n|---|---|\n| `a  b` $x + y$ | [a link](https://example.org) **bold** a\\|b |\n";
    for target in [TableStyle::Simple, TableStyle::Multiline] {
        let output = convert(input, target).unwrap();
        assert!(output.starts_with(": A *caption* {#tbl-demo}\n\n"));
        for literal in [
            "`a  b`",
            "$x + y$",
            "[a link](https://example.org)",
            "**bold**",
            "a\\|b",
        ] {
            assert!(output.contains(literal), "missing {literal}: {output}");
        }
        assert!(format(&output, None, None).contains("`a  b`"));
    }
}

#[test]
fn wraps_prose_but_keeps_inline_fragments_whole() {
    let input = "| A | B |\n|---|---|\n| one two three four five six seven eight nine ten | prefix `some long code span` suffix |\n";
    let output = convert(input, TableStyle::Multiline).unwrap();
    assert!(!output.contains("one two three four five six seven eight nine ten"));
    assert!(output.contains("`some long code span`"));
    let formatted = format(&output, None, None);
    assert!(formatted.contains("`some long code span`"));
    assert_eq!(format(&formatted, None, None), formatted);
}

#[test]
fn retains_unicode_cells() {
    let input = "| Name | Value |\n|---|---|\n| 界 café e\u{301} | 😀 |\n";
    for target in [TableStyle::Simple, TableStyle::Multiline] {
        let output = convert(input, target).unwrap();
        assert!(output.contains("界 café e\u{301}"));
        assert!(output.contains('😀'));
        assert_eq!(
            format(&format(&output, None, None), None, None),
            format(&output, None, None)
        );
    }
}

#[test]
fn rejects_ragged_rows_and_missing_body() {
    assert_eq!(
        convert("| A | B |\n|---|---|\n| x |\n", TableStyle::Simple),
        Err(TableConversionError::RaggedRows)
    );
    assert_eq!(
        convert("| A | B |\n|---|---|\n", TableStyle::Simple),
        Err(TableConversionError::MissingBody)
    );
}

#[test]
fn rejects_hard_breaks_in_multiline_cells() {
    let input = "---------------------\nA         B\n--------- -----------\none\\      two\nmore\n\n---------------------\n";
    assert_eq!(
        convert(input, TableStyle::Simple),
        Err(TableConversionError::HardLineBreak)
    );
}

#[test]
fn honors_disabled_target_extension() {
    let mut config = Config::default();
    config.parser_extensions.simple_tables = false;
    let tree = panache_formatter::parser::parse(
        "| A | B |\n|---|---|\n| x | y |\n",
        Some(config.parser_options()),
    );
    let table = tree.descendants().find_map(Table::cast).unwrap();
    assert_eq!(
        convert_table(&table, TableStyle::Simple, &config, 80),
        Err(TableConversionError::DisabledExtension)
    );
}

#[test]
fn caption_without_final_newline_is_preserved() {
    let output = convert(
        "| A | B |\n|---|---|\n| x | y |\n\n: Caption {#tbl-id}",
        TableStyle::Simple,
    )
    .unwrap();
    assert!(output.contains(": Caption {#tbl-id}"));
}

#[test]
fn rejects_unsupported_structures_without_replacements() {
    for (source, expected) in [
        (
            "+-------+\n| A B   |\n+===+===+\n| x | y |\n+---+---+\n",
            TableConversionError::SpanningCells,
        ),
        (
            "| A | B |\n|---|---|\n| one | two | extra |\n",
            TableConversionError::RaggedRows,
        ),
        (
            "| | B |\n|:---:|---|\n| x | y |\n",
            TableConversionError::Alignment,
        ),
        (
            "--------------------\nA         B\n--------- ----------\n`code     x\nspan`\n\n--------------------\n",
            TableConversionError::MultilineLiteral,
        ),
    ] {
        for target in [TableStyle::Simple, TableStyle::Multiline] {
            assert_eq!(convert(source, target), Err(expected), "{source}");
        }
    }
}

#[test]
fn does_not_replace_east_asian_line_breaks_with_spaces() {
    let mut config = Config::default();
    config.parser_extensions.east_asian_line_breaks = true;
    config.formatter_extensions.east_asian_line_breaks = true;
    let source = "-------------------\nA         B\n--------- ---------\n漢字      x\n漢語\n\n-------------------\n";
    let tree = panache_formatter::parser::parse(source, Some(config.parser_options()));
    let table = tree.descendants().find_map(Table::cast).unwrap();
    assert_eq!(
        convert_table(&table, TableStyle::Simple, &config, 80),
        Err(TableConversionError::EastAsianLineBreak)
    );
}

fn pandoc_output(text: &str, writer: &str) -> String {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let mut child = Command::new("pandoc")
        .args(["-f", "markdown", "-t", writer, "--wrap=none"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn pandoc_document(text: &str) -> serde_json::Value {
    serde_json::from_str(&pandoc_output(text, "json")).unwrap()
}

fn pandoc_colspecs(text: &str) -> Vec<serde_json::Value> {
    pandoc_document(text)["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|block| block["t"] == "Table")
        .map(|table| table["c"][2].clone())
        .collect()
}

fn pandoc(text: &str) -> serde_json::Value {
    let mut value = pandoc_document(text);
    fn normalize(value: &mut serde_json::Value) {
        if value["t"] == "Table" {
            for col in value["c"][2].as_array_mut().unwrap() {
                col[1] = serde_json::json!({"t": "ColWidthDefault"});
            }
        }
        if value["t"] == "AlignDefault" {
            value["t"] = "AlignLeft".into();
        }
        if value["t"] == "SoftBreak" {
            value["t"] = "Space".into();
        }
        match value {
            serde_json::Value::Array(items) => items.iter_mut().for_each(normalize),
            serde_json::Value::Object(map) => map.values_mut().for_each(normalize),
            _ => {}
        }
    }
    normalize(&mut value);
    value
}

#[test]
fn multiline_formatting_preserves_column_widths() {
    if std::process::Command::new("pandoc")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("Skipping table width oracle: pandoc is unavailable");
        return;
    }
    let fixture =
        include_str!("../../../tests/fixtures/cases/multiline_table_reflow_stability/input.md");
    let unbreakable = "-------------------\nA         B\n--------- ---------\none       `a long code span`\n\n-------------------\n";
    let headerless = include_str!(
        "../../../tests/fixtures/cases/multiline_table_headerless_single_column/input.md"
    );
    let two_dashes =
        include_str!("../../../tests/fixtures/cases/multiline_table_two_dash_borders/input.md");
    for source in [fixture, unbreakable, headerless, two_dashes] {
        for wrap in [
            panache_formatter::WrapMode::Reflow,
            panache_formatter::WrapMode::Preserve,
        ] {
            let config = Config {
                wrap: Some(wrap),
                ..Config::default()
            };
            let output = format(source, Some(config.clone()), None);
            assert_eq!(
                pandoc_colspecs(&output),
                pandoc_colspecs(source),
                "column widths changed:\n{output}"
            );
            assert_eq!(
                pandoc(&output),
                pandoc(source),
                "table content changed:\n{output}"
            );
            assert_eq!(
                pandoc_output(&output, "html"),
                pandoc_output(source, "html"),
                "rendered table changed:\n{output}"
            );
            assert_eq!(format(&output, Some(config), None), output);
        }
    }
}

#[test]
fn conversion_to_simple_discards_explicit_widths_but_preserves_content() {
    let source = "-------------------\nA         B\n--------- ---------\none       `a long code span`\n\n-------------------\n";
    let output = convert(source, TableStyle::Simple).unwrap();
    assert_style(&output, SyntaxKind::SIMPLE_TABLE);

    if std::process::Command::new("pandoc")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("Skipping table conversion width oracle: pandoc is unavailable");
        return;
    }
    let before = pandoc_colspecs(source);
    let after = pandoc_colspecs(&output);
    assert_eq!(before.len(), 1);
    assert_eq!(after.len(), 1);
    assert!(
        before[0]
            .as_array()
            .unwrap()
            .iter()
            .all(|col| col[1]["t"] == "ColWidth")
    );
    assert!(
        after[0]
            .as_array()
            .unwrap()
            .iter()
            .all(|col| col[1]["t"] == "ColWidthDefault")
    );
    assert_eq!(pandoc(&output), pandoc(source));
}

#[test]
fn matches_pandoc_content_and_structure() {
    if std::process::Command::new("pandoc")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("Skipping table conversion oracle: pandoc is unavailable");
        return;
    }
    let samples = [
        "| Left | Center | Right | Default |\n|:---|:---:|---:|---|\n| one | two | three | four |\n",
        "| A | B |\n|---|---|\n| `a  b` | some **strong words** and *emphasis* |\n",
        "| A | B |\n|---|---|\n| [a link](https://example.org) | a\\|b a\\ b &amp; |\n",
        "| A | B |\n|---|---|\n| [reference][r] | ![image](a.png) $x + y$ |\n",
        "| Name | Value |\n|---|---|\n| 界 café e\u{301} | 😀 |\n",
        "| A | B |\n|---|---|\n| | two |\n| three | |\n\n: Caption {#tbl-demo}\n",
        ": Caption *before* {#tbl-demo}\n\n| A | B |\n|---|---|\n| one | two |\n",
        "----- -----\none   two\n----- -----\n",
        "---\none\n---\n",
        "| A | B |\n|---|---|\n| one two three four five six seven eight nine ten | prefix `some long code span` suffix |\n",
    ];
    for source in samples {
        let expected = pandoc(source);
        for target in [
            TableStyle::Pipe,
            TableStyle::Simple,
            TableStyle::Multiline,
            TableStyle::Grid,
        ] {
            let output = convert(source, target).unwrap();
            assert_eq!(
                pandoc(&output),
                expected,
                "{source}\nconverted to {target:?}:\n{output}"
            );
            // Pandoc stores the literal TeX source in its AST, so keep math
            // verbatim when comparing document structure after formatting.
            let config = Config {
                math: panache_formatter::MathMode::Verbatim,
                ..Config::default()
            };
            assert_eq!(
                pandoc(&format(&output, Some(config), None)),
                expected,
                "format after conversion: {output}"
            );
            if target == TableStyle::Multiline {
                let simple = convert(&output, TableStyle::Simple).unwrap();
                assert_eq!(
                    pandoc(&simple),
                    expected,
                    "multiline back to simple: {simple}"
                );
            }
            if target != TableStyle::Pipe {
                let pipe = convert(&output, TableStyle::Pipe).unwrap();
                assert_eq!(pandoc(&pipe), expected, "{target:?} back to pipe: {pipe}");
            }
        }
    }
}

#[test]
fn pipe_conversion_matches_pandoc_for_literal_pipes_and_wrapped_prose() {
    if std::process::Command::new("pandoc")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("Skipping table conversion oracle: pandoc is unavailable");
        return;
    }
    let config = Config {
        math: panache_formatter::MathMode::Verbatim,
        ..Config::default()
    };
    for cell in [
        "one two three four five six seven eight nine ten",
        "a|b **c|d** [e|f](url) ![g|h](image.png)",
        r"a\|b a\\|b",
        "`a|b` `` a`|b `` $x|y$ $$x|y$$",
        r"`a\|b` $x\|y$",
        "界 café e\u{301} 😀",
    ] {
        let simple = simple_cell(cell);
        let expected = pandoc(&simple);
        for source in [
            simple.clone(),
            convert(&simple, TableStyle::Multiline).unwrap(),
        ] {
            let pipe = convert(&source, TableStyle::Pipe).unwrap();
            assert_eq!(pandoc(&pipe), expected, "{source}\n{pipe}");
            let formatted = format(&pipe, Some(config.clone()), None);
            assert_eq!(
                pandoc(&formatted),
                expected,
                "format after conversion: {formatted}"
            );
            assert_eq!(format(&formatted, Some(config.clone()), None), formatted);
        }
    }
}
