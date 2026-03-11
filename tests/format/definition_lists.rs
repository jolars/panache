use panache::Config;
use panache::config::WrapMode;
use panache::format;

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
