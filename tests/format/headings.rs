use panache::format;

#[tokio::test]
async fn atx_trailing_hashes_are_removed() {
    let input = "### A level-three heading ###\n";
    let expected = "### A level-three heading\n";
    let out = format(input, None).await;
    assert_eq!(out, expected);

    // idempotent
    assert_eq!(format(&out, None).await, expected);
}

#[tokio::test]
async fn atx_leading_spaces_are_normalized() {
    let input = "   ##   Title   \n";
    let expected = "## Title\n";
    let out = format(input, None).await;
    assert_eq!(out, expected);
    assert_eq!(format(&out, None).await, expected);
}
