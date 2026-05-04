use panache_formatter::config::{Extensions, Flavor};
use panache_formatter::format;
use panache_formatter::{Config, ConfigBuilder};

#[test]
fn math_no_wrap() {
    let cfg = ConfigBuilder::default().line_width(10).build();
    let input = "$$\n\\begin{matrix}\nA & B\\\\\nC & D\n\\end{matrix}\n$$\n";
    let output = format(input, Some(cfg), None);

    // Math blocks should not be wrapped
    similar_asserts::assert_eq!(output, input);
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
