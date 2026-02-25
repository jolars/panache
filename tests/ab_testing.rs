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

    // ===== Comprehensive A/B Test Coverage (98 tests) =====
    // Each test verifies that old parser (flag=false) produces identical
    // output to new parser (flag=true) for all golden test cases.

    /// A/B test: Blankline Concatenation
    #[test]
    fn ab_test_blankline_concatenation() {
        run_ab_test("blankline_concatenation");
    }

    /// A/B test: Blockquote Depth Change
    #[test]
    fn ab_test_blockquote_depth_change() {
        run_ab_test("blockquote_depth_change");
    }

    /// A/B test: Blockquote List Blanks
    #[test]
    fn ab_test_blockquote_list_blanks() {
        run_ab_test("blockquote_list_blanks");
    }

    /// A/B test: Blockquote List Blockquote
    #[test]
    fn ab_test_blockquote_list_blockquote() {
        run_ab_test("blockquote_list_blockquote");
    }

    /// A/B test: Blockquotes
    #[test]
    fn ab_test_blockquotes() {
        run_ab_test("blockquotes");
    }

    /// A/B test: Bracketed Spans
    #[test]
    fn ab_test_bracketed_spans() {
        run_ab_test("bracketed_spans");
    }

    /// A/B test: Chunk Options Complex
    #[test]
    fn ab_test_chunk_options_complex() {
        run_ab_test("chunk_options_complex");
    }

    /// A/B test: Code Blocks Executable
    #[test]
    fn ab_test_code_blocks_executable() {
        run_ab_test("code_blocks_executable");
    }

    /// A/B test: Code Blocks Explicit Style
    #[test]
    fn ab_test_code_blocks_explicit_style() {
        run_ab_test("code_blocks_explicit_style");
    }

    /// A/B test: Code Blocks Raw
    #[test]
    fn ab_test_code_blocks_raw() {
        run_ab_test("code_blocks_raw");
    }

    /// A/B test: Code Blocks Shortcut Style
    #[test]
    fn ab_test_code_blocks_shortcut_style() {
        run_ab_test("code_blocks_shortcut_style");
    }

    /// A/B test: Code Spans
    #[test]
    fn ab_test_code_spans() {
        run_ab_test("code_spans");
    }

    /// A/B test: Crlf Basic
    #[test]
    fn ab_test_crlf_basic() {
        run_ab_test("crlf_basic");
    }

    /// A/B test: Crlf Code Blocks
    #[test]
    fn ab_test_crlf_code_blocks() {
        run_ab_test("crlf_code_blocks");
    }

    /// A/B test: Crlf Definition Lists
    #[test]
    fn ab_test_crlf_definition_lists() {
        run_ab_test("crlf_definition_lists");
    }

    /// A/B test: Crlf Display Math
    #[test]
    fn ab_test_crlf_display_math() {
        run_ab_test("crlf_display_math");
    }

    /// A/B test: Crlf Fenced Divs
    #[test]
    fn ab_test_crlf_fenced_divs() {
        run_ab_test("crlf_fenced_divs");
    }

    /// A/B test: Crlf Headerless Table
    #[test]
    fn ab_test_crlf_headerless_table() {
        run_ab_test("crlf_headerless_table");
    }

    /// A/B test: Crlf Horizontal Rules
    #[test]
    fn ab_test_crlf_horizontal_rules() {
        run_ab_test("crlf_horizontal_rules");
    }

    /// A/B test: Crlf Line Endings
    #[test]
    fn ab_test_crlf_line_endings() {
        run_ab_test("crlf_line_endings");
    }

    /// A/B test: Crlf Raw Blocks
    #[test]
    fn ab_test_crlf_raw_blocks() {
        run_ab_test("crlf_raw_blocks");
    }

    /// A/B test: Crlf Yaml Metadata
    #[test]
    fn ab_test_crlf_yaml_metadata() {
        run_ab_test("crlf_yaml_metadata");
    }

    /// A/B test: Definition List
    #[test]
    fn ab_test_definition_list() {
        run_ab_test("definition_list");
    }

    /// A/B test: Definition List Nesting
    #[test]
    fn ab_test_definition_list_nesting() {
        run_ab_test("definition_list_nesting");
    }

    /// A/B test: Definition List Wrapping
    #[test]
    fn ab_test_definition_list_wrapping() {
        run_ab_test("definition_list_wrapping");
    }

    /// A/B test: Display Math
    #[test]
    fn ab_test_display_math() {
        run_ab_test("display_math");
    }

    /// A/B test: Display Math Blank Line Termination
    #[test]
    fn ab_test_display_math_blank_line_termination() {
        run_ab_test("display_math_blank_line_termination");
    }

    /// A/B test: Display Math Content On Fence Line
    #[test]
    fn ab_test_display_math_content_on_fence_line() {
        run_ab_test("display_math_content_on_fence_line");
    }

    /// A/B test: Display Math Escaped Dollar
    #[test]
    fn ab_test_display_math_escaped_dollar() {
        run_ab_test("display_math_escaped_dollar");
    }

    /// A/B test: Display Math Trailing Text
    #[test]
    fn ab_test_display_math_trailing_text() {
        run_ab_test("display_math_trailing_text");
    }

    /// A/B test: Double Backslash Math
    #[test]
    fn ab_test_double_backslash_math() {
        run_ab_test("double_backslash_math");
    }

    /// A/B test: Emphasis
    #[test]
    fn ab_test_emphasis() {
        run_ab_test("emphasis");
    }

    /// A/B test: Emphasis Complex
    #[test]
    fn ab_test_emphasis_complex() {
        run_ab_test("emphasis_complex");
    }

    /// A/B test: Emphasis Nested Inlines
    #[test]
    fn ab_test_emphasis_nested_inlines() {
        run_ab_test("emphasis_nested_inlines");
    }

    /// A/B test: Equation Attributes
    #[test]
    fn ab_test_equation_attributes() {
        run_ab_test("equation_attributes");
    }

    /// A/B test: Equation Attributes Disabled
    #[test]
    fn ab_test_equation_attributes_disabled() {
        run_ab_test("equation_attributes_disabled");
    }

    /// A/B test: Equation Attributes Single Line
    #[test]
    fn ab_test_equation_attributes_single_line() {
        run_ab_test("equation_attributes_single_line");
    }

    /// A/B test: Escapes
    #[test]
    fn ab_test_escapes() {
        run_ab_test("escapes");
    }

    /// A/B test: Fenced Code
    #[test]
    fn ab_test_fenced_code() {
        run_ab_test("fenced_code");
    }

    /// A/B test: Fenced Code Quarto
    #[test]
    fn ab_test_fenced_code_quarto() {
        run_ab_test("fenced_code_quarto");
    }

    /// A/B test: Fenced Divs
    #[test]
    fn ab_test_fenced_divs() {
        run_ab_test("fenced_divs");
    }

    /// A/B test: Footnote Definition List
    #[test]
    fn ab_test_footnote_definition_list() {
        run_ab_test("footnote_definition_list");
    }

    /// A/B test: Footnote Def Paragraph
    #[test]
    fn ab_test_footnote_def_paragraph() {
        run_ab_test("footnote_def_paragraph");
    }

    /// A/B test: Grid Table
    #[test]
    fn ab_test_grid_table() {
        run_ab_test("grid_table");
    }

    /// A/B test: Grid Table Caption Before
    #[test]
    fn ab_test_grid_table_caption_before() {
        run_ab_test("grid_table_caption_before");
    }

    /// A/B test: Headerless Table
    #[test]
    fn ab_test_headerless_table() {
        run_ab_test("headerless_table");
    }

    /// A/B test: Headings
    #[test]
    fn ab_test_headings() {
        run_ab_test("headings");
    }

    /// A/B test: Horizontal Rules
    #[test]
    fn ab_test_horizontal_rules() {
        run_ab_test("horizontal_rules");
    }

    /// A/B test: Html Block
    #[test]
    fn ab_test_html_block() {
        run_ab_test("html_block");
    }

    /// A/B test: Images
    #[test]
    fn ab_test_images() {
        run_ab_test("images");
    }

    /// A/B test: Indented Code
    #[test]
    fn ab_test_indented_code() {
        run_ab_test("indented_code");
    }

    /// A/B test: Inline Footnotes
    #[test]
    fn ab_test_inline_footnotes() {
        run_ab_test("inline_footnotes");
    }

    /// A/B test: Inline Math
    #[test]
    fn ab_test_inline_math() {
        run_ab_test("inline_math");
    }

    /// A/B test: Latex Environment
    #[test]
    fn ab_test_latex_environment() {
        run_ab_test("latex_environment");
    }

    /// A/B test: Lazy Continuation Deep
    #[test]
    fn ab_test_lazy_continuation_deep() {
        run_ab_test("lazy_continuation_deep");
    }

    /// A/B test: Line Blocks
    #[test]
    fn ab_test_line_blocks() {
        run_ab_test("line_blocks");
    }

    /// A/B test: Line Ending Crlf
    #[test]
    fn ab_test_line_ending_crlf() {
        run_ab_test("line_ending_crlf");
    }

    /// A/B test: Line Ending Lf
    #[test]
    fn ab_test_line_ending_lf() {
        run_ab_test("line_ending_lf");
    }

    /// A/B test: Links
    #[test]
    fn ab_test_links() {
        run_ab_test("links");
    }

    /// A/B test: Lists Bullet
    #[test]
    fn ab_test_lists_bullet() {
        run_ab_test("lists_bullet");
    }

    /// A/B test: Lists Code
    #[test]
    fn ab_test_lists_code() {
        run_ab_test("lists_code");
    }

    /// A/B test: Lists Example
    #[test]
    fn ab_test_lists_example() {
        run_ab_test("lists_example");
    }

    /// A/B test: Lists Fancy
    #[test]
    fn ab_test_lists_fancy() {
        run_ab_test("lists_fancy");
    }

    /// A/B test: Lists Nested
    #[test]
    fn ab_test_lists_nested() {
        run_ab_test("lists_nested");
    }

    /// A/B test: Lists Ordered
    #[test]
    fn ab_test_lists_ordered() {
        run_ab_test("lists_ordered");
    }

    /// A/B test: Lists Task
    #[test]
    fn ab_test_lists_task() {
        run_ab_test("lists_task");
    }

    /// A/B test: Lists Tight
    #[test]
    fn ab_test_lists_tight() {
        run_ab_test("lists_tight");
    }

    /// A/B test: Lists Wrapping Nested
    #[test]
    fn ab_test_lists_wrapping_nested() {
        run_ab_test("lists_wrapping_nested");
    }

    /// A/B test: Lists Wrapping Simple
    #[test]
    fn ab_test_lists_wrapping_simple() {
        run_ab_test("lists_wrapping_simple");
    }

    /// A/B test: Multiline Table Basic
    #[test]
    fn ab_test_multiline_table_basic() {
        run_ab_test("multiline_table_basic");
    }

    /// A/B test: Multiline Table Caption
    #[test]
    fn ab_test_multiline_table_caption() {
        run_ab_test("multiline_table_caption");
    }

    /// A/B test: Multiline Table Caption After
    #[test]
    fn ab_test_multiline_table_caption_after() {
        run_ab_test("multiline_table_caption_after");
    }

    /// A/B test: Multiline Table Headerless
    #[test]
    fn ab_test_multiline_table_headerless() {
        run_ab_test("multiline_table_headerless");
    }

    /// A/B test: Multiline Table Inline Formatting
    #[test]
    fn ab_test_multiline_table_inline_formatting() {
        run_ab_test("multiline_table_inline_formatting");
    }

    /// A/B test: Multiline Table Single Row
    #[test]
    fn ab_test_multiline_table_single_row() {
        run_ab_test("multiline_table_single_row");
    }

    /// A/B test: Pandoc Title Block
    #[test]
    fn ab_test_pandoc_title_block() {
        run_ab_test("pandoc_title_block");
    }

    /// A/B test: Paragraph Continuation
    #[test]
    fn ab_test_paragraph_continuation() {
        run_ab_test("paragraph_continuation");
    }

    /// A/B test: Paragraph Plain Mixed
    #[test]
    fn ab_test_paragraph_plain_mixed() {
        run_ab_test("paragraph_plain_mixed");
    }

    /// A/B test: Paragraphs
    #[test]
    fn ab_test_paragraphs() {
        run_ab_test("paragraphs");
    }

    /// A/B test: Paragraph Simple
    #[test]
    fn ab_test_paragraph_simple() {
        run_ab_test("paragraph_simple");
    }

    /// A/B test: Paragraph Wrapping
    #[test]
    fn ab_test_paragraph_wrapping() {
        run_ab_test("paragraph_wrapping");
    }

    /// A/B test: Pipe Table
    #[test]
    fn ab_test_pipe_table() {
        run_ab_test("pipe_table");
    }

    /// A/B test: Pipe Table Caption Before
    #[test]
    fn ab_test_pipe_table_caption_before() {
        run_ab_test("pipe_table_caption_before");
    }

    /// A/B test: Pipe Table Unicode
    #[test]
    fn ab_test_pipe_table_unicode() {
        run_ab_test("pipe_table_unicode");
    }

    /// A/B test: Plain Continuation Edge Cases
    #[test]
    fn ab_test_plain_continuation_edge_cases() {
        run_ab_test("plain_continuation_edge_cases");
    }

    /// A/B test: Quarto Code Blocks
    #[test]
    fn ab_test_quarto_code_blocks() {
        run_ab_test("quarto_code_blocks");
    }

    /// A/B test: Quarto Hashpipe
    #[test]
    fn ab_test_quarto_hashpipe() {
        run_ab_test("quarto_hashpipe");
    }

    /// A/B test: Quarto Shortcodes
    #[test]
    fn ab_test_quarto_shortcodes() {
        run_ab_test("quarto_shortcodes");
    }

    /// A/B test: Raw Blocks
    #[test]
    fn ab_test_raw_blocks() {
        run_ab_test("raw_blocks");
    }

    /// A/B test: Reference Footnotes
    #[test]
    fn ab_test_reference_footnotes() {
        run_ab_test("reference_footnotes");
    }

    /// A/B test: Reference Images
    #[test]
    fn ab_test_reference_images() {
        run_ab_test("reference_images");
    }

    /// A/B test: Reference Links
    #[test]
    fn ab_test_reference_links() {
        run_ab_test("reference_links");
    }

    /// A/B test: Rmarkdown Math
    #[test]
    fn ab_test_rmarkdown_math() {
        run_ab_test("rmarkdown_math");
    }

    /// A/B test: Simple Table
    #[test]
    fn ab_test_simple_table() {
        run_ab_test("simple_table");
    }

    /// A/B test: Standardize Bullets
    #[test]
    fn ab_test_standardize_bullets() {
        run_ab_test("standardize_bullets");
    }

    /// A/B test: Table With Caption
    #[test]
    fn ab_test_table_with_caption() {
        run_ab_test("table_with_caption");
    }

    /// A/B test: Trailing Blanklines
    #[test]
    fn ab_test_trailing_blanklines() {
        run_ab_test("trailing_blanklines");
    }

    /// A/B test: Yaml Metadata
    #[test]
    fn ab_test_yaml_metadata() {
        run_ab_test("yaml_metadata");
    }
}
