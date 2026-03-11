use panache::format;

#[test]
fn definition_list_wrapped_continuation_is_idempotent() {
    let input = "Markdown, Emacs Org mode, ConTeXt, ZimWiki\n:   It will appear verbatim surrounded by `$...$` (for inline\n                math) or `$$...$$` (for display math).\n";

    let output1 = format(input, None, None);
    let output2 = format(&output1, None, None);

    similar_asserts::assert_eq!(output1, output2, "Formatting should be idempotent");
}
