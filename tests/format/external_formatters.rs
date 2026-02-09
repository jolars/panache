use panache::{Config, format};
use std::collections::HashMap;

#[tokio::test]
async fn code_block_with_external_formatter() {
    // Use 'tr' to uppercase as a simple mock formatter
    let mut formatters = HashMap::new();
    formatters.insert(
        "test".to_string(),
        panache::config::FormatterConfig {
            cmd: "tr".to_string(),
            args: vec!["[:lower:]".to_string(), "[:upper:]".to_string()],
            enabled: true,
            stdin: true,
        },
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

    let output = format(input, Some(config)).await;

    // Code should be uppercased by the formatter
    assert!(output.contains("HELLO WORLD"));
    assert!(output.contains("```test"));
    assert!(output.contains("```\n"));
}

#[tokio::test]
async fn code_block_without_formatter_unchanged() {
    let config = Config::default();

    let input = r#"
```python
hello world
```
"#
    .trim_start();

    let output = format(input, Some(config)).await;

    // Code should be unchanged (no formatter configured)
    assert!(output.contains("hello world"));
    assert!(!output.contains("HELLO WORLD"));
}

#[tokio::test]
async fn code_block_with_disabled_formatter() {
    let mut formatters = HashMap::new();
    formatters.insert(
        "test".to_string(),
        panache::config::FormatterConfig {
            cmd: "tr".to_string(),
            args: vec!["[:lower:]".to_string(), "[:upper:]".to_string()],
            enabled: false,
            stdin: true,
        },
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

    let output = format(input, Some(config)).await;

    // Code should be unchanged (formatter disabled)
    assert!(output.contains("hello world"));
    assert!(!output.contains("HELLO WORLD"));
}

#[tokio::test]
async fn code_block_with_failing_formatter() {
    let mut formatters = HashMap::new();
    formatters.insert(
        "test".to_string(),
        panache::config::FormatterConfig {
            cmd: "false".to_string(), // Always fails
            args: vec![],
            enabled: true,
            stdin: true,
        },
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

    let output = format(input, Some(config)).await;

    // Code should be unchanged on formatter failure
    assert!(output.contains("hello world"));
    assert!(!output.contains("HELLO WORLD"));
}
