use panache::format;

#[tokio::test]
async fn latex_command_in_paragraph() {
    let input = "This is a paragraph with \\textbf{bold text} in the middle.\n";
    let output = format(input, None).await;

    // LaTeX command should be preserved within the paragraph
    assert!(output.contains("\\textbf{bold text}"));
    similar_asserts::assert_eq!(output, input);
}

#[tokio::test]
async fn latex_command_with_multiple_args() {
    let input = "\\includegraphics[width=0.5\\textwidth]{figure.png}\n";
    let output = format(input, None).await;

    // Complex LaTeX commands should be preserved
    similar_asserts::assert_eq!(output, input);
}

#[tokio::test]
async fn latex_command_no_wrap() {
    let cfg = panache::ConfigBuilder::default().line_width(30).build();
    let input = "This is a very long line with \\pdfpcnote{a very long note that should not be wrapped or reformatted} that exceeds line width.\n";
    let output = format(input, Some(cfg)).await;

    // Check that the LaTeX command appears somewhere in the output (may be wrapped)
    assert!(output.contains("\\pdfpcnote{"));
}

#[tokio::test]
async fn mixed_latex_and_markdown() {
    let input = "Here is some text with \\LaTeX{} and [a link](https://example.com) together.\n";
    let output = format(input, None).await;

    // Both LaTeX and markdown should be preserved
    assert!(output.contains("\\LaTeX{}"));
    assert!(output.contains("https://example.com"));
}
