use panache::format;

#[test]
fn quote_single_line() {
    let input = "> This is a single line quote.\n";
    let output = format(input, None);

    assert!(output.starts_with("> "));
    assert!(output.contains("single line quote"));
}

#[test]
fn quote_multi_line_continuous() {
    let input = "> This is a multi-line quote\n> that continues on the next line.\n";
    let output = format(input, None);

    for line in output.lines() {
        assert!(
            line.starts_with("> "),
            "Line should start with '>': '{line}'"
        );
    }
    assert!(output.contains("multi-line quote"));
    assert!(output.contains("continues on the next line"));
}
