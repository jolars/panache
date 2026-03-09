use panache::Config;
use panache::format;

#[test]
fn preserves_inline_code_whitespace() {
    let input = "This is `foo   bar` inline code.";
    let output = format(input, None, None);
    similar_asserts::assert_eq!(output, "This is `foo   bar` inline code.\n");
}

#[test]
fn preserves_inline_math_whitespace() {
    let input = "Math: $x   +   y$";
    let output = format(input, None, None);
    similar_asserts::assert_eq!(output, "Math: $x   +   y$\n");
}

#[test]
fn text_reference_paragraph_is_not_wrapped() {
    let input = "(ref:foo) A scatterplot of the data `cars` using **base** R graphics.\n";
    let mut config = Config::default();
    config.extensions.bookdown_references = true;
    config.line_width = 40;
    let output = format(input, Some(config), None);
    similar_asserts::assert_eq!(output, input);
}

#[test]
fn abbreviation_year_stays_on_same_line_when_wrapping() {
    let input = "M.A. 2007\n";
    let config = Config {
        line_width: 6,
        ..Default::default()
    };
    let output = format(input, Some(config), None);
    similar_asserts::assert_eq!(output, "M.A. 2007\n");
}
