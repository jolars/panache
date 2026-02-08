use panache::format;

#[tokio::test]
async fn heading_with_simple_id() {
    let input = "# Heading {#my-id}\n";
    let output = format(input, None).await;
    assert!(output.contains("# Heading {#my-id}"));
}

#[tokio::test]
async fn heading_with_single_class() {
    let input = "# Heading {.myclass}\n";
    let output = format(input, None).await;
    assert!(output.contains("# Heading {.myclass}"));
}

#[tokio::test]
async fn heading_with_multiple_classes() {
    let input = "## Section {.class1 .class2 .class3}\n";
    let output = format(input, None).await;
    assert!(output.contains("## Section {.class1 .class2 .class3}"));
}

#[tokio::test]
async fn heading_with_key_value() {
    let input = "# Title {key=value}\n";
    let output = format(input, None).await;
    assert!(output.contains("# Title {key=value}"));
}

#[tokio::test]
async fn heading_with_quoted_value() {
    let input = "# Title {key=\"value with spaces\"}\n";
    let output = format(input, None).await;
    assert!(output.contains("# Title {key=\"value with spaces\"}"));
}

#[tokio::test]
async fn heading_with_full_attributes() {
    let input = "# Heading {#id .class1 .class2 key1=val1 key2=\"val 2\"}\n";
    let output = format(input, None).await;
    assert!(output.contains("# Heading {#id .class1 .class2 key1=val1 key2=\"val 2\"}"));
}

#[tokio::test]
async fn multiple_headings_with_attributes() {
    let input = "# First {#first}\n\n## Second {#second .important}\n\n### Third {.section}\n";
    let output = format(input, None).await;
    assert!(output.contains("# First {#first}"));
    assert!(output.contains("## Second {#second .important}"));
    assert!(output.contains("### Third {.section}"));
}

#[tokio::test]
async fn heading_without_attributes() {
    let input = "# Plain Heading\n";
    let output = format(input, None).await;
    assert!(output.contains("# Plain Heading"));
    assert!(!output.contains("{"));
}

#[tokio::test]
async fn heading_with_trailing_hashes_and_attributes() {
    let input = "# Heading ### {#id}\n";
    let output = format(input, None).await;
    assert!(output.contains("# Heading {#id}"));
}

#[tokio::test]
async fn level_2_heading_with_attributes() {
    let input = "## Section Title {#sec-intro .unnumbered}\n";
    let output = format(input, None).await;
    assert!(output.contains("## Section Title {#sec-intro .unnumbered}"));
}

#[tokio::test]
async fn heading_preserves_whitespace_in_attributes() {
    let input = "# Title {  #id   .class   key=val  }\n";
    let output = format(input, None).await;
    // Should normalize whitespace
    assert!(output.contains("# Title {#id .class key=val}"));
}

#[tokio::test]
async fn heading_with_mixed_content() {
    let input = "# Introduction to *Pandoc* {#intro}\n\nSome text.\n";
    let output = format(input, None).await;
    assert!(output.contains("# Introduction to *Pandoc* {#intro}"));
}

#[tokio::test]
async fn idempotent_formatting() {
    let input = "# Heading {#id .class key=value}\n";
    let first = format(input, None).await;
    let second = format(&first, None).await;
    assert_eq!(first, second);
}
