use panache::format;

#[tokio::test]
async fn simple_strikeout() {
    let input = "This is ~~strikethrough~~ text.\n";
    let output = format(input, None).await;
    assert!(output.contains("~~strikethrough~~"));
}

#[tokio::test]
async fn strikeout_with_nested_emphasis() {
    let input = "This is ~~**bold strikethrough**~~ text.\n";
    let output = format(input, None).await;
    assert!(output.contains("~~**bold strikethrough**~~"));
}

#[tokio::test]
async fn strikeout_with_code() {
    let input = "This is ~~`code strikethrough`~~ text.\n";
    let output = format(input, None).await;
    assert!(output.contains("~~`code strikethrough`~~"));
}

#[tokio::test]
async fn multiple_strikeouts() {
    let input = "Text with ~~first~~ and ~~second~~ strikethrough.\n";
    let output = format(input, None).await;
    assert!(output.contains("~~first~~"));
    assert!(output.contains("~~second~~"));
}

#[tokio::test]
async fn strikeout_not_confused_with_subscript() {
    // Single ~ should not be parsed as anything yet
    let input = "This is ~not~ strikethrough.\n";
    let output = format(input, None).await;
    assert!(output.contains("~not~"));
    assert!(!output.contains("~~not~~"));
}

#[tokio::test]
async fn strikeout_preserves_spaces() {
    let input = "This is ~~multiple word strikethrough~~ text.\n";
    let output = format(input, None).await;
    assert!(output.contains("~~multiple word strikethrough~~"));
}

#[tokio::test]
async fn strikeout_in_paragraph_with_wrapping() {
    let cfg = panache::ConfigBuilder::default().line_width(40).build();
    let input = "This is a long paragraph with ~~strikethrough text~~ that should wrap at the configured line width.";
    let output = format(input, Some(cfg)).await;

    // Should preserve strikeout even with wrapping
    assert!(output.contains("~~strikethrough text~~"));
}
