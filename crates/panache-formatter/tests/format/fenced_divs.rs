use panache_formatter::format;

#[test]
fn fenced_div_strips_leading_blank_line_in_body() {
    let input = "\
::: declare

A

:::
";
    let expected = "\
::: declare
A

:::
";
    let output = format(input, None, None);
    assert_eq!(output, expected);
}

#[test]
fn fenced_div_in_list_strips_leading_blank_line_in_body() {
    let input = "\
- item

  ::: {layout-ncol=\"2\"}

  para

  :::
";
    let expected = "\
- item

  ::: {layout-ncol=\"2\"}
  para

  :::
";
    let output = format(input, None, None);
    assert_eq!(output, expected);
}

#[test]
fn fenced_div_before_following_paragraph_keeps_single_separator() {
    let input = "\
::: declare

A
:::
B
";
    let expected = "\
::: declare
A
:::

B
";
    let output = format(input, None, None);
    assert_eq!(output, expected);
}

#[test]
fn paragraph_with_fence_like_lines_stays_multiline_for_idempotency() {
    let input = "\
::: 
A
:::
";
    let expected = "\
::: 
A
:::
";
    let output = format(input, None, None);
    assert_eq!(output, expected);
}
