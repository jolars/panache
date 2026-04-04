use panache::format;

#[test]
fn multiline_inline_footnote_is_normalized_inline() {
    let input = "A^[\nB]\n";
    let output = format(input, None, None);
    similar_asserts::assert_eq!(output, "A^[B]\n");
}
