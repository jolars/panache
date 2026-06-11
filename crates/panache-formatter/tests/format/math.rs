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
    // Regression lock: an alignment block is emitted verbatim by default.
    let input = "$$\n\\begin{aligned}\nx &= 1 \\\\\ny &= 22\n\\end{aligned}\n$$\n";
    let output = format(input, Some(math_config(false)), None);
    similar_asserts::assert_eq!(output, input);
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
    let cfg = ConfigBuilder::default().line_width(10).build();
    let input = "$$\n\\begin{matrix}\nA & B\\\\\nC & D\n\\end{matrix}\n$$\n";
    let output = format(input, Some(cfg), None);

    // Math blocks should not be wrapped
    similar_asserts::assert_eq!(output, input);
}

/// Config like [`math_config`] but with an explicit `line-width` for the
/// experimental display line-breaker.
fn math_config_width(format_math: bool, width: usize) -> Config {
    Config {
        line_width: width,
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
fn experimental_format_math_leaves_fitting_display_untouched() {
    // The same equation under the default 80-col width is not broken.
    let cfg = math_config(true);
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
