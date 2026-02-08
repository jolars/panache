use panache::format;

#[tokio::test]
async fn inline_code_with_class() {
    let input = "Text with `code`{.python} inline.\n";
    let output = format(input, None).await;
    assert!(output.contains("`code`{.python}"));
}

#[tokio::test]
async fn inline_code_with_id() {
    let input = "See `result`{#main-result} for details.\n";
    let output = format(input, None).await;
    assert!(output.contains("`result`{#main-result}"));
}

#[tokio::test]
async fn inline_code_with_multiple_classes() {
    let input = "Run `script`{.python .eval .hide}.\n";
    let output = format(input, None).await;
    assert!(output.contains("`script`{.python .eval .hide}"));
}

#[tokio::test]
async fn inline_code_with_full_attributes() {
    let input = "Use `func()`{#fn .haskell key=val}.\n";
    let output = format(input, None).await;
    assert!(output.contains("`func()`{#fn .haskell key=val}"));
}

#[tokio::test]
async fn inline_code_without_attributes() {
    let input = "Plain `code` works.\n";
    let output = format(input, None).await;
    assert!(output.contains("`code`"));
    assert!(!output.contains("{"));
}

#[tokio::test]
async fn multiple_inline_code_with_attributes() {
    let input = "Call `foo()`{.py} then `bar()`{.r}.\n";
    let output = format(input, None).await;
    assert!(output.contains("`foo()`{.py}"));
    assert!(output.contains("`bar()`{.r}"));
}

#[tokio::test]
async fn double_backtick_with_attributes() {
    let input = "Code: ``has `backtick` inside``{.lang}.\n";
    let output = format(input, None).await;
    assert!(output.contains("``has `backtick` inside``{.lang}"));
}

#[tokio::test]
async fn inline_code_attributes_in_wrapping_paragraph() {
    let cfg = panache::ConfigBuilder::default().line_width(40).build();
    let input =
        "This is a long paragraph with `code`{.python} that should wrap at the configured width.\n";
    let output = format(input, Some(cfg)).await;
    // Should preserve inline code with attributes even when wrapping
    assert!(output.contains("`code`{.python}"));
}

#[tokio::test]
async fn inline_code_space_before_attributes() {
    // Space between backticks and { should not parse as attributes
    let input = "Code: `test` {.python}.\n";
    let output = format(input, None).await;
    assert!(output.contains("`test`"));
    // The {.python} should be treated as regular text
    assert!(output.contains("{.python}"));
}

#[tokio::test]
async fn inline_code_mixed_with_emphasis() {
    let input = "This *emphasized* and `code`{.lang} text.\n";
    let output = format(input, None).await;
    assert!(output.contains("*emphasized*"));
    assert!(output.contains("`code`{.lang}"));
}

#[tokio::test]
async fn idempotent_formatting() {
    let input = "Text with `code`{#id .class key=value} inline.\n";
    let first = format(input, None).await;
    let second = format(&first, None).await;
    assert_eq!(first, second);
}

#[tokio::test]
async fn inline_code_attributes_quoted_values() {
    let input = "Use `func()`{key=\"value with spaces\"}.\n";
    let output = format(input, None).await;
    assert!(output.contains("`func()`{key=\"value with spaces\"}"));
}
