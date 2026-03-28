use panache::{
    config::{Config, Extensions, Flavor},
    format,
};

fn quarto_config() -> Config {
    let flavor = Flavor::Quarto;
    Config {
        flavor,
        extensions: Extensions::for_flavor(flavor),
        ..Default::default()
    }
}

#[test]
fn r_chunk_with_comma_separated_options() {
    let input = "```{r, echo=FALSE}\n1 + 1\n```\n";
    let output = format(input, Some(quarto_config()), None);

    // Should convert to hashpipe format in Quarto
    assert!(output.contains("```{r}"));
    assert!(output.contains("#| echo: false"));
    assert!(
        !output.contains("echo=\"FALSE\""),
        "Should not add quotes to boolean FALSE"
    );
}

#[test]
fn r_chunk_with_multiple_options() {
    let input = "```{r, echo=FALSE, warning=TRUE, message=FALSE}\nx <- 1\n```\n";
    let output = format(input, Some(quarto_config()), None);

    // All boolean values should be converted to hashpipe with lowercase
    assert!(output.contains("#| echo: false"));
    assert!(output.contains("#| warning: true"));
    assert!(output.contains("#| message: false"));
}

#[test]
fn r_chunk_with_quoted_string() {
    let input = "```{r, label=\"my chunk\", echo=FALSE}\ny <- 2\n```\n";
    let output = format(input, Some(quarto_config()), None);

    // Quoted strings should be converted to hashpipe, preserving quotes
    assert!(output.contains("#| label: \"my chunk\""));
    assert!(output.contains("#| echo: false"));
    assert!(!output.contains("echo=\"FALSE\""));
}

#[test]
fn r_chunk_without_spaces_after_commas() {
    let input = "```{r,echo=TRUE,warning=FALSE}\na <- 4\n```\n";
    let output = format(input, Some(quarto_config()), None);

    // Should convert to hashpipe format
    assert!(output.contains("#| echo: true"));
    assert!(output.contains("#| warning: false"));
}

#[test]
fn chunk_idempotency() {
    let input = "```{r, echo=FALSE, warning=TRUE}\nx <- 1\n```\n";
    let output1 = format(input, Some(quarto_config()), None);
    let output2 = format(&output1, Some(quarto_config()), None);

    assert_eq!(output1, output2, "Formatting should be idempotent");
}

#[test]
fn chunk_with_numeric_value() {
    let input = "```{r, fig.width=7, fig.height=5}\nplot(x)\n```\n";
    let output = format(input, Some(quarto_config()), None);

    // Numeric values should be converted to hashpipe with normalized names
    assert!(output.contains("#| fig-width: 7"));
    assert!(output.contains("#| fig-height: 5"));
}

#[test]
fn display_block_vs_executable_chunk() {
    let input_display = "```python\nprint('hello')\n```\n";
    let input_exec = "```{python}\nprint('hello')\n```\n";

    let output_display = format(input_display, Some(quarto_config()), None);
    let output_exec = format(input_exec, Some(quarto_config()), None);

    // Display blocks use shortcut syntax in Quarto flavor
    assert!(output_display.starts_with("```python"));
    assert!(!output_display.contains("{python}"));

    // Executable chunks keep braces
    assert!(output_exec.contains("```{python}"));
}

#[test]
fn deprecated_code_block_style_config_is_noop() {
    let config: Config = toml::from_str(
        r#"
        flavor = "quarto"

        [format.code-blocks]
        attribute-style = "preserve"
    "#,
    )
    .unwrap();
    let input = "```{r,echo=FALSE}\n1 + 1\n```\n";
    let output = format(input, Some(config), None);

    // Deprecated option is ignored; executable chunks still use normalized output.
    assert!(output.contains("```{r}"));
    assert!(output.contains("#| echo: false"));
}

#[test]
fn r_chunk_label_with_spaces_stays_single_label() {
    let input = "```{r several words}\n#\n```\n";
    let output = format(input, Some(quarto_config()), None);

    assert!(output.contains("#| label: several words"));
    assert_eq!(output.matches("#| label:").count(), 1);
}

#[test]
fn inline_options_override_existing_hashpipe_options() {
    let input = "```{r, echo=TRUE}\n#| echo: false\n#| label: \"from-content\"\nx <- 1\n```\n";
    let output = format(input, Some(quarto_config()), None);

    assert_eq!(output.matches("#| echo:").count(), 1);
    assert!(output.contains("#| echo: true"));
    assert_eq!(output.matches("#| label:").count(), 1);
    assert!(output.contains("#| label: \"from-content\""));
}

#[test]
fn multiline_hashpipe_value_continuation_is_not_dropped() {
    let input = "```{r}\n#| fig-cap: \"A multiline caption\n#|  that spans multiple lines and demonstrates\n#|  wrapping.\"\na <- 1\n```\n";
    let output = format(input, Some(quarto_config()), None);

    assert!(output.contains(
        "#| fig-cap: \"A multiline caption that spans multiple lines and demonstrates wrapping.\""
    ));
    assert!(!output.contains("#|  that spans multiple lines and demonstrates"));
    assert!(output.contains("a <- 1"));
}

#[test]
fn inline_options_override_hashpipe_block_scalar_without_leaking_old_lines() {
    let input = "```{r, fig.cap=\"Inline caption\"}\n#| fig-cap: |\n#|   A caption\n#|   spanning some lines\na <- 1\n```\n";
    let output = format(input, Some(quarto_config()), None);

    assert!(output.contains("#| fig-cap: \"Inline caption\""));
    assert!(!output.contains("#| fig-cap: |"));
    assert!(!output.contains("#|   A caption"));
    assert!(!output.contains("#|   spanning some lines"));
    assert!(output.contains("a <- 1"));
}

#[test]
fn inline_options_override_hashpipe_folded_block_scalar_without_leaking_old_lines() {
    let input = "```{r, fig.cap=\"Inline caption\"}\n#| fig-cap: >-\n#|   A folded caption\n#|   spanning some lines\na <- 1\n```\n";
    let output = format(input, Some(quarto_config()), None);

    assert!(output.contains("#| fig-cap: \"Inline caption\""));
    assert!(!output.contains("#| fig-cap: >-"));
    assert!(!output.contains("#|   A folded caption"));
    assert!(!output.contains("#|   spanning some lines"));
    assert!(output.contains("a <- 1"));
}

#[test]
fn hashpipe_indented_yaml_value_is_preserved_as_hashpipe_header() {
    let input = "```{r}\n#| list:\n#|   - a\n#|   - b\na <- 1\n```\n";
    let output = format(input, Some(quarto_config()), None);

    assert!(output.contains("#| list:"));
    assert!(output.contains("#|   - a"));
    assert!(output.contains("#|   - b"));
    assert!(output.contains("a <- 1"));
}

#[test]
fn hashpipe_block_scalar_formatting_is_idempotent() {
    let input =
        "```{r}\n#| fig-cap: |\n#|   A caption\n#|   spanning some lines\nplot(1:10)\n```\n";
    let output1 = format(input, Some(quarto_config()), None);
    let output2 = format(&output1, Some(quarto_config()), None);

    assert_eq!(output1, output2, "Formatting should be idempotent");
    assert!(output2.contains("#| fig-cap: |"));
    assert!(output2.contains("#|   A caption"));
    assert!(output2.contains("#|   spanning some lines"));
}

#[test]
fn hashpipe_folded_block_scalar_formatting_is_idempotent() {
    let input = "```{r}\n#| fig-cap: >-\n#|   A folded caption\n#|   spanning some lines\nplot(1:10)\n```\n";
    let output1 = format(input, Some(quarto_config()), None);
    let output2 = format(&output1, Some(quarto_config()), None);

    assert_eq!(output1, output2, "Formatting should be idempotent");
    assert!(output2.contains("#| fig-cap: >-"));
    assert!(output2.contains("#|   A folded caption"));
    assert!(output2.contains("#|   spanning some lines"));
}

#[test]
fn hashpipe_yaml_wrap_accounts_for_comment_prefix_width() {
    let input = "```{r}\n#| fig-cap: This is a very long caption that should be truncated in the output to ensure that it does not take up too much space in the rendered document. The caption continues with more details about the figure and its significance in the context of the analysis being performed.\n```\n";
    let config = quarto_config();
    let output = format(input, Some(config.clone()), None);

    for line in output.lines() {
        if line.starts_with("#|") {
            assert!(
                line.len() <= config.line_width,
                "hashpipe line exceeded width ({} > {}): {}",
                line.len(),
                config.line_width,
                line
            );
        }
    }
}

#[test]
fn hashpipe_fig_cap_list_value_is_idempotent() {
    let input = "```{r}\n#| fig-cap:\n#|   - A\n#|   - B\n```\n";
    let output1 = format(input, Some(quarto_config()), None);
    let output2 = format(&output1, Some(quarto_config()), None);

    assert_eq!(output1, output2, "Formatting should be idempotent");
    assert_eq!(output2.matches("#|   - A").count(), 1);
    assert_eq!(output2.matches("#|   - B").count(), 1);
}
