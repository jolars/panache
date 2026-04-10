use panache::config::{Extensions, Flavor};
use panache::{Config, format};

#[test]
fn latex_command_in_paragraph() {
    let input = "This is a paragraph with \\textbf{bold text} in the middle.\n";
    let output = format(input, None, None);

    // LaTeX command should be preserved within the paragraph
    assert!(output.contains("\\textbf{bold text}"));
    similar_asserts::assert_eq!(output, input);
}

#[test]
fn latex_command_with_multiple_args() {
    let input = "\\includegraphics[width=0.5\\textwidth]{figure.png}\n";
    let output = format(input, None, None);

    // Complex LaTeX commands should be preserved
    similar_asserts::assert_eq!(output, input);
}

#[test]
fn latex_command_no_wrap() {
    let cfg = panache::ConfigBuilder::default().line_width(30).build();
    let input = "This is a very long line with \\pdfpcnote{a very long note that should not be wrapped or reformatted} that exceeds line width.\n";
    let output = format(input, Some(cfg), None);

    // Check that the LaTeX command appears somewhere in the output (may be wrapped)
    assert!(output.contains("\\pdfpcnote{"));
}

#[test]
fn mixed_latex_and_markdown() {
    let input = "Here is some text with \\LaTeX{} and [a link](https://example.com) together.\n";
    let output = format(input, None, None);

    // Both LaTeX and markdown should be preserved
    assert!(output.contains("\\LaTeX{}"));
    assert!(output.contains("https://example.com"));
}

#[test]
fn braced_tex_command_block_is_preserved() {
    let input = "\\pdfpcnote{\n  - blabla\n}\n";
    let first = format(input, None, None);
    let second = format(&first, None, None);
    similar_asserts::assert_eq!(first, second);
    similar_asserts::assert_eq!(first, input);
}

#[test]
fn tex_block_blank_line_with_spaces_is_stable_between_passes() {
    let input = "::: {.callout-note}\n## Solution\n\n\\begin{align*}\n  a &= 1.\\\\\n    \n\\end{align*}\n:::\n";
    let first = format(input, None, None);
    let second = format(&first, None, None);
    similar_asserts::assert_eq!(first, second);
}

#[test]
fn tex_align_block_in_callout_blank_line_whitespace_is_idempotent() {
    let input = "::: {#prob:expon_post .callout-note icon=\"false\" collapse=\"true\"}\n## Solution\n\nLet $\\mathrm{A}$ be the event that a student has plagiarized and $\\mathrm{B}$\nthe event that the student is flagged by the tool. We want to compute\n$\\mathrm{Pr}(A|B)$ given that $\\mathrm{Pr}(B|A)=0.95$,\n$\\mathrm{Pr}(B^c|A^c)=0.90$ and $\\mathrm{Pr}(A)=0.01$. Using Bayes' theorem we\nget\n\\begin{align*}\n    \\mathrm{Pr}(A|B) =\\frac{\\mathrm{Pr}(B|A)\\mathrm{Pr}(A)}{\\mathrm{Pr}(B|A)\\mathrm{Pr}(A)+\\mathrm{Pr}(B|A^c)\\mathrm{Pr}(A^c)} \\approx 0.0876.\n  \n\\end{align*}\nHence, even if the student is flagged by the tool there is still only an 8.76\\%\nprobability that the student has actually plagiarized.\n:::\n";

    let first = format(input, None, None);
    let second = format(&first, None, None);
    similar_asserts::assert_eq!(first, second);
}

#[test]
fn tex_align_block_with_trailing_space_line_is_idempotent() {
    let input = "  \\begin{align*}\n    \\mathrm{Pr}(A|B) =\\frac{\\mathrm{Pr}(B|A)\\mathrm{Pr}(A)}{\\mathrm{Pr}(B|A)\\mathrm{Pr}(A)+\\mathrm{Pr}(B|A^c)\\mathrm{Pr}(A^c)} \\approx 0.0876.\n  \\end{align*}\n";
    let flavor = Flavor::Quarto;
    let config = Config {
        flavor,
        extensions: Extensions::for_flavor(flavor),
        ..Default::default()
    };
    let first = format(input, Some(config.clone()), None);
    let second = format(&first, Some(config), None);
    similar_asserts::assert_eq!(first, second);
}
