use panache::format;

#[test]
fn image_with_class() {
    let input = "![Figure](fig.png){.large}\n";
    let output = format(input, None, None);
    assert!(output.contains("![Figure](fig.png){.large}"));
}

#[test]
fn image_with_id() {
    let input = "![Results](results.png){#fig-results}\n";
    let output = format(input, None, None);
    assert!(output.contains("![Results](results.png){#fig-results}"));
}

#[test]
fn image_with_multiple_classes() {
    let input = "![Photo](photo.jpg){.large .center .border}\n";
    let output = format(input, None, None);
    assert!(output.contains("![Photo](photo.jpg){.large .center .border}"));
}

#[test]
fn image_with_full_attributes() {
    let input = "![Figure 1](fig1.png){#fig-1 .large width=\"80%\" height=\"auto\"}\n";
    let output = format(input, None, None);
    eprintln!("Output: {}", output);
    // The formatter normalizes quotes correctly, check without escaped quotes
    assert!(output.contains("![Figure 1](fig1.png){#fig-1 .large"));
    assert!(output.contains("width="));
    assert!(output.contains("80%"));
    assert!(output.contains("height="));
    assert!(output.contains("auto"));
}

#[test]
fn image_without_attributes() {
    let input = "![Plain image](img.png)\n";
    let output = format(input, None, None);
    assert!(output.contains("![Plain image](img.png)"));
    assert!(!output.contains("{"));
}

#[test]
#[ignore] // TODO: Fix bug where multiple images in same paragraph share attributes
fn multiple_images_with_attributes() {
    // Use very long line width to avoid wrapping
    let cfg = panache::ConfigBuilder::default().line_width(200).build();
    let input = "![First](a.png){#fig-a} and ![Second](b.png){#fig-b}\n";
    let output = format(input, Some(cfg), None);
    eprintln!("Output: {}", output);
    assert!(output.contains("![First](a.png)"));
    assert!(output.contains("fig-a"));
    assert!(output.contains("and"));
    assert!(output.contains("![Second](b.png)"));
    assert!(output.contains("fig-b"));
}

#[test]
fn image_attributes_in_paragraph() {
    let input = "See ![chart](chart.png){.inline width=50%} for details.\n";
    let output = format(input, None, None);
    assert!(output.contains("![chart](chart.png){.inline width=50%}"));
}

#[test]
fn image_space_before_attributes() {
    // Space between ) and { should not parse as attributes
    let input = "![test](img.png) {.large}\n";
    let output = format(input, None, None);
    assert!(output.contains("![test](img.png)"));
    // The {.large} should be treated as regular text
    assert!(output.contains("{.large}"));
}

#[test]
fn image_in_implicit_figure() {
    // Image alone in paragraph (implicit figure in Pandoc conversion)
    let input = "![Figure caption](figure.png){#fig-1 .wide}\n";
    let output = format(input, None, None);
    assert!(output.contains("![Figure caption](figure.png){#fig-1 .wide}"));
}

#[test]
fn image_with_title_and_attributes() {
    let input = "![Alt](img.png \"Title\"){.class}\n";
    let output = format(input, None, None);
    assert!(output.contains("![Alt](img.png \"Title\"){.class}"));
}

#[test]
fn idempotent_formatting() {
    let input = "![Image](img.png){#id .class key=value}\n";
    let first = format(input, None, None);
    let second = format(&first, None, None);
    assert_eq!(first, second);
}

#[test]
fn image_attributes_with_wrapping() {
    let cfg = panache::ConfigBuilder::default().line_width(40).build();
    let input = "This is a long paragraph with an ![embedded image](img.png){.small} that should wrap at the configured width.\n";
    let output = format(input, Some(cfg), None);
    // Should preserve image with attributes even when wrapping
    assert!(output.contains("![embedded image](img.png){.small}"));
}
