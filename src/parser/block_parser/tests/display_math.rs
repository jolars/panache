use crate::config::Config;
use crate::parser::block_parser::BlockParser;

fn parse(input: &str) -> String {
    let config = Config::default();
    let tree = BlockParser::new(input, &config).parse().0;
    format!("{:#?}", tree)
}

#[test]
fn test_standalone_display_math() {
    let input = r#"$$
x = y + z
$$"#;
    let output = parse(input);
    assert!(output.contains("MathBlock"));
    assert!(output.contains("BlockMathMarker"));
    assert!(output.contains("x = y + z"));
}

#[test]
fn test_display_math_with_multiple_lines() {
    let input = r#"$$
\frac{a}{b}
\int_0^1 x dx
$$"#;
    let output = parse(input);
    assert!(output.contains("MathBlock"));
    assert!(output.contains("\\frac{a}{b}"));
    assert!(output.contains("\\int_0^1 x dx"));
}

#[test]
fn test_display_math_triple_dollars() {
    let input = r#"$$$
x = y
$$$"#;
    let output = parse(input);
    assert!(output.contains("MathBlock"));
    assert!(output.contains("x = y"));
}

#[test]
fn test_display_math_no_blank_line_required() {
    let input = r#"Some text
$$
math
$$"#;
    let output = parse(input);
    // Display math does NOT require blank line before it per Pandoc spec
    assert!(output.contains("MathBlock"));
}
