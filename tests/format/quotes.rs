use panache::format;

#[tokio::test]
async fn quote_single_line() {
    let input = "> This is a single line quote.\n";
    let output = format(input, None).await;

    assert!(output.starts_with("> "));
    assert!(output.contains("single line quote"));
}

#[tokio::test]
async fn quote_multi_line_continuous() {
    let input = "> This is a multi-line quote\n> that continues on the next line.\n";
    let output = format(input, None).await;

    for line in output.lines() {
        assert!(
            line.starts_with("> "),
            "Line should start with '>': '{line}'"
        );
    }
    assert!(output.contains("multi-line quote"));
    assert!(output.contains("continues on the next line"));
}
