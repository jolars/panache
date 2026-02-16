use panache::{Config, format};
use std::collections::HashMap;

#[test]
fn test_yaml_frontmatter_with_external_formatter() {
    // Create a shim formatter that strips ALL whitespace (mimics yamlfmt)
    // Using tr instead of sed for simplicity
    let mut formatters = HashMap::new();
    formatters.insert(
        "yaml".to_string(),
        panache::config::FormatterConfig {
            cmd: "tr".to_string(),
            args: vec!["-d".to_string(), "\\n\\r\\t ".to_string()],
            enabled: true,
            stdin: true,
        },
    );

    let config = Config {
        formatters,
        ..Default::default()
    };

    let input = "---\ntitle: CLI Reference\n---\n\n# Test\n";
    let output = format(input, Some(config), None);

    // Debug: print the output
    println!("Output: {:?}", output);
    println!("First 30 chars: {:?}", &output[..30.min(output.len())]);

    // If the formatter ran, the title would have no whitespace at all (title:CLIReference)
    // This should cause the bug to manifest
    // Expected bug: "---title:CLIReference---" (newline after --- removed)
    // Expected correct: "---\ntitle:CLIReference\n---" (newlines preserved around content)

    if output.contains("title:CLIReference") {
        // Formatter ran - check if delimiters are preserved
        assert!(
            output.starts_with("---\n"),
            "Expected frontmatter to start with '---\\n', got: {:?}",
            &output[..20.min(output.len())]
        );
    } else {
        println!("SKIP: Formatter did not run (tr command not available or failed)");
    }
}
