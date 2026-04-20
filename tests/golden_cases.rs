//! Golden test cases for panache formatter.
//!
//! Each test case is a directory under `tests/fixtures/cases/` containing:
//! - `input.*` - Source file (`.md`, `.qmd`, or `.Rmd`)
//! - `expected.*` - Expected formatted output (same extension as input)
//! - `panache.toml` - (Optional) Config to test specific flavors/extensions
//!
//! Run with `UPDATE_EXPECTED=1 cargo test` to regenerate expected outputs.

use panache::{Config, format};
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
        .join("fixtures")
        .join("cases")
        .join(case_name);

    let update_expected = std::env::var_os("UPDATE_EXPECTED").is_some();
    // Find input file with any supported extension
    let input_path = find_file_with_extension(&dir, "input")
        .unwrap_or_else(|| panic!("No input file found in {}", case_name));

    // Determine expected path based on input extension
    let input_ext = input_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("qmd");
    let expected_path = dir.join(format!("expected.{}", input_ext));

    // Load optional config
    let config = load_test_config(&dir);

    // Read input file - preserve line endings exactly
    let input = fs::read_to_string(&input_path).unwrap();

    // Test formatting
    let output = format(&input, config.clone(), None);

    // Idempotency: formatting twice should equal once
    let output_twice = format(&output, config.clone(), None);
    similar_asserts::assert_eq!(output, output_twice, "idempotency: {}", case_name);

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
// 1. Create a new directory under tests/fixtures/cases/
// 2. Add the directory name to this list
golden_test_cases!(
    alerts,
    alerts_disabled,
    blankline_concatenation,
    blockquote_depth_change,
    blockquote_list_blanks,
    blockquote_list_blockquote,
    blockquotes,
    bracketed_spans,
    bookdown,
    chunk_options_complex,
    code_blocks_executable,
    code_blocks_raw,
    code_spans,
    crlf_basic,
    crlf_code_blocks,
    crlf_definition_lists,
    crlf_display_math,
    crlf_fenced_divs,
    crlf_headerless_table,
    crlf_horizontal_rules,
    crlf_line_endings,
    crlf_raw_blocks,
    crlf_yaml_metadata,
    citations,
    definition_list,
    definition_list_nesting,
    definition_list_pandoc_loose_compact,
    definition_list_wrapping,
    definition_colon_ratio_idempotency_134,
    display_math,
    display_math_blank_line_termination,
    display_math_content_on_fence_line,
    display_math_escaped_dollar,
    display_math_trailing_text,
    double_backslash_math,
    emphasis,
    emphasis_complex,
    emphasis_nested_inlines,
    equation_attributes,
    equation_attributes_disabled,
    equation_attributes_single_line,
    escapes,
    fenced_code,
    fenced_code_quarto,
    fenced_divs,
    fenced_div_list_idempotency_setup,
    fenced_div_close_grid_table,
    footnote_continuation_idempotency,
    footnote_continuation_idempotency_reflow,
    footnote_numeric_continuation_idempotency_134,
    footnote_tex_block_boundary_idempotency_134,
    footnote_def_paragraph,
    footnote_definition_list,
    headings,
    setext_headings,
    headerless_table,
    horizontal_rules,
    html_block,
    ignore_directives,
    images,
    indented_code,
    inline_code,
    inline_footnotes,
    inline_math,
    grid_table,
    grid_table_nordics,
    grid_table_planets,
    latex_environment,
    lazy_continuation_deep,
    leading_blanklines,
    line_blocks,
    list_alpha_nested_idempotency_143,
    list_deep_roman_idempotency_137,
    list_nested_roman_idempotency_136,
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
    multiline_table_basic,
    multiline_table_caption,
    multiline_table_caption_after,
    multiline_table_headerless,
    multiline_table_inline_formatting,
    mmd_title_block,
    mmd_link_attributes,
    mmd_link_attributes_disabled,
    nested_headings_in_containers,
    multiline_table_single_row,
    mmd_header_identifiers,
    pandoc_title_block,
    paragraph_continuation,
    paragraph_plain_mixed,
    paragraph_wrapping,
    paragraphs,
    pipe_table,
    pipe_table_unicode,
    plain_continuation_edge_cases,
    quarto_code_blocks,
    quarto_hashpipe,
    quarto_shortcodes,
    raw_blocks,
    raw_tex_commands,
    reference_footnotes,
    reference_images,
    reference_links,
    rmarkdown_math,
    simple_table,
    standardize_bullets,
    sentence_wrap_basic,
    sentence_wrap_abbreviations,
    sentence_wrap_contextual_abbrev,
    sentence_wrap_lang_metadata,
    sentence_wrap_list_blockquote,
    sentence_wrap_lazy_continuation,
    sentence_wrap_links_figures,
    sentence_wrap_lists,
    sentence_wrap_ellipsis,
    sentence_wrap_inline_code_sentence_end,
    sentence_wrap_quote_multisentence,
    sentence_wrap_inline_code_question,
    sentence_wrap_table_caption,
    table_with_caption,
    tab_handling,
    tab_preserve,
    trailing_blanklines,
    umlauts,
    unicode,
    issue_171_gfm_inline_links,
    issue_172_hashpipe_inline_list_idempotency,
    issue_hashpipe_nested_list_indent,
    issue_176_display_math_colon_idempotency,
    issue_181_hashpipe_fig_subcap_idempotency,
    issue_189_table_caption_heading_idempotency,
    issue_190_hashpipe_blank_line_losslessness,
    issue_177_list_blockquote_idempotency,
    writer_autolinks,
    writer_blockquote_not,
    writer_definition_lists_multiblock,
    writer_headers,
    writer_html_blocks,
    writer_paragraphs,
    writer_indented_code_escapes,
    yaml_metadata,
    yaml_metadata_dots_closer,
    yaml_metadata_normalization,
    yaml_metadata_opening_blank_not_metadata,
);
