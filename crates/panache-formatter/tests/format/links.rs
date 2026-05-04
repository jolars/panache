use panache_formatter::format;

#[test]
fn markdown_link_no_break() {
    let cfg = panache_formatter::ConfigBuilder::default()
        .line_width(30)
        .build();
    let input = "after this line comes a link ![a link](https://alink.com)\n";
    let output = format(input, Some(cfg), None);

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
    let cfg = panache_formatter::ConfigBuilder::default()
        .line_width(25)
        .build();
    let input2 = "here is a regular [link text](https://example.com) in text\n";
    let output2 = format(input2, Some(cfg), None);

    // Regular links can be broken, but shouldn't break ](
    assert!(
        !output2.contains("]\n("),
        "Link text and URL should not be separated"
    );

    // The link should still be functional
    assert!(output2.contains("https://example.com"));
}

#[test]
fn link_destination_title_single_quotes_normalized() {
    let input = "A [link](https://example.com 'Title Here') in text.\n";
    let output = format(input, None, None);
    similar_asserts::assert_eq!(
        output,
        "A [link](https://example.com \"Title Here\") in text.\n"
    );
}

#[test]
fn image_destination_title_single_quotes_normalized() {
    let input = "An ![alt](https://example.com/img.png 'Image Title') in text.\n";
    let output = format(input, None, None);
    similar_asserts::assert_eq!(
        output,
        "An ![alt](https://example.com/img.png \"Image Title\") in text.\n"
    );
}

#[test]
fn unresolved_shortcut_reference_round_trips() {
    // Pandoc dialect: a `[label]` shortcut with no matching refdef is
    // emitted as `UNRESOLVED_REFERENCE`; the formatter must round-trip
    // it back to the original bracket bytes (idempotent).
    let input = "See [foo].\n";
    let output = format(input, None, None);
    similar_asserts::assert_eq!(output, input);
    let output2 = format(&output, None, None);
    assert_eq!(output, output2, "format must be idempotent");
}

#[test]
fn unresolved_full_reference_round_trips() {
    let input = "See [link text][missing].\n";
    let output = format(input, None, None);
    similar_asserts::assert_eq!(output, input);
    let output2 = format(&output, None, None);
    assert_eq!(output, output2, "format must be idempotent");
}

#[test]
fn unresolved_collapsed_reference_round_trips() {
    let input = "See [link text][].\n";
    let output = format(input, None, None);
    similar_asserts::assert_eq!(output, input);
    let output2 = format(&output, None, None);
    assert_eq!(output, output2, "format must be idempotent");
}

#[test]
fn unresolved_image_reference_round_trips() {
    let input = "See ![alt][missing].\n";
    let output = format(input, None, None);
    similar_asserts::assert_eq!(output, input);
    let output2 = format(&output, None, None);
    assert_eq!(output, output2, "format must be idempotent");
}
