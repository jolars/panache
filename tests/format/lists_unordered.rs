use panache::format;

#[test]
fn list_item_link_no_break() {
    let cfg = panache::ConfigBuilder::default().line_width(30).build();
    let input = "- A list item with a link ![some link that is very long](./example.com/very/long/path/to/file) in it\n";
    let output = format(input, Some(cfg), None);

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
    let output = format(input, None, None);

    // Should have two distinct list items
    let lines: Vec<&str> = output.lines().collect();
    assert!(lines[0].starts_with("- In R"));
    assert!(lines[1].starts_with("  is created"));
    assert!(lines[2].starts_with("- For instance"));

    // Should not merge the list items
    assert!(!output.contains("is created. - For instance"));
}

#[test]
fn nested_ordered_list_after_upper_alpha_is_idempotent() {
    let input = "A.  Upper Alpha\n    I.  Upper Roman.\n        (6) Decimal start with 6\n            c)  Lower alpha with paren\n";
    let output1 = format(input, None, None);
    let output2 = format(&output1, None, None);
    assert_eq!(output1, output2, "Formatting should be idempotent");
}

#[test]
fn list_item_with_fenced_div_is_idempotent() {
    let input = "- Item intro\n\n  ::: {layout-ncol=2}\n  content\n  :::\n";
    let output1 = format(input, None, None);
    let output2 = format(&output1, None, None);
    assert_eq!(output1, output2, "Formatting should be idempotent");
}

#[test]
fn list_item_with_fenced_div_containing_code_block_is_idempotent() {
    let input = "- Item intro\n\n  ::: {layout-ncol=2}\n  ```{.markdown}\n  text\n  ```\n  :::\n";
    let output1 = format(input, None, None);
    let output2 = format(&output1, None, None);
    assert_eq!(output1, output2, "Formatting should be idempotent");
}

#[test]
fn list_item_details_with_indented_markdown_fence_content_is_idempotent() {
    let input = "- Item\n\t<details>\n\t<summary>\n\t\tExample\n\t</summary>\n\n\t[Playground](https://example.test)\n\n\t```ts\n\tconst x = 1;\n\t```\n\t</details>\n";
    let output1 = format(input, None, None);
    let output2 = format(&output1, None, None);
    assert_eq!(output1, output2, "Formatting should be idempotent");
}

#[test]
fn nested_list_summary_continuation_indent_is_idempotent() {
    let input = "- Parent\n  - Child <details> <summary>\n    Example </summary>\n\n\t[Playground](https://example.test)\n";
    let output1 = format(input, None, None);
    let output2 = format(&output1, None, None);
    assert_eq!(output1, output2, "Formatting should be idempotent");
}

#[test]
fn nested_tight_list_with_followup_paragraph_is_idempotent() {
    let input = "- Parent\n\n  - A\n  - B\n\n    Continuation paragraph.\n";
    let output1 = format(input, None, None);
    let output2 = format(&output1, None, None);
    assert_eq!(output1, output2, "Formatting should be idempotent");
}

#[test]
fn parenthesized_marker_in_list_continuation_stays_paragraph_text() {
    let input = "- Parent item\n\n  - First paragraph line that introduces context and\n    (b) continues as regular text, not a nested list marker.\n";
    let output1 = format(input, None, None);
    let output2 = format(&output1, None, None);
    assert_eq!(output1, output2, "Formatting should be idempotent");
}

#[test]
fn list_item_with_hard_breaks_stays_idempotent() {
    let input = "- `some(.x, .p)` returns `TRUE` if *any* element matches;  \n  `every(.x, .p)` returns `TRUE` if *all* elements match;  \n  `none(.x, .p)` returns `TRUE` if *no* element matches.\n";
    let output1 = format(input, None, None);
    let output2 = format(&output1, None, None);
    assert_eq!(output1, output2, "Formatting should be idempotent");
}
