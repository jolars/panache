//! Golden test cases for panache formatter.
//!
//! Each test case is a directory under `tests/cases/` containing:
//! - `input.*` - Source file (`.md`, `.qmd`, or `.Rmd`)
//! - `expected.*` - Expected formatted output (same extension as input)
//! - `ast.txt` - (Optional) Expected AST structure for parse regression testing
//! - `panache.toml` - (Optional) Config to test specific flavors/extensions
//!
//! Run with `UPDATE_EXPECTED=1 cargo test` to regenerate expected outputs and AST files.

use panache::{Config, format, parse};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn normalize(s: &str) -> String {
    s.replace("\r\n", "\n")
}

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

#[test]
fn golden_cases() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("cases");

    let mut entries: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    let update = std::env::var_os("UPDATE_EXPECTED").is_some();

    for entry in entries {
        let dir = entry.path();
        let case_name = dir.file_name().unwrap().to_string_lossy();

        // Find input file with any supported extension
        let input_path = find_file_with_extension(&dir, "input")
            .unwrap_or_else(|| panic!("No input file found in {}", case_name));

        // Determine expected path based on input extension
        let input_ext = input_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("qmd");
        let expected_path = dir.join(format!("expected.{}", input_ext));

        let ast_path = dir.join("ast.txt");

        // Load optional config
        let config = load_test_config(&dir);

        let input = normalize(&fs::read_to_string(&input_path).unwrap());

        // Test formatting
        let output = format(&input, config.clone());

        // Idempotency: formatting twice should equal once
        let output_twice = format(&output, config.clone());
        similar_asserts::assert_eq!(output, output_twice, "idempotency: {}", case_name);

        // Test AST parsing (if ast.txt exists or we're updating)
        if ast_path.exists() || update {
            let ast = parse(&input, config.clone());
            let ast_output = format!("{:#?}\n", ast);

            if update {
                fs::write(&ast_path, &ast_output).unwrap();
            } else {
                let expected_ast = fs::read_to_string(&ast_path)
                    .unwrap_or_else(|_| panic!("Failed to read ast.txt in {}", case_name));
                similar_asserts::assert_eq!(
                    normalize(&expected_ast),
                    normalize(&ast_output),
                    "AST mismatch: {}",
                    case_name
                );
            }
        }

        if update {
            fs::write(&expected_path, &output).unwrap();
            continue;
        }

        let expected = fs::read_to_string(&expected_path)
            .map(|s| normalize(&s))
            .unwrap_or_else(|_| input.clone());

        similar_asserts::assert_eq!(expected, output, "case: {}", case_name);
    }
}
