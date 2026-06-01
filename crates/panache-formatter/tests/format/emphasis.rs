use panache_formatter::format;

// Emphasis that OPENS with a code span must keep the space between the code
// span and the following word. Regression: `**`reg_schema` treats**` lost the
// space and rendered as `**`reg_schema`treats**`.
#[test]
fn strong_opening_with_code_span_keeps_space() {
    let input = "See **`reg_schema` treats** the block.\n";
    let output = format(input, None, None);
    similar_asserts::assert_eq!(output, "See **`reg_schema` treats** the block.\n");
    similar_asserts::assert_eq!(format(&output, None, None), output);
}

// The same applies to single-underscore emphasis (normalized to `*`): the space
// after the leading code span must survive.
#[test]
fn emphasis_opening_with_code_span_keeps_space() {
    let input = "_`x` y_\n";
    let output = format(input, None, None);
    similar_asserts::assert_eq!(output, "*`x` y*\n");
    similar_asserts::assert_eq!(format(&output, None, None), output);
}

// Guard: only the code-span-FIRST shape was affected. A code span in the middle
// of an emphasis run is unchanged.
#[test]
fn strong_with_code_span_in_middle_is_unchanged() {
    let input = "**text `code` word**\n";
    let output = format(input, None, None);
    similar_asserts::assert_eq!(output, "**text `code` word**\n");
}
