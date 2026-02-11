use panache::format;

#[test]
fn simple_subscript() {
    let input = "H~2~O\n";
    let output = format(input, None);
    assert!(output.contains("H~2~O"));
}

#[test]
fn subscript_variables() {
    let input = "x~i~ and y~j~\n";
    let output = format(input, None);
    assert!(output.contains("x~i~"));
    assert!(output.contains("y~j~"));
}

#[test]
fn subscript_with_nested_elements() {
    let input = "a~**i**~\n";
    let output = format(input, None);
    assert!(output.contains("a~**i**~"));
}

#[test]
fn subscript_math() {
    let input = "x~i+1~ notation\n";
    let output = format(input, None);
    assert!(output.contains("x~i+1~"));
}

#[test]
fn multiple_subscripts() {
    let input = "a~1~ + a~2~ + a~3~\n";
    let output = format(input, None);
    assert!(output.contains("a~1~"));
    assert!(output.contains("a~2~"));
    assert!(output.contains("a~3~"));
}

#[test]
fn subscript_not_confused_with_strikeout() {
    let input = "H~2~O and ~~strikethrough~~ text.\n";
    let output = format(input, None);
    assert!(output.contains("H~2~O"));
    assert!(output.contains("~~strikethrough~~"));
}

#[test]
fn subscript_and_superscript_together() {
    let input = "x~i~^2^ notation\n";
    let output = format(input, None);
    assert!(output.contains("x~i~^2^"));
}

#[test]
fn subscript_with_multiple_words() {
    let input = "Something~some text~ here.\n";
    let output = format(input, None);
    assert!(output.contains("~some text~"));
}

#[test]
fn subscript_in_paragraph() {
    let cfg = panache::ConfigBuilder::default().line_width(40).build();
    let input = "This is a long paragraph with H~2~O subscript that should wrap at the configured line width.";
    let output = format(input, Some(cfg));

    // Should preserve subscript even with wrapping
    assert!(output.contains("H~2~O"));
}

#[test]
fn all_formatting_together() {
    let input = "Text with *emphasis*, **bold**, ~~strikeout~~, ^superscript^, and ~subscript~.\n";
    let output = format(input, None);
    assert!(output.contains("*emphasis*"));
    assert!(output.contains("**bold**"));
    assert!(output.contains("~~strikeout~~"));
    assert!(output.contains("^superscript^"));
    assert!(output.contains("~subscript~"));
}
