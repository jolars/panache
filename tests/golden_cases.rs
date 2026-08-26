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
    for ext in &["md", "qmd", "Rmd", "svx"] {
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
        Some("svx") => Flavor::Mdsvex,
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
    adjacent_simple_then_pipe_table_captions,
    alerts,
    alerts_disabled,
    myst_directive,
    myst_toctree,
    myst_block_break,
    myst_code_block_body,
    blankline_concatenation,
    blockquote_depth_change,
    blockquote_html_block,
    blockquote_lazy_html_block_interrupt,
    blockquote_lazy_line_folds,
    blockquote_lazy_fence_payload,
    blockquote_in_fenced_div_idempotency_310,
    blockquote_list_blanks,
    blockquote_list_blockquote,
    blockquote_list_item_atx_heading,
    blockquote_list_item_html_block,
    blockquote_multiline_table,
    blockquote_multiline_table_non_ascii,
    blockquote_multiline_table_spaced_border,
    blockquote_pipe_table,
    blockquote_pipe_table_caption,
    blockquote_pipe_table_surplus_cells,
    blockquote_simple_table,
    blockquote_simple_table_non_ascii,
    blockquotes,
    bracketed_spans,
    bookdown,
    bookdown_math_labels_host,
    gfm_bare_uri_trailing_delimiters,
    chunk_options_complex,
    code_blocks_executable,
    code_blocks_raw,
    code_spans,
    code_spans_unmatched_backtick_run_commonmark,
    commonmark_code_fence_info_string_preserved,
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
    east_asian_line_breaks,
    citations,
    citation_colon_paragraph_reflow,
    citation_prefix_paren_escape_idempotency_278,
    definition_body_code_block_blank_line_idempotency,
    definition_body_simple_table,
    definition_body_table_nested_containers,
    definition_list,
    definition_list_bare_marker_body,
    definition_list_heading_content,
    definition_list_in_blockquote,
    definition_list_nested_second_term_indent,
    definition_list_nesting,
    definition_list_pandoc_loose_compact,
    definition_list_in_list_item,
    definition_list_in_list_item_four_space,
    definition_list_wrapping,
    definition_marker_ends_definition_body_block,
    definition_marker_promotes_definition_body_term,
    definition_marker_promotes_definition_body_term_blank_line,
    definition_marker_caption_probe_container_boundary,
    definition_marker_ends_list_item_block,
    definition_marker_tab_straddle,
    definition_term_requires_single_line_block,
    definition_two_blank_lines_detach_term,
    definition_colon_ratio_idempotency_134,
    display_math,
    display_math_blank_line_termination,
    display_math_content_on_fence_line,
    display_math_escaped_dollar,
    display_math_long_dollar_runs,
    display_math_trailing_text,
    double_backslash_math,
    emphasis,
    emphasis_complex,
    emphasis_nested_inlines,
    equation_attributes,
    equation_attributes_disabled,
    equation_attributes_single_line,
    escaped_bracket_math_idempotency_213,
    escaped_bracket_math_in_link_238,
    escapes,
    exec_code_in_list,
    fenced_code,
    fenced_code_indented_fence,
    fenced_code_quarto,
    fenced_code_unclosed_commonmark,
    fenced_divs,
    fenced_div_indented_close_top_level,
    fenced_div_list_idempotency_setup,
    fenced_div_close_grid_table,
    fenced_div_trim_blank_lines,
    fenced_div_missing_blank_line_340,
    bracketed_span_footnote_reflow_291,
    footnote_body_simple_table,
    footnote_bare_marker_body_heading,
    footnote_bare_marker_body_table,
    footnote_marker_line_blocks,
    simple_table_in_footnote_stops_at_note_marker,
    simple_table_in_list_item_stops_at_sibling_marker,
    simple_table_in_nested_item_stops_at_band_marker,
    nested_list_band_marker_terminates_item,
    blockquote_nested_list_keeps_indent,
    quoted_band_marker_sibling_list,
    quoted_item_nested_quote,
    simple_table_closer_kept_before_sibling_marker,
    pipe_table_in_footnote_stops_at_note_marker,
    pipe_table_in_list_item_stops_at_sibling_marker,
    pipe_table_marker_line_in_blockquote,
    simple_table_in_div_stops_at_closer,
    simple_table_closer_before_div_closer,
    simple_table_cell_whitespace_collapse,
    pipe_table_cell_whitespace_collapse,
    caption_fence_continuation_before_table,
    table_caption_in_footnote_stops_at_note_marker,
    multiline_table_in_div_stops_at_closer,
    footnote_continuation_idempotency,
    footnote_continuation_idempotency_reflow,
    footnote_numeric_continuation_idempotency_134,
    footnote_tex_block_boundary_idempotency_134,
    footnote_def_paragraph,
    footnote_definition_list,
    footnote_definition_in_list_item,
    footnote_defs_consecutive_no_blanks,
    four_space_rule_bullet,
    four_space_rule_continuation,
    four_space_rule_ordered,
    four_space_rule_wide_marker,
    four_space_rule_wrapping,
    atx_interrupts_lazy_blockquote_no_blank_before_header,
    atx_interrupts_reduced_marker_lazy_blockquote_no_blank_before_header,
    atx_interrupts_paragraph_commonmark,
    atx_interrupts_paragraph_no_blank_before_header,
    list_gobble_chain_lazy_continuation,
    list_interrupts_paragraph_commonmark,
    list_item_blockquote_inner_list_siblings,
    list_item_blockquote_nested_list_issue_292,
    list_item_lazy_blockquote_marker,
    quoted_item_lazy_blockquote_marker,
    list_item_html_comment_trailing_split,
    list_item_html_div_multiline_open,
    list_item_html_div_multiline_para,
    list_item_html_div_same_line,
    list_item_html_div_unclosed,
    list_item_html_pre_multiline,
    list_item_html_section_multiline,
    list_item_leading_block_blank_separator,
    list_item_leading_code_block,
    list_item_leading_fenced_div,
    list_item_nested_div_close,
    list_item_simple_table_caption,
    list_item_table_first_bullet,
    list_item_table_first_pipeless,
    list_item_table_under_bare_marker,
    list_item_table_first_caption_bullet,
    list_item_table_first_grid,
    table_ordered_marker_first_line_caption,
    table_ordered_marker_first_line_caption_exact,
    table_pipe_short_row_padded,
    table_pipe_surplus_cells_verbatim,
    table_pipe_surplus_in_list_item,
    list_mixed_bullets_commonmark,
    headings,
    heading_attributes_commonmark,
    heading_closing_run,
    heading_in_list_item,
    heading_trailing_backslash,
    hard_break_backslash_gfm,
    hard_break_backslash_whitespace,
    hard_break_backslash_whitespace_preserve,
    setext_headings,
    setext_after_setext_pandoc,
    setext_heading_in_list_item,
    setext_multiline_commonmark,
    indented_code_after_atx_heading_commonmark,
    setext_text_thematic_break_commonmark,
    thematic_break_interrupts_paragraph_commonmark,
    blockquote_list_no_marker_closes_commonmark,
    empty_list_marker_blank_then_content_commonmark,
    hr_as_list_item_content,
    hr_closes_list_commonmark,
    headerless_table,
    headerless_table_no_closer,
    horizontal_rule_div_fence,
    horizontal_rule_in_list_item,
    horizontal_rules,
    horizontal_rules_compact,
    html_block,
    html_block_div_fenced_div_body,
    html_block_commonmark_type6_type7,
    html_block_raw_comment,
    html_block_raw_pre,
    html_block_standalone_tags,
    html_block_matched_pair_inter_tag_text,
    html_block_blockquote_standalone_tags,
    html_block_void_strict_block_tags,
    html_block_open_trailing,
    html_block_blockquote_open_trailing,
    html_block_comment_trailing_softbreak,
    html_block_comment_trailing_split,
    html_block_details_blockquote_child,
    html_block_div_attr_entities,
    html_block_div_blockquote_idempotent,
    html_block_div_blockquote_messy_idempotent,
    html_block_div_blockquote_multiline_open_idempotent,
    html_block_div_blockquote_multiline_open_trailing_idempotent,
    html_block_div_blockquote_multiline_same_line_close,
    html_block_div_blockquote_multiline_trailing_close_text,
    html_block_div_idempotent,
    html_block_div_inter_tag_text,
    html_block_div_inter_tag_no_space,
    html_block_comment_trailing_softbreak_fenced_div,
    html_block_comment_trailing_softbreak_blockquote,
    html_block_comment_definition_body_trailing_softbreak,
    html_block_div_definition_body,
    html_block_div_definition_body_multiline,
    html_block_div_definition_body_later_line,
    html_block_div_definition_body_later_line_blockquote,
    html_block_div_definition_body_later_line_unclosed,
    html_block_div_footnote_body,
    html_block_div_multiline_open_trailing_idempotent,
    html_block_div_multiline_same_line_close,
    html_block_div_multiline_trailing_close_text,
    html_block_div_nested_idempotent,
    html_block_div_nested_same_line,
    html_block_div_same_line_trailing,
    html_block_div_trailing_close,
    html_block_div_unclosed,
    list_item_html_div_same_line_trailing,
    html_block_paragraph_demote,
    html_block_paragraph_then_style,
    html_block_pre_close_tag_inline_commonmark,
    html_block_strict_block_inner_lift,
    html_block_strict_block_lift_shapes,
    html_block_strict_block_multiline_open,
    html_block_section_blockquote_multiline_open_idempotent,
    html_block_strict_blockquote_idempotent,
    html_block_strict_blockquote_messy_idempotent,
    html_block_video_matched_pair,
    html_inline_span_idempotent,
    ignore_directives,
    images,
    implicit_figure_in_containers,
    indented_code,
    inline_code,
    inline_code_attribute_normalization,
    inline_link_dest_strict_commonmark,
    inline_footnotes,
    inline_footnote_crossref_spacing_430,
    inline_math,
    inline_math_interior_newline_reflow,
    math_align_experimental,
    math_binary_relation_lhs_experimental,
    math_blockquote_experimental,
    math_delimited_body_trailing_break_experimental,
    math_embedded_environment_experimental,
    math_inline_stays_flat_experimental,
    math_linebreak_binary_chain_experimental,
    math_linebreak_experimental,
    math_linebreak_indent_budget_experimental,
    math_linebreak_nested_experimental,
    math_operator_spacing_experimental,
    math_scripts_experimental,
    math_relation_chain_long_lhs_experimental,
    math_relation_chain_hardbreak_experimental,
    math_malformed_delimiter_experimental,
    math_format_off_default,
    grid_table,
    grid_table_column_span,
    blockquote_grid_table_colspan,
    list_item_grid_table_colspan,
    grid_table_nordics,
    grid_table_reflow,
    grid_table_planets,
    grid_table_rowspan_aligned,
    grid_table_rowspan_colspan_2d,
    grid_table_rowspan_partial_separator,
    blockquote_grid_table_rowspan,
    list_item_grid_table_rowspan,
    blockquote_list_item_grid_table_rowspan,
    latex_environment,
    lazy_continuation_deep,
    leading_blanklines,
    line_block_continuation_line,
    line_block_indented_marker_continues,
    line_block_on_list_marker_line,
    line_block_in_blockquote,
    line_blocks,
    list_alpha_nested_idempotency_143,
    list_deep_roman_idempotency_137,
    list_nested_roman_idempotency_136,
    list_nested_same_line_marker,
    list_orphan_indent4_marker_after_blank_becomes_codeblock,
    line_ending_crlf,
    line_ending_lf,
    link_inside_link_text_commonmark,
    links,
    lists_bullet,
    lists_code,
    lists_example,
    lists_fancy,
    lists_indented_code,
    lists_nested,
    lists_ordered,
    lists_task,
    lists_task_nested,
    lists_task_no_content,
    lists_wrapping_nested,
    lists_wrapping_simple,
    multiline_table_basic,
    multiline_table_blank_after_opener_idempotent,
    multiline_table_column_spacing,
    multiline_table_no_blank_rows,
    multiline_table_caption,
    multiline_table_caption_after,
    multiline_table_headerless,
    multiline_table_headerless_single_column,
    multiline_table_inline_formatting,
    multiline_table_reflow,
    multiline_table_single_col_code_fence_438,
    multiline_table_two_dash_borders,
    mmd_title_block,
    mmd_link_attributes,
    mmd_link_attributes_disabled,
    nested_headings_in_containers,
    multiline_table_single_row,
    mmd_header_identifiers,
    overindented_fence_stays_paragraph_text,
    pandoc_title_block,
    paragraph_continuation,
    paragraph_plain_mixed,
    paragraph_wrapping,
    paragraphs,
    nested_pipe_table_indent,
    pipe_table,
    pipe_table_unicode,
    pipe_table_without_body_rows,
    plain_continuation_edge_cases,
    python_markdown_admonitions,
    quarto_chunk_after_list_item_idempotency_471,
    quarto_code_blocks,
    quarto_executable_class_attrs,
    quarto_hashpipe,
    quarto_shortcodes,
    quiz_options_after_chunk_idempotency_471,
    quoted_para_in_list_item_stops_at_html_closer,
    raw_blocks,
    raw_tex_commands,
    reference_definition_attached_title_commonmark,
    reflow_wrap_block_markers,
    reference_footnotes,
    reference_images,
    reference_links,
    unresolved_reference_intraword_underscore_pandoc,
    rmarkdown_math,
    simple_table,
    simple_table_alignment_widened_column,
    simple_table_header_closer,
    simple_table_headerless_single_column,
    simple_table_wide_cell,
    standardize_bullets,
    svelte_block,
    svelte_template,
    sentence_wrap_basic,
    sentence_wrap_abbreviations,
    sentence_wrap_contextual_abbrev,
    sentence_wrap_lang_metadata,
    sentence_wrap_list_blockquote,
    sentence_wrap_lazy_continuation,
    sentence_wrap_links_figures,
    sentence_wrap_lists,
    sentence_wrap_block_markers_commonmark,
    sentence_wrap_ellipsis,
    sentence_wrap_inline_code_sentence_end,
    sentence_wrap_quote_multisentence,
    sentence_wrap_inline_code_question,
    sentence_wrap_table_caption,
    sentence_wrap_trailing_citations_505,
    sentence_wrap_lang_cs,
    sentence_wrap_lang_de,
    sentence_wrap_no_break_flat,
    sentence_wrap_no_break_per_lang,
    sentence_wrap_config_lang_fallback,
    semantic_wrap_moser,
    semantic_wrap_preserve_clause_breaks,
    semantic_wrap_abbreviations,
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
    issue_501_gfm_gitlab_quick_action,
    issue_172_hashpipe_inline_list_idempotency,
    issue_179_hashpipe_one_space_list_idempotency,
    issue_hashpipe_nested_list_indent,
    issue_176_display_math_colon_idempotency,
    issue_381_display_math_marker_trailing_space,
    issue_383_backslash_display_math_marker_trailing_space,
    issue_449_display_math_in_pipe_table_cell,
    issue_487_math_definition_colon_experimental,
    issue_510_math_mixed_environment_idempotency,
    issue_511_math_multiple_environments_idempotency,
    issue_187_list_plus_wrap_idempotency,
    issue_181_hashpipe_fig_subcap_idempotency,
    issue_189_hashpipe_figcap_idempotency,
    issue_hashpipe_folded_figcap_wrap,
    issue_hashpipe_folded_scalar_no_blank,
    yaml_folded_description_reflow,
    issue_366_rmd_vignette_preamble,
    issue_189_table_caption_heading_idempotency,
    issue_190_hashpipe_blank_line_losslessness,
    issue_192_chunk_options_idempotency,
    issue_186_list_blockquote_lazy_idempotency,
    issue_177_list_blockquote_idempotency,
    issue_247_nested_list_same_line_marker_idempotency,
    issue_246_nested_list_idempotency,
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
    issue_248_tilde_fence_paragraph_idempotency,
    issue_248_blockquote_subscript_marker_idempotency,
    issue_258_gfm_autolink,
    issue_277_list_bullet_outdent_after_blank_idempotency,
    issue_279_list_item_html_block_trailing_idempotency,
    issue_280_hashpipe_yaml_tag_and_dotted_key_idempotency,
    issue_286_fenced_div_collapse_blanks,
    issue_332_space_after_inline_code_in_strong,
    issue_344_table_indent_zero,
    issue_398_headerless_multiline_dash_closer,
    issue_467_attribute_unnumbered_shorthand,
    issue_479_fenced_div_after_wrapped_list_item,
    issue_482_chunk_after_wrapped_list_item,
    issue_484_fenced_div_after_wrapped_list_item,
    issue_496_preserve_trims_trailing_whitespace,
    issue_497_definition_body_chunk_blank_line,
    issue_498_definition_body_indented_code,
    writer_autolinks,
    writer_blockquote_not,
    writer_definition_lists_multiblock,
    writer_headers,
    writer_html_blocks,
    writer_paragraphs,
    writer_indented_code_escapes,
    wikilinks_after_pipe,
    wikilinks_before_pipe,
    wikilinks_image,
    wikilinks_in_paragraph,
    wrapped_multiline_code_span_stays_lazy,
    yaml_metadata,
    yaml_metadata_dots_closer,
    yaml_metadata_normalization,
    yaml_metadata_opening_blank_not_metadata,
);
