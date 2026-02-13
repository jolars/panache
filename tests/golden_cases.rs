//! Golden test cases for panache formatter.
//!
//! Each test case is a directory under `tests/cases/` containing:
//! - `input.*` - Source file (`.md`, `.qmd`, or `.Rmd`)
//! - `expected.*` - Expected formatted output (same extension as input)
//! - `ast.txt` - (Optional) Expected AST structure for parse regression testing
//! - `panache.toml` - (Optional) Config to test specific flavors/extensions
//!
//! Run with `UPDATE_EXPECTED=1 cargo test` to regenerate expected outputs.
//! Run with `UPDATE_AST=1 cargo test` to regenerate AST files.
//! Run with both flags to update both: `UPDATE_EXPECTED=1 UPDATE_AST=1 cargo test`.

use panache::{Config, format, parse};
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

/// Run a single golden test case.
fn run_golden_case(case_name: &str) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("cases")
        .join(case_name);

    let update_expected = std::env::var_os("UPDATE_EXPECTED").is_some();
    let update_ast = std::env::var_os("UPDATE_AST").is_some();

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

    // Read input file - preserve line endings exactly
    let input = fs::read_to_string(&input_path).unwrap();

    // Test losslessness: parser must preserve input byte-for-byte
    // This is critical for LSP, linting, and range formatting
    let ast = parse(&input, config.clone());
    let tree_text = ast.text().to_string();

    // Use similar_asserts for nice diff output showing exactly where bytes are lost
    similar_asserts::assert_eq!(
        input,
        tree_text,
        "losslessness check failed for {} (tree text does not match input, diff: {:+} bytes)",
        case_name,
        tree_text.len() as i64 - input.len() as i64
    );

    // Test formatting
    let output = format(&input, config.clone(), None);

    // Idempotency: formatting twice should equal once
    let output_twice = format(&output, config.clone(), None);
    similar_asserts::assert_eq!(output, output_twice, "idempotency: {}", case_name);

    // Test AST parsing (if ast.txt exists or we're updating AST)
    if ast_path.exists() || update_ast {
        let ast_output = format!("{:#?}\n", ast);

        if update_ast {
            fs::write(&ast_path, &ast_output).unwrap();
        } else {
            let expected_ast = fs::read_to_string(&ast_path)
                .unwrap_or_else(|_| panic!("Failed to read ast.txt in {}", case_name));
            similar_asserts::assert_eq!(expected_ast, ast_output, "AST mismatch: {}", case_name);
        }
    }

    if update_expected {
        fs::write(&expected_path, &output).unwrap();
        return;
    }

    let expected = fs::read_to_string(&expected_path).unwrap_or_else(|_| input.clone());

    similar_asserts::assert_eq!(expected, output, "case: {}", case_name);
}

/// Macro to generate individual test functions for each golden case.
///
/// Usage: `golden_test_cases!(case1, case2, case3);`
///
/// This generates separate test functions named `golden_case1`, `golden_case2`, etc.
/// Each test runs independently, so failures don't stop other tests from running.
macro_rules! golden_test_cases {
    ($($case:ident),+ $(,)?) => {
        $(
            #[test]
            fn $case() {
                run_golden_case(stringify!($case));
            }
        )+
    };
}

// Generate test functions for each case directory.
// To add a new test case:
// 1. Create a new directory under tests/cases/
// 2. Add the directory name to this list
golden_test_cases!(
    blockquotes,
    bracketed_spans,
    code_spans,
    crlf_basic,
    crlf_code_blocks,
    crlf_definition_lists,
    crlf_display_math,
    crlf_headerless_table,
    crlf_horizontal_rules,
    crlf_raw_blocks,
    crlf_yaml_metadata,
    crlf_fenced_divs,
    definition_list,
    definition_list_nesting,
    display_math,
    emphasis,
    escapes,
    fenced_code,
    fenced_code_quarto,
    fenced_divs,
    headerless_table,
    horizontal_rules,
    html_block,
    images,
    indented_code,
    inline_math,
    latex_environment,
    line_ending_crlf,
    line_ending_lf,
    links,
    lists_bullet,
    lists_code,
    lists_example,
    lists_fancy,
    lists_nested,
    lists_ordered,
    lists_task,
    lists_wrapping_nested,
    lists_wrapping_simple,
    pandoc_title_block,
    paragraph_wrapping,
    paragraphs,
    pipe_table,
    pipe_table_unicode,
    quarto_code_blocks,
    quarto_hashpipe,
    raw_blocks,
    reference_footnotes,
    reference_images,
    reference_links,
    rmarkdown_math,
    simple_table,
    standardize_bullets,
    table_with_caption,
    yaml_metadata,
);
