//! Integration tests for external linter support

#[cfg(test)]
mod tests {
    use panache::{Config, linter, parse};
    use std::collections::HashMap;

    #[test]
    fn test_jarl_linter_integration() {
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
        let diagnostics = linter::lint_with_external_sync(&tree, input, &config);

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

    #[test]
    fn test_arity_linter_integration() {
        // Skip if arity not available
        if which::which("arity").is_err() {
            println!("Skipping arity test - arity not installed");
            return;
        }

        let input = r#"# Test

```r
any(is.na(x))
result <- TRUE
```
"#;

        let mut config = Config::default();
        let mut linters = HashMap::new();
        linters.insert("r".to_string(), "arity".to_string());
        config.linters = linters;

        let tree = parse(input, Some(config.clone()));
        let diagnostics = linter::lint_with_external_sync(&tree, input, &config);

        assert!(!diagnostics.is_empty(), "Expected diagnostics from arity");

        let any_is_na_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == "any-is-na")
            .collect();
        assert_eq!(any_is_na_diags.len(), 1, "Expected 1 any-is-na diagnostic");

        assert_eq!(any_is_na_diags[0].location.line, 4); // any(is.na(x)) is on line 4

        let fix = any_is_na_diags[0]
            .fix
            .as_ref()
            .expect("arity fixes should map through block mappings");
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(fix.edits[0].replacement, "anyNA(x)");
        // The fix edit must target the code inside the block, not the fences.
        let start: usize = fix.edits[0].range.start().into();
        let end: usize = fix.edits[0].range.end().into();
        assert_eq!(&input[start..end], "any(is.na(x))");
    }

    #[test]
    fn test_fatou_linter_integration() {
        // Two julia blocks exercise concatenation and line mapping; the second
        // block carries a fixable violation.
        if which::which("fatou").is_err() {
            println!("Skipping fatou test - fatou not installed");
            return;
        }

        let input = r#"# Test

```julia
import Printf
x = 1
```

Some text.

```julia
if x == nothing
    y = 1
end
```
"#;

        let mut config = Config::default();
        let mut linters = HashMap::new();
        linters.insert("julia".to_string(), "fatou".to_string());
        config.linters = linters;

        let tree = parse(input, Some(config.clone()));
        let diagnostics = linter::lint_with_external_sync(&tree, input, &config);

        let unused_import_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == "unused-import")
            .collect();
        assert_eq!(
            unused_import_diags.len(),
            1,
            "Expected 1 unused-import diagnostic"
        );
        assert_eq!(unused_import_diags[0].location.line, 4); // import Printf is on line 4

        let nothing_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == "nothing-comparison")
            .collect();
        assert_eq!(
            nothing_diags.len(),
            1,
            "Expected 1 nothing-comparison diagnostic"
        );
        assert_eq!(nothing_diags[0].location.line, 11); // x == nothing is on line 11

        let fix = nothing_diags[0]
            .fix
            .as_ref()
            .expect("fatou fixes should map through block mappings");
        assert_eq!(fix.edits[0].replacement, "===");
        let start: usize = fix.edits[0].range.start().into();
        let end: usize = fix.edits[0].range.end().into();
        assert_eq!(&input[start..end], "==");
    }

    /// True when the installed badness supports `--output json` (added in
    /// 0.14). The runner silently skips failing linters, so without this probe
    /// an old binary would make the assertions below vacuous rather than
    /// skipped.
    fn badness_supports_json() -> bool {
        std::process::Command::new("badness")
            .args(["lint", "--help"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("--output"))
            .unwrap_or(false)
    }

    #[test]
    fn test_badness_linter_integration() {
        if which::which("badness").is_err() {
            println!("Skipping badness test - badness not installed");
            return;
        }
        if !badness_supports_json() {
            println!("Skipping badness test - installed badness lacks --output json");
            return;
        }

        let input = r#"# Test

```latex
Wait ... what
```
"#;

        let mut config = Config::default();
        let mut linters = HashMap::new();
        linters.insert("latex".to_string(), "badness".to_string());
        config.linters = linters;

        let tree = parse(input, Some(config.clone()));
        let diagnostics = linter::lint_with_external_sync(&tree, input, &config);

        let ellipsis_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == "ellipsis")
            .collect();
        assert_eq!(ellipsis_diags.len(), 1, "Expected 1 ellipsis diagnostic");
        assert_eq!(ellipsis_diags[0].location.line, 4); // Wait ... what is on line 4

        let fix = ellipsis_diags[0]
            .fix
            .as_ref()
            .expect("badness fixes should map through block mappings");
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(fix.edits[0].replacement, "\\dots");
        let start: usize = fix.edits[0].range.start().into();
        let end: usize = fix.edits[0].range.end().into();
        assert_eq!(&input[start..end], "...");
    }

    #[test]
    fn test_fatou_quarto_cell_integration() {
        // A Quarto executable cell (```{julia}) should route to the julia linter.
        if which::which("fatou").is_err() {
            println!("Skipping fatou test - fatou not installed");
            return;
        }

        use panache::config::{Extensions, Flavor};

        let input = "# Test\n\n```{julia}\nimport Printf\nx = 1\n```\n";

        let mut config = Config {
            flavor: Flavor::Quarto,
            extensions: Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        };
        let mut linters = HashMap::new();
        linters.insert("julia".to_string(), "fatou".to_string());
        config.linters = linters;

        let tree = parse(input, Some(config.clone()));
        let diagnostics = linter::lint_with_external_sync(&tree, input, &config);

        let unused_import_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == "unused-import")
            .collect();
        assert_eq!(
            unused_import_diags.len(),
            1,
            "Expected 1 unused-import diagnostic in the Quarto cell"
        );
        assert_eq!(unused_import_diags[0].location.line, 4);
    }

    #[test]
    fn test_multiple_r_blocks_concatenation() {
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
        let diagnostics = linter::lint_with_external_sync(&tree, input, &config);

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

    #[test]
    fn test_myst_directive_body_linted() {
        // A verbatim MyST `{code-block}` body should be routed to the external
        // linter keyed by the directive argument, with diagnostics mapped back
        // onto the body's source line.
        if which::which("ruff").is_err() {
            println!("Skipping ruff test - ruff not installed");
            return;
        }

        use panache::config::{Extensions, Flavor};

        let input = "# Test\n\n```{code-block} python\nimport os\n```\n";

        let mut config = Config {
            flavor: Flavor::Myst,
            extensions: Extensions::for_flavor(Flavor::Myst),
            ..Default::default()
        };
        let mut linters = HashMap::new();
        linters.insert("python".to_string(), "ruff".to_string());
        config.linters = linters;

        let tree = parse(input, Some(config.clone()));
        let diagnostics = linter::lint_with_external_sync(&tree, input, &config);

        let ruff_diags: Vec<_> = diagnostics.iter().filter(|d| d.code == "F401").collect();
        assert_eq!(ruff_diags.len(), 1, "Expected 1 Ruff F401 diagnostic");
        assert_eq!(ruff_diags[0].location.line, 4); // `import os` is on line 4
    }

    #[test]
    fn test_no_external_linters_configured() {
        let input = r#"```r
x = 1
```
"#;

        let config = Config::default(); // No linters configured

        let tree = parse(input, Some(config.clone()));
        let diagnostics = linter::lint_with_external_sync(&tree, input, &config);

        // Should only have built-in rule diagnostics (if any)
        // No jarl diagnostics
        let external_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == "any_is_na")
            .collect();
        assert_eq!(external_diags.len(), 0);
    }

    #[test]
    fn test_ruff_linter_integration() {
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
        let diagnostics = linter::lint_with_external_sync(&tree, input, &config);

        let ruff_diags: Vec<_> = diagnostics.iter().filter(|d| d.code == "F401").collect();
        assert_eq!(ruff_diags.len(), 1, "Expected 1 Ruff F401 diagnostic");

        assert_eq!(ruff_diags[0].location.line, 4); // import os is on line 4
        assert_eq!(
            ruff_diags[0].origin,
            panache::linter::diagnostics::DiagnosticOrigin::External
        );
        assert!(ruff_diags[0].fix.is_some(), "Ruff fixes should be enabled");
    }

    #[test]
    fn test_ruff_fix_application_end_to_end() {
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
        let diagnostics = linter::lint_with_external_sync(&tree, input, &config);

        let with_fixes: Vec<_> = diagnostics.iter().filter(|d| d.fix.is_some()).collect();
        assert!(!with_fixes.is_empty(), "Expected at least one Ruff fix");

        // What this test guards is the offset mapping: ruff reports
        // line/column inside the extracted chunk, and those have to come back
        // as ranges into the *document*. Ruff may report several rules for the
        // unused import (0.16 emits both `F401` and `I001`); each one's edits
        // must land on the `import os\n` line, never on the fence or the
        // surrounding Markdown.
        let import_line = input.find("import os").expect("fixture has the import");
        let import_end = import_line + "import os\n".len();

        let edits: Vec<_> = diagnostics
            .iter()
            .filter_map(|d| d.fix.as_ref())
            .flat_map(|f| &f.edits)
            .collect();
        assert!(!edits.is_empty(), "Expected at least one Ruff edit");

        for edit in &edits {
            let start: usize = edit.range.start().into();
            let end: usize = edit.range.end().into();
            assert!(
                start >= import_line && end <= import_end,
                "edit {start}..{end} escaped the import line ({import_line}..{import_end})"
            );
        }

        // Applying any one of those edits on its own removes the import from
        // the document. (Applying *all* of them is the caller's job: ruff's
        // two fixes overlap, so the CLI applies one per pass --- see the
        // `apply_fixes` unit tests in `src/main.rs`.)
        let removes_import = edits.iter().any(|edit| {
            let start: usize = edit.range.start().into();
            let end: usize = edit.range.end().into();
            let patched = format!("{}{}{}", &input[..start], edit.replacement, &input[end..]);
            !patched.contains("import os")
        });
        assert!(
            removes_import,
            "no Ruff fix removed the unused import: {:?}",
            edits.iter().map(|e| &e.replacement).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_shellcheck_linter_integration() {
        if which::which("shellcheck").is_err() {
            println!("Skipping shellcheck test - shellcheck not installed");
            return;
        }

        let input = r#"# Test

```sh
echo $UNSET
```
"#;

        let mut config = Config::default();
        let mut linters = HashMap::new();
        linters.insert("sh".to_string(), "shellcheck".to_string());
        config.linters = linters;

        let tree = parse(input, Some(config.clone()));
        let diagnostics = linter::lint_with_external_sync(&tree, input, &config);

        let shell_diags: Vec<_> = diagnostics.iter().filter(|d| d.code == "SC2086").collect();
        assert_eq!(
            shell_diags.len(),
            1,
            "Expected 1 ShellCheck SC2086 diagnostic"
        );
        assert_eq!(shell_diags[0].location.line, 4); // echo $UNSET on line 4
        assert_eq!(
            shell_diags[0].severity,
            panache::linter::diagnostics::Severity::Info
        );
        assert!(
            shell_diags[0].fix.is_some(),
            "ShellCheck fixes should be enabled"
        );
    }

    /// Exercises the multi-language fan-out path (two configured linters in one
    /// document → the parallel branch in `LintRunner`). Asserts both linters'
    /// diagnostics survive the merge with their per-language line mapping
    /// intact, regardless of completion order.
    #[test]
    fn test_multi_language_linters_run_in_parallel() {
        if which::which("ruff").is_err() || which::which("shellcheck").is_err() {
            println!("Skipping multi-language test - ruff and/or shellcheck not installed");
            return;
        }

        let input = r#"# Test

```python
import os
```

```sh
echo $UNSET
```
"#;

        let mut config = Config::default();
        let mut linters = HashMap::new();
        linters.insert("python".to_string(), "ruff".to_string());
        linters.insert("sh".to_string(), "shellcheck".to_string());
        config.linters = linters;

        let tree = parse(input, Some(config.clone()));
        let diagnostics = linter::lint_with_external_sync(&tree, input, &config);

        let ruff_diags: Vec<_> = diagnostics.iter().filter(|d| d.code == "F401").collect();
        assert_eq!(ruff_diags.len(), 1, "Expected 1 Ruff F401 diagnostic");
        assert_eq!(ruff_diags[0].location.line, 4); // import os

        let shell_diags: Vec<_> = diagnostics.iter().filter(|d| d.code == "SC2086").collect();
        assert_eq!(
            shell_diags.len(),
            1,
            "Expected 1 ShellCheck SC2086 diagnostic"
        );
        assert_eq!(shell_diags[0].location.line, 8); // echo $UNSET
    }

    #[test]
    fn test_shellcheck_sc2148_not_reported_when_shell_is_known() {
        if which::which("shellcheck").is_err() {
            println!("Skipping shellcheck test - shellcheck not installed");
            return;
        }

        let input = r#"# External Linter Playground

```sh
echo "hello"
```
"#;

        let mut config = Config::default();
        let mut linters = HashMap::new();
        linters.insert("sh".to_string(), "shellcheck".to_string());
        config.linters = linters;

        let tree = parse(input, Some(config.clone()));
        let diagnostics = linter::lint_with_external_sync(&tree, input, &config);

        let sc2148: Vec<_> = diagnostics.iter().filter(|d| d.code == "SC2148").collect();
        assert!(
            sc2148.is_empty(),
            "SC2148 should be suppressed by passing --shell for known shell languages"
        );
    }

    #[test]
    fn test_shellcheck_fix_application_end_to_end() {
        if which::which("shellcheck").is_err() {
            println!("Skipping shellcheck test - shellcheck not installed");
            return;
        }

        let input = r#"# Test

```sh
echo $UNSET
```
"#;

        let mut config = Config::default();
        let mut linters = HashMap::new();
        linters.insert("sh".to_string(), "shellcheck".to_string());
        config.linters = linters;

        let tree = parse(input, Some(config.clone()));
        let diagnostics = linter::lint_with_external_sync(&tree, input, &config);

        let with_fixes: Vec<_> = diagnostics.iter().filter(|d| d.fix.is_some()).collect();
        assert!(
            !with_fixes.is_empty(),
            "Expected at least one ShellCheck fix"
        );

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

        assert!(output.contains("echo \"$UNSET\""));
        assert!(output.contains("```sh"));
    }

    #[test]
    fn test_eslint_linter_integration() {
        if which::which("eslint").is_err() {
            println!("Skipping eslint test - eslint not installed");
            return;
        }

        let input = r#"# Test

```js
const x = 1;
console.log(1)
```
"#;

        let mut config = Config::default();
        let mut linters = HashMap::new();
        linters.insert("js".to_string(), "eslint".to_string());
        config.linters = linters;

        let tree = parse(input, Some(config.clone()));
        let diagnostics = linter::lint_with_external_sync(&tree, input, &config);

        let eslint_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == "no-unused-vars")
            .collect();
        assert_eq!(
            eslint_diags.len(),
            1,
            "Expected 1 ESLint no-unused-vars diagnostic"
        );
        assert_eq!(eslint_diags[0].location.line, 4);
        assert!(
            eslint_diags[0].fix.is_some(),
            "Expected ESLint fix or suggestion mapping"
        );
    }

    #[test]
    fn test_eslint_fix_application_end_to_end() {
        if which::which("eslint").is_err() {
            println!("Skipping eslint test - eslint not installed");
            return;
        }

        let input = r#"# Test

```js
const x = 1;
console.log(1)
```
"#;

        let mut config = Config::default();
        let mut linters = HashMap::new();
        linters.insert("js".to_string(), "eslint".to_string());
        config.linters = linters;

        let tree = parse(input, Some(config.clone()));
        let diagnostics = linter::lint_with_external_sync(&tree, input, &config);

        let with_fixes: Vec<_> = diagnostics.iter().filter(|d| d.fix.is_some()).collect();
        assert!(
            !with_fixes.is_empty(),
            "Expected at least one ESLint fix or suggestion"
        );

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

        assert!(!output.contains("const x = 1;"));
        assert!(output.contains("console.log(1)"));
        assert!(output.contains("```js"));
    }

    #[test]
    fn test_staticcheck_linter_integration() {
        if which::which("staticcheck").is_err() || which::which("go").is_err() {
            println!("Skipping staticcheck test - staticcheck and/or go not installed");
            return;
        }

        let input = r#"# Test

```go
package main
import "fmt"
func main() {
    fmt.Printf("%d", "x")
}
```
"#;

        let mut config = Config::default();
        let mut linters = HashMap::new();
        linters.insert("go".to_string(), "staticcheck".to_string());
        config.linters = linters;

        let tree = parse(input, Some(config.clone()));
        let diagnostics = linter::lint_with_external_sync(&tree, input, &config);

        // Ensure we don't surface fallback package-level compile diagnostics caused
        // by bad temp-file naming/placement.
        assert!(
            diagnostics.iter().all(|d| d.code != "compile"),
            "Staticcheck should run against the generated Go file, not package fallback"
        );
        assert!(
            diagnostics.iter().any(|d| d.code == "SA5009"),
            "Expected staticcheck code-level diagnostic for mismatched Printf format"
        );
    }

    #[test]
    fn test_clippy_linter_integration() {
        if which::which("clippy-driver").is_err() {
            println!("Skipping clippy test - clippy-driver not installed");
            return;
        }

        let input = r#"# Test

```rust
fn main() {
    let x = vec![1,2,3];
    println!("{}", x.len());
}
```
"#;

        let mut config = Config::default();
        let mut linters = HashMap::new();
        linters.insert("rust".to_string(), "clippy".to_string());
        config.linters = linters;

        let tree = parse(input, Some(config.clone()));
        let diagnostics = linter::lint_with_external_sync(&tree, input, &config);

        if diagnostics.is_empty() {
            println!(
                "Skipping strict clippy assertion - clippy produced no diagnostics in this environment"
            );
            return;
        }

        assert!(
            diagnostics
                .iter()
                .any(|d| d.code.starts_with("clippy::") || d.code == "clippy"),
            "Expected clippy diagnostic code in rust block"
        );
    }

    #[test]
    fn test_unknown_linter() {
        let input = r#"```r
x <- 1
```
"#;

        let mut config = Config::default();
        let mut linters = HashMap::new();
        linters.insert("r".to_string(), "unknown_linter_12345".to_string());
        config.linters = linters;

        let tree = parse(input, Some(config.clone()));
        let _diagnostics = linter::lint_with_external_sync(&tree, input, &config);

        // Should handle gracefully - just skip external linting
        // Test passes if no panic occurs
    }

    #[test]
    fn test_unsupported_linter_language_mapping_is_skipped() {
        let input = r#"# Test

```python
import os
```
"#;

        let mut config = Config::default();
        let mut linters = HashMap::new();
        linters.insert("python".to_string(), "jarl".to_string());
        config.linters = linters;

        let tree = parse(input, Some(config.clone()));
        let diagnostics = linter::lint_with_external_sync(&tree, input, &config);

        // External linter mapping is unsupported, so no external diagnostics should appear.
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != "any_is_na" && d.code != "F401"),
            "Expected unsupported linter-language mapping to be skipped"
        );
    }

    #[test]
    fn test_fix_application_end_to_end() {
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
        let diagnostics = linter::lint_with_external_sync(&tree, input, &config);

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

    #[test]
    fn test_arity_fix_application_end_to_end() {
        // Same end-to-end fix application flow as the jarl test above, but for
        // arity, whose diagnostics carry byte-offset ranges rather than
        // line/column positions.
        if which::which("arity").is_err() {
            println!("Skipping arity test - arity not installed");
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
        linters.insert("r".to_string(), "arity".to_string());
        config.linters = linters;

        let tree = parse(input, Some(config.clone()));
        let diagnostics = linter::lint_with_external_sync(&tree, input, &config);

        let with_fixes: Vec<_> = diagnostics.iter().filter(|d| d.fix.is_some()).collect();
        assert!(!with_fixes.is_empty(), "Expected at least one fix");

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
            output.contains("anyNA(x)"),
            "Fix should replace any(is.na(x)) with anyNA(x)"
        );
        assert!(
            output.contains("anyNA(y)"),
            "Fix should replace any(is.na(y)) with anyNA(y)"
        );

        assert!(output.contains("# Test Document"));
        assert!(output.contains("Some text here."));
        assert!(output.contains("More text."));
        assert!(output.contains("```r"));
    }
}
