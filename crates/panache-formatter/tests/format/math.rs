use panache_formatter::config::{Extensions, Flavor, WrapMode};
use panache_formatter::format;
use panache_formatter::{Config, ConfigBuilder, MathMode};
use panache_parser::semantic::math::{ArgKind, ArgumentDomain};

fn math_config(format_math: bool) -> Config {
    let flavor = Flavor::Quarto;
    Config {
        flavor,
        parser_extensions: Extensions::for_flavor(flavor),
        math: if format_math {
            MathMode::Reflow
        } else {
            MathMode::Verbatim
        },
        ..Default::default()
    }
}

fn math_mode_config(math: MathMode) -> Config {
    Config {
        math,
        ..math_config(true)
    }
}

fn math_host_matrix_config(format_math: bool) -> Config {
    let mut config = math_config(format_math);
    config.parser_extensions.tex_math_single_backslash = true;
    config
}

#[derive(Clone, Copy, Debug)]
enum MathHost {
    DollarInline,
    ParenInline,
    DollarDisplay,
    BracketDisplay,
    RawEnvironment(&'static str),
}

impl MathHost {
    fn document(self, body: &str) -> String {
        match self {
            Self::DollarInline => format!("${body}$\n"),
            Self::ParenInline => format!("\\({body}\\)\n"),
            Self::DollarDisplay => format!("$$\n{body}\n$$\n"),
            Self::BracketDisplay => format!("\\[\n{body}\n\\]\n"),
            Self::RawEnvironment(name) => {
                format!("\\begin{{{name}}}\n{body}\n\\end{{{name}}}\n")
            }
        }
    }

    fn body(self, document: &str) -> &str {
        let (open, close) = match self {
            Self::DollarInline => ("$", "$\n"),
            Self::ParenInline => (r"\(", "\\)\n"),
            Self::DollarDisplay => ("$$\n", "\n$$\n"),
            Self::BracketDisplay => ("\\[\n", "\n\\]\n"),
            Self::RawEnvironment(name) => {
                let open = format!("\\begin{{{name}}}\n");
                let close = format!("\n\\end{{{name}}}\n");
                return document
                    .strip_prefix(&open)
                    .and_then(|body| body.strip_suffix(&close))
                    .expect("formatted raw math host should retain its delimiters");
            }
        };
        document
            .strip_prefix(open)
            .and_then(|body| body.strip_suffix(close))
            .expect("formatted math host should retain its delimiters")
    }
}

fn indent_math_body(body: &str) -> String {
    body.lines()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_math_host_body(
    host: MathHost,
    input_body: &str,
    expected_body: &str,
    config: &Config,
) -> String {
    let input = host.document(input_body);
    let expected = host.document(expected_body);
    let output = format(&input, Some(config.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    similar_asserts::assert_eq!(format(&output, Some(config.clone()), None), output);
    host.body(&output).to_string()
}

#[test]
fn multiline_environment_suffix_breaks_only_in_reflow_mode() {
    let environment = "\\begin{matrix}\na&={b % inner\n+c}\\\\\nd&=e\n\\end{matrix}^T";
    let expected = "  x\n  + \\begin{matrix}\n      a & = {b % inner\n             + c} \\\\\n      d & = e\n    \\end{matrix}^T";
    for mode in [MathMode::Preserve, MathMode::SingleLine, MathMode::Reflow] {
        for punctuation in [",", ";", ".", ",;"] {
            let input = format!("x+{environment}{punctuation}y");
            let separator = if mode == MathMode::Reflow { "\n  " } else { "" };
            assert_math_host_body(
                MathHost::DollarDisplay,
                &input,
                &format!("{expected}{punctuation}{separator}y"),
                &math_mode_config(mode),
            );
        }
    }
}

#[test]
fn environment_leading_signs_preserve_authored_spacing() {
    for mode in [MathMode::Preserve, MathMode::SingleLine, MathMode::Reflow] {
        let config = math_mode_config(mode);
        for environment in ["multlined", "multline", "aligned", "matrix", "cases"] {
            for (body, expected) in [
                ("1 \\\\\n+   2", "1 \\\\\n+ 2"),
                ("1 \\\\\n+2", "1 \\\\\n+2"),
                ("1 \\\\\n-   2", "1 \\\\\n- 2"),
                ("1 \\\\\n-2", "1 \\\\\n-2"),
                ("1\n+   2", "1\n+ 2"),
                ("1 \\\\\n&+   2", "1 \\\\\n & + 2"),
                ("1 \\\\\n+ {- x}", "1 \\\\\n+ {-x}"),
                ("1 \\\\\nx = - y", "1 \\\\\nx = -y"),
                ("1 \\\\\n+ \\frac{- x}{+ y}", "1 \\\\\n+ \\frac{-x}{+y}"),
                (
                    "1 \\\\\n+ \\begin{matrix}2\\end{matrix}",
                    "1 \\\\\n+ \\begin{matrix}\n    2\n  \\end{matrix}",
                ),
            ] {
                let input = format!("\\begin{{{environment}}}\n{body}\n\\end{{{environment}}}");
                let expected = format!(
                    "  \\begin{{{environment}}}\n{}\n  \\end{{{environment}}}",
                    indent_math_body(&indent_math_body(expected))
                );
                assert_math_host_body(MathHost::DollarDisplay, &input, &expected, &config);
            }
        }
    }
}

#[test]
fn raw_environment_leading_signs_preserve_authored_spacing() {
    for mode in [MathMode::Preserve, MathMode::SingleLine, MathMode::Reflow] {
        for name in ["multline", "align"] {
            let input = format!("\\begin{{{name}}}\n1 \\\\\n+ 2\n\\end{{{name}}}\n");
            let config = math_mode_config(mode);
            let output = format(&input, Some(config.clone()), None);
            assert!(output.contains("\n  + 2\n"), "{output}");
            assert_eq!(format(&output, Some(config), None), output);
        }
    }
}

#[test]
fn verbatim_mode_preserves_math_content() {
    let input = "$$\n\\begin{aligned}\nx &= 1 \\\\\ny &= 22\n\\end{aligned}\n$$\n";
    let expected = "$$\n  \\begin{aligned}\n  x &= 1 \\\\\n  y &= 22\n  \\end{aligned}\n$$\n";
    let output = format(input, Some(math_mode_config(MathMode::Verbatim)), None);
    similar_asserts::assert_eq!(output, expected);
}

#[test]
fn stable_math_host_matrix_formats_simple_and_scripted_bodies_identically() {
    let config = math_host_matrix_config(true);
    let hosts = [
        MathHost::DollarInline,
        MathHost::ParenInline,
        MathHost::DollarDisplay,
        MathHost::BracketDisplay,
        MathHost::RawEnvironment("equation"),
    ];

    for (input, expected) in [("a   +   b", "a + b"), ("H _{ 00}^{-1 }", "H_{00}^{-1}")] {
        for host in hosts {
            let expected_host_body = match host {
                MathHost::DollarInline | MathHost::ParenInline => expected.to_string(),
                MathHost::DollarDisplay
                | MathHost::BracketDisplay
                | MathHost::RawEnvironment(_) => indent_math_body(expected),
            };
            let body = assert_math_host_body(host, input, &expected_host_body, &config);
            let logical_body = match host {
                MathHost::DollarInline | MathHost::ParenInline => body,
                _ => body
                    .lines()
                    .map(|line| {
                        line.strip_prefix("  ")
                            .expect("host body should be indented")
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            };
            assert_eq!(logical_body, expected, "body differed through {host:?}");
        }
    }
}

#[test]
fn stable_math_host_matrix_formats_commented_bodies_identically() {
    let config = math_host_matrix_config(true);
    let input = "a=% relation before comment\n-b";
    let expected = "a =% relation before comment\n-b";

    for host in [
        MathHost::DollarInline,
        MathHost::ParenInline,
        MathHost::DollarDisplay,
        MathHost::BracketDisplay,
        MathHost::RawEnvironment("equation"),
    ] {
        let expected_host_body = match host {
            MathHost::DollarInline | MathHost::ParenInline => expected.to_string(),
            _ => indent_math_body(expected),
        };
        let body = assert_math_host_body(host, input, &expected_host_body, &config);
        let logical_body = match host {
            MathHost::DollarInline | MathHost::ParenInline => body,
            _ => body
                .lines()
                .map(|line| {
                    line.strip_prefix("  ")
                        .expect("host body should be indented")
                })
                .collect::<Vec<_>>()
                .join("\n"),
        };
        assert_eq!(logical_body, expected, "body differed through {host:?}");
    }
}

#[test]
fn stable_math_host_matrix_wraps_display_bodies_identically() {
    let config = Config {
        line_width: 30,
        math_indent: 0,
        ..math_host_matrix_config(true)
    };
    let input = "A = aaaaaaaaaa + bbbbbbbbbb = cccccccccc + dddddddddd";
    let expected = "A = aaaaaaaaaa + bbbbbbbbbb\n  = cccccccccc + dddddddddd";
    let dollar = assert_math_host_body(MathHost::DollarDisplay, input, expected, &config);
    let bracket = assert_math_host_body(MathHost::BracketDisplay, input, expected, &config);

    assert_eq!(dollar, bracket);
}

#[test]
fn math_modes_share_structural_formatting_but_choose_display_breaks() {
    let input = "$$\nA   =   alpha\n+   beta   =   gamma\n$$\n";

    let verbatim_config = math_mode_config(MathMode::Verbatim);
    let verbatim = format(input, Some(verbatim_config.clone()), None);
    assert!(verbatim.contains("A   =   alpha\n  +   beta   =   gamma"));
    similar_asserts::assert_eq!(format(&verbatim, Some(verbatim_config), None), verbatim);

    let preserve_config = math_mode_config(MathMode::Preserve);
    let preserve = format(input, Some(preserve_config.clone()), None);
    assert!(preserve.contains("A = alpha\n  + beta = gamma"));
    similar_asserts::assert_eq!(format(&preserve, Some(preserve_config), None), preserve);

    let single_line_config = math_mode_config(MathMode::SingleLine);
    let single_line = format(input, Some(single_line_config.clone()), None);
    assert!(single_line.contains("A = alpha + beta = gamma"));
    similar_asserts::assert_eq!(
        format(&single_line, Some(single_line_config), None),
        single_line
    );

    let reflow_config = math_mode_config(MathMode::Reflow);
    let reflow = format(input, Some(reflow_config.clone()), None);
    assert!(reflow.contains("A = alpha + beta = gamma"));
    similar_asserts::assert_eq!(format(&reflow, Some(reflow_config), None), reflow);
}

#[test]
fn control_space_survives_math_body_end_and_preserved_line_end() {
    let inline_config = math_mode_config(MathMode::Reflow);
    assert_math_host_body(MathHost::DollarInline, r"x\ ", r"x\ ", &inline_config);

    let preserve_config = math_mode_config(MathMode::Preserve);
    assert_math_host_body(
        MathHost::DollarDisplay,
        "x\\ \ny",
        "  x\\ \n  y",
        &preserve_config,
    );

    let verbatim_config = math_mode_config(MathMode::Verbatim);
    assert_math_host_body(
        MathHost::DollarDisplay,
        "x\\ \ny",
        "  x\\ \n  y",
        &verbatim_config,
    );
}

#[test]
fn single_line_overflows_while_reflow_breaks_to_line_width() {
    let input = "$$\nA = aaaaaaaaaa + bbbbbbbbbb = cccccccccc + dddddddddd\n$$\n";
    let single_line = Config {
        line_width: 30,
        math_indent: 0,
        ..math_mode_config(MathMode::SingleLine)
    };
    let reflow = Config {
        math: MathMode::Reflow,
        ..single_line.clone()
    };

    let single_line = format(input, Some(single_line), None);
    assert!(single_line.contains("A = aaaaaaaaaa + bbbbbbbbbb = cccccccccc + dddddddddd"));

    let reflow = format(input, Some(reflow), None);
    assert!(reflow.contains("A = aaaaaaaaaa + bbbbbbbbbb\n  = cccccccccc + dddddddddd"));
}

#[test]
fn every_non_verbatim_mode_aligns_environment_columns() {
    let input = "$$\n\\begin{aligned}\nx&=1 \\\\\nyy&=22\n\\end{aligned}\n$$\n";
    for mode in [MathMode::Preserve, MathMode::SingleLine, MathMode::Reflow] {
        let output = format(input, Some(math_mode_config(mode)), None);
        assert!(
            output.contains("    x  & = 1  \\\\\n    yy & = 22"),
            "mode {mode:?}: {output}"
        );
    }
}

#[test]
fn stable_math_host_matrix_formats_environment_bodies_identically() {
    let config = math_host_matrix_config(true);
    let input_rows = "x &= 1 \\\\\ny &= 22";
    let expected_rows = "  x & = 1  \\\\\n  y & = 22";
    let input_environment = format!("\\begin{{aligned}}\n{input_rows}\n\\end{{aligned}}");
    let inline_environment = r"\begin{aligned} x & = 1  \\ y & = 22 \end{aligned}";
    let display_environment = indent_math_body(&format!(
        "\\begin{{aligned}}\n{expected_rows}\n\\end{{aligned}}"
    ));

    let dollar_inline = assert_math_host_body(
        MathHost::DollarInline,
        &input_environment,
        inline_environment,
        &config,
    );
    let paren_inline = assert_math_host_body(
        MathHost::ParenInline,
        &input_environment,
        inline_environment,
        &config,
    );
    assert_eq!(dollar_inline, paren_inline);

    let dollar_display = assert_math_host_body(
        MathHost::DollarDisplay,
        &input_environment,
        &display_environment,
        &config,
    );
    let bracket_display = assert_math_host_body(
        MathHost::BracketDisplay,
        &input_environment,
        &display_environment,
        &config,
    );
    assert_eq!(dollar_display, bracket_display);

    let raw_rows = assert_math_host_body(
        MathHost::RawEnvironment("align"),
        input_rows,
        expected_rows,
        &config,
    );
    let logical_display = dollar_display
        .lines()
        .map(|line| {
            line.strip_prefix("  ")
                .expect("display environment should inherit the host indent")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let display_rows = logical_display
        .strip_prefix("\\begin{aligned}\n")
        .and_then(|body| body.strip_suffix("\n\\end{aligned}"))
        .expect("display environment should retain its TeX delimiters");
    assert_eq!(display_rows, raw_rows);
}

#[test]
fn array_column_specification_stays_on_the_begin_line() {
    let input = concat!(
        "$$\n",
        "\\left(\\begin{array}{cc} a & b \\\\ c & d \\end{array}\\right)\n",
        "$$\n",
    );
    let config = math_config(true);
    let output = format(input, Some(config.clone()), None);

    assert!(
        output.contains("\\begin{array}{cc}\n"),
        "the required column specification was detached:\n{output}",
    );
    assert!(!output.contains("\\begin{array}\n"), "{output}");
    similar_asserts::assert_eq!(format(&output, Some(config), None), output);
}

#[test]
fn rwr_math_bodies_drop_trailing_whitespace_per_line() {
    let cases = [
        (
            "2_linear.qmd separated matrices",
            concat!(
                r"\mathbf{Y} = \left(\begin{array}{c} Y_1 \\ Y_2 \end{array}\right)",
                "\n",
                r"\qquad ",
                "\n",
                r"\mathbf{X} = \left(\begin{array}{c} X_1^T \\ X_2^T \end{array}\right)",
            ),
        ),
        (
            "2_linear.qmd weight matrix",
            concat!(
                r"\mathbf{W} = \left(\begin{array}{cc} ",
                "\n",
                r"w_1 & 0 \\ 0 & w_2 \end{array}\right)",
            ),
        ),
        (
            "2_linear.qmd augmented normal equation",
            concat!(
                r"\left(\begin{array}{cc} \mathbf{X}^T \mathbf{L} & \mathbf{D} \end{array}\right) ",
                r"\left(\begin{array}{c} \mathbf{L}^T \mathbf{X} \\ \mathbf{D}^T \end{array}\right) \beta = ",
                "\n",
                r"\left(\begin{array}{cc} \mathbf{X}^T \mathbf{L} & \mathbf{D} \end{array}\right)",
                r"\left(\begin{array}{c} \mathbf{L}^T \mathbf{Y} \\ 0 \end{array}\right).",
            ),
        ),
        (
            "2_linear.qmd block inverse",
            concat!(
                r"\Big((\mathbf{1} \ \ \mathbf{X})^T (\mathbf{1} \ \ \mathbf{X})\Big)^{-1} = ",
                "\n",
                r"\left(\begin{array}{cc} * & * \\ * & \frac{1}{n} \hat{\Sigma}^{-1} \end{array}\right).",
            ),
        ),
        (
            "2_linear.qmd covariance matrix",
            concat!(
                r"\hat{\Sigma} = \left(\begin{array}{cc} ",
                "\n",
                r"\hat{\sigma}_1^2 & \hat{\gamma}^T \\ \hat{\gamma} & \hat{\Sigma}_{-1} \end{array}\right).",
            ),
        ),
        (
            "5_generalized_linear.qmd score derivative",
            concat!(
                r"\partial_{\eta_i} U(\boldsymbol{\eta})_j = ",
                "\n",
                r"\left\{\begin{array}{ll} U'(\eta_j) & \text{if } i=j \\ 0 & \text{if } i \neq j \end{array}\right.",
            ),
        ),
        (
            "8_assessment.qmd diagonal penalty",
            concat!(
                r"\boldsymbol{\Omega} = \lambda \left( ",
                "\n",
                r"\begin{array}{cc} ",
                "\n",
                r"\omega_1 & 0 \\ 0 & \omega_2 \end{array}",
                "\n",
                r"\right)",
            ),
        ),
    ];
    let config = math_config(true);

    for (name, body) in cases {
        let display = MathHost::DollarDisplay.document(body);
        let input = if name == "2_linear.qmd separated matrices"
            || name == "5_generalized_linear.qmd score derivative"
        {
            format!("A preceding paragraph line.\n[Corpus aside:\n{display}]{{.aside}}\n")
        } else if name == "2_linear.qmd block inverse" {
            display
                .lines()
                .map(|line| format!("   {line}"))
                .collect::<Vec<_>>()
                .join("\n")
                + "\n"
        } else {
            display
        };
        let output = format(&input, Some(config.clone()), None);
        let trailing_lines = output
            .lines()
            .enumerate()
            .filter(|(_, line)| line.trim_end_matches([' ', '\t']).len() != line.len())
            .map(|(index, line)| format!("{}: {line:?}", index + 1))
            .collect::<Vec<_>>();

        assert!(
            trailing_lines.is_empty(),
            "{name} retained trailing whitespace:\n{}\noutput:\n{output:?}",
            trailing_lines.join("\n"),
        );
        similar_asserts::assert_eq!(format(&output, Some(config.clone()), None), output);
    }
}

#[test]
fn raw_math_environment_with_leading_bookdown_label_is_idempotent() {
    let mut config = math_host_matrix_config(true);
    config.parser_extensions.bookdown_equation_references = true;
    assert_math_host_body(
        MathHost::RawEnvironment("equation"),
        "(\\#eq:oracle)\nh_N = x.",
        "  (\\#eq:oracle)\n  h_N = x.",
        &config,
    );
}

#[test]
fn math_host_matrix_verbatim_preserves_established_output() {
    let config = math_host_matrix_config(false);
    let input = "a   +   b";

    for host in [MathHost::DollarInline, MathHost::ParenInline] {
        assert_math_host_body(host, input, input, &config);
    }
    for host in [MathHost::DollarDisplay, MathHost::BracketDisplay] {
        assert_math_host_body(host, input, &indent_math_body(input), &config);
    }
    assert_math_host_body(MathHost::RawEnvironment("equation"), input, input, &config);
}

#[test]
fn display_math_default_indent_is_two() {
    let input = "$$\nx + y\n$$\n";
    let expected = "$$\n  x + y\n$$\n";
    let output = format(input, Some(math_config(false)), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(math_config(false)), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn display_math_default_indent_multiline_idempotent() {
    let input = "$$\n\\begin{aligned}\nx &= 1 \\\\\ny &= 22\n\\end{aligned}\n$$\n";
    let expected = "$$\n  \\begin{aligned}\n  x &= 1 \\\\\n  y &= 22\n  \\end{aligned}\n$$\n";
    let output = format(input, Some(math_config(false)), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(math_config(false)), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn bracket_display_math_preserves_following_markdown_structure() {
    let mut config = math_config(false);
    config.parser_extensions.tex_math_single_backslash = true;

    let input = "Before\n\n\\[\n\\begin{bmatrix}1\\\\1\\\\0\\end{bmatrix}\n\\]\n\nAfter\n\n# Heading\nText\n";
    let expected = "Before\n\n\\[\n  \\begin{bmatrix}1\\\\1\\\\0\\end{bmatrix}\n\\]\n\nAfter\n\n# Heading\n\nText\n";
    let output = format(input, Some(config.clone()), None);

    similar_asserts::assert_eq!(output, expected);
    similar_asserts::assert_eq!(format(&output, Some(config), None), output);
}

#[test]
fn list_item_bracket_display_math_preserves_following_markdown_structure() {
    let mut config = math_config(false);
    config.parser_extensions.tex_math_single_backslash = true;

    let input = "- item\n\n  \\[\n  \\begin{bmatrix}1\\\\1\\\\0\\end{bmatrix}\n  \\]\n\n# Heading\n\nText\n";
    let output = format(input, Some(config.clone()), None);

    assert!(
        output.contains("# Heading"),
        "the heading after the list must survive formatting, got:\n{output}"
    );
    similar_asserts::assert_eq!(format(&output, Some(config), None), output);
}

#[test]
fn list_display_math_respects_indent_across_wrap_modes() {
    for wrap in [WrapMode::Reflow, WrapMode::Sentence, WrapMode::Semantic] {
        for math_indent in [0, 2, 4] {
            let config = Config {
                wrap: Some(wrap.clone()),
                math_indent,
                ..math_config(true)
            };
            let input = "1. A\n   $$\n   a=b\n   $$\n";
            let expected = format!("1. A\n   $$\n   {}a = b\n   $$\n", " ".repeat(math_indent));
            let output = format(input, Some(config.clone()), None);

            similar_asserts::assert_eq!(output, expected, "wrap: {wrap:?}, indent: {math_indent}");
            similar_asserts::assert_eq!(format(&output, Some(config), None), output);
        }
    }
}

#[test]
fn nested_list_display_math_keeps_environment_indentation() {
    let input = "- outer\n  - inner\n    $$\n      \\begin{aligned}\n        a & = b\n      \\end{aligned}\n    $$\n";
    let output = format(input, Some(math_config(true)), None);

    similar_asserts::assert_eq!(output, input);
    similar_asserts::assert_eq!(format(&output, Some(math_config(true)), None), output);
}

#[test]
fn display_math_indent_zero_stays_flush() {
    let cfg = Config {
        math_indent: 0,
        ..math_config(false)
    };
    let input = "$$\n  x + y\n$$\n";
    let expected = "$$\nx + y\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn display_environment_is_nested_inside_math_indent() {
    let input = "$$\n\\begin{aligned}\n  a & = b\n\\end{aligned}\n$$\n";
    let expected = "$$\n  \\begin{aligned}\n    a & = b\n  \\end{aligned}\n$$\n";
    let output = format(input, Some(math_config(true)), None);

    similar_asserts::assert_eq!(output, expected);
    similar_asserts::assert_eq!(format(&output, Some(math_config(true)), None), output);
}

#[test]
fn stable_format_math_aligns_environment() {
    let input = "$$\n\\begin{aligned}\nx &= 1 \\\\\ny &= 22 \\\\\nz &= 333\n\\end{aligned}\n$$\n";
    let expected = "$$\n  \\begin{aligned}\n    x & = 1   \\\\\n    y & = 22  \\\\\n    z & = 333\n  \\end{aligned}\n$$\n";
    let output = format(input, Some(math_config(true)), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(math_config(true)), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn stable_format_math_collapses_inline_whitespace() {
    let input = "Inline $a   +   b$ end.\n";
    let output = format(input, Some(math_config(true)), None);
    assert!(output.contains("$a + b$"), "got: {output}");
    let twice = format(&output, Some(math_config(true)), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn stable_format_math_preserves_malformed() {
    let input = "$$\n\\frac{1}{2\n$$\n";
    let output = format(input, Some(math_config(true)), None);
    assert!(output.contains("\\frac{1}{2"), "got: {output}");
    let twice = format(&output, Some(math_config(true)), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn math_no_wrap() {
    let cfg = ConfigBuilder::default()
        .math(MathMode::Verbatim)
        .line_width(10)
        .math_indent(0)
        .build();
    let input = "$$\n\\begin{matrix}\nA & B\\\\\nC & D\n\\end{matrix}\n$$\n";
    let output = format(input, Some(cfg), None);

    similar_asserts::assert_eq!(output, input);
}

/// Config like [`math_config`] but with an explicit `line-width` for the
/// display line-breaker. Pins `math_indent` to 0 so these
/// line-break geometry assertions are isolated from the default base indent
/// (covered separately by the `display_math_default_indent_*` tests).
fn math_config_width(format_math: bool, width: usize) -> Config {
    Config {
        line_width: width,
        math_indent: 0,
        ..math_config(format_math)
    }
}

#[test]
fn stable_format_math_breaks_overwidth_display_chain() {
    let cfg = math_config_width(true, 30);
    let input = "$$\nA = aaaaaaaaaa + bbbbbbbbbb = cccccccccc + dddddddddd\n$$\n";
    let expected = "$$\nA = aaaaaaaaaa + bbbbbbbbbb\n  = cccccccccc + dddddddddd\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn stable_line_break_budget_accounts_for_math_indent() {
    let cfg = Config {
        line_width: 22,
        ..math_config(true) // default math_indent = 2
    };
    let input = "$$\naa = bbbbbb = ccccccc\n$$\n";
    let expected = "$$\n  aa = bbbbbb\n     = ccccccc\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn stable_format_math_nests_binary_under_relations() {
    let cfg = math_config_width(true, 20);
    let input = "$$\nA = aaaaaaaaaa + bbbbbbbbbb = cccccccccc + dddddddddd\n$$\n";
    let expected = "$$\nA = aaaaaaaaaa\n    + bbbbbbbbbb\n  = cccccccccc\n    + dddddddddd\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn stable_binary_continuations_flush_under_operand_no_relation() {
    let cfg = Config {
        line_width: 20,
        ..math_config(true)
    };
    let input = "$$\naaaaaaaa + bbbbbbbb + cccccccc + dddddddd\n$$\n";
    let expected = "$$\n  aaaaaaaa\n  + bbbbbbbb\n  + cccccccc\n  + dddddddd\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn stable_binary_continuations_flush_under_rhs_one_relation() {
    let cfg = Config {
        line_width: 20,
        ..math_config(true)
    };
    let input = "$$\nA = aaaaaaaaaa + bbbbbbbbbb + cccccccccc\n$$\n";
    let expected = "$$\n  A = aaaaaaaaaa\n      + bbbbbbbbbb\n      + cccccccccc\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn stable_relation_continuations_keep_alignment_with_math_indent() {
    let cfg = Config {
        line_width: 20,
        ..math_config(true)
    };
    let input = "$$\nA = aaaaaaaaaa + bbbbbbbbbb = cccccccccc + dddddddddd\n$$\n";
    let expected =
        "$$\n  A = aaaaaaaaaa\n      + bbbbbbbbbb\n    = cccccccccc\n      + dddddddddd\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn stable_format_math_leaves_fitting_display_untouched() {
    let cfg = Config {
        math_indent: 0,
        ..math_config(true)
    };
    let input = "$$\nA = aaaaaaaaaa + bbbbbbbbbb = cccccccccc + dddddddddd\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, input);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn stable_format_math_does_not_break_overwidth_fraction() {
    let cfg = math_config_width(true, 12);
    let input = "$$\n\\frac{aaaaaaaa}{bbbbbbbb}\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, input);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn stable_format_math_breaks_standalone_binary_chain() {
    let cfg = math_config_width(true, 12);
    let input = "$$\naaaa + bbbb + cccc + dddd\n$$\n";
    let expected = "$$\naaaa + bbbb\n+ cccc\n+ dddd\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn display_punctuation_starts_independent_relation_chains() {
    let config = Config {
        line_width: 24,
        ..math_config(true)
    };
    for punctuation in [",", ";"] {
        assert_math_host_body(
            MathHost::DollarDisplay,
            &format!("A = bbbbbbbbbb{punctuation} B = cccccccccc = dddddddddd"),
            &format!("  A = bbbbbbbbbb{punctuation}\n  B = cccccccccc\n    = dddddddddd"),
            &config,
        );
    }
}

#[test]
fn display_punctuation_keeps_fitting_expressions_together() {
    assert_math_host_body(
        MathHost::DollarDisplay,
        "A = b, B = c; C = d",
        "  A = b, B = c; C = d",
        &math_config(true),
    );
}

#[test]
fn display_qquad_separates_wrapped_expressions_on_its_own_line() {
    let config = Config {
        line_width: 24,
        ..math_config(true)
    };
    for punctuation in ["", ",", ";"] {
        assert_math_host_body(
            MathHost::DollarDisplay,
            &format!(r"A = bbbbbbbbbb{punctuation} \qquad BBBB = cccccccccc = dddddddddd"),
            &format!(
                "  A = bbbbbbbbbb{punctuation}\n  \\qquad\n  BBBB = cccccccccc\n       = dddddddddd"
            ),
            &config,
        );
    }
}

#[test]
fn display_qquad_stays_inline_when_expressions_fit() {
    assert_math_host_body(
        MathHost::DollarDisplay,
        "A = b,\n\\qquad\nB = c",
        r"  A = b, \qquad B = c",
        &math_config(true),
    );
}

#[test]
fn display_nested_punctuation_is_not_a_break_point() {
    let config = Config {
        line_width: 20,
        ..math_config(true)
    };
    for body in [
        "(aaaaaaaaaa, bbbbbbbbbb)",
        "[aaaaaaaaaa; bbbbbbbbbb]",
        "{aaaaaaaaaa, bbbbbbbbbb}",
        r"\left( aaaaaaaaaa, bbbbbbbbbb \right)",
        r"\frac{aaaaaaaaaa, bbbbbbbbbb}{c}",
        r"(aaaaaaaaaa \qquad bbbbbbbbbb)",
        r"{aaaaaaaaaa \qquad bbbbbbbbbb}",
    ] {
        assert_math_host_body(
            MathHost::DollarDisplay,
            body,
            &indent_math_body(body),
            &config,
        );
    }
}

#[test]
fn display_relations_follow_the_wrapped_first_relation() {
    let config = Config {
        line_width: 20,
        ..math_config(true)
    };
    assert_math_host_body(
        MathHost::DollarDisplay,
        "aaaaaaaaaa = bbbbbbbbbb = cccccccccc",
        "  aaaaaaaaaa\n  = bbbbbbbbbb\n  = cccccccccc",
        &config,
    );
}

#[test]
fn rwr_free_displays_choose_complete_operator_segments() {
    let config = math_config(true);
    let cases = [
        (
            "8_assessment.qmd AUC definition",
            r"\mathrm{AUC} = \mathbf{P}(\eta_0 < \eta_1) + \frac{1}{2} \mathbf{P}(\eta_0 = \eta_1).",
            r"\mathrm{AUC} = \mathbf{P}(\eta_0 < \eta_1)
               + \frac{1}{2} \mathbf{P}(\eta_0 = \eta_1).",
        ),
        (
            r"8_assessment.qmd conditional AUC variables",
            r"\eta_0 = \eta(X) \mid Y = 0 \quad \text{and} \quad \eta_1 = \eta(X) \mid Y = 1.",
            r"\eta_0 = \eta(X) \mid Y = 0 \quad \text{and} \quad \eta_1 = \eta(X) \mid Y = 1.",
        ),
        (
            "2_linear.qmd confidence interval",
            r"a^T\hat{\beta} \pm z_{n-p} \cdot \hat{\sigma} \sqrt{a^T(\mathbf{X}^T\mathbf{X})^{-1}a}",
            r"a^T\hat{\beta} \pm z_{n-p}
\cdot \hat{\sigma} \sqrt{a^T(\mathbf{X}^T\mathbf{X})^{-1}a}",
        ),
        (
            "5_generalized_linear.qmd confidence interval",
            r"a^T\hat{\beta} \pm z \cdot \sqrt{\hat{\varphi} a^T(\mathbf{X}^T\hat{\mathbf{W}}\mathbf{X})^{-1}a}",
            r"a^T\hat{\beta} \pm z
\cdot \sqrt{\hat{\varphi} a^T(\mathbf{X}^T\hat{\mathbf{W}}\mathbf{X})^{-1}a}",
        ),
        (
            "13_interval.qmd standard confidence interval",
            r"\hat{\gamma} \pm z \cdot \hat{\mathrm{se}} = (\hat{\gamma} - z \cdot \hat{\mathrm{se}}, \hat{\gamma} + z \cdot \hat{\mathrm{se}}).",
            r"\hat{\gamma} \pm z \cdot \hat{\mathrm{se}}
= (\hat{\gamma} - z \cdot \hat{\mathrm{se}}, \hat{\gamma} + z \cdot \hat{\mathrm{se}}).",
        ),
        (
            "13_interval.qmd bootstrap confidence interval",
            r"\{ \gamma \mid (\hat{\gamma} - \gamma)^2 < \hat{c}_{\alpha} {\hat{\mathrm{se}}^2}\} = \hat{\gamma} \pm \sqrt{\hat{c}_{\alpha}} \cdot \hat{\mathrm{se}}.",
            r"\{ \gamma \mid (\hat{\gamma} - \gamma)^2 < \hat{c}_{\alpha} {\hat{\mathrm{se}}^2}\}
= \hat{\gamma} \pm \sqrt{\hat{c}_{\alpha}}\cdot\hat{\mathrm{se}}.",
        ),
    ];

    for (name, input, expected) in cases {
        let output = assert_math_host_body(
            MathHost::DollarDisplay,
            input,
            &indent_math_body(expected),
            &config,
        );
        assert_eq!(output, indent_math_body(expected), "{name}");
    }
}

#[test]
fn stable_format_math_nests_binary_under_single_relation() {
    let cfg = math_config_width(true, 20);
    let input = "$$\nA = aaaaaaaaaa + bbbbbbbbbb + cccccccccc\n$$\n";
    let expected = "$$\nA = aaaaaaaaaa\n    + bbbbbbbbbb\n    + cccccccccc\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn relation_chain_long_lhs_anchors_under_rhs() {
    let cfg = math_config(true); // default line-width 80, math-indent 2
    let input = "$$\n  \\beta_0 \\gets \\beta_0 + \\frac{4}{n} \\sum_{i = 1}^n (y_i - p_i)\n          = \\beta_0 - \\frac{1}{L_0} \\partial_0 F, \\qquad L_0\n          = 1/4,\n$$\n";
    let expected = "$$\n  \\beta_0 \\gets \\beta_0 + \\frac{4}{n} \\sum_{i=1}^n (y_i - p_i)\n                = \\beta_0 - \\frac{1}{L_0} \\partial_0 F, \\qquad L_0 = 1/4,\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn relation_chain_uniform_relations_align_under_first() {
    let cfg = Config {
        line_width: 20,
        ..math_config(true)
    };
    let input = "$$\nA = bbbbbbbbbb = cccccccccc\n$$\n";
    let expected = "$$\n  A = bbbbbbbbbb\n    = cccccccccc\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn relation_chain_math_indent_zero_aligns_relations() {
    let cfg = math_config_width(true, 20); // math_indent 0
    let input = "$$\nA = bbbbbbbbbb = cccccccccc\n$$\n";
    let expected = "$$\nA = bbbbbbbbbb\n  = cccccccccc\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn hardbreak_equality_chain_aligns_relations() {
    let cfg = math_config(true);
    let input = "$$\nx = a \\\\\n= b \\\\\n= c\n$$\n";
    let expected = "$$\n  x = a \\\\\n    = b \\\\\n    = c\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn hardbreak_assignment_chain_anchors_under_rhs() {
    let cfg = math_config(true);
    let input = "$$\nx \\gets a \\\\\n= b \\\\\n= c\n$$\n";
    let expected = "$$\n  x \\gets a \\\\\n          = b \\\\\n          = c\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn hardbreak_definition_relations_follow_badness_rows() {
    let cfg = math_config(true);
    let input = "$$\nA :=_i a \\\\\n:=_j b \\\\\n= c\n$$\n";
    let expected = "$$\n  A :=_i a \\\\\n  :=_j b \\\\\n  = c\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn hardbreak_non_chain_stays_flush() {
    let cfg = math_config(true);
    let input = "$$\na \\\\\nb \\\\\nc\n$$\n";
    let expected = "$$\n  a \\\\\n  b \\\\\n  c\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn hardbreak_ampersand_block_not_implicitly_aligned() {
    let cfg = math_config(true);
    let input = "$$\nx &= a \\\\\n&= b\n$$\n";
    let expected = "$$\n  x & = a \\\\\n  & = b\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn hardbreak_overwidth_continuation_nests_under_its_column() {
    let cfg = Config {
        line_width: 24,
        ..math_config(true)
    };
    let input = "$$\nx = a \\\\\n= bbbbbbbb + cccccccc + dddddddd\n$$\n";
    let expected = "$$\n  x = a \\\\\n    = bbbbbbbb\n      + cccccccc\n      + dddddddd\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn hardbreak_relation_chain_in_verbatim_mode_is_not_aligned() {
    let cfg = math_config(false);
    let input = "$$\nx = a \\\\\n= b \\\\\n= c\n$$\n";
    let expected = "$$\n  x = a \\\\\n  = b \\\\\n  = c\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn display_math_with_followup_text_is_idempotent_in_rmarkdown() {
    let flavor = Flavor::RMarkdown;
    let config = Config {
        flavor,
        parser_extensions: Extensions::for_flavor(flavor),
        ..Default::default()
    };
    let input = r#"Assuming that the moment generating function of $X$ is finite, 
$M(t) = \E(e^{tX}) < \infty$, for some suitable $t \in \mathbb{R}$, it follows from
[Markov's inequality](https://en.wikipedia.org/wiki/Markov%27s_inequality) that
$$P(X - \mu > \varepsilon) = P(e^{tX} > e^{t(\varepsilon + \mu)}) \leq e^{-t(\varepsilon + \mu)}M(t),$$
which can provide a very tight upper bound by minimizing the bound over $t$. This 
requires some knowledge of the moment generating function. We illustrate the 
usage of this inequality below by considering the gamma distribution where the 
moment generating function is well known.
"#;
    let output1 = format(input, Some(config.clone()), None);
    let output2 = format(&output1, Some(config), None);
    assert_eq!(output1, output2, "Formatting should be idempotent");
}

#[test]
fn stable_format_math_tightens_scripts() {
    let input = "$$\n  H _{ 00}^{-1 }\n$$\n";
    let expected = "$$\n  H_{00}^{-1}\n$$\n";
    let output = format(input, Some(math_config(true)), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(math_config(true)), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn stable_format_math_trims_math_group_interiors() {
    let input = "Inline $x_{ a }$ and ${ { a } }$ end.\n";
    let output = format(input, Some(math_config(true)), None);
    assert!(output.contains("$x_{a}$"), "got: {output}");
    assert!(output.contains("${{a}}$"), "got: {output}");
    let twice = format(&output, Some(math_config(true)), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn stable_format_math_preserves_text_group_interiors() {
    let input = "Inline $\\text{ a }$ and $\\text{a {b} c}$ end.\n";
    let output = format(input, Some(math_config(true)), None);
    assert!(output.contains("$\\text{ a }$"), "got: {output}");
    assert!(output.contains("$\\text{a {b} c}$"), "got: {output}");
    let twice = format(&output, Some(math_config(true)), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn stable_format_math_preserves_unproven_argument_interiors() {
    let input = "Inline $\\unknown{ a   +   b }\\sqrt{ c   +   d }{ e   +   f }$ end.\n";
    let expected = "Inline $\\unknown{ a   +   b }\\sqrt{c + d}{ e   +   f }$ end.\n";
    let output = format(input, Some(math_config(true)), None);
    similar_asserts::assert_eq!(output, expected);
    similar_asserts::assert_eq!(format(&output, Some(math_config(true)), None), output);
}

#[test]
fn stable_format_math_preserves_malformed_argument_bytes() {
    let input = "Inline $\\frac{ a   +   b }{ c   +   d$ end.\n";
    similar_asserts::assert_eq!(format(input, Some(math_config(true)), None), input);
}

#[test]
fn configured_math_signature_controls_argument_recursion() {
    let mut cfg = math_config(true);
    cfg.math_signatures.insert(
        "custom".to_string(),
        vec![
            panache_formatter::MathArgumentConfig {
                kind: ArgKind::Bracket,
                domain: ArgumentDomain::Text,
            },
            panache_formatter::MathArgumentConfig {
                kind: ArgKind::Brace,
                domain: ArgumentDomain::Unknown,
            },
            panache_formatter::MathArgumentConfig {
                kind: ArgKind::Brace,
                domain: ArgumentDomain::Math,
            },
        ],
    );
    let input = "Inline $\\custom[ t + u ]{ v   +   w }{ a   +   b }{ x   +   y }$ end.\n";
    let expected = "Inline $\\custom[ t + u ]{ v   +   w }{a + b}{ x   +   y }$ end.\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    similar_asserts::assert_eq!(format(&output, Some(cfg), None), output);
}

#[test]
fn configured_math_signature_is_inert_in_verbatim_mode() {
    let mut cfg = math_config(false);
    cfg.math_signatures.insert(
        "custom".to_string(),
        vec![panache_formatter::MathArgumentConfig {
            kind: ArgKind::Brace,
            domain: ArgumentDomain::Math,
        }],
    );
    let input = "Inline $\\custom{ a   +   b }$ end.\n";
    similar_asserts::assert_eq!(format(input, Some(cfg), None), input);
}

#[test]
fn raw_definition_shadows_configured_math_signature() {
    let mut cfg = math_config(true);
    cfg.math_signatures.insert(
        "custom".to_string(),
        vec![panache_formatter::MathArgumentConfig {
            kind: ArgKind::Brace,
            domain: ArgumentDomain::Math,
        }],
    );
    let input = "\\newcommand{\\custom}[1]{#1}\n\nInline $\\custom{ a   +   b }$ end.\n";
    let output = format(input, Some(cfg.clone()), None);
    assert!(output.contains("$\\custom{ a   +   b }$"), "got: {output}");
    similar_asserts::assert_eq!(format(&output, Some(cfg), None), output);
}

#[test]
fn display_math_block_inside_paragraph_stays_idempotent_in_rmarkdown() {
    let flavor = Flavor::RMarkdown;
    let config = Config {
        flavor,
        parser_extensions: Extensions::for_flavor(flavor),
        ..Default::default()
    };
    let input = r#"Modulus distribution:

Note that for $m \neq 0, N/2$,  $\beta_m = 0$ and $y \sim \mathcal{N}(\Phi\beta, \sigma^2 I_N)$ then
$$(\mathrm{Re}(\hat{\beta}_m), \mathrm{Im}(\hat{\beta}_m))^T \sim \mathcal{N}\left(0, \frac{\sigma^2}{2} I_2\right),$$

hence
"#;
    let output1 = format(input, Some(config.clone()), None);
    let output2 = format(&output1, Some(config), None);
    assert_eq!(output1, output2, "Formatting should be idempotent");
}

#[test]
fn wrapped_inline_math_marker_boundary_is_idempotent_in_rmarkdown() {
    let flavor = Flavor::RMarkdown;
    let config = Config {
        flavor,
        parser_extensions: Extensions::for_flavor(flavor),
        ..Default::default()
    };
    let input = r#"If the mean depends on the predictors in a log-linear way, $\log(\mu(x_i)) = x_i^T \beta$,
then
$$p_i(y_i \mid x_i) = e^{\beta^T x_i y_i - \exp( x_i^T \beta)} \frac{1}{y_i!}.$$
"#;
    let output1 = format(input, Some(config.clone()), None);
    let output2 = format(&output1, Some(config), None);
    assert_eq!(output1, output2, "Formatting should be idempotent");
}

#[test]
fn poisson_example_snippet_is_idempotent_in_rmarkdown() {
    let flavor = Flavor::RMarkdown;
    let config = Config {
        flavor,
        parser_extensions: Extensions::for_flavor(flavor),
        ..Default::default()
    };
    let input = r#"::: {.example .boxed #poisson-regression} 
If $y_i \in \mathbb{N}_0$ are counts we often use a Poisson regression model 
with point probabilities (density w.r.t. counting measure)
$$
p_i(y_i \mid x_i) = e^{-\mu(x_i)} \frac{\mu(x_i)^{y_i}}{y_i!}.
$$
If the mean depends on the predictors in a log-linear way, $ 
\log(\mu(x_i)) = x_i^T \beta$, then
$$
p_i(y_i \mid x_i) = e^{\beta^T x_i y_i - \exp( x_i^T \beta)} \frac{1}{y_i!}.
$$
The factor $1/y_i!$ can be absorbed into the base measure, and we recognize this
Poisson regression model as an exponential family with sufficient statistics
$$
t_i(y_i) = x_i y_i
$$
and
$$
\log \varphi_i(\beta) =  \exp( x_i^T \beta).
$$
 
To implement numerical optimization algorithms for computing the 
maximum-likelihood estimate we note that 
$$t(\mathbf{y}) = \sum_{i=1}^N x_i y_i = \mathbf{X}^T \mathbf{y} \quad \text{and} \quad
\kappa(\beta) = \sum_{i=1}^N e^{x_i^T \beta},$$
"#;
    let output1 = format(input, Some(config.clone()), None);
    let output2 = format(&output1, Some(config), None);
    assert_eq!(output1, output2, "Formatting should be idempotent");
}
