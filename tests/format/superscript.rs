use panache::format;

#[test]
fn simple_superscript() {
    let input = "2^nd^ place.\n";
    let output = format(input, None);
    assert!(output.contains("2^nd^"));
}

#[test]
fn superscript_numbers() {
    let input = "x^2^ + y^3^\n";
    let output = format(input, None);
    assert!(output.contains("x^2^"));
    assert!(output.contains("y^3^"));
}

#[test]
fn superscript_with_nested_elements() {
    let input = "E = mc^**2**^\n";
    let output = format(input, None);
    assert!(output.contains("E = mc^**2**^"));
}

#[test]
fn superscript_trademark() {
    let input = "Product^(tm)^ name.\n";
    let output = format(input, None);
    assert!(output.contains("Product^(tm)^"));
}

#[test]
fn multiple_superscripts() {
    let input = "a^2^ + b^2^ = c^2^\n";
    let output = format(input, None);
    assert!(output.contains("a^2^"));
    assert!(output.contains("b^2^"));
    assert!(output.contains("c^2^"));
}

#[test]
fn superscript_not_confused_with_footnote() {
    let input = "Text^[footnote] and x^2^ here.\n";
    let output = format(input, None);
    assert!(output.contains("Text^[footnote]"));
    assert!(output.contains("x^2^"));
}

#[test]
fn superscript_with_multiple_words() {
    let input = "Something^some text^ here.\n";
    let output = format(input, None);
    assert!(output.contains("^some text^"));
}

#[test]
fn superscript_in_paragraph() {
    let cfg = panache::ConfigBuilder::default().line_width(40).build();
    let input = "This is a long paragraph with x^2^ superscript that should wrap at the configured line width.";
    let output = format(input, Some(cfg));

    // Should preserve superscript even with wrapping
    assert!(output.contains("x^2^"));
}
