use panache_formatter::config::{Extensions, Flavor};
use panache_formatter::format;
use panache_formatter::{Config, ConfigBuilder};
use panache_parser::semantic::math::{ArgKind, ArgumentDomain};

fn math_config(format_math: bool) -> Config {
    let flavor = Flavor::Quarto;
    Config {
        flavor,
        parser_extensions: Extensions::for_flavor(flavor),
        experimental_format_math: format_math,
        ..Default::default()
    }
}

#[test]
fn experimental_format_math_defaults_off() {
    let input = "$$\n\\begin{aligned}\nx &= 1 \\\\\ny &= 22\n\\end{aligned}\n$$\n";
    let expected = "$$\n  \\begin{aligned}\n  x &= 1 \\\\\n  y &= 22\n  \\end{aligned}\n$$\n";
    let output = format(input, Some(math_config(false)), None);
    similar_asserts::assert_eq!(output, expected);
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
fn experimental_format_math_aligns_environment() {
    let input = "$$\n\\begin{aligned}\nx &= 1 \\\\\ny &= 22 \\\\\nz &= 333\n\\end{aligned}\n$$\n";
    let expected = "$$\n\\begin{aligned}\n  x & = 1   \\\\\n  y & = 22  \\\\\n  z & = 333\n\\end{aligned}\n$$\n";
    let output = format(input, Some(math_config(true)), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(math_config(true)), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn experimental_format_math_collapses_inline_whitespace() {
    let input = "Inline $a   +   b$ end.\n";
    let output = format(input, Some(math_config(true)), None);
    assert!(output.contains("$a + b$"), "got: {output}");
    let twice = format(&output, Some(math_config(true)), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn experimental_format_math_preserves_malformed() {
    let input = "$$\n\\frac{1}{2\n$$\n";
    let output = format(input, Some(math_config(true)), None);
    assert!(output.contains("\\frac{1}{2"), "got: {output}");
    let twice = format(&output, Some(math_config(true)), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn math_no_wrap() {
    let cfg = ConfigBuilder::default()
        .line_width(10)
        .math_indent(0)
        .build();
    let input = "$$\n\\begin{matrix}\nA & B\\\\\nC & D\n\\end{matrix}\n$$\n";
    let output = format(input, Some(cfg), None);

    similar_asserts::assert_eq!(output, input);
}

/// Config like [`math_config`] but with an explicit `line-width` for the
/// experimental display line-breaker. Pins `math_indent` to 0 so these
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
fn experimental_format_math_breaks_overwidth_display_chain() {
    let cfg = math_config_width(true, 30);
    let input = "$$\nA = aaaaaaaaaa + bbbbbbbbbb = cccccccccc + dddddddddd\n$$\n";
    let expected = "$$\nA = aaaaaaaaaa + bbbbbbbbbb\n  = cccccccccc + dddddddddd\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn experimental_line_break_budget_accounts_for_math_indent() {
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
fn experimental_format_math_nests_binary_under_relations() {
    let cfg = math_config_width(true, 20);
    let input = "$$\nA = aaaaaaaaaa + bbbbbbbbbb = cccccccccc + dddddddddd\n$$\n";
    let expected = "$$\nA = aaaaaaaaaa\n    + bbbbbbbbbb\n  = cccccccccc\n    + dddddddddd\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn experimental_binary_continuations_flush_under_operand_no_relation() {
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
fn experimental_binary_continuations_flush_under_rhs_one_relation() {
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
fn experimental_relation_continuations_keep_alignment_with_math_indent() {
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
fn experimental_format_math_leaves_fitting_display_untouched() {
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
fn experimental_format_math_does_not_break_overwidth_fraction() {
    let cfg = math_config_width(true, 12);
    let input = "$$\n\\frac{aaaaaaaa}{bbbbbbbb}\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, input);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn experimental_format_math_breaks_standalone_binary_chain() {
    let cfg = math_config_width(true, 12);
    let input = "$$\naaaa + bbbb + cccc + dddd\n$$\n";
    let expected = "$$\naaaa\n+ bbbb\n+ cccc\n+ dddd\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn experimental_format_math_nests_binary_under_single_relation() {
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
    let expected = "$$\n  \\beta_0 \\gets \\beta_0 + \\frac{4}{n} \\sum_{i = 1}^n (y_i - p_i)\n                = \\beta_0 - \\frac{1}{L_0} \\partial_0 F, \\qquad L_0\n                = 1/4,\n$$\n";
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
fn hardbreak_repeated_assignments_align_the_operators() {
    let cfg = math_config(true);
    let input = "$$\nA :=_i a \\\\\n:=_j b \\\\\n= c\n$$\n";
    let expected = "$$\n  A :=_i a \\\\\n    :=_j b \\\\\n         = c\n$$\n";
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
fn hardbreak_relation_chain_gate_off_is_not_aligned() {
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
fn experimental_format_math_tightens_scripts() {
    let input = "$$\n  H _{ 00}^{-1 }\n$$\n";
    let expected = "$$\n  H_{00}^{-1}\n$$\n";
    let output = format(input, Some(math_config(true)), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(math_config(true)), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn experimental_format_math_trims_math_group_interiors() {
    let input = "Inline $x_{ a }$ and ${ { a } }$ end.\n";
    let output = format(input, Some(math_config(true)), None);
    assert!(output.contains("$x_{a}$"), "got: {output}");
    assert!(output.contains("${{a}}$"), "got: {output}");
    let twice = format(&output, Some(math_config(true)), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn experimental_format_math_preserves_text_group_interiors() {
    let input = "Inline $\\text{ a }$ and $\\text{a {b} c}$ end.\n";
    let output = format(input, Some(math_config(true)), None);
    assert!(output.contains("$\\text{ a }$"), "got: {output}");
    assert!(output.contains("$\\text{a {b} c}$"), "got: {output}");
    let twice = format(&output, Some(math_config(true)), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn experimental_format_math_preserves_unproven_argument_interiors() {
    let input = "Inline $\\unknown{ a   +   b }\\sqrt{ c   +   d }{ e   +   f }$ end.\n";
    let expected = "Inline $\\unknown{ a   +   b }\\sqrt{c + d}{ e   +   f }$ end.\n";
    let output = format(input, Some(math_config(true)), None);
    similar_asserts::assert_eq!(output, expected);
    similar_asserts::assert_eq!(format(&output, Some(math_config(true)), None), output);
}

#[test]
fn experimental_format_math_preserves_malformed_argument_bytes() {
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
fn configured_math_signature_is_inert_when_gate_is_off() {
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
