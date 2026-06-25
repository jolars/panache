use panache_formatter::{Config, ConfigBuilder, WrapMode, format};

fn config() -> Config {
    let mut cfg = ConfigBuilder::default().build();
    cfg.parser_extensions.python_markdown_admonitions = true;
    cfg.parser_extensions.pymdownx_details = true;
    cfg
}

#[test]
fn issue_396_body_is_not_a_code_block() {
    // Regression for #396: 4-space-indented admonition content was parsed as
    // an indented code block, destroying the admonition. It must reflow as a
    // normal paragraph instead.
    let input = "\
!!! note

    Lorem ipsum dolor sit amet, consectetur adipiscing elit. Nulla et euismod
    nulla. Curabitur feugiat.
";
    let output = format(input, Some(config()), None);
    assert_eq!(
        output,
        "\
!!! note
    Lorem ipsum dolor sit amet, consectetur adipiscing elit. Nulla et euismod
    nulla. Curabitur feugiat.
"
    );
}

#[test]
fn title_is_preserved_and_marker_line_not_wrapped() {
    let input = "!!! note \"Heads up\"\n\n    Body text here.\n";
    let output = format(input, Some(config()), None);
    assert_eq!(output, "!!! note \"Heads up\"\n    Body text here.\n");
}

#[test]
fn extra_classes_collapse_internal_whitespace() {
    let input = "!!! danger   highlight \"Don't\"\n\n    Body.\n";
    let output = format(input, Some(config()), None);
    assert_eq!(output, "!!! danger highlight \"Don't\"\n    Body.\n");
}

#[test]
fn collapsible_details_markers_are_preserved() {
    let collapsed = format("??? note\n\n    Body.\n", Some(config()), None);
    assert_eq!(collapsed, "??? note\n    Body.\n");

    let expanded = format("???+ note\n\n    Body.\n", Some(config()), None);
    assert_eq!(expanded, "???+ note\n    Body.\n");
}

#[test]
fn body_reflows_to_indent_adjusted_width() {
    let input = "!!! note\n\n    aaa bbb ccc ddd eee fff ggg hhh iii jjj kkk lll mmm nnn ooo ppp qqq rrr sss ttt\n";
    let output = format(input, Some(config()), None);
    for line in output.lines() {
        assert!(line.len() <= 80, "line exceeds width: {line:?}");
    }
}

#[test]
fn nested_list_in_body() {
    let input = "!!! tip\n\n    - one\n    - two\n";
    let output = format(input, Some(config()), None);
    assert_eq!(output, "!!! tip\n    - one\n    - two\n");
}

#[test]
fn marker_line_not_split_under_sentence_wrap() {
    let mut cfg = config();
    cfg.wrap = Some(WrapMode::Sentence);
    let input = "!!! note \"My Title\"\n\n    First sentence. Second sentence.\n";
    let output = format(input, Some(cfg), None);
    assert_eq!(
        output,
        "!!! note \"My Title\"\n    First sentence.\n    Second sentence.\n"
    );
}

#[test]
fn disabled_by_default_is_unchanged() {
    // Without the extensions, the `!!!` line stays a literal paragraph and the
    // 4-space body remains a code block (the default formatter renders it as a
    // fenced block) — i.e. no admonition handling kicks in.
    let input = "!!! note\n\n    Body line.\n";
    let output = format(input, None, None);
    assert!(
        output.starts_with("!!! note"),
        "marker should remain literal text, got: {output:?}"
    );
    assert!(
        output.contains("```"),
        "body should be treated as a code block, got: {output:?}"
    );
}

#[test]
fn formatting_is_idempotent() {
    let input = "\
!!! note \"Heads up\"

    Lorem ipsum dolor sit amet, consectetur adipiscing elit. Nulla et euismod
    nulla.

    A second paragraph.
";
    let first = format(input, Some(config()), None);
    let second = format(&first, Some(config()), None);
    assert_eq!(first, second);
}
