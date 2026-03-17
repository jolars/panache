use panache::config::WrapMode;
use panache::{Config, format};
use std::collections::HashMap;

#[test]
fn test_yaml_frontmatter_ignores_external_yaml_formatter() {
    let mut formatters = HashMap::new();
    formatters.insert(
        "yaml".to_string(),
        vec![panache::config::FormatterConfig {
            cmd: "tr".to_string(),
            args: vec!["-d".to_string(), "\\n\\r\\t ".to_string()],
            enabled: true,
            stdin: true,
        }],
    );

    let config = Config {
        formatters,
        ..Default::default()
    };

    let input = "---\ntitle: CLI Reference\n---\n\n# Test\n";
    let output = format(input, Some(config), None);

    assert!(
        !output.contains("title:CLIReference"),
        "External YAML formatter should not be applied to frontmatter"
    );
    assert!(output.contains("title: CLI Reference"));
}

#[test]
fn test_yaml_frontmatter_uses_builtin_yaml_formatter_by_default() {
    let input = "---\necho:    false\nlist:\n  -  a\n  -     b\n---\n\n# Test\n";
    let output = format(input, None, None);

    assert!(output.contains("\necho: false\n"));
    assert!(output.contains("\nlist:\n  - a\n  - b\n"));
}

#[test]
fn test_yaml_frontmatter_reflow_respects_wrap_mode() {
    let input = "---\ntitle: This is a very long yaml scalar that should format differently when wrapping is enabled.\n---\n\n# Test\n";
    let preserve = Config {
        wrap: Some(WrapMode::Preserve),
        line_width: 30,
        ..Default::default()
    };
    let reflow = Config {
        wrap: Some(WrapMode::Reflow),
        line_width: 30,
        ..Default::default()
    };
    let preserved = format(input, Some(preserve), None);
    let reflowed = format(input, Some(reflow), None);
    assert_ne!(preserved, reflowed);
}
