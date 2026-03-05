use panache::ConfigBuilder;
use panache::format;
use panache::format_with_defaults;

#[test]
fn test_ignore_format_basic() {
    let input = r#"<!-- panache-ignore-format-start -->
This    has    weird     spacing
and  should   be  preserved
<!-- panache-ignore-format-end -->

This paragraph should be formatted normally with proper wrapping.
"#;

    let output = format_with_defaults(input);

    // The ignore region should be preserved exactly
    assert!(output.contains("This    has    weird     spacing"));
    assert!(output.contains("and  should   be  preserved"));

    // The normal paragraph should be formatted
    assert!(output.contains("This paragraph should be formatted normally with proper wrapping."));
}

#[test]
fn test_ignore_both() {
    let input = r#"<!-- panache-ignore-start -->
# Heading    with    weird   spacing
- List  item  with   spacing
<!-- panache-ignore-end -->

# Normal heading
"#;

    let output = format_with_defaults(input);

    // Content in ignore region should be preserved
    assert!(output.contains("# Heading    with    weird   spacing"));
    assert!(output.contains("- List  item  with   spacing"));

    // Normal content should be formatted
    assert!(output.contains("# Normal heading"));
}

#[test]
fn test_ignore_format_nested_in_list() {
    let input = r#"- Item 1
- Item 2
  <!-- panache-ignore-format-start -->
  Nested    content   with    spacing
  <!-- panache-ignore-format-end -->
- Item 3
"#;

    let output = format_with_defaults(input);

    assert!(output.contains("Nested    content   with    spacing"));
    assert!(output.contains("- Item 1"));
    assert!(output.contains("- Item 3"));
}

#[test]
fn test_ignore_format_multiple_regions() {
    let input = r#"Normal paragraph.

<!-- panache-ignore-format-start -->
First   ignored   region
<!-- panache-ignore-format-end -->

Another normal paragraph.

<!-- panache-ignore-format-start -->
Second   ignored   region
<!-- panache-ignore-format-end -->

Final paragraph.
"#;

    let output = format_with_defaults(input);

    assert!(output.contains("First   ignored   region"));
    assert!(output.contains("Second   ignored   region"));
    assert!(output.contains("Normal paragraph."));
    assert!(output.contains("Another normal paragraph."));
    assert!(output.contains("Final paragraph."));
}

#[test]
fn test_ignore_preserves_directives() {
    let input = r#"<!-- panache-ignore-format-start -->
Content
<!-- panache-ignore-format-end -->
"#;

    let output = format_with_defaults(input);

    // Directives themselves should be preserved
    assert!(output.contains("<!-- panache-ignore-format-start -->"));
    assert!(output.contains("<!-- panache-ignore-format-end -->"));
}

#[test]
fn test_ignore_with_long_lines() {
    let cfg = ConfigBuilder::default().line_width(40).build();

    let input = r#"<!-- panache-ignore-format-start -->
This is a very long line that would normally be wrapped but should not be wrapped because it is in an ignore region
<!-- panache-ignore-format-end -->

This is a very long line that should be wrapped because it is not in an ignore region and exceeds the line width
"#;

    let output = format(input, Some(cfg), None);

    // Long line in ignore region should NOT be wrapped
    assert!(output.contains("This is a very long line that would normally be wrapped but should not be wrapped because it is in an ignore region"));

    // Long line outside ignore region SHOULD be wrapped (split across multiple lines)
    // Check that no single line contains the entire long text
    let has_full_line = output
        .lines()
        .any(|l| l.contains("This is a very long line that should be wrapped because it is not in an ignore region and exceeds the line width"));

    assert!(
        !has_full_line,
        "Long line outside ignore region should be wrapped"
    );
}

#[test]
fn test_non_directive_comments_unaffected() {
    let input = r#"<!-- This is a regular comment -->

Normal paragraph.

<!-- Another regular comment -->
"#;

    let output = format_with_defaults(input);

    // Regular comments should pass through
    assert!(output.contains("<!-- This is a regular comment -->"));
    assert!(output.contains("<!-- Another regular comment -->"));
    assert!(output.contains("Normal paragraph."));
}

#[test]
fn test_ignore_lint_does_not_affect_formatting() {
    let input = r#"<!-- panache-ignore-lint-start -->
This    should    be     formatted
<!-- panache-ignore-lint-end -->
"#;

    let output = format_with_defaults(input);

    // ignore-lint should NOT prevent formatting
    assert!(!output.contains("This    should    be     formatted"));
    assert!(output.contains("This should be formatted"));
}

#[test]
fn test_ignore_region_with_code_block() {
    let input = r#"<!-- panache-ignore-format-start -->
```python
def   foo():
    return   42
```
<!-- panache-ignore-format-end -->
"#;

    let output = format_with_defaults(input);

    // Code block in ignore region should be preserved exactly
    assert!(output.contains("def   foo():"));
    assert!(output.contains("    return   42"));
}
