//! Golden parser regression cases for panache-parser.
//!
//! Each test case is a directory under
//! `crates/panache-parser/tests/fixtures/cases/` containing:
//! - `input.*` - Source file (`.md`, `.qmd`, or `.Rmd`)
//! - `parser-options.toml` - (Optional) parser-only options (`flavor`, `[extensions]`)
//!
//! CST snapshots are stored via insta in
//! `crates/panache-parser/tests/snapshots/`.
//! Run `INSTA_UPDATE=always cargo test -p panache-parser --test golden_parser_cases`
//! to update snapshots intentionally.

use panache_parser::{Extensions, Flavor, ParserOptions, parse};
use std::{
    collections::HashMap,
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

/// Load parser options from test case directory if parser-options.toml exists.
fn load_test_parser_options(dir: &Path) -> Option<ParserOptions> {
    let config_path = dir.join("parser-options.toml");
    if !config_path.exists() {
        return None;
    }

    let content = fs::read_to_string(config_path).ok()?;
    let value: toml::Value = toml::from_str(&content).ok()?;

    let mut options = ParserOptions::default();

    if let Some(flavor_str) = value.get("flavor").and_then(toml::Value::as_str) {
        let flavor = match flavor_str {
            "pandoc" => Flavor::Pandoc,
            "quarto" => Flavor::Quarto,
            "rmarkdown" => Flavor::RMarkdown,
            "gfm" => Flavor::Gfm,
            "commonmark" => Flavor::CommonMark,
            "multimarkdown" => Flavor::MultiMarkdown,
            _ => Flavor::default(),
        };
        options.flavor = flavor;
        options.extensions = Extensions::for_flavor(flavor);
    }

    if let Some(ext_table) = value.get("extensions").and_then(toml::Value::as_table) {
        let mut overrides: HashMap<String, bool> = HashMap::new();
        for (key, val) in ext_table {
            if let Some(v) = val.as_bool() {
                overrides.insert(key.clone(), v);
            }
        }
        options.extensions = Extensions::merge_with_flavor(overrides, options.flavor);
    }

    Some(options)
}

/// Run parser-only checks for a single golden case.
fn run_golden_case(case_name: &str) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("cases")
        .join(case_name);

    let input_path = find_file_with_extension(&dir, "input")
        .unwrap_or_else(|| panic!("No input file found in {}", case_name));
    let parser_options = load_test_parser_options(&dir);

    let input = fs::read_to_string(&input_path).unwrap();

    let tree = parse(&input, parser_options);
    let tree_text = tree.text().to_string();

    assert_eq!(
        input,
        tree_text,
        "losslessness check failed for {} (tree text does not match input, diff: {:+} bytes)",
        case_name,
        tree_text.len() as i64 - input.len() as i64
    );

    let cst_output = format!("{:#?}\n", tree);
    insta::assert_snapshot!(format!("parser_cst_{}", case_name), cst_output);
}

#[test]
fn issue_195_canonical_shape_delta() {
    let once_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("cases")
        .join("issue_195_blockquote_lazy_continuation_shape")
        .join("input.Rmd");
    let canonical_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
        .join("cases")
        .join("issue_177_list_blockquote_idempotency")
        .join("expected.Rmd");

    let once_input = fs::read_to_string(once_path).unwrap();
    let canonical_input = fs::read_to_string(canonical_path).unwrap();

    let once_tree = parse(&once_input, None);
    let canonical_tree = parse(&canonical_input, None);

    let once_cst = format!("{:#?}\n", once_tree);
    let canonical_cst = format!("{:#?}\n", canonical_tree);

    assert!(
        once_cst.contains("BLOCK_QUOTE_MARKER@417..418 \">\""),
        "expected issue_195 CST to keep shifted continuation marker as a structural token"
    );
    assert!(
        canonical_cst.contains("INLINE_CODE_CONTENT") && canonical_cst.contains("\"env\""),
        "expected canonical CST to retain inline-code content for env"
    );
}

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
// 1. Create a new directory under crates/panache-parser/tests/fixtures/cases/
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
    issue_164_unicode_autolink_panic,
    issue_174_blockquote_list_reorder_losslessness,
    issue_175_native_span_unicode_panic,
    issue_186_list_blockquote_lazy_idempotency,
    issue_195_blockquote_lazy_continuation_shape,
    issue_197_gfm_non_idempotent_bare_uri_escape,
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
