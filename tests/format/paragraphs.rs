use panache::format;

#[test]
fn preserves_inline_code_whitespace() {
    let input = "This is `foo   bar` inline code.";
    let output = format(input, None);
    similar_asserts::assert_eq!(output, "This is `foo   bar` inline code.\n");
}

#[test]
fn preserves_inline_math_whitespace() {
    let input = "Math: $x   +   y$";
    let output = format(input, None);
    similar_asserts::assert_eq!(output, "Math: $x   +   y$\n");
}
