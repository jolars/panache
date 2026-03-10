use panache::config::{Extensions, Flavor};
use panache::format;
use panache::{Config, ConfigBuilder};

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
        extensions: Extensions::for_flavor(flavor),
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
        extensions: Extensions::for_flavor(flavor),
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
