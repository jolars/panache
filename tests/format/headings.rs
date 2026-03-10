use panache::format;

#[test]
fn atx_trailing_hashes_are_removed() {
    let input = "### A level-three heading ###\n";
    let expected = "### A level-three heading\n";
    let out = format(input, None, None);
    assert_eq!(out, expected);

    // idempotent
    assert_eq!(format(&out, None, None), expected);
}

#[test]
fn atx_leading_spaces_are_normalized() {
    let input = "   ##   Title   \n";
    let expected = "## Title\n";
    let out = format(input, None, None);
    assert_eq!(out, expected);
    assert_eq!(format(&out, None, None), expected);
}

#[test]
fn consecutive_atx_headings_without_blank_lines_stay_separate() {
    let input = "# unremarkable header 1\n## unremarkable header 2\n### unremarkable header 3\n### unremarkable header 3 ##\n";
    let out = format(input, None, None);
    assert_eq!(format(&out, None, None), out);
}
