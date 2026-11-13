use panache::config::{Extensions, Flavor};
use panache::{Config, format};
use std::collections::HashMap;

#[test]
fn code_style_width_is_scoped_to_each_code_block() {
    if which::which("arity").is_err() {
        return;
    }

    let mut config = Config {
        flavor: Flavor::Quarto,
        extensions: Extensions::for_flavor(Flavor::Quarto),
        ..Default::default()
    };
    config.formatters.insert(
        "r".to_string(),
        vec![panache::config::get_formatter_preset("arity").unwrap()],
    );

    let input = "```{r}\n#| code-style:\n#|   line-width: 40\n\nresult <- some_function(first_argument, second_argument)\n```\n\n```{.r code-style=\"{line-width: 80}\"}\nresult <- some_function(first_argument, second_argument)\n```\n\n```{r}\nresult <- some_function(first_argument, second_argument)\n```\n";
    let output = format(input, Some(config.clone()), None);

    assert!(
        output.contains("result <- some_function(\n  first_argument,\n  second_argument\n)"),
        "{output}"
    );
    assert_eq!(
        output
            .matches("result <- some_function(first_argument, second_argument)")
            .count(),
        2,
        "{output}"
    );
    assert_eq!(format(&output, Some(config), None), output);
}

#[test]
fn document_code_style_defaults_merge_with_block_options() {
    if which::which("arity").is_err() {
        return;
    }

    let mut config = Config {
        flavor: Flavor::Quarto,
        extensions: Extensions::for_flavor(Flavor::Quarto),
        ..Default::default()
    };
    config.formatters.insert(
        "r".to_string(),
        vec![panache::config::get_formatter_preset("arity").unwrap()],
    );

    let input = "---\ncode-style:\n  line-width: 40\n  indent-width: 4\n---\n\n```{r}\nresult <- some_function(first_argument, second_argument)\n```\n\n```{r}\n#| code-style: {indent-width: 2}\n\nresult <- some_function(first_argument, second_argument)\n```\n\n```{.r code-style=\"{line-width: 80}\"}\nresult <- some_function(first_argument, second_argument)\n```\n";
    let output = format(input, Some(config.clone()), None);

    assert!(
        output.contains("result <- some_function(\n    first_argument,\n    second_argument\n)"),
        "{output}"
    );
    assert!(
        output.contains("result <- some_function(\n  first_argument,\n  second_argument\n)"),
        "{output}"
    );
    assert!(
        output.contains("result <- some_function(first_argument, second_argument)"),
        "{output}"
    );
    assert_eq!(format(&output, Some(config), None), output);
}

#[test]
fn custom_formatter_translates_code_style_keys() {
    if which::which("arity").is_err() {
        return;
    }

    let mut config: Config = toml::from_str(
        r#"
[formatters]
r = "local-r"

[formatters.local-r]
cmd = "arity"
args = ["format"]

[formatters.local-r.code-style-args]
line-width = ["--line-width", "{value}"]
indent-width = ["--indent-width", "{value}"]
"#,
    )
    .unwrap();
    config.flavor = Flavor::Quarto;
    config.extensions = Extensions::for_flavor(Flavor::Quarto);

    let input = "```{r}\n#| code-style: {line-width: 40, indent-width: 4}\n\nresult <- some_function(first_argument, second_argument)\n```\n";
    let output = format(input, Some(config), None);
    assert!(
        output.contains("result <- some_function(\n    first_argument,\n    second_argument\n)"),
        "{output}"
    );
}

#[test]
fn ruff_preset_translates_code_style_width() {
    if which::which("ruff").is_err() {
        return;
    }

    let mut config = Config {
        flavor: Flavor::Quarto,
        extensions: Extensions::for_flavor(Flavor::Quarto),
        ..Default::default()
    };
    config.formatters.insert(
        "python".to_string(),
        vec![panache::config::get_formatter_preset("ruff").unwrap()],
    );

    let input = "```{python}\n#| code-style:\n#|   line-width: 40\n\nx = some_function(first_argument, second_argument)\n```\n";
    let output = format(input, Some(config), None);
    assert!(
        output.contains("x = some_function(\n    first_argument, second_argument\n)"),
        "{output}"
    );
}

#[test]
fn code_block_with_shfmt() {
    // Skip if shfmt not available
    if which::which("shfmt").is_err() {
        println!("Skipping shfmt test - shfmt not installed");
        return;
    }

    let mut formatters = HashMap::new();
    formatters.insert(
        "sh".to_string(),
        vec![panache::config::FormatterConfig {
            cmd: "shfmt".to_string(),
            args: vec![],
            stdin: true,
            code_style_args: Default::default(),
        }],
    );

    let config = Config {
        flavor: Flavor::Quarto,
        extensions: Extensions::for_flavor(Flavor::Quarto),
        formatters,
        ..Default::default()
    };

    let input = r#"
```sh
if true; then echo ok; fi
```
"#
    .trim_start();

    let output = format(input, Some(config), None);

    // shfmt should format the shell code (expands one-liner)
    assert!(output.contains("```sh"));
    assert!(output.contains("if true; then"));
}

#[test]
fn formatter_key_resolves_via_language_alias() {
    // The formatter is configured under `bash`, but the code block is tagged
    // `sh`. These are the same language for formatting purposes, so the alias
    // must resolve (previously the exact-match lookup silently skipped it).
    if which::which("shfmt").is_err() {
        println!("Skipping shfmt test - shfmt not installed");
        return;
    }

    let mut formatters = HashMap::new();
    formatters.insert(
        "bash".to_string(),
        vec![panache::config::FormatterConfig {
            cmd: "shfmt".to_string(),
            args: vec![],
            stdin: true,
            code_style_args: Default::default(),
        }],
    );

    let config = Config {
        flavor: Flavor::Quarto,
        extensions: Extensions::for_flavor(Flavor::Quarto),
        formatters,
        ..Default::default()
    };

    let input = r#"
```sh
if true; then echo ok; fi
```
"#
    .trim_start();

    let output = format(input, Some(config), None);

    assert!(output.contains("```sh"));
    assert!(
        output.contains("if true; then"),
        "shfmt should expand the one-liner even though the key is `bash`, got:\n{output}"
    );
}

#[test]
fn identical_blocks_are_deduplicated_and_all_formatted() {
    // Multiple identical same-language blocks share one formatter invocation
    // (dedup), but every block must still receive the formatted output.
    if which::which("shfmt").is_err() {
        println!("Skipping shfmt test - shfmt not installed");
        return;
    }

    let mut formatters = HashMap::new();
    formatters.insert(
        "sh".to_string(),
        vec![panache::config::FormatterConfig {
            cmd: "shfmt".to_string(),
            args: vec![],
            stdin: true,
            code_style_args: Default::default(),
        }],
    );

    let config = Config {
        flavor: Flavor::Quarto,
        extensions: Extensions::for_flavor(Flavor::Quarto),
        formatters,
        ..Default::default()
    };

    // Three byte-identical blocks (2-space indent, which shfmt rewrites to a
    // tab) plus one distinct block.
    let input = "```sh\nif true; then\n  echo ok\nfi\n```\n\n```sh\nif true; then\n  echo ok\nfi\n```\n\n```sh\nif true; then\n  echo ok\nfi\n```\n\n```sh\nif false; then\n  echo no\nfi\n```\n";

    let output = format(input, Some(config), None);

    // Every occurrence of the repeated block is formatted (2-space -> tab from
    // shfmt, then expanded to `tab_width` spaces by panache), and the distinct
    // block is formatted too.
    assert_eq!(
        output.matches("if true; then\n    echo ok\nfi").count(),
        3,
        "all three identical blocks should be formatted:\n{output}"
    );
    assert!(
        output.contains("if false; then\n    echo no\nfi"),
        "distinct block should be formatted:\n{output}"
    );
}

#[test]
fn code_block_with_external_formatter() {
    // Use 'tr' to uppercase as a simple mock formatter
    let mut formatters = HashMap::new();
    formatters.insert(
        "test".to_string(),
        vec![panache::config::FormatterConfig {
            cmd: "tr".to_string(),
            args: vec!["[:lower:]".to_string(), "[:upper:]".to_string()],
            stdin: true,
            code_style_args: Default::default(),
        }],
    );

    let config = Config {
        flavor: Flavor::Quarto,
        extensions: Extensions::for_flavor(Flavor::Quarto),
        formatters,
        ..Default::default()
    };

    let input = r#"
```test
hello world
```
"#
    .trim_start();

    let output = format(input, Some(config), None);

    // Code should be uppercased by the formatter
    assert!(output.contains("HELLO WORLD"));
    assert!(output.contains("```test"));
    assert!(output.contains("```\n"));
}

#[test]
fn myst_directive_body_with_external_formatter() {
    // A verbatim MyST `{code-block}` body should be routed to the external
    // formatter keyed by the directive argument (the language), like a fenced
    // code block. Use `tr` to uppercase as a deterministic mock formatter.
    let mut formatters = HashMap::new();
    formatters.insert(
        "test".to_string(),
        vec![panache::config::FormatterConfig {
            cmd: "tr".to_string(),
            args: vec!["[:lower:]".to_string(), "[:upper:]".to_string()],
            stdin: true,
            code_style_args: Default::default(),
        }],
    );

    let config = Config {
        flavor: Flavor::Myst,
        extensions: Extensions::for_flavor(Flavor::Myst),
        formatters,
        ..Default::default()
    };

    let input = "```{code-block} test\n:linenos:\nhello world\n```\n";

    let output = format(input, Some(config.clone()), None);

    // The body is uppercased, fences/argument/options are preserved.
    assert!(
        output.contains("HELLO WORLD"),
        "body should be formatted:\n{output}"
    );
    assert!(
        output.contains("```{code-block} test"),
        "opener preserved:\n{output}"
    );
    assert!(output.contains(":linenos:"), "option preserved:\n{output}");

    // Idempotency: formatting the result again is a no-op.
    let output2 = format(&output, Some(config), None);
    assert_eq!(output, output2, "formatting must be idempotent");
}

#[test]
fn myst_non_code_directives_preserve_bodies_with_matching_formatter() {
    let mut config = Config {
        flavor: Flavor::Myst,
        extensions: Extensions::for_flavor(Flavor::Myst),
        ..Default::default()
    };
    config.formatters.insert(
        "test".to_string(),
        vec![panache::config::FormatterConfig {
            cmd: "tr".to_string(),
            args: vec!["[:lower:]".to_string(), "[:upper:]".to_string()],
            stdin: true,
            code_style_args: Default::default(),
        }],
    );

    for name in [
        "custom-directive",
        "domain:custom",
        "raw",
        "eval-rst",
        "math",
        "toctree",
    ] {
        let input = format!("```{{{name}}} test\nhello   world\n    indented\n```\n");
        let output = format(&input, Some(config.clone()), None);
        assert_eq!(output, input, "{name} must not invoke a code formatter");
    }
}

#[test]
fn formatter_args_substitute_lang_placeholder() {
    // `sed s/{lang}/REPL/g` should rewrite the language literal in the code
    // body, proving the {lang} placeholder is substituted at dispatch time.
    if which::which("sed").is_err() {
        println!("Skipping sed test - sed not installed");
        return;
    }

    let mut formatters = HashMap::new();
    formatters.insert(
        "python".to_string(),
        vec![panache::config::FormatterConfig {
            cmd: "sed".to_string(),
            args: vec!["s/{lang}/REPL/g".to_string()],
            stdin: true,
            code_style_args: Default::default(),
        }],
    );

    let config = Config {
        flavor: Flavor::Quarto,
        extensions: Extensions::for_flavor(Flavor::Quarto),
        formatters,
        ..Default::default()
    };

    let input = r#"
```python
print("python rocks")
```
"#
    .trim_start();

    let output = format(input, Some(config), None);

    assert!(
        output.contains("REPL rocks"),
        "expected `python` literal in body to be rewritten to `REPL` by `s/{{lang}}/REPL/g`; got:\n{output}"
    );
}

#[test]
fn untagged_code_block_with_empty_string_formatter_key() {
    // `[formatters.""]` matches only truly untagged blocks, never ```plain.
    let mut formatters = HashMap::new();
    formatters.insert(
        String::new(),
        vec![panache::config::FormatterConfig {
            cmd: "tr".to_string(),
            args: vec!["[:lower:]".to_string(), "[:upper:]".to_string()],
            stdin: true,
            code_style_args: Default::default(),
        }],
    );

    let config = Config {
        flavor: Flavor::Quarto,
        extensions: Extensions::for_flavor(Flavor::Quarto),
        formatters,
        ..Default::default()
    };

    let input = r#"
```
bare block
```

```plain
plain tagged block
```
"#
    .trim_start();

    let output = format(input, Some(config), None);

    assert!(
        output.contains("BARE BLOCK"),
        "untagged block should be upcased by `\"\"` formatter; got:\n{output}"
    );
    assert!(
        output.contains("plain tagged block"),
        "```plain block must not be touched by `\"\"` formatter; got:\n{output}"
    );
    assert!(
        !output.contains("PLAIN TAGGED BLOCK"),
        "```plain block must not be upcased by `\"\"` formatter; got:\n{output}"
    );
}

#[test]
fn code_block_without_formatter_unchanged() {
    // Create config with empty formatters (no built-in defaults)
    let config = Config {
        formatters: HashMap::new(),
        ..Default::default()
    };

    let input = r#"
```python
hello world
```
"#
    .trim_start();

    let output = format(input, Some(config), None);

    // Code should be unchanged (no formatter configured)
    assert!(output.contains("hello world"));
    assert!(!output.contains("HELLO WORLD"));
}

#[test]
fn code_block_with_disabled_formatter() {
    // In the new format, disabled formatters are handled by not including them in the map
    // This test now verifies that an empty formatter list means no formatting
    let formatters = HashMap::new(); // No formatter configured

    let config = Config {
        flavor: Flavor::Quarto,
        extensions: Extensions::for_flavor(Flavor::Quarto),
        formatters,
        ..Default::default()
    };

    let input = r#"
```test
hello world
```
"#
    .trim_start();

    let output = format(input, Some(config), None);

    // Code should be unchanged (no formatter configured)
    assert!(output.contains("hello world"));
    assert!(!output.contains("HELLO WORLD"));
}

#[test]
fn code_block_with_failing_formatter() {
    let mut formatters = HashMap::new();
    formatters.insert(
        "test".to_string(),
        vec![panache::config::FormatterConfig {
            cmd: "false".to_string(), // Always fails
            args: vec![],
            stdin: true,
            code_style_args: Default::default(),
        }],
    );

    let config = Config {
        formatters,
        ..Default::default()
    };

    let input = r#"
```test
hello world
```
"#
    .trim_start();

    let output = format(input, Some(config), None);

    // Code should be unchanged on formatter failure
    assert!(output.contains("hello world"));
    assert!(!output.contains("HELLO WORLD"));
}

#[test]
fn python_hashpipe_prefix_preserved_with_external_formatter() {
    let mut formatters = HashMap::new();
    formatters.insert(
        "python".to_string(),
        vec![panache::config::FormatterConfig {
            cmd: "tr".to_string(),
            args: vec!["[:lower:]".to_string(), "[:upper:]".to_string()],
            stdin: true,
            code_style_args: Default::default(),
        }],
    );

    let flavor = Flavor::Quarto;
    let config = Config {
        flavor,
        extensions: Extensions::for_flavor(flavor),
        formatters,
        ..Default::default()
    };

    let input = r#"
```{python}
#| label: setup
#| fig-cap: "My figure"

print("ok")
```
"#
    .trim_start();

    let output = format(input, Some(config), None);

    assert!(output.contains("#| label: setup"));
    assert!(output.contains("#| fig-cap: \"My figure\""));
    assert!(output.contains("PRINT(\"OK\")"));
    assert!(!output.contains("# |"));
}

#[test]
fn r_air_formats_equals_spacing_in_quarto_r_block() {
    if which::which("air").is_err() {
        println!("Skipping air test - air not installed");
        return;
    }

    let mut formatters = HashMap::new();
    formatters.insert(
        "r".to_string(),
        vec![panache::config::FormatterConfig {
            cmd: "air".to_string(),
            args: vec!["format".to_string(), "{}".to_string()],
            stdin: false,
            code_style_args: Default::default(),
        }],
    );

    let config = Config {
        flavor: Flavor::Quarto,
        extensions: Extensions::for_flavor(Flavor::Quarto),
        formatters,
        ..Default::default()
    };

    // Use a named argument (`x=1`) rather than a bare assignment (`a=1`):
    // air normalizes top-level assignment to `<-`, but keeps `=` in call
    // arguments and only inserts the surrounding spaces, which is the
    // equals-spacing behavior this test pins.
    let input = r#"
```{r}
plot(x=1)
```
"#
    .trim_start();

    let output = format(input, Some(config), None);
    assert!(output.contains("plot(x = 1)"));
}

#[test]
fn r_air_preserves_single_blank_line_between_hashpipe_options_and_code() {
    if which::which("air").is_err() {
        println!("Skipping air test - air not installed");
        return;
    }

    let mut formatters = HashMap::new();
    formatters.insert(
        "r".to_string(),
        vec![panache::config::FormatterConfig {
            cmd: "air".to_string(),
            args: vec!["format".to_string(), "{}".to_string()],
            stdin: false,
            code_style_args: Default::default(),
        }],
    );

    let config = Config {
        flavor: Flavor::Quarto,
        extensions: Extensions::for_flavor(Flavor::Quarto),
        formatters,
        ..Default::default()
    };

    let input = r#"
```{r}
#| include: false

1+2
```
"#
    .trim_start();

    let output = format(input, Some(config.clone()), None);
    assert!(
        output.contains("#| include: false\n\n1 + 2"),
        "expected exactly one blank line between options and code:\n{output}"
    );
    assert!(
        !output.contains("#| include: false\n1 + 2"),
        "expected code not to follow options immediately:\n{output}"
    );

    let output_twice = format(&output, Some(config), None);
    assert_eq!(output, output_twice, "Formatting should be idempotent");
}
