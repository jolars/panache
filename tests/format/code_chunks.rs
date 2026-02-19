use panache::{
    config::{AttributeStyle, Config, Extensions, Flavor},
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
fn python_chunk_with_space_separated_options() {
    let input = "```{python echo=False warning=True}\nz = 3\n```\n";
    let output = format(input, Some(quarto_config()), None);

    // Python chunks should also preserve unquoted values
    // Formatter adds comma after language for consistency in Quarto
    assert!(output.contains("echo=False"));
    assert!(output.contains("warning=True"));
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
fn preserve_mode_keeps_original() {
    // Create config with Preserve mode
    let mut config = Config::default();
    config.code_blocks.attribute_style = AttributeStyle::Preserve;

    let input = "```{r,echo=FALSE}\n1 + 1\n```\n";
    let output = format(input, Some(config), None);

    // In preserve mode, should keep original format exactly
    assert!(output.contains("{r,echo=FALSE}"));
}
