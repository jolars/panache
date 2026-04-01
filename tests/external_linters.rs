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
any(is.na(x))
result <- TRUE
```
"#;

        // Create config with jarl enabled
        let mut config = Config::default();
        let mut linters = HashMap::new();
        linters.insert("r".to_string(), "jarl".to_string());
        config.linters = linters;

        let tree = parse(input, Some(config.clone()));
        let diagnostics = linter::lint_with_external(&tree, input, &config).await;

        assert!(!diagnostics.is_empty(), "Expected diagnostics from jarl");

        let any_is_na_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == "any_is_na")
            .collect();
        assert_eq!(any_is_na_diags.len(), 1, "Expected 1 any_is_na diagnostic");

        assert_eq!(any_is_na_diags[0].location.line, 4); // any(is.na(x)) is on line 4

        assert!(
            any_is_na_diags[0].fix.is_some(),
            "Auto-fixes should be enabled with byte offset mapping"
        );

        let fix = any_is_na_diags[0].fix.as_ref().unwrap();
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(fix.edits[0].replacement, "anyNA(x)");
    }

    #[tokio::test]
    async fn test_multiple_r_blocks_concatenation() {
        if which::which("jarl").is_err() {
            println!("Skipping jarl test - jarl not installed");
            return;
        }

        let input = r#"```r
any(is.na(x))
```

Some text in between.

```r
any(is.na(y))
```
"#;

        let mut config = Config::default();
        let mut linters = HashMap::new();
        linters.insert("r".to_string(), "jarl".to_string());
        config.linters = linters;

        let tree = parse(input, Some(config.clone()));
        let diagnostics = linter::lint_with_external(&tree, input, &config).await;

        // Should find 2 any_is_na violations
        let any_is_na_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == "any_is_na")
            .collect();
        assert_eq!(any_is_na_diags.len(), 2);

        // Check both line numbers are correct
        assert_eq!(any_is_na_diags[0].location.line, 2); // First block content, line 2
        assert_eq!(any_is_na_diags[1].location.line, 8); // Second block content, line 8

        // Both should have fixes
        assert!(any_is_na_diags[0].fix.is_some());
        assert!(any_is_na_diags[1].fix.is_some());
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
            .filter(|d| d.code == "any_is_na")
            .collect();
        assert_eq!(external_diags.len(), 0);
    }

    #[tokio::test]
    async fn test_ruff_linter_integration() {
        // Skip if ruff not available
        if which::which("ruff").is_err() {
            println!("Skipping ruff test - ruff not installed");
            return;
        }

        let input = r#"# Test

```python
import os
```
"#;

        let mut config = Config::default();
        let mut linters = HashMap::new();
        linters.insert("python".to_string(), "ruff".to_string());
        config.linters = linters;

        let tree = parse(input, Some(config.clone()));
        let diagnostics = linter::lint_with_external(&tree, input, &config).await;

        let ruff_diags: Vec<_> = diagnostics.iter().filter(|d| d.code == "F401").collect();
        assert_eq!(ruff_diags.len(), 1, "Expected 1 Ruff F401 diagnostic");

        assert_eq!(ruff_diags[0].location.line, 4); // import os is on line 4
        assert_eq!(
            ruff_diags[0].severity,
            panache::linter::diagnostics::Severity::Error
        );
        assert!(ruff_diags[0].fix.is_some(), "Ruff fixes should be enabled");
    }

    #[tokio::test]
    async fn test_ruff_fix_application_end_to_end() {
        if which::which("ruff").is_err() {
            println!("Skipping ruff test - ruff not installed");
            return;
        }

        let input = r#"# Test

```python
import os
print("ok")
```
"#;

        let mut config = Config::default();
        let mut linters = HashMap::new();
        linters.insert("python".to_string(), "ruff".to_string());
        config.linters = linters;

        let tree = parse(input, Some(config.clone()));
        let diagnostics = linter::lint_with_external(&tree, input, &config).await;

        let with_fixes: Vec<_> = diagnostics.iter().filter(|d| d.fix.is_some()).collect();
        assert!(!with_fixes.is_empty(), "Expected at least one Ruff fix");

        use panache::linter::diagnostics::Edit;

        let mut edits: Vec<&Edit> = diagnostics
            .iter()
            .filter_map(|d| d.fix.as_ref())
            .flat_map(|f| &f.edits)
            .collect();

        edits.sort_by_key(|e| e.range.start());

        let mut output = String::new();
        let mut last_end = 0;

        for edit in &edits {
            let start: usize = edit.range.start().into();
            let end: usize = edit.range.end().into();

            output.push_str(&input[last_end..start]);
            output.push_str(&edit.replacement);
            last_end = end;
        }

        output.push_str(&input[last_end..]);

        assert!(
            !output.contains("import os"),
            "Ruff fix should remove unused import"
        );
        assert!(output.contains("print(\"ok\")"));
        assert!(output.contains("```python"));
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
        // Test passes if no panic occurs
    }

    #[tokio::test]
    async fn test_fix_application_end_to_end() {
        // This test demonstrates that auto-fixes work end-to-end:
        // 1. Parse markdown with R code
        // 2. Run Jarl to get diagnostics with fixes
        // 3. Apply the fixes to the original document
        // 4. Verify the result is correct

        if which::which("jarl").is_err() {
            println!("Skipping jarl test - jarl not installed");
            return;
        }

        let input = r#"# Test Document

Some text here.

```r
any(is.na(x))
any(is.na(y))
```

More text.
"#;

        let mut config = Config::default();
        let mut linters = HashMap::new();
        linters.insert("r".to_string(), "jarl".to_string());
        config.linters = linters;

        let tree = parse(input, Some(config.clone()));
        let diagnostics = linter::lint_with_external(&tree, input, &config).await;

        // Get diagnostics with fixes
        let with_fixes: Vec<_> = diagnostics.iter().filter(|d| d.fix.is_some()).collect();
        assert!(!with_fixes.is_empty(), "Expected at least one fix");

        // Simulate applying fixes (same logic as CLI --fix)
        use panache::linter::diagnostics::Edit;

        let mut edits: Vec<&Edit> = diagnostics
            .iter()
            .filter_map(|d| d.fix.as_ref())
            .flat_map(|f| &f.edits)
            .collect();

        edits.sort_by_key(|e| e.range.start());

        let mut output = String::new();
        let mut last_end = 0;

        for edit in &edits {
            let start: usize = edit.range.start().into();
            let end: usize = edit.range.end().into();

            output.push_str(&input[last_end..start]);
            output.push_str(&edit.replacement);
            last_end = end;
        }

        output.push_str(&input[last_end..]);

        // Verify the fixes were applied correctly
        assert!(
            output.contains("anyNA(x)"),
            "Fix should replace any(is.na(x)) with anyNA(x)"
        );
        assert!(
            output.contains("anyNA(y)"),
            "Fix should replace any(is.na(y)) with anyNA(y)"
        );

        // Verify surrounding markdown is unchanged
        assert!(output.contains("# Test Document"));
        assert!(output.contains("Some text here."));
        assert!(output.contains("More text."));
        assert!(output.contains("```r"));
    }
}
