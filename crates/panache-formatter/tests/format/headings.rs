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
fn atx_heading_interrupting_paragraph_keeps_document_order() {
    let mut cfg = panache_formatter::Config::default();
    cfg.parser_extensions.blank_before_header = false;
    cfg.formatter_extensions.blank_before_header = false;

    let input = "Text\n## Title\nMore\n";
    let expected = "Text\n\n## Title\n\nMore\n";
    let out = format(input, Some(cfg.clone()), None);

    assert_eq!(out, expected);
    assert_eq!(
        format(&out, Some(cfg), None),
        expected,
        "must be idempotent"
    );
}

#[test]
fn horizontal_rule_before_setext_like_paragraph_stays_idempotent() {
    let input = "---\nSIL OPEN FONT LICENSE Version 1.1 - 26 February 2007\n-----------------------------------------------------------\n";
    let first = format(input, None, None);
    let second = format(&first, None, None);
    assert_eq!(first, second);
}

#[test]
fn list_nested_heading_normalizes_inline_code_like_top_level() {
    // Headings inside list items went through a separate formatter that dumped
    // raw `child.text()` instead of formatting inline nodes, so a code span's
    // padding (`  code  `) was reformatted at the top level but not in a list.
    let input = "- # `  code  `\n";
    let out = format(input, None, None);
    assert!(out.contains("`code`"), "code span not normalized: {out:?}");
    assert!(!out.contains("`  code  `"), "raw code span left: {out:?}");
    assert_eq!(format(&out, None, None), out, "must be idempotent");
}

#[test]
fn list_nested_heading_normalizes_attributes_like_top_level() {
    // The list-nested heading path also skipped attribute normalization.
    let input = "- # Title {#id .a key=val}\n";
    let out = format(input, None, None);
    assert!(
        out.contains("key=\"val\""),
        "attributes not normalized: {out:?}"
    );
    assert_eq!(format(&out, None, None), out, "must be idempotent");
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
