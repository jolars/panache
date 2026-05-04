use panache_formatter::Config;
use panache_formatter::format;

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
    config.parser_extensions.bookdown_references = true;
    config.formatter_extensions.bookdown_references = true;
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

#[test]
fn standalone_presentation_pause_is_preserved() {
    let input = "Before\n\n. . .\n\nAfter\n";
    let output = format(input, None, None);
    similar_asserts::assert_eq!(output, input);
}

#[test]
fn standalone_presentation_pause_stays_idempotent_with_wrapping() {
    let input = "Before\n\n. . .\n\nAfter\n";
    let config = Config {
        line_width: 8,
        ..Default::default()
    };
    let first = format(input, Some(config.clone()), None);
    let second = format(&first, Some(config), None);
    similar_asserts::assert_eq!(first, second);
    assert!(first.contains("\n\n. . .\n\n"));
}
