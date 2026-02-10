use panache::{Config, format};

#[tokio::test]
async fn test_raw_inline_html() {
    let input = "This is `<a>html</a>`{=html} text.";
    let config = Config::default();
    let output = format(input, Some(config)).await;
    assert_eq!(output.trim(), "This is `<a>html</a>`{=html} text.");
}

#[tokio::test]
async fn test_raw_inline_latex() {
    let input = r"This is `\LaTeX`{=latex} formatted.";
    let config = Config::default();
    let output = format(input, Some(config)).await;
    assert_eq!(output.trim(), r"This is `\LaTeX`{=latex} formatted.");
}

#[tokio::test]
async fn test_raw_inline_openxml() {
    let input = "This is `<w:br/>`{=openxml} a pagebreak.";
    let config = Config::default();
    let output = format(input, Some(config)).await;
    assert_eq!(output.trim(), "This is `<w:br/>`{=openxml} a pagebreak.");
}

#[tokio::test]
async fn test_raw_inline_with_double_backticks() {
    let input = "This is `` `backtick` ``{=html} text.";
    let config = Config::default();
    let output = format(input, Some(config)).await;
    assert_eq!(output.trim(), "This is `` `backtick` ``{=html} text.");
}

#[tokio::test]
async fn test_raw_inline_disabled() {
    let input = "This is `<a>html</a>`{=html} text.";
    let mut config = Config::default();
    config.extensions.raw_attribute = false;
    let output = format(input, Some(config)).await;
    // Should be treated as regular code span with attributes
    assert_eq!(output.trim(), "This is `<a>html</a>`{=html} text.");
}

#[tokio::test]
async fn test_code_span_with_regular_class() {
    // Regular code span with .class should not be treated as raw inline
    let input = "This is `code`{.python} text.";
    let config = Config::default();
    let output = format(input, Some(config)).await;
    assert_eq!(output.trim(), "This is `code`{.python} text.");
}

#[tokio::test]
async fn test_raw_inline_mixed_with_code_spans() {
    let input = "Regular `code` and raw `<html>`{=html} in one sentence.";
    let config = Config::default();
    let output = format(input, Some(config)).await;
    assert_eq!(
        output.trim(),
        "Regular `code` and raw `<html>`{=html} in one sentence."
    );
}

#[tokio::test]
async fn test_raw_inline_multiple_formats() {
    let input = "HTML `<a>`{=html} and LaTeX `\\cmd`{=latex} together.";
    let config = Config::default();
    let output = format(input, Some(config)).await;
    assert_eq!(
        output.trim(),
        "HTML `<a>`{=html} and LaTeX `\\cmd`{=latex} together."
    );
}

#[tokio::test]
async fn test_raw_inline_preservation() {
    // Test that content is preserved exactly (not reformatted)
    let input = "Raw `  spaced  content  `{=html} here.";
    let config = Config::default();
    let output = format(input, Some(config)).await;
    assert_eq!(output.trim(), "Raw `  spaced  content  `{=html} here.");
}
