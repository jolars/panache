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

#[test]
fn test_display_math_blank_line_terminates() {
    // Per Pandoc spec: "there can be no blank lines between the opening and closing $$ delimiters"
    let input = "$$\n\nmath\n$$";
    let output = parse(input);
    // Math block should terminate at the blank line
    assert!(output.contains("MathBlock"));
    assert!(output.contains("BlankLine"));
    assert!(output.contains("PARAGRAPH"));
    // The content after blank line should be in a paragraph, not in math block
    assert!(output.contains("TEXT") && output.contains("math"));
}

#[test]
fn test_backslash_bracket_blank_line_terminates() {
    // Same blank line restriction applies to \[...\]
    let input = "\\[\n\ne = mc^2\n\\]";
    let config = Config {
        extensions: crate::config::Extensions {
            tex_math_single_backslash: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let tree = BlockParser::new(input, &config).parse().0;
    let output = format!("{:#?}", tree);

    // Math block should terminate at the blank line
    assert!(output.contains("MathBlock"));
    assert!(output.contains("BlankLine"));
    assert!(output.contains("PARAGRAPH"));
    // The content after blank line should be in paragraph
    assert!(output.contains("e = mc"));
}

#[test]
fn test_display_math_content_on_same_line() {
    // Per Pandoc spec, content can be on the same line as opening delimiter
    let input = "$$ e = mc^2\n$$";
    let output = parse(input);
    assert!(output.contains("MathBlock"));
    assert!(output.contains("e = mc"));
}

#[test]
fn test_backslash_bracket_content_on_same_line() {
    // Content can be on the same line as \[
    let input = "\\[ e = mc^2 \n\\]";
    let config = Config {
        extensions: crate::config::Extensions {
            tex_math_single_backslash: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let tree = BlockParser::new(input, &config).parse().0;
    let output = format!("{:#?}", tree);

    assert!(output.contains("MathBlock"));
    assert!(output.contains("e = mc"));
    assert!(output.contains("BlockMathMarker"));
}
