use panache_formatter::config::WrapMode;
use panache_formatter::{Config, format};

#[test]
fn list_item_link_no_break() {
    let cfg = panache_formatter::ConfigBuilder::default()
        .line_width(30)
        .build();
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
fn parenthesized_citation_in_list_continuation_stays_idempotent() {
    let input = "- When should you take a dependency? What are the risk and rewards? In @sec-dependencies-pros-cons we provide a framework for deciding whether a dependency is worth it. This chapter also includes specific sections for deciding between `Imports` and `Suggests` (@sec-dependencies-imports-vs-suggests) and between `Imports` and `Depends` (@sec-dependencies-imports-vs-depends).\n";
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

#[test]
fn list_item_wraps_at_configured_line_width() {
    let cfg = panache_formatter::ConfigBuilder::default()
        .line_width(80)
        .build();
    let input =
        "- Foo and bar are (univariate) data variables, used when there is a need for foob\n";
    let expected =
        "- Foo and bar are (univariate) data variables, used when there is a need for\n  foob\n";
    let output = format(input, Some(cfg), None);
    assert_eq!(output, expected);
}

#[test]
fn outdented_item_after_nested_list_formats_at_outer_level() {
    let input = "* Item 1\n  + Nested item\n      *  Deeply nested\n +  Item 2\n";
    let expected = "- Item 1\n  - Nested item\n    - Deeply nested\n- Item 2\n";
    let output = format(input, None, None);
    assert_eq!(output, expected);
}

#[test]
fn empty_brackets_in_list_item_are_escaped() {
    let input = "- [] a\n";
    let expected = "- \\[\\] a\n";
    let output = format(input, None, None);
    assert_eq!(output, expected);
}

#[test]
fn escaped_double_underscore_in_list_item_stays_idempotent() {
    let input = "- b. WHERE firstName LIKE 'Ma_\\_';\n";
    let output1 = format(input, None, None);
    let output2 = format(&output1, None, None);
    assert_eq!(output1, output2, "Formatting should be idempotent");
}

#[test]
fn list_item_keeps_initialism_year_together_when_wrapping() {
    let cfg = Config {
        line_width: 8,
        ..Default::default()
    };
    let input = "- M.A. 2007\n";
    let expected = "- M.A. 2007\n";
    let output = format(input, Some(cfg), None);
    assert_eq!(output, expected);
}

#[test]
fn sentence_wrap_keeps_task_list_hanging_indent() {
    let cfg = Config {
        wrap: Some(WrapMode::Sentence),
        line_width: 40,
        ..Default::default()
    };
    let input = "- [ ] First sentence. Second sentence! Third sentence?\n";
    let expected = "\
- [ ] First sentence.
      Second sentence!
      Third sentence?
";
    let output = format(input, Some(cfg), None);
    assert_eq!(output, expected);
}

#[test]
fn blockquote_list_reflow_does_not_emit_inline_quote_markers() {
    let cfg = Config {
        line_width: 60,
        ..Default::default()
    };
    let input = "> - This line is intentionally long so that wrapping happens inside the list item within a blockquote context.\n";
    let expected = "\
> - This line is intentionally long so that wrapping happens
>   inside the list item within a blockquote context.
";
    let output = format(input, Some(cfg), None);
    assert_eq!(output, expected);
}

#[test]
fn loose_list_paragraph_after_image_continuation_stays_idempotent() {
    let cfg = panache_formatter::ConfigBuilder::default()
        .line_width(80)
        .build();
    let input = "\
- If you commit your rendered book output to GitHub (e.g., the `_book`
  directory), then you can use the Static Document deployment

![Static Deployment Icon on the Connect Cloud publish page](images/static-deploy.png)

  When configuring this deployment, select the rendered index.html as the
  Primary file. Example:

![Screenshot showing selection of \"\\_book/index.html\" as Primary file for deploy on Connect Cloud](images/primary-doc.png)
";
    let output1 = format(input, Some(cfg.clone()), None);
    let output2 = format(&output1, Some(cfg), None);
    assert_eq!(output1, output2, "Formatting should be idempotent");
}

#[test]
fn loose_list_figure_and_followup_paragraph_keep_list_indentation() {
    let input = "\
- Item intro

    ![Figure alt](images/figure.png)

    Follow-up paragraph after figure.

- Next item
";
    let expected = "\
- Item intro

  ![Figure alt](images/figure.png)

  Follow-up paragraph after figure.

- Next item
";
    let output = format(input, None, None);
    assert_eq!(output, expected);
}

#[test]
fn list_item_with_inline_fenced_div_opener_text_stays_idempotent() {
    let input = "\
- Parent item

  ::: {.callout-note}
  Body text in callout.
  :::

- Next item
";
    let output1 = format(input, None, None);
    let output2 = format(&output1, None, None);
    assert_eq!(output1, output2, "Formatting should be idempotent");
}

#[test]
fn list_item_with_layout_div_and_nested_table_div_stays_idempotent() {
    let input = "\
- Parent list item introducing an example:

  ::: {layout-ncol=\"2\"}
  ```markdown
  ::: {#tbl-table}

  ![](table.png)

  An image treated like a table

  :::
  ```

  ::: {#tbl-table}

  ![](images/crossref-div-table.png)

  An image treated like a table

  :::

  :::
";
    let output1 = format(input, None, None);
    let output2 = format(&output1, None, None);
    assert_eq!(output1, output2, "Formatting should be idempotent");
}

#[test]
fn content_visible_div_with_ordered_item_does_not_gain_leading_blank_line() {
    let input = "\
::: {.content-visible when-meta=\"tool.is_jupyterlab\"}
3.  You'll be working inside this directory throughout the tutorial, so if you are ready to proceed, navigate inside the directory, and start Jupyter Lab:

    ``` {.bash filename=\"Terminal\"}
    cd manuscript-tutorial
    python3 -m jupyter lab
    ```
:::
";
    let output1 = format(input, None, None);
    let output2 = format(&output1, None, None);
    assert_eq!(output1, output2, "Formatting should be idempotent");
}
