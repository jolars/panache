use panache::config::{Extensions, Flavor};
use panache::{Config, format};

#[test]
fn quote_single_line() {
    let input = "> This is a single line quote.\n";
    let output = format(input, None, None);

    assert!(output.starts_with("> "));
    assert!(output.contains("single line quote"));
}

#[test]
fn quote_multi_line_continuous() {
    let input = "> This is a multi-line quote\n> that continues on the next line.\n";
    let output = format(input, None, None);

    for line in output.lines() {
        assert!(
            line.starts_with("> "),
            "Line should start with '>': '{line}'"
        );
    }
    assert!(output.contains("multi-line quote"));
    assert!(output.contains("continues on the next line"));
}

#[test]
fn quote_with_fenced_code_block() {
    let input = r#"> A blockquote with code:
>
> ```python
> def hello():
>     print("world")
> ```
>
> Text after code.
"#;

    // Use config with empty formatters to avoid external formatter invocation
    let config = panache::Config {
        formatters: std::collections::HashMap::new(),
        ..Default::default()
    };

    let output = format(input, Some(config), None);

    // Should preserve blockquote markers
    assert!(output.contains("> ```python"));
    assert!(output.contains("> def hello():"));
    assert!(output.contains(">     print(\"world\")"));
    assert!(output.contains("> ```"));
    assert!(output.contains("> Text after code"));
}

#[test]
fn quote_with_inline_code() {
    let input = "> Quote with `inline code` in it.\n";
    let output = format(input, None, None);

    assert!(output.contains("> Quote with `inline code` in it"));
}

#[test]
fn quote_with_indented_code_block() {
    let input = r#"> Blockquote with indented code:
>
>     x = 1
>     y = 2
"#;

    let output = format(input, None, None);

    // Indented code blocks get normalized to fenced code blocks
    assert!(output.contains("> ```"));
    assert!(output.contains("> x = 1"));
    assert!(output.contains("> y = 2"));
}

#[test]
fn nested_quote_with_code() {
    let input = r#"> Outer quote
>
> > Nested quote with code:
> >
> > ```
> > code here
> > ```
"#;

    let output = format(input, None, None);

    // Should handle nested blockquotes with code
    assert!(output.contains("> > ```"));
    assert!(output.contains("> > code here"));
}

#[test]
fn quote_with_list() {
    let input = r#"> A list in a blockquote:
>
> 1. First item
> 2. Second item
"#;

    let output = format(input, None, None);

    assert!(output.contains("> 1. First item"));
    assert!(output.contains("> 2. Second item"));
}

#[test]
fn quote_with_multiple_code_blocks() {
    let input = r#"> Multiple code blocks:
>
> ```
> first
> ```
>
> Some text.
>
> ```
> second
> ```
"#;

    let output = format(input, None, None);

    // Should handle multiple code blocks in same blockquote
    assert!(output.contains("> ```"));
    assert!(output.contains("> first"));
    assert!(output.contains("> Some text"));
    assert!(output.contains("> second"));
}

#[test]
fn quote_idempotency_with_code() {
    let input = r#"> Quote with code:
>
> ```rust
> fn main() {
>     println!("test");
> }
> ```
"#;

    let output1 = format(input, None, None);
    let output2 = format(&output1, None, None);

    assert_eq!(output1, output2, "Formatting should be idempotent");
}

#[test]
fn quote_display_math_attribute_idempotency() {
    let input = r#"> Let $x$ be modeled
> by a Poisson distribution $$
> p(x) = \frac{e^{-\lambda} \lambda^{x}}{x !}
> $$ {#eq-poisson}
> where $\lambda$ is the rate. Using @eq-poisson, the probability can be
> calculated.
"#;

    let output1 = format(input, None, None);
    let output2 = format(&output1, None, None);
    assert_eq!(output1, output2, "Formatting should be idempotent");
}

#[test]
fn quote_tex_command_lines_idempotency() {
    let input = r#"> ... the problem with object-oriented languages is they’ve got all this
> implicit environment that they carry around with them. You wanted a banana
> but what you got was a gorilla holding the banana and the entire jungle.
>
> \medskip
> \hfill---Joe Armstrong
"#;

    let output1 = format(input, None, None);
    let output2 = format(&output1, None, None);
    assert_eq!(output1, output2, "Formatting should be idempotent");
}

#[test]
fn display_math_followup_paragraph_idempotency() {
    let input = r#"::: {.exercise #NW}
Show that the kernel density estimator

\[
  \hat{f}_1(x) = \frac{1}{N h} \sum_{i=1}^N K\left(\frac{x - x_i}{h}\right)
\]
  is also the marginal distribution of $x$ under $\hat{f}$, and that the 
Nadaraya-Watson kernel smoother is the conditional expectation of $y$    given
$x$ under $\hat{f}$.
:::
"#;

    let flavor = Flavor::RMarkdown;
    let config = Config {
        flavor,
        extensions: Extensions::for_flavor(flavor),
        ..Default::default()
    };

    let output1 = format(input, Some(config.clone()), None);
    let output2 = format(&output1, Some(config), None);
    assert_eq!(output1, output2, "Formatting should be idempotent");
}

#[test]
fn blockquote_codespan_continuation_line_stays_idempotent() {
    let input = "> If list `x` is a train carrying objects, then `x[[5]]` is the object in car 5;\n> `x[4:6]` is a train of cars 4-6.\n>\n> --- \\@RLangTip, <https://twitter.com/RLangTip/status/268375867468681216>\n";
    let output1 = format(input, None, None);
    let output2 = format(&output1, None, None);
    assert_eq!(output1, output2, "Formatting should be idempotent");
}

#[test]
fn blockquote_autolink_line_stays_idempotent() {
    let input = "> Information about previous years' prize winners can be found at:\n> <https://statistikframjandet.se/cramersallskapet/cramerpriset/>\n";
    let output1 = format(input, None, None);
    let output2 = format(&output1, None, None);
    assert_eq!(output1, output2, "Formatting should be idempotent");
}

#[test]
fn blockquote_bracketed_span_continuation_stays_idempotent() {
    let input = "> Roses are [red and **bold**]{color=\"red\"} and\n> violets are [blue]{color=\"blue\"}.\n";
    let output1 = format(input, None, None);
    let output2 = format(&output1, None, None);
    assert_eq!(output1, output2, "Formatting should be idempotent");
}
