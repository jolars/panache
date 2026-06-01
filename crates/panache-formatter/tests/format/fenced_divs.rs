use panache_formatter::format;

#[test]
fn fenced_div_strips_leading_and_trailing_blank_lines_in_body() {
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
fn fenced_div_in_list_strips_leading_and_trailing_blank_lines_in_body() {
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

#[test]
fn paragraph_pulling_in_fence_opener_preserves_source_layout_340() {
    // Issue #340: missing blank line between `[]{#hmm}` and `::: {lang=...}`
    // pulls the would-be fence into the paragraph. Reflow used to collapse
    // everything into a single line, hiding the cause. Preserve source
    // linebreaks so the missing blank line stays visible to the user.
    let input = "\
[]{#hmm}
::: {lang=zh-TW}
bla
:::
";
    let output = format(input, None, None);
    assert_eq!(
        output, input,
        "swept fence shape should preserve source layout"
    );
    // Idempotency: a second pass returns the same output.
    assert_eq!(format(&output, None, None), output);
}

#[test]
fn paragraph_pulling_in_bare_fence_closer_preserves_source_layout() {
    // Same shape with a class-name opener.
    let input = "\
prelude text
::: warning
:::
";
    let output = format(input, None, None);
    assert_eq!(output, input);
}

#[test]
fn plain_paragraph_without_fence_shape_still_reflows() {
    // Sanity: the swept-fence guard must not block reflow of ordinary
    // paragraphs whose continuation lines happen to contain colons.
    let input = "\
A line that mentions colons:
just a continuation line here.
";
    let expected = "\
A line that mentions colons: just a continuation line here.
";
    let output = format(input, None, None);
    assert_eq!(output, expected);
}
