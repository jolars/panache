use panache::Config;
use panache::format;

#[test]
fn preserves_nbsp_in_paragraph_text() {
    let input = "A\nB\u{00A0}C\n";
    let output = format(input, None, None);
    similar_asserts::assert_eq!(output, "A B\u{00A0}C\n");
}

#[test]
fn preserves_nbsp_when_wrapping_enabled() {
    let input = "A\nB\u{00A0}C\n";
    let config = Config {
        line_width: 3,
        ..Default::default()
    };
    let output = format(input, Some(config), None);
    similar_asserts::assert_eq!(output, "A\nB\u{00A0}C\n");
}
