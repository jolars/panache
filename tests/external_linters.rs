//! Integration tests for external linter support

#[cfg(test)]
mod tests {
    use panache::{Config, linter, parse};
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_jarl_linter_integration() {
        // Skip if jarl not available
        if which::which("jarl").is_err() {
            println!("Skipping jarl test - jarl not installed");
            return;
        }

        let input = r#"# Test

```r
x = 1
result <- T
```
"#;

        // Create config with jarl enabled
        let mut config = Config::default();
        let mut linters = HashMap::new();
        linters.insert("r".to_string(), "jarl".to_string());
        config.linters = linters;

        let tree = parse(input, Some(config.clone()));
        let diagnostics = linter::lint_with_external(&tree, input, &config).await;

        // Should find at least the assignment violation (x = 1)
        assert!(!diagnostics.is_empty(), "Expected diagnostics from jarl");

        // Check that we found the assignment issue
        let assignment_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == "assignment")
            .collect();
        assert_eq!(
            assignment_diags.len(),
            1,
            "Expected 1 assignment diagnostic (x = 1)"
        );

        // Check line numbers are correct
        assert_eq!(assignment_diags[0].location.line, 4); // x = 1 is on line 4
        assert!(assignment_diags[0].fix.is_some(), "Expected auto-fix");
    }

    #[tokio::test]
    async fn test_multiple_r_blocks_concatenation() {
        if which::which("jarl").is_err() {
            println!("Skipping jarl test - jarl not installed");
            return;
        }

        let input = r#"```r
x = 1
```

Some text in between.

```r
y = 2
```
"#;

        let mut config = Config::default();
        let mut linters = HashMap::new();
        linters.insert("r".to_string(), "jarl".to_string());
        config.linters = linters;

        let tree = parse(input, Some(config.clone()));
        let diagnostics = linter::lint_with_external(&tree, input, &config).await;

        // Should find 2 assignment violations
        let assignment_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == "assignment")
            .collect();
        assert_eq!(assignment_diags.len(), 2);

        // Check both line numbers are correct
        assert_eq!(assignment_diags[0].location.line, 2); // First block content, line 2
        assert_eq!(assignment_diags[1].location.line, 8); // Second block content, line 8
    }

    #[tokio::test]
    async fn test_no_external_linters_configured() {
        let input = r#"```r
x = 1
```
"#;

        let config = Config::default(); // No linters configured

        let tree = parse(input, Some(config.clone()));
        let diagnostics = linter::lint_with_external(&tree, input, &config).await;

        // Should only have built-in rule diagnostics (if any)
        // No jarl diagnostics
        let external_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == "assignment")
            .collect();
        assert_eq!(external_diags.len(), 0);
    }

    #[tokio::test]
    async fn test_unknown_linter() {
        let input = r#"```r
x <- 1
```
"#;

        let mut config = Config::default();
        let mut linters = HashMap::new();
        linters.insert("r".to_string(), "unknown_linter_12345".to_string());
        config.linters = linters;

        let tree = parse(input, Some(config.clone()));
        let _diagnostics = linter::lint_with_external(&tree, input, &config).await;

        // Should handle gracefully - just skip external linting
        // No panics or errors
        assert!(true);
    }
}
