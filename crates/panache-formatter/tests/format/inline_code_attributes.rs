use panache_formatter::format;

#[test]
fn inline_code_with_class() {
    let input = "Text with `code`{.python} inline.\n";
    let output = format(input, None, None);
    assert!(output.contains("`code`{.python}"));
}

#[test]
fn inline_code_with_id() {
    let input = "See `result`{#main-result} for details.\n";
    let output = format(input, None, None);
    assert!(output.contains("`result`{#main-result}"));
}

#[test]
fn inline_code_with_multiple_classes() {
    let input = "Run `script`{.python .eval .hide}.\n";
    let output = format(input, None, None);
    assert!(output.contains("`script`{.python .eval .hide}"));
}

#[test]
fn inline_code_with_full_attributes() {
    let input = "Use `func()`{#fn .haskell key=val}.\n";
    let output = format(input, None, None);
    assert!(output.contains("`func()`{#fn .haskell key=\"val\"}"));
}

#[test]
fn inline_code_without_attributes() {
    let input = "Plain `code` works.\n";
    let output = format(input, None, None);
    assert!(output.contains("`code`"));
    assert!(!output.contains("{"));
}

#[test]
fn multiple_inline_code_with_attributes() {
    let input = "Call `foo()`{.py} then `bar()`{.r}.\n";
    let output = format(input, None, None);
    assert!(output.contains("`foo()`{.py}"));
    assert!(output.contains("`bar()`{.r}"));
}

#[test]
fn double_backtick_with_attributes() {
    let input = "Code: ``has `backtick` inside``{.lang}.\n";
    let output = format(input, None, None);
    assert!(output.contains("``has `backtick` inside``{.lang}"));
}

#[test]
fn inline_code_attributes_in_wrapping_paragraph() {
    let cfg = panache_formatter::ConfigBuilder::default()
        .line_width(40)
        .build();
    let input =
        "This is a long paragraph with `code`{.python} that should wrap at the configured width.\n";
    let output = format(input, Some(cfg), None);
    // Should preserve inline code with attributes even when wrapping
    assert!(output.contains("`code`{.python}"));
}

#[test]
fn inline_code_space_before_attributes() {
    // Space between backticks and { should not parse as attributes
    let input = "Code: `test` {.python}.\n";
    let output = format(input, None, None);
    assert!(output.contains("`test`"));
    // The {.python} should be treated as regular text
    assert!(output.contains("{.python}"));
}

#[test]
fn inline_code_mixed_with_emphasis() {
    let input = "This *emphasized* and `code`{.lang} text.\n";
    let output = format(input, None, None);
    assert!(output.contains("*emphasized*"));
    assert!(output.contains("`code`{.lang}"));
}

#[test]
fn idempotent_formatting() {
    let input = "Text with `code`{#id .class key=value} inline.\n";
    let first = format(input, None, None);
    let second = format(&first, None, None);
    assert_eq!(first, second);
}

#[test]
fn inline_code_attributes_quoted_values() {
    let input = "Use `func()`{key=\"value with spaces\"}.\n";
    let output = format(input, None, None);
    assert!(output.contains("`func()`{key=\"value with spaces\"}"));
}

#[test]
fn multiline_triple_backtick_codespan_stays_idempotent() {
    let input = "Add thin space between single and double quotes.\n```\n% pandoc -t latex+smart\n---\nlang: en-GB\n---\n'[\"On the Outside\"]{}: Constructing Cycling Citizenship.'\n^D\n`\\,{``On the Outside''}: Constructing Cycling Citizenship.'\n```\n";
    let first = format(input, None, None);
    let second = format(&first, None, None);
    assert_eq!(first, second);
}

#[test]
fn backticks_inside_codespan_stays_idempotent() {
    let input = "`` hi````there ``\n";
    let first = format(input, None, None);
    let second = format(&first, None, None);
    assert_eq!(first, second);
}

#[test]
fn whitespace_only_codespan_stays_idempotent() {
    let input = "Hard line breaks, which are created either by ending a line with two or more spaces (`  `) or by using a backslash (`\\\\n`), are preserved regardless of the wrapping mode:\n";
    let first = format(input, None, None);
    let second = format(&first, None, None);
    assert_eq!(first, second);
    assert!(first.contains("spaces (``  ``)"));
}

#[test]
fn nested_backtick_rmarkdown_paragraph_stays_idempotent() {
    let cfg = panache_formatter::ConfigBuilder::default()
        .line_width(72)
        .build();
    let input = "There are two types of R code in R Markdown/**knitr** documents: R code chunks\\index{code chunk}, and inline R code\\index{inline R code}. The syntax for the latter is `` ``r ''`r R_CODE` ``, and it can be embedded inline with other document elements. R code chunks look like plain code blocks, but have `{r}` after the three backticks and (optionally) chunk options inside `{}`, e.g.,\n";
    let first = format(input, Some(cfg.clone()), None);
    let second = format(&first, Some(cfg), None);
    assert_eq!(first, second);
}
