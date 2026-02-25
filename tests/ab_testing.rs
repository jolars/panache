//! A/B testing harness for parser refactoring.
//!
//! This module provides utilities to run golden tests with both the legacy
//! two-pass parser (use_integrated_inline_parsing=false) and the new
//! integrated inline parser (use_integrated_inline_parsing=true).
//!
//! This ensures that the migration doesn't change behavior.

use panache::{Config, ConfigBuilder, format, parse};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Find a file with given base name and any supported extension.
fn find_file_with_extension(dir: &Path, base: &str) -> Option<PathBuf> {
    for ext in &["md", "qmd", "Rmd"] {
        let path = dir.join(format!("{}.{}", base, ext));
        if path.exists() {
            return Some(path);
        }
    }
    None
}

/// Load config from test case directory if it exists.
fn load_test_config(dir: &Path) -> Option<Config> {
    let config_path = dir.join("panache.toml");
    if config_path.exists() {
        let content = fs::read_to_string(config_path).ok()?;
        toml::from_str(&content).ok()
    } else {
        None
    }
}

/// Run a golden test case with both parser configurations (A/B testing).
///
/// This function:
/// 1. Runs the test with use_integrated_inline_parsing=false (legacy)
/// 2. Runs the test with use_integrated_inline_parsing=true (new)
/// 3. Compares CST structures (must be identical)
/// 4. Compares formatted output (must be identical)
///
/// # Arguments
/// * `case_name` - Name of the test case directory under `tests/cases/`
///
/// # Panics
/// - If CST structures differ between old and new parser
/// - If formatted outputs differ between old and new parser
/// - If either parser fails losslessness check
/// - If either parser fails idempotency check
pub fn run_ab_test(case_name: &str) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("cases")
        .join(case_name);

    // Find input file
    let input_path = find_file_with_extension(&dir, "input")
        .unwrap_or_else(|| panic!("No input file found in {}", case_name));

    // Load base config (if any)
    let base_config = load_test_config(&dir);

    // Read input
    let input = fs::read_to_string(&input_path).unwrap();

    // Test with legacy parser (use_integrated_inline_parsing=false)
    let config_old = base_config
        .clone()
        .map(|c| {
            let mut c = c;
            c.parser.use_integrated_inline_parsing = false;
            c
        })
        .or_else(|| {
            Some(
                ConfigBuilder::default()
                    .use_integrated_inline_parsing(false)
                    .build(),
            )
        });

    let ast_old = parse(&input, config_old.clone());
    let tree_text_old = ast_old.text().to_string();
    let output_old = format(&input, config_old.clone(), None);
    let output_old_twice = format(&output_old, config_old.clone(), None);

    // Test losslessness with old parser
    similar_asserts::assert_eq!(
        input,
        tree_text_old,
        "[OLD PARSER] losslessness check failed for {}",
        case_name
    );

    // Test idempotency with old parser
    similar_asserts::assert_eq!(
        output_old,
        output_old_twice,
        "[OLD PARSER] idempotency check failed for {}",
        case_name
    );

    // Test with new parser (use_integrated_inline_parsing=true)
    let config_new = base_config
        .map(|c| {
            let mut c = c;
            c.parser.use_integrated_inline_parsing = true;
            c
        })
        .or_else(|| {
            Some(
                ConfigBuilder::default()
                    .use_integrated_inline_parsing(true)
                    .build(),
            )
        });

    let ast_new = parse(&input, config_new.clone());
    let tree_text_new = ast_new.text().to_string();
    let output_new = format(&input, config_new.clone(), None);
    let output_new_twice = format(&output_new, config_new.clone(), None);

    // Test losslessness with new parser
    similar_asserts::assert_eq!(
        input,
        tree_text_new,
        "[NEW PARSER] losslessness check failed for {}",
        case_name
    );

    // Test idempotency with new parser
    similar_asserts::assert_eq!(
        output_new,
        output_new_twice,
        "[NEW PARSER] idempotency check failed for {}",
        case_name
    );

    // A/B comparison: CST structure must be identical
    let cst_old = format!("{:#?}", ast_old);
    let cst_new = format!("{:#?}", ast_new);
    similar_asserts::assert_eq!(
        cst_old,
        cst_new,
        "[A/B TEST] CST structure differs between old and new parser for {}",
        case_name
    );

    // A/B comparison: formatted output must be identical
    similar_asserts::assert_eq!(
        output_old,
        output_new,
        "[A/B TEST] formatted output differs between old and new parser for {}",
        case_name
    );

    eprintln!("✓ A/B test passed for {}", case_name);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Example A/B test - currently will pass because integrated inline parsing
    /// isn't actually used yet (Phase 3).
    ///
    /// Once we start migrating blocks in Phase 3, this test will verify that
    /// the migration doesn't change behavior.
    #[test]
    fn ab_test_blockquotes() {
        run_ab_test("blockquotes");
    }

    // Add more A/B tests as we migrate blocks:
    // - Headings (when Phase 3 migrates headings)
    // - Tables (when Phase 3 migrates table cells)
    // - etc.
}
