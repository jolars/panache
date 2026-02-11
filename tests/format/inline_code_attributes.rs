use panache::format;

#[test]
fn inline_code_with_class() {
    let input = "Text with `code`{.python} inline.\n";
    let output = format(input, None);
    assert!(output.contains("`code`{.python}"));
}

#[test]
fn inline_code_with_id() {
    let input = "See `result`{#main-result} for details.\n";
    let output = format(input, None);
    assert!(output.contains("`result`{#main-result}"));
}

#[test]
fn inline_code_with_multiple_classes() {
    let input = "Run `script`{.python .eval .hide}.\n";
    let output = format(input, None);
    assert!(output.contains("`script`{.python .eval .hide}"));
}

#[test]
fn inline_code_with_full_attributes() {
    let input = "Use `func()`{#fn .haskell key=val}.\n";
    let output = format(input, None);
    assert!(output.contains("`func()`{#fn .haskell key=val}"));
}

#[test]
fn inline_code_without_attributes() {
    let input = "Plain `code` works.\n";
    let output = format(input, None);
    assert!(output.contains("`code`"));
    assert!(!output.contains("{"));
}

#[test]
fn multiple_inline_code_with_attributes() {
    let input = "Call `foo()`{.py} then `bar()`{.r}.\n";
    let output = format(input, None);
    assert!(output.contains("`foo()`{.py}"));
    assert!(output.contains("`bar()`{.r}"));
}

#[test]
fn double_backtick_with_attributes() {
    let input = "Code: ``has `backtick` inside``{.lang}.\n";
    let output = format(input, None);
    assert!(output.contains("``has `backtick` inside``{.lang}"));
}

#[test]
fn inline_code_attributes_in_wrapping_paragraph() {
    let cfg = panache::ConfigBuilder::default().line_width(40).build();
    let input =
        "This is a long paragraph with `code`{.python} that should wrap at the configured width.\n";
    let output = format(input, Some(cfg));
    // Should preserve inline code with attributes even when wrapping
    assert!(output.contains("`code`{.python}"));
}

#[test]
fn inline_code_space_before_attributes() {
    // Space between backticks and { should not parse as attributes
    let input = "Code: `test` {.python}.\n";
    let output = format(input, None);
    assert!(output.contains("`test`"));
    // The {.python} should be treated as regular text
    assert!(output.contains("{.python}"));
}

#[test]
fn inline_code_mixed_with_emphasis() {
    let input = "This *emphasized* and `code`{.lang} text.\n";
    let output = format(input, None);
    assert!(output.contains("*emphasized*"));
    assert!(output.contains("`code`{.lang}"));
}

#[test]
fn idempotent_formatting() {
    let input = "Text with `code`{#id .class key=value} inline.\n";
    let first = format(input, None);
    let second = format(&first, None);
    assert_eq!(first, second);
}

#[test]
fn inline_code_attributes_quoted_values() {
    let input = "Use `func()`{key=\"value with spaces\"}.\n";
    let output = format(input, None);
    assert!(output.contains("`func()`{key=\"value with spaces\"}"));
}
