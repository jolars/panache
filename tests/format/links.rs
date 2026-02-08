use panache::format;

#[tokio::test]
async fn markdown_link_no_break() {
    let cfg = panache::ConfigBuilder::default().line_width(30).build();
    let input = "after this line comes a link ![a link](https://alink.com)\n";
    let output = format(input, Some(cfg)).await;

    // The ![a link](https://alink.com) should stay together
    assert!(
        !output.contains("!\n["),
        "Image link should not be broken across lines"
    );

    assert!(
        !output.contains("]\n("),
        "Link text and URL should not be separated"
    );

    // Test regular links too - they can be broken, but not at critical points
    let cfg = panache::ConfigBuilder::default().line_width(25).build();
    let input2 = "here is a regular [link text](https://example.com) in text\n";
    let output2 = format(input2, Some(cfg)).await;

    // Regular links can be broken, but shouldn't break ](
    assert!(
        !output2.contains("]\n("),
        "Link text and URL should not be separated"
    );

    // The link should still be functional
    assert!(output2.contains("https://example.com"));
}
