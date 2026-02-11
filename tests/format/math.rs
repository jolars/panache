use panache::ConfigBuilder;
use panache::format;

#[test]
#[ignore = "Display math blocks not yet implemented in block parser"]
fn math_no_wrap() {
    // TODO: Block parser needs to recognize $$ display math blocks
    // Currently treated as TEXT in PARAGRAPH, so backslash escapes are processed
    // Should be treated like CodeBlock - verbatim context where escapes don't work
    let cfg = ConfigBuilder::default().line_width(10).build();
    let input = "$$\n\\begin{matrix}\nA & B\\\\\nC & D\n\\end{matrix}\n$$\n";
    let output = format(input, Some(cfg));

    // Math blocks should not be wrapped
    similar_asserts::assert_eq!(output, input);
}
