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

        // Auto-fixes should now be enabled!
        assert!(
            assignment_diags[0].fix.is_some(),
            "Auto-fixes should be enabled with byte offset mapping"
        );

        // Verify the fix is correct
        let fix = assignment_diags[0].fix.as_ref().unwrap();
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(fix.edits[0].replacement, "x <- 1");
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

        // Both should have fixes
        assert!(assignment_diags[0].fix.is_some());
        assert!(assignment_diags[1].fix.is_some());
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
x = 1
y = 2
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
        // The R code should now use <- instead of =
        assert!(
            output.contains("x <- 1"),
            "Fix should replace x = 1 with x <- 1"
        );
        assert!(
            output.contains("y <- 2"),
            "Fix should replace y = 2 with y <- 2"
        );

        // Verify surrounding markdown is unchanged
        assert!(output.contains("# Test Document"));
        assert!(output.contains("Some text here."));
        assert!(output.contains("More text."));
        assert!(output.contains("```r"));
    }
}
