use panache_formatter::format;

#[test]
fn collapses_internal_newline_in_citation() {
    // A citation split across an input line break must not preserve the
    // embedded newline; it should reflow like ordinary inline text.
    let input = "glmnet\\ [@friedman2010;\n@tay2023], biglasso\\ [@zeng2017].\n";
    let output = format(input, None, None);
    similar_asserts::assert_eq!(
        output,
        "glmnet\\ [@friedman2010; @tay2023], biglasso\\ [@zeng2017].\n"
    );
}

#[test]
fn preserves_prefix_and_suffix_spacing_in_citation() {
    let input = "See [see @doe, pp. 33--35; also @smith].\n";
    let output = format(input, None, None);
    similar_asserts::assert_eq!(output, input);
}

#[test]
fn collapses_newline_in_crossref() {
    let input = "as in [-@friedman2010;\n-@tay2023] above.\n";
    let output = format(input, None, None);
    similar_asserts::assert_eq!(output, "as in [-@friedman2010; -@tay2023] above.\n");
}
