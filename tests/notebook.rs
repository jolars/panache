use panache::{
    Config,
    config::{Extensions, Flavor, FormatterExtensions},
    format_notebook,
};
use serde_json::{Value, json};
fn config() -> Config {
    Config {
        flavor: Flavor::Jupyter,
        extensions: Extensions::for_flavor(Flavor::Jupyter),
        formatter_extensions: FormatterExtensions::for_flavor(Flavor::Jupyter),
        ..Config::default()
    }
}
#[test]
fn notebook_preserves_everything_outside_changed_sources() {
    let input = r##"{ "metadata":{"unknown":1e+09},"nbformat":4,"nbformat_minor":99,"cells":[{"cell_type":"code","source":["x=  1"],"outputs":[{"text":"KEEP  ME"}]},{"id":"m","cell_type":"markdown","source":"#   Title","attachments":{"plot":{"image/png":"KEEP"}}},{"cell_type":"raw","source":"KEEP  RAW"},{"cell_type":"future","source":null}]}"##;
    let output = format_notebook(input, &config()).unwrap();
    assert_eq!(
        output,
        input.replace(r##""#   Title""##, r##""# Title\n""##)
    );
    assert_eq!(format_notebook(&output, &config()).unwrap(), output);
}
#[test]
fn notebook_sources_keep_representation_and_layout() {
    for source in [
        json!("#   Hello"),
        json!(["#   Hello"]),
        json!([]),
        json!(""),
    ] {
        let input = json!({"nbformat":4,"nbformat_minor":0,"cells":[{"cell_type":"markdown","source":source}]}).to_string();
        let output = format_notebook(&input, &config()).unwrap();
        let parsed: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["cells"][0]["source"].is_array(), source.is_array());
        if source == json!("") || source == json!([]) {
            assert_eq!(parsed["cells"][0]["source"], source);
        }
        assert_eq!(format_notebook(&output, &config()).unwrap(), output);
    }
    let input = "{\r\n \"nbformat\":4,\"nbformat_minor\":0,\"cells\":[{\"cell_type\":\"markdown\",\"source\":[\r\n   \"#   Héllo\"\r\n  ]}]}\r\n";
    let output = format_notebook(input, &config()).unwrap();
    assert!(output.contains("[\r\n   \"# Héllo\\n\"\r\n  ]"));
    assert!(output.ends_with("\r\n"));
}
#[test]
fn notebook_rejects_invalid_input_without_partial_output() {
    for input in [
        "{}",
        "{",
        r#"{"nbformat":3,"nbformat_minor":0,"cells":[]}"#,
        r#"{"nbformat":4,"nbformat_minor":0,"cells":[{"cell_type":"markdown","source":[1]}]}"#,
    ] {
        assert!(format_notebook(input, &config()).is_err());
    }
}
#[test]
fn jupyter_math_is_lossless_and_idempotent() {
    for input in [
        "$ a * b $2\n",
        "$a\n+ b$\n",
        r"\\(x * y\\)",
        "\\begin{align}\nx &= y\n\\end{align}\n",
        "`$foo` and `$bar`\n",
        "![plot](attachment:plot.png)\n",
    ] {
        let tree = panache::parse(input, Some(config()));
        assert_eq!(tree.text().to_string(), input);
        let output = panache::format(input, Some(config()), None);
        assert_eq!(
            panache::format(&output, Some(config()), None),
            output,
            "{input}"
        );
    }
}

#[test]
fn jupyter_math_boundaries_match_upstream_examples() {
    // JupyterLab packages/rendermime/test/latex.spec.ts, default MathJax mode.
    use panache::syntax::SyntaxKind;
    for input in ["$ /alpha $", "$5 and $10", "$x\n+ y$", r"\\(x * y\\)"] {
        let tree = panache::parse(input, Some(config()));
        assert_eq!(
            tree.descendants()
                .filter(|n| n.kind() == SyntaxKind::INLINE_MATH)
                .count(),
            1,
            "{input}"
        );
        assert!(!tree.descendants().any(|n| n.kind() == SyntaxKind::EMPHASIS));
    }
    for input in [r"\(literal\)", "`$foo` and `$bar`", "$x\n\ny$"] {
        let tree = panache::parse(input, Some(config()));
        assert!(
            !tree
                .descendants()
                .any(|n| n.kind() == SyntaxKind::INLINE_MATH),
            "{input}"
        );
    }
}

#[test]
fn jupyter_math_pipes_stay_inside_table_cells() {
    use panache::syntax::SyntaxKind;

    let input = "| $a|b$ | note |\n| --- | --- |\n";
    let tree = panache::parse(input, Some(config()));
    assert_eq!(tree.text().to_string(), input);
    assert!(
        tree.descendants()
            .any(|n| n.kind() == SyntaxKind::PIPE_TABLE)
    );
    assert!(
        tree.descendants()
            .any(|n| n.kind() == SyntaxKind::INLINE_MATH)
    );

    let mut disabled = config();
    disabled.extensions.tex_math_jupyter = false;
    let tree = panache::parse(input, Some(disabled));
    assert!(
        !tree
            .descendants()
            .any(|n| n.kind() == SyntaxKind::PIPE_TABLE)
    );
}

#[test]
fn jupyter_multiline_math_stays_in_one_table_row() {
    use panache::syntax::SyntaxKind;

    for input in [
        "| A | B |\n| --- | --- |\n| $x\n+y$ | z |\n",
        "| A | B |\r\n| --- | --- |\r\n| $x\r\n+y$ | z |\r\n",
        "| $x\n|y$ | B |\n| --- | --- |\n| a | b |\n",
        "| A | B |\n| --- | --- |\n$x\n+y$ | z |\n",
        "> | A | B |\n> | --- | --- |\n> | $x\n> +y$ | z |\n",
        "- | A | B |\n  | --- | --- |\n  | $x\n  +y$ | z |\n",
        "> - | A | B |\n>   | --- | --- |\n>   | $x\n>   +y$ | z |\n",
        "- > | A | B |\n  > | --- | --- |\n  > | $x\n  > +y$ | z |\n",
        "| A | B |\n| --- | --- |\n| $x\ny$ and $z\nw$ | z |\n",
        "- | A | B |\n  | --- | --- |\n  | \\begin{align}x\n  +y\\end{align} | z |\n",
    ] {
        let tree = panache::parse(input, Some(config()));
        assert_eq!(tree.text().to_string(), input, "{tree:#?}");
        assert_eq!(
            tree.descendants()
                .filter(|node| node.kind() == SyntaxKind::TABLE_ROW)
                .count(),
            1,
            "{input}\n{tree:#?}"
        );
        assert_eq!(
            tree.descendants()
                .filter(|node| node.kind() == SyntaxKind::TABLE_CELL)
                .count(),
            4,
            "{input}\n{tree:#?}"
        );
        let output = panache::format(input, Some(config()), None);
        assert_eq!(panache::format(&output, Some(config()), None), output);
    }
}

#[test]
fn jupyter_multiline_table_math_verbatim_preserves_content() {
    use panache::syntax::{AstNode, DisplayMath, InlineMath};

    let cfg = Config {
        math: panache::MathMode::Verbatim,
        ..config()
    };
    let contents = |text: &str| {
        panache::parse(text, Some(cfg.clone()))
            .descendants()
            .filter_map(|node| {
                InlineMath::cast(node.clone())
                    .map(|math| math.content())
                    .or_else(|| DisplayMath::cast(node).map(|math| math.content()))
            })
            .collect::<Vec<_>>()
    };
    for input in [
        "| A | B |\n| --- | --- |\n| $x\n  + y$ | z |\n",
        "| A | B |\n| --- | --- |\n| $$x\n  + y$$ | z |\n",
        "> | A | B |\n> | --- | --- |\n> | $x\n>   + y$ | z |\n",
        "- | A | B |\n  | --- | --- |\n  | $x\n    + y$ | z |\n",
    ] {
        let before = contents(input);
        assert_eq!(before.len(), 1);
        let output = panache::format(input, Some(cfg.clone()), None);
        assert_eq!(contents(&output), before, "{input:?} -> {output:?}");
        assert_eq!(panache::format(&output, Some(cfg.clone()), None), output);
    }
}

#[test]
fn jupyter_multiline_table_rows_align_closing_pipes() {
    for input in [
        "| A | B |\n| --- | --- |\n| $$a % comment\n+b$$ | z |\n",
        "| A | B |\n| --- | --- |\n| left | $$a % comment\n+b$$ |\n",
        "| A | B |\n| ---: | :---: |\n| $$a % comment\n+b$$ | z |\n",
        "| A | B |\n| --- | --- |\n| $$a % comment\n+界$$ | z |\n",
        "| A | B |\n| --- | --- |\n| $$a % comment\n+b$$ | $$c % comment\n+d$$ |\n",
        "| A | B |\n| --- | --- |\n| \\begin{align}x\n+y\\end{align} | z |\n",
    ] {
        for math in [panache::MathMode::Reflow, panache::MathMode::Verbatim] {
            let cfg = Config { math, ..config() };
            let output = panache::format(input, Some(cfg.clone()), None);
            let closing_columns: Vec<_> = output
                .lines()
                .filter(|line| line.ends_with('|'))
                .map(|line| {
                    use unicode_width::UnicodeWidthStr;
                    line[..line.len() - 1].width()
                })
                .collect();
            assert_eq!(closing_columns.len(), 3, "{output}");
            assert!(
                closing_columns
                    .iter()
                    .all(|column| *column == closing_columns[0]),
                "{math:?}: closing pipes should align:\n{output}"
            );
            assert_eq!(panache::format(&output, Some(cfg), None), output);
        }
    }
}

#[test]
fn jupyter_multiline_table_display_math_keeps_comment_boundaries() {
    use panache::syntax::{AstNode, DisplayMath};

    let contents = |text: &str| {
        panache::parse(text, Some(config()))
            .descendants()
            .filter_map(DisplayMath::cast)
            .map(|math| math.content())
            .collect::<Vec<_>>()
    };
    for input in [
        "| A | B |\n| --- | --- |\n| $$a % comment\n+b$$ | z |\n",
        "> | A | B |\n> | --- | --- |\n> | \\\\[a % comment\n> +b\\\\] | z |\n",
    ] {
        let before = contents(input);
        assert_eq!(before.len(), 1);
        let output = panache::format(input, Some(config()), None);
        assert_eq!(contents(&output), before, "{input:?} -> {output:?}");
        assert_eq!(panache::format(&output, Some(config()), None), output);
    }
}

#[test]
fn jupyter_multiline_math_can_be_a_setext_heading() {
    use panache::syntax::SyntaxKind;

    for input in [
        "$x\ny$\n---\n",
        "$x\ny$\n===\n",
        "$x\r\ny$\r\n---\r\n",
        "> $x\n> y$\n> ---\n",
        "- $x\n  y$\n  ===\n",
        "> - $x\n>   y$\n>   ---\n",
        "- > $x\n  > y$\n  > ===\n",
    ] {
        let tree = panache::parse(input, Some(config()));
        assert_eq!(tree.text().to_string(), input);
        let heading = tree
            .descendants()
            .find(|node| node.kind() == SyntaxKind::HEADING)
            .unwrap_or_else(|| panic!("{input}\n{tree:#?}"));
        assert!(
            heading
                .descendants()
                .any(|node| node.kind() == SyntaxKind::INLINE_MATH)
        );
        let output = panache::format(input, Some(config()), None);
        assert_eq!(panache::format(&output, Some(config()), None), output);
    }

    let input = "$x\ny$ and $z\n---\nw$\n";
    let tree = panache::parse(input, Some(config()));
    assert_eq!(tree.text().to_string(), input);
    assert!(
        !tree
            .descendants()
            .any(|node| node.kind() == SyntaxKind::HEADING)
    );
}

#[test]
fn jupyter_unmatched_double_dollars_keep_markdown_emphasis() {
    use panache::syntax::SyntaxKind;

    for input in ["$$a *b*$\n", "$$a *b\nc*$\n"] {
        let tree = panache::parse(input, Some(config()));
        assert_eq!(tree.text().to_string(), input);
        assert!(
            tree.descendants()
                .any(|node| node.kind() == SyntaxKind::EMPHASIS),
            "{tree:#?}"
        );
        assert!(!tree.descendants().any(|node| matches!(
            node.kind(),
            SyntaxKind::INLINE_MATH | SyntaxKind::DISPLAY_MATH
        )));
        let output = panache::format(input, Some(config()), None);
        assert_eq!(
            output,
            input.replace('\n', " ").trim_end().to_string() + "\n"
        );
    }
}

#[test]
fn jupyter_empty_math_survives_modes_and_delimiter_normalization() {
    use panache::MathMode;
    use panache::config::MathDelimiterStyle;
    use panache::syntax::SyntaxKind;

    let math_counts = |text: &str, cfg: &Config| {
        let tree = panache::parse(text, Some(cfg.clone()));
        [SyntaxKind::INLINE_MATH, SyntaxKind::DISPLAY_MATH].map(|kind| {
            tree.descendants()
                .filter(|node| node.kind() == kind)
                .count()
        })
    };
    for math in [
        MathMode::Reflow,
        MathMode::Preserve,
        MathMode::SingleLine,
        MathMode::Verbatim,
    ] {
        for math_delimiter_style in [
            MathDelimiterStyle::Preserve,
            MathDelimiterStyle::Dollars,
            MathDelimiterStyle::Backslash,
        ] {
            let cfg = Config {
                math,
                math_delimiter_style,
                ..config()
            };
            for input in [
                "$x$ $ $ $y$\n",
                "$   $\n",
                "\\\\(\\\\)\n",
                "\\\\( \\\\)\n",
                "$$ $$\n",
                "$$\n$$\n",
                "\\\\[\\\\]\n",
                "\\\\[ \\\\]\n",
                "Empty $$ $$ display.\n",
                "| $ $ | $$ $$ |\n| --- | --- |\n",
            ] {
                let output = panache::format(input, Some(cfg.clone()), None);
                assert_eq!(
                    math_counts(&output, &cfg),
                    math_counts(input, &cfg),
                    "{math:?}, {math_delimiter_style:?}: {input:?} -> {output:?}"
                );
                assert_eq!(
                    panache::format(&output, Some(cfg.clone()), None),
                    output,
                    "{math:?}, {math_delimiter_style:?}: {input:?}"
                );
            }
        }
    }
}

#[test]
fn jupyter_bare_environments_survive_block_like_continuations() {
    use panache::syntax::SyntaxKind;

    for input in [
        "Here \\begin{align}\n+ x &= y\n\\end{align}\n",
        "- Here \\begin{align}\n  + x &= y\n  \\end{align}\n",
        "> - Here \\begin{align}\n>   + x &= y\n>   \\end{align}\n",
        "- > Here \\begin{align}\n  > + x &= y\n  > \\end{align}\n",
        "- > Here \\begin{custom}\n  > + x &= y\n  > \\end{custom}\n",
    ] {
        let tree = panache::parse(input, Some(config()));
        assert_eq!(tree.text().to_string(), input);
        assert!(
            tree.descendants()
                .any(|n| n.kind() == SyntaxKind::LATEX_COMMAND),
            "{input}\n{tree:#?}"
        );
        let output = panache::format(input, Some(config()), None);
        assert_eq!(panache::format(&output, Some(config()), None), output);
    }
}

#[test]
fn jupyter_braced_dollars_do_not_end_multiline_math() {
    use panache::syntax::SyntaxKind;

    for input in [
        "$x + \\def\\foo{$}\n+ y$\n",
        "- $x + \\def\\foo{$}\n  + y$\n",
    ] {
        let tree = panache::parse(input, Some(config()));
        assert_eq!(tree.text().to_string(), input);
        assert!(
            tree.descendants()
                .any(|n| n.kind() == SyntaxKind::INLINE_MATH
                    && n.text().to_string().contains("+ y$")),
            "{input}\n{tree:#?}"
        );
    }
}

#[test]
fn jupyter_headings_include_matched_multiline_math() {
    use panache::syntax::SyntaxKind;

    for input in [
        "# $x\n+ y$\n",
        "# $x\r\n+ y$\r\n",
        "> # $x\n> + y$\n",
        "- # $x\n  + y$\n",
        "> - # $x\n>   + y$\n",
        "- > # $x\n  > + y$\n",
    ] {
        let tree = panache::parse(input, Some(config()));
        assert_eq!(tree.text().to_string(), input);
        let heading = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::HEADING)
            .expect("heading remains a heading");
        assert!(
            heading
                .descendants()
                .any(|n| n.kind() == SyntaxKind::INLINE_MATH),
            "{input}\n{tree:#?}"
        );
        assert_eq!(
            panache::format(input, Some(config()), None)
                .matches("$x + y$")
                .count(),
            1
        );
    }
}

#[test]
fn jupyter_plain_markdown_is_not_a_json_container() {
    let input = "---\ntitle: ordinary prose\n---\n";
    let tree = panache::parse(input, Some(config()));
    assert!(
        !tree
            .descendants_with_tokens()
            .any(|element| element.kind() == panache::syntax::SyntaxKind::YAML_METADATA_DELIM)
    );
    let input = r#"{"cells": []}"#;
    assert_eq!(
        panache::format(input, Some(config()), None),
        "{\"cells\": \\[\\]}\n"
    );
}

#[test]
fn jupyter_unclosed_math_does_not_swallow_headings() {
    for input in [
        "$unclosed\n# Heading\n",
        "$$unclosed\n# Heading\n",
        "\\begin{align}\n# Heading\n",
        "Here \\begin{align}\n# Heading\n",
        "- Here \\begin{align}\n  # Heading\n",
    ] {
        let tree = panache::parse(input, Some(config()));
        assert!(
            tree.descendants()
                .any(|n| n.kind() == panache::syntax::SyntaxKind::HEADING),
            "{input}\n{tree:#?}"
        );
    }
}

#[test]
fn jupyter_unclosed_dollar_does_not_capture_a_code_fence() {
    for fence in ["```", "~~~"] {
        let input = format!("$unclosed\n{fence}python\nprice = '$5'\n{fence}\n");
        let tree = panache::parse(&input, Some(config()));
        assert!(
            tree.descendants()
                .any(|n| n.kind() == panache::syntax::SyntaxKind::CODE_BLOCK),
            "{tree:#?}"
        );
        assert!(
            !tree
                .descendants()
                .any(|n| n.kind() == panache::syntax::SyntaxKind::INLINE_MATH)
        );
    }
}

#[test]
fn jupyter_backslash_normalization_remains_math() {
    let cfg = Config {
        math_delimiter_style: panache::config::MathDelimiterStyle::Backslash,
        ..config()
    };
    for (input, marker) in [
        ("$x + y$\n", r"\\("),
        ("$$\nx + y\n$$\n", r"\\["),
        ("- $x + y$\n", r"\\("),
    ] {
        let output = panache::format(input, Some(cfg.clone()), None);
        assert!(output.contains(marker), "{output}");
        assert_eq!(panache::format(&output, Some(cfg.clone()), None), output);
    }
}

#[test]
fn jupyter_dollar_normalization_keeps_adjacent_inline_math_separate() {
    use panache::config::MathDelimiterStyle;
    use panache::syntax::{AstNode, InlineMath};

    fn contents(text: &str, cfg: &Config) -> Vec<String> {
        panache::parse(text, Some(cfg.clone()))
            .descendants()
            .filter_map(InlineMath::cast)
            .map(|math| math.content())
            .collect()
    }

    let input = include_str!("fixtures/cases/jupyter_math_adjacent_dollars/input.md");
    for math in [
        panache::MathMode::Reflow,
        panache::MathMode::Preserve,
        panache::MathMode::SingleLine,
        panache::MathMode::Verbatim,
    ] {
        let cfg = Config {
            math,
            math_delimiter_style: MathDelimiterStyle::Dollars,
            ..config()
        };
        let before = contents(input, &cfg);
        assert!(!before.is_empty());
        let output = panache::format(input, Some(cfg.clone()), None);
        assert_eq!(contents(&output, &cfg), before, "{math:?}: {output}");
        assert_eq!(panache::format(&output, Some(cfg), None), output);
    }
}

#[test]
fn jupyter_display_math_verbatim_preserves_content_across_block_like_lines() {
    use panache::syntax::{AstNode, DisplayMath};

    fn math_contents(text: &str, cfg: &Config) -> Vec<String> {
        panache::parse(text, Some(cfg.clone()))
            .descendants()
            .filter_map(DisplayMath::cast)
            .map(|math| math.content())
            .collect()
    }

    let cfg = Config {
        math: panache::MathMode::Verbatim,
        // Keep the host's display indent out of this body-preservation check.
        math_indent: 0,
        ..config()
    };
    let input = include_str!("fixtures/cases/jupyter_math_block_precedence/input.md");
    let before = math_contents(input, &cfg);
    assert!(!before.is_empty());
    let output = panache::format(input, Some(cfg.clone()), None);
    assert_eq!(math_contents(&output, &cfg), before);
}

#[test]
fn jupyter_inline_math_verbatim_preserves_content_across_block_like_lines() {
    use panache::syntax::{AstNode, InlineMath};

    let cfg = Config {
        math: panache::MathMode::Verbatim,
        ..config()
    };
    let math_contents = |text: &str| {
        panache::parse(text, Some(cfg.clone()))
            .descendants()
            .filter_map(InlineMath::cast)
            .map(|math| math.content())
            .collect::<Vec<_>>()
    };
    for input in [
        include_str!("fixtures/cases/jupyter_math_braced_dollars/input.md"),
        include_str!("fixtures/cases/jupyter_math_headings/input.md"),
    ] {
        let before = math_contents(input);
        assert!(!before.is_empty());
        let output = panache::format(input, Some(cfg.clone()), None);
        assert_eq!(math_contents(&output), before);
        assert_eq!(panache::format(&output, Some(cfg.clone()), None), output);
    }
}
