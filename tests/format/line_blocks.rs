use panache::format;

#[tokio::test]
async fn test_simple_line_block() {
    let input = r#"| Line one
| Line two
| Line three"#;

    let expected = r#"
| Line one
| Line two
| Line three
"#;

    let result = format(input, None).await;
    assert_eq!(result.trim(), expected.trim());
}

#[tokio::test]
async fn test_line_block_with_indentation() {
    let input = r#"| The limerick packs laughs anatomical
| In space that is quite economical.
|    But the good ones I've seen
|    So seldom are clean
| And the clean ones so seldom are comical"#;

    let expected = r#"
| The limerick packs laughs anatomical
| In space that is quite economical.
|    But the good ones I've seen
|    So seldom are clean
| And the clean ones so seldom are comical
"#;

    let result = format(input, None).await;
    assert_eq!(result.trim(), expected.trim());
}

#[tokio::test]
async fn test_line_block_with_address() {
    let input = r#"| 200 Main St.
| Berkeley, CA 94718"#;

    let expected = r#"
| 200 Main St.
| Berkeley, CA 94718
"#;

    let result = format(input, None).await;
    assert_eq!(result.trim(), expected.trim());
}

#[tokio::test]
async fn test_line_block_preserves_empty_lines() {
    let input = r#"| First stanza
| Second line
|
| After blank line"#;

    let expected = r#"
| First stanza
| Second line
|
| After blank line
"#;

    let result = format(input, None).await;
    assert_eq!(result.trim(), expected.trim());
}

#[tokio::test]
async fn test_line_block_followed_by_paragraph() {
    let input = r#"| Line one
| Line two

This is a paragraph."#;

    let expected = r#"
| Line one
| Line two

This is a paragraph.
"#;

    let result = format(input, None).await;
    assert_eq!(result.trim(), expected.trim());
}

#[tokio::test]
async fn test_multiple_line_blocks() {
    let input = r#"| First block
| Line two

| Second block
| Another line"#;

    let expected = r#"
| First block
| Line two

| Second block
| Another line
"#;

    let result = format(input, None).await;
    assert_eq!(result.trim(), expected.trim());
}

#[tokio::test]
async fn test_line_block_idempotency() {
    let input = r#"| Line one
| Line two
| Line three"#;

    let result1 = format(input, None).await;
    let result2 = format(&result1, None).await;

    assert_eq!(result1, result2, "Formatting should be idempotent");
}
