use panache::format;

#[test]
fn list_item_link_no_break() {
    let cfg = panache::ConfigBuilder::default().line_width(30).build();
    let input = "- A list item with a link ![some link that is very long](./example.com/very/long/path/to/file) in it\n";
    let output = format(input, Some(cfg));

    // The link should not be broken at the ]( boundary
    assert!(
        !output.contains("]\n("),
        "Link text and URL should not be separated in list items"
    );

    // The link should still be functional
    assert!(output.contains("./example.com/very/long/path/to/file"));
}

#[test]
fn list_items_separate_properly() {
    let input = "- In R, objects are passed by reference, but when an object is modified a copy\n  is created.\n- For instance, when subsetting a matrix, a copy is created. It's not possible\n  to access for instance a column by reference.\n";
    let output = format(input, None);

    // Should have two distinct list items
    let lines: Vec<&str> = output.lines().collect();
    assert!(lines[0].starts_with("- In R"));
    assert!(lines[1].starts_with("  is created"));
    assert!(lines[2].starts_with("- For instance"));

    // Should not merge the list items
    assert!(!output.contains("is created. - For instance"));
}
