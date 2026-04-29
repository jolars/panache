//! Golden test cases for panache formatter.
//!
//! Each test case is a directory under `tests/fixtures/cases/` containing:
//! - `input.*` - Source file (`.md`, `.qmd`, or `.Rmd`)
//! - `expected.*` - Expected formatted output (same extension as input)
//! - `panache.toml` - (Optional) Config to test specific flavors/extensions
//!
//! Run with `UPDATE_EXPECTED=1 cargo test` to regenerate expected outputs.

use panache::{
    Config,
    config::{Extensions, Flavor, FormatterExtensions},
    format,
};
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

fn detect_fixture_flavor(input_path: &Path, fallback: Flavor) -> Flavor {
    let ext = input_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());

    match ext.as_deref() {
        Some("qmd") => Flavor::Quarto,
        Some("rmd") | Some("rmarkdown") => Flavor::RMarkdown,
        _ => fallback,
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

    // Load optional config, then apply extension-based flavor detection for
    // config-less fixtures (matching CLI behavior for qmd/Rmd inputs).
    //
    // When a fixture's `panache.toml` sets only `flavor = "..."` (no
    // `[extensions]` table), serde leaves `extensions` at its `Default` —
    // which is Pandoc — so dialect-divergent parser paths gated on the
    // CommonMark/GFM flavor would silently fall back to Pandoc behavior.
    // Re-derive extension defaults from the resolved flavor so the fixture
    // actually exercises the flavor it declares.
    let config = match load_test_config(&dir) {
        Some(mut config) => {
            let toml_text = fs::read_to_string(dir.join("panache.toml")).unwrap_or_default();
            let toml_value: toml::Value =
                toml::from_str(&toml_text).unwrap_or(toml::Value::Table(toml::value::Table::new()));
            let extensions_overridden = toml_value.get("extensions").is_some();
            let formatter_extensions_overridden = toml_value.get("formatter-extensions").is_some()
                || toml_value.get("formatter_extensions").is_some();

            config.flavor = detect_fixture_flavor(&input_path, config.flavor);
            if !extensions_overridden {
                config.extensions = Extensions::for_flavor(config.flavor);
            }
            if !formatter_extensions_overridden {
                config.formatter_extensions = FormatterExtensions::for_flavor(config.flavor);
            }
            Some(config)
        }
        None => {
            let mut config = Config::default();
            let flavor = detect_fixture_flavor(&input_path, config.flavor);
            config.flavor = flavor;
            config.extensions = Extensions::for_flavor(flavor);
            config.formatter_extensions = FormatterExtensions::for_flavor(flavor);
            Some(config)
        }
    };

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
    blockquote_html_block,
    blockquote_list_blanks,
    blockquote_list_blockquote,
    blockquotes,
    bracketed_spans,
    bookdown,
    chunk_options_complex,
    code_blocks_executable,
    code_blocks_raw,
    code_spans,
    code_spans_unmatched_backtick_run_commonmark,
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
    escaped_bracket_math_idempotency_213,
    escapes,
    exec_code_in_list,
    fenced_code,
    fenced_code_quarto,
    fenced_code_unclosed_commonmark,
    fenced_divs,
    fenced_div_list_idempotency_setup,
    fenced_div_close_grid_table,
    footnote_continuation_idempotency,
    footnote_continuation_idempotency_reflow,
    footnote_numeric_continuation_idempotency_134,
    footnote_tex_block_boundary_idempotency_134,
    footnote_def_paragraph,
    footnote_definition_list,
    footnote_defs_consecutive_no_blanks,
    atx_interrupts_paragraph_commonmark,
    list_interrupts_paragraph_commonmark,
    list_mixed_bullets_commonmark,
    headings,
    setext_headings,
    setext_multiline_commonmark,
    indented_code_after_atx_heading_commonmark,
    setext_text_thematic_break_commonmark,
    thematic_break_interrupts_paragraph_commonmark,
    hr_as_list_item_content,
    hr_closes_list_commonmark,
    headerless_table,
    horizontal_rules,
    html_block,
    html_block_commonmark_type6_type7,
    ignore_directives,
    images,
    indented_code,
    inline_code,
    inline_link_dest_strict_commonmark,
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
    table_caption_duplicate,
    tables_sequential,
    tab_handling,
    tab_preserve,
    trailing_blanklines,
    umlauts,
    unicode,
    issue_171_gfm_inline_links,
    issue_231_gfm_tilde_idempotency,
    issue_172_hashpipe_inline_list_idempotency,
    issue_hashpipe_nested_list_indent,
    issue_176_display_math_colon_idempotency,
    issue_187_list_plus_wrap_idempotency,
    issue_181_hashpipe_fig_subcap_idempotency,
    issue_189_hashpipe_figcap_idempotency,
    issue_189_table_caption_heading_idempotency,
    issue_190_hashpipe_blank_line_losslessness,
    issue_192_chunk_options_idempotency,
    issue_186_list_blockquote_lazy_idempotency,
    issue_177_list_blockquote_idempotency,
    issue_185_hashpipe_double_space_idempotency,
    issue_193_cluster_a_fancy_markers_idempotency,
    issue_193_cluster_b_inline_footnote_citation_spacing,
    issue_194_idempotency_lsj_tbl_cap,
    issue_195_blockquote_wrapping_idempotency,
    issue_198_blockquote_chunk_header_idempotency,
    issue_198_cookbook_chunk_hooks_idempotency,
    issue_197_gfm_non_idempotent_bare_uri_escape,
    issue_200_adjacent_pipe_table_captions,
    issue_201_hashpipe_literal_blank_line_idempotency,
    issue_202_adjacent_mixed_table_captions,
    issue_203_quarto_layout_tables_idempotency,
    issue_212_license_nested_list_idempotency,
    issue_214_escaped_display_math_in_strong_idempotency,
    issue_214_mixed_marker_nested_ordered_idempotency,
    issue_225_chunk_option_nested_quotes_idempotency,
    issue_235_gfm_bare_uri_in_link_text_idempotency,
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
