use panache::format;

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
