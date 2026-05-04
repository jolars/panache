use panache_formatter::Config;
use panache_formatter::config::{PandocCompat, WrapMode};
use panache_formatter::format;

#[test]
fn definition_list_wrapped_continuation_is_idempotent() {
    let input = "Markdown, Emacs Org mode, ConTeXt, ZimWiki\n:   It will appear verbatim surrounded by `$...$` (for inline\n                math) or `$$...$$` (for display math).\n";

    let output1 = format(input, None, None);
    let output2 = format(&output1, None, None);

    similar_asserts::assert_eq!(output1, output2, "Formatting should be idempotent");
}

#[test]
fn definition_list_underscore_emphasis_with_asterisks_is_idempotent() {
    let input = "`--highlight-style=`*STYLE*|*FILE*\n\n:   _Deprecated, use `--syntax-highlighting=`*STYLE*|*FILE* instead._\n";

    let output1 = format(input, None, None);
    let output2 = format(&output1, None, None);

    assert!(
        output1.contains("*Deprecated, use `--syntax-highlighting=`*STYLE*\\|*FILE* instead.*")
    );
    similar_asserts::assert_eq!(output1, output2, "Formatting should be idempotent");
}

#[test]
fn definition_list_underscore_emphasis_with_asterisks_is_idempotent_in_sentence_mode() {
    let input = "`--highlight-style=`*STYLE*|*FILE*\n\n:   _Deprecated, use `--syntax-highlighting=`*STYLE*|*FILE* instead._\n";
    let cfg = Config {
        wrap: Some(WrapMode::Sentence),
        ..Default::default()
    };

    let output1 = format(input, Some(cfg.clone()), None);
    let output2 = format(&output1, Some(cfg), None);

    assert!(
        output1.contains("*Deprecated, use `--syntax-highlighting=`*STYLE*\\|*FILE* instead.*")
    );
    similar_asserts::assert_eq!(output1, output2, "Formatting should be idempotent");
}

#[test]
fn definition_list_fenced_code_info_has_no_space_after_fence() {
    let input = "Input\n:   ``` markdown \n    # Heading 1\n    \n    # Heading 2\n    ``` \n";
    let output = format(input, None, None);
    assert!(output.contains(":   ```markdown"));
    assert!(!output.contains(":   ``` markdown"));
    assert!(output.contains("# Heading 1\n\n    # Heading 2"));
    assert!(!output.contains("# Heading 1\n    \n    # Heading 2"));
}

#[test]
fn definition_list_fenced_code_preserves_yaml_nested_list_indent() {
    let input = "Output\n:   ```markdown\n    ---\n    echo: false\n    list:\n      - a\n      - b\n    ---\n    ```\n";
    let output = format(input, None, None);

    assert!(output.contains("list:\n      - a\n      - b"));
    assert!(!output.contains("list:\n    - a\n    - b"));
}

#[test]
fn definition_list_unclosed_fence_with_info_stays_unclosed() {
    let input = "Input\n:   \n\n````markdown\n";
    let output = format(input, None, None);

    assert!(output.contains("\\`\\`\\`\\`markdown\n"));
    assert!(!output.contains("```markdown\n```\n"));
}

#[test]
fn definition_list_blankline_continuation_uses_four_space_rule_in_pandoc_3_7_compat() {
    let input = "apple\n: pomaceous\n\n  fruit\n";
    let cfg = Config {
        parser: PandocCompat::V3_7,
        ..Default::default()
    };

    let output = format(input, Some(cfg), None);
    assert_eq!(output, "apple\n:   pomaceous\n\nfruit\n");
}

#[test]
fn definition_list_blankline_continuation_is_dynamic_in_latest_pandoc_compat() {
    let input = "apple\n: pomaceous\n\n  fruit\n";
    let cfg = Config {
        parser: PandocCompat::Latest,
        ..Default::default()
    };

    let output = format(input, Some(cfg), None);
    assert_eq!(output, "apple\n\n:   pomaceous\n\n    fruit\n");
}

#[test]
fn definition_list_preserves_item_loose_compactness_with_list_blocks() {
    let input = "Input\n:   - a\n    - b\n\nTerm Loose 1\n\n:   Definition 1\n\nTerm Loose 2\n:   Definition 2\n\nOrange\n:   Also a fruit\n\n:   Also a color\n\nOrange\n\n:   Also a fruit\n\n:   Also a color\n\nOrange\n:   - a\n    - b\n:   Also a color\n";

    let expected = "Input\n\n:   - a\n    - b\n\nTerm Loose 1\n\n:   Definition 1\n\nTerm Loose 2\n:   Definition 2\n\nOrange\n:   Also a fruit\n:   Also a color\n\nOrange\n\n:   Also a fruit\n\n:   Also a color\n\nOrange\n\n:   - a\n    - b\n\n:   Also a color\n";

    let output1 = format(input, None, None);
    let output2 = format(&output1, None, None);

    assert_eq!(output1, expected);
    similar_asserts::assert_eq!(output1, output2, "Formatting should be idempotent");
}

#[test]
fn definition_item_with_code_block_formats_as_loose() {
    let input = "Example violation\n: ```r\n  a <- 1\n  ```\n";
    let expected = "Example violation\n\n:   ```r\n    a <- 1\n    ```\n";

    let output = format(input, None, None);
    assert_eq!(output, expected);
}
