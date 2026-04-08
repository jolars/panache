use panache::format;

#[test]
fn multiline_inline_footnote_is_normalized_inline() {
    let input = "A^[\nB]\n";
    let output = format(input, None, None);
    similar_asserts::assert_eq!(output, "A^[B]\n");
}

#[test]
fn inline_footnote_content_wraps_with_paragraph() {
    let input = "A footnote about the installer scripts below.^[These scripts download from this repository's latest GitHub release and install to a user-local directory by default. If you prefer, download and inspect the script before running it.]\n";
    let cfg = panache::ConfigBuilder::default().line_width(80).build();
    let output = format(input, Some(cfg), None);

    similar_asserts::assert_eq!(
        output,
        "A footnote about the installer scripts below.^[These scripts download from this\nrepository's latest GitHub release and install to a user-local directory by\ndefault. If you prefer, download and inspect the script before running it.]\n"
    );
}

#[test]
fn inline_footnote_preserves_inline_syntax_while_wrapping() {
    let input = "Text^[This has *emphasis* and a [link](https://example.com) plus `code span` and more words to force wrapping at width.]\n";
    let cfg = panache::ConfigBuilder::default().line_width(60).build();
    let output = format(input, Some(cfg), None);

    similar_asserts::assert_eq!(
        output,
        "Text^[This has *emphasis* and a [link](https://example.com)\nplus `code span` and more words to force wrapping at width.]\n"
    );
}

#[test]
fn inline_footnote_with_emphasis_whitespace_stays_stable() {
    let input = "Text^[   * emphasized*   tail words ]\n";
    let cfg = panache::ConfigBuilder::default().line_width(60).build();
    let output = format(input, Some(cfg), None);
    similar_asserts::assert_eq!(output, "Text^[\\* emphasized\\* tail words]\n");
}

#[test]
fn inline_footnote_in_list_item_wraps_without_losing_marker_or_indent() {
    let cfg = panache::ConfigBuilder::default().line_width(70).build();
    let input = "- Intro text before note.^[This inline footnote has enough words to force wrapping and should preserve inline syntax while aligning with list continuation indentation.]\n";
    let expected = "\
- Intro text before note.^[This inline footnote has enough words to
  force wrapping and should preserve inline syntax while aligning with
  list continuation indentation.]
";
    let output = format(input, Some(cfg), None);
    similar_asserts::assert_eq!(output, expected);
}

#[test]
fn inline_footnote_with_link_and_codespan_stays_idempotent() {
    let cfg = panache::ConfigBuilder::default().line_width(80).build();
    let input = "a^[For the [`bs4_book()`](#bs4-book)format, the `edit`, `history`, and `view` fields have no effect and similar configuration can be specified with the [repo](#specifying-the-repository) argument of the output function.]\n";
    let output1 = format(input, Some(cfg.clone()), None);
    let output2 = format(&output1, Some(cfg), None);
    similar_asserts::assert_eq!(output1, output2, "Formatting should be idempotent");
}
