use panache_formatter::format;

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

#[test]
fn horizontal_rule_before_setext_like_paragraph_stays_idempotent() {
    let input = "---\nSIL OPEN FONT LICENSE Version 1.1 - 26 February 2007\n-----------------------------------------------------------\n";
    let first = format(input, None, None);
    let second = format(&first, None, None);
    assert_eq!(first, second);
}

#[test]
fn horizontal_rule_expands_to_line_width() {
    let cfg = panache_formatter::ConfigBuilder::default()
        .line_width(12)
        .build();
    let input = "***\n";
    let expected = "------------\n";
    let out = format(input, Some(cfg), None);
    assert_eq!(out, expected);
}

#[test]
fn blockquote_horizontal_rule_respects_available_width() {
    let cfg = panache_formatter::ConfigBuilder::default()
        .line_width(12)
        .build();
    let input = "> ***\n";
    let expected = "> ----------\n";
    let out = format(input, Some(cfg), None);
    assert_eq!(out, expected);
}
