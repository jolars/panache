use panache_formatter::{
    config::{Config, Extensions, Flavor},
    format,
};

fn quarto_config() -> Config {
    let flavor = Flavor::Quarto;
    Config {
        flavor,
        parser_extensions: Extensions::for_flavor(flavor),
        ..Default::default()
    }
}

fn rmarkdown_config() -> Config {
    let flavor = Flavor::RMarkdown;
    Config {
        flavor,
        parser_extensions: Extensions::for_flavor(flavor),
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
fn rcpp_chunk_label_is_rendered_as_hashpipe_label_in_rmarkdown() {
    let input = "```{Rcpp, rcpp-sum-ref}\n// [[Rcpp::export]]\ndouble sum_cpp_ref(Rcpp::NumericVector& x) {\n  return 0.0;\n}\n```\n";
    let output = format(input, Some(rmarkdown_config()), None);

    assert!(output.contains("```{Rcpp}"));
    assert!(output.contains("//| label: rcpp-sum-ref"));
    assert!(!output.contains("```{Rcpp, rcpp-sum-ref}"));
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
fn complex_inline_option_overrides_hashpipe_option_and_stays_inline() {
    let input = "```{r, fig.cap=knitr::current_input(), echo=TRUE}\n#| fig-cap: \"from-content\"\n#| warning: false\nx <- 1\n```\n";
    let output = format(input, Some(quarto_config()), None);

    assert!(output.contains("```{r, fig.cap=knitr::current_input()}"));
    assert!(output.contains("#| echo: true"));
    assert!(output.contains("#| warning: false"));
    assert!(!output.contains("#| fig-cap:"));
}

#[test]
fn complex_inline_option_remains_inline_when_hashpipe_header_exists() {
    let input = "```{r, results=knitr::asis_output(\"ok\")}\n#| echo: false\nx <- 1\n```\n";
    let output = format(input, Some(quarto_config()), None);

    assert!(output.contains("```{r, results=knitr::asis_output(\"ok\")}"));
    assert!(output.contains("#| echo: false"));
    assert!(!output.contains("#| results:"));
}

#[test]
fn multiline_hashpipe_value_continuation_is_not_dropped() {
    let input = "```{r}\n#| fig-cap: \"A multiline caption\n#|  that spans multiple lines and demonstrates\n#|  wrapping.\"\na <- 1\n```\n";
    let output = format(input, Some(quarto_config()), None);

    assert!(output.contains("#| fig-cap: \"A multiline caption"));
    assert!(output.contains("#|   that spans multiple lines and demonstrates"));
    assert!(output.contains("#|   wrapping.\""));
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

#[test]
fn hashpipe_long_quoted_fig_cap_stays_idempotent() {
    let input = "```{r}\n#| fig-cap: \"Relationship between inflation and GDP growth for Australia, Ethiopia, India, and the United States\"\n1+1\n```\n";
    let output1 = format(input, Some(quarto_config()), None);
    let output2 = format(&output1, Some(quarto_config()), None);

    assert_eq!(output1, output2, "Formatting should be idempotent");
}

#[test]
fn hashpipe_multiline_flow_collection_is_preserved_and_idempotent() {
    let input = "```{r}\n#| categories: [\n#|   \"alpha\",\n#|   \"beta\"\n#| ]\nx <- 1\n```\n";
    let output1 = format(input, Some(quarto_config()), None);
    let output2 = format(&output1, Some(quarto_config()), None);

    assert_eq!(output1, output2, "Formatting should be idempotent");
    assert!(output2.contains("#| categories: ["));
    assert!(output2.contains("#|   \"alpha\","));
    assert!(output2.contains("#|   \"beta\""));
    assert!(output2.contains("#| ]"));
    assert!(output2.contains("x <- 1"));
}

#[test]
fn hashpipe_indented_map_continuation_for_empty_value_is_preserved() {
    let input = "```{r}\n#| metadata:\n#|   author: Jane Doe\n#|   topic: YAML\nx <- 1\n```\n";
    let output1 = format(input, Some(quarto_config()), None);
    let output2 = format(&output1, Some(quarto_config()), None);

    assert_eq!(output1, output2, "Formatting should be idempotent");
    assert!(output2.contains("#| metadata:"));
    assert!(output2.contains("author: Jane Doe"));
    assert!(output2.contains("topic: YAML"));
    assert!(output2.contains("x <- 1"));
}

#[test]
fn hashpipe_options_have_exactly_one_blank_line_before_code() {
    let input = "```{r}\n#| include: false\n\n1 + 2\n```\n";
    let output1 = format(input, Some(quarto_config()), None);
    let output2 = format(&output1, Some(quarto_config()), None);

    assert!(
        output1.contains("#| include: false\n\n1 + 2"),
        "expected exactly one blank line between options and code:\n{output1}"
    );
    assert!(
        !output1.contains("#| include: false\n1 + 2"),
        "expected code not to follow options immediately:\n{output1}"
    );
    assert_eq!(output1, output2, "Formatting should be idempotent");
}

#[test]
fn hashpipe_continuation_marker_stays_attached_to_header() {
    let input = "```{r}\n#| fig-cap: \"Two scatterplots of the relationship between `x` and `y`.\"\n#|\n### x\nx <- rnorm(n)\n```\n";
    let output1 = format(input, Some(quarto_config()), None);
    let output2 = format(&output1, Some(quarto_config()), None);

    assert!(
        output1.contains(
            "#| fig-cap: \"Two scatterplots of the relationship between `x` and `y`.\"\n\n### x"
        ),
        "expected marker-only separator to normalize to one blank line:\n{output1}"
    );
    assert!(
        !output1.contains(
            "#| fig-cap: \"Two scatterplots of the relationship between `x` and `y`.\"\n#|"
        ),
        "expected marker-only separator line to be removed:\n{output1}"
    );
    assert_eq!(output1, output2, "Formatting should be idempotent");
}

#[test]
fn hashpipe_prefix_variants_are_used_for_executable_chunk_languages() {
    let input = "```{r, echo=FALSE}\n1 + 1\n```\n\n```{cpp, echo=FALSE}\nint x = 1;\n```\n\n```{sql, echo=FALSE}\nSELECT 1;\n```\n";
    let output = format(input, Some(quarto_config()), None);

    assert!(output.contains("```{r}"));
    assert!(output.contains("#| echo: false"));
    assert!(output.contains("```{cpp}"));
    assert!(output.contains("//| echo: false"));
    assert!(output.contains("```{sql}"));
    assert!(output.contains("--| echo: false"));
}

#[test]
fn cpp_hashpipe_block_scalar_continuation_is_idempotent() {
    let input =
        "```{cpp}\n//| fig-cap: |\n//|   A caption line\n//|   Continued line\nint x = 1;\n```\n";
    let output1 = format(input, Some(quarto_config()), None);
    let output2 = format(&output1, Some(quarto_config()), None);

    assert_eq!(output1, output2, "Formatting should be idempotent");
    assert!(output2.contains("//| fig-cap: |"));
    assert!(output2.contains("//|   A caption line"));
    assert!(output2.contains("//|   Continued line"));
    assert!(output2.contains("int x = 1;"));
}
