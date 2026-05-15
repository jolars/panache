use panache::config::{Extensions, Flavor};
use panache::{Config, format};
use std::collections::HashMap;

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
            enabled: true,
            stdin: true,
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
fn code_block_with_external_formatter() {
    // Use 'tr' to uppercase as a simple mock formatter
    let mut formatters = HashMap::new();
    formatters.insert(
        "test".to_string(),
        vec![panache::config::FormatterConfig {
            cmd: "tr".to_string(),
            args: vec!["[:lower:]".to_string(), "[:upper:]".to_string()],
            enabled: true,
            stdin: true,
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
            enabled: true,
            stdin: true,
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
            enabled: true,
            stdin: true,
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
            enabled: true,
            stdin: true,
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
            enabled: true,
            stdin: true,
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
            enabled: true,
            stdin: false,
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
a=1
```
"#
    .trim_start();

    let output = format(input, Some(config), None);
    assert!(output.contains("a = 1"));
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
            enabled: true,
            stdin: false,
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
