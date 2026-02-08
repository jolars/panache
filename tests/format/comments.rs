use panache::format;
use panache::format_with_defaults;

#[tokio::test]
async fn comment_roundtrip() {
    let input = "<!-- This is a comment -->\n";
    let output = format_with_defaults(input).await;
    assert_eq!(output, input);
}

#[tokio::test]
async fn comment_within_content() {
    let cfg = panache::ConfigBuilder::default().line_width(160).build();
    let input =
        "Some text before the comment.\n<!-- This is a comment -->\nSome text after the comment.\n";
    let output = format(input, Some(cfg)).await;
    assert!(output.contains("Some text before the comment."));
    assert!(output.contains("<!-- This is a comment -->"));
    assert!(output.contains("Some text after the comment."));
}

#[tokio::test]
async fn comment_no_wrap() {
    let cfg = panache::ConfigBuilder::default().line_width(40).build();
    let input = "Some text before the comment.\n<!-- This is a very long comment that should not be wrapped or reformatted -->\nSome text after the comment.\n";
    let output = format(input, Some(cfg)).await;
    assert!(output.contains(
        "<!-- This is a very long comment that should not be wrapped or reformatted -->"
    ));
}
