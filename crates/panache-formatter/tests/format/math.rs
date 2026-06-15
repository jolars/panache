use panache_formatter::config::{Extensions, Flavor};
use panache_formatter::format;
use panache_formatter::{Config, ConfigBuilder};

/// Config with `tex_math_dollars` on (Quarto default) and the experimental math
/// formatter toggled.
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
    // Regression lock: with the experimental formatter off, alignment columns
    // are NOT reformatted (`x &= 1` stays `x &= 1`). The default two-space
    // `math-indent` still applies, so content is indented but otherwise verbatim.
    let input = "$$\n\\begin{aligned}\nx &= 1 \\\\\ny &= 22\n\\end{aligned}\n$$\n";
    let expected = "$$\n  \\begin{aligned}\n  x &= 1 \\\\\n  y &= 22\n  \\end{aligned}\n$$\n";
    let output = format(input, Some(math_config(false)), None);
    similar_asserts::assert_eq!(output, expected);
}

#[test]
fn display_math_default_indent_is_two() {
    // The default config indents `$$` content by two spaces.
    let input = "$$\nx + y\n$$\n";
    let expected = "$$\n  x + y\n$$\n";
    let output = format(input, Some(math_config(false)), None);
    similar_asserts::assert_eq!(output, expected);
    // Idempotent across passes (no indent stacking).
    let twice = format(&output, Some(math_config(false)), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn display_math_default_indent_multiline_idempotent() {
    // Multiline content must re-indent idempotently: the second pass sees the
    // two-space indent on every line, strips it as common indentation, and
    // re-applies the same pad rather than stacking.
    let input = "$$\n\\begin{aligned}\nx &= 1 \\\\\ny &= 22\n\\end{aligned}\n$$\n";
    let expected = "$$\n  \\begin{aligned}\n  x &= 1 \\\\\n  y &= 22\n  \\end{aligned}\n$$\n";
    let output = format(input, Some(math_config(false)), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(math_config(false)), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn display_math_indent_zero_stays_flush() {
    // Explicit `math-indent = 0` keeps the old flush-left behavior.
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
    // `&` columns and trailing `\\` both align.
    let expected = "$$\n\\begin{aligned}\n  x & = 1   \\\\\n  y & = 22  \\\\\n  z & = 333\n\\end{aligned}\n$$\n";
    let output = format(input, Some(math_config(true)), None);
    similar_asserts::assert_eq!(output, expected);
    // Idempotent.
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
    // Unclosed group → bail to verbatim even with the gate on.
    let input = "$$\n\\frac{1}{2\n$$\n";
    let output = format(input, Some(math_config(true)), None);
    assert!(output.contains("\\frac{1}{2"), "got: {output}");
    let twice = format(&output, Some(math_config(true)), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn math_no_wrap() {
    // Pin `math_indent` to 0 so this asserts only that math content is not
    // wrapped (the default base indent is covered elsewhere).
    let cfg = ConfigBuilder::default()
        .line_width(10)
        .math_indent(0)
        .build();
    let input = "$$\n\\begin{matrix}\nA & B\\\\\nC & D\n\\end{matrix}\n$$\n";
    let output = format(input, Some(cfg), None);

    // Math blocks should not be wrapped
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
    // Breaks at the second (top-level) relation; the continuation aligns under
    // the first `=`. The `+` sub-terms stay put (binary outranked by relations).
    let expected = "$$\nA = aaaaaaaaaa + bbbbbbbbbb\n  = cccccccccc + dddddddddd\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    // Idempotent: the already-broken multi-line form re-joins and re-breaks
    // to the identical layout.
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn experimental_line_break_budget_accounts_for_math_indent() {
    // The flat `math-indent` is charged against `line-width`: the chain is 21
    // chars wide, which fits in `line-width` 22 on its own, but the two-space
    // `math-indent` would push it to 23. So it is broken at the second relation,
    // keeping every emitted line within `line-width`.
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
    // Narrow enough that each relation segment overflows ⇒ the `+` terms nest
    // one level deeper under the relation right-hand side.
    let expected = "$$\nA = aaaaaaaaaa\n    + bbbbbbbbbb\n  = cccccccccc\n    + dddddddddd\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn experimental_binary_continuations_pick_up_math_indent_no_relation() {
    // With the default two-space `math-indent`, a broken binary chain's
    // continuation lines nest one `math-indent` deeper than the head instead of
    // sitting flush under it.
    let cfg = Config {
        line_width: 20,
        ..math_config(true)
    };
    let input = "$$\naaaaaaaa + bbbbbbbb + cccccccc + dddddddd\n$$\n";
    let expected = "$$\n  aaaaaaaa\n    + bbbbbbbb\n    + cccccccc\n    + dddddddd\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn experimental_binary_continuations_pick_up_math_indent_one_relation() {
    // The binary terms nest one `math-indent` past the relation right-hand side.
    let cfg = Config {
        line_width: 20,
        ..math_config(true)
    };
    let input = "$$\nA = aaaaaaaaaa + bbbbbbbbbb + cccccccccc\n$$\n";
    let expected = "$$\n  A = aaaaaaaaaa\n        + bbbbbbbbbb\n        + cccccccccc\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn experimental_relation_continuations_keep_alignment_with_math_indent() {
    // Relation continuations still align under the first `=` (both at column 4);
    // only the binary terms pick up the extra `math-indent`.
    let cfg = Config {
        line_width: 20,
        ..math_config(true)
    };
    let input = "$$\nA = aaaaaaaaaa + bbbbbbbbbb = cccccccccc + dddddddddd\n$$\n";
    let expected =
        "$$\n  A = aaaaaaaaaa\n        + bbbbbbbbbb\n    = cccccccccc\n        + dddddddddd\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, expected);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn experimental_format_math_leaves_fitting_display_untouched() {
    // The same equation under the default 80-col width is not broken.
    // Pin `math_indent` to 0 so "untouched" means byte-identical content.
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
    // No top-level relation ⇒ nothing to break against; the over-width fraction
    // stays on one line (like an unbreakable long word in prose reflow).
    let cfg = math_config_width(true, 12);
    let input = "$$\n\\frac{aaaaaaaa}{bbbbbbbb}\n$$\n";
    let output = format(input, Some(cfg.clone()), None);
    similar_asserts::assert_eq!(output, input);
    let twice = format(&output, Some(cfg), None);
    similar_asserts::assert_eq!(twice, output);
}

#[test]
fn experimental_format_math_breaks_standalone_binary_chain() {
    // No relation at all: the first term is the head and each `+ term` aligns
    // flush under it (the unifying rule — a binary continuation sits under the
    // first term of its operand sequence).
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
    // One relation with an over-width binary RHS: the `+` terms nest under the
    // right-hand side (no second relation to start a continuation against).
    let cfg = math_config_width(true, 20);
    let input = "$$\nA = aaaaaaaaaa + bbbbbbbbbb + cccccccccc\n$$\n";
    let expected = "$$\nA = aaaaaaaaaa\n    + bbbbbbbbbb\n    + cccccccccc\n$$\n";
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
