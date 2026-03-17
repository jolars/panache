use super::helpers::{
    assert_block_kinds, assert_block_kinds_for_node, find_all, find_first, parse_blocks,
    parse_blocks_quarto, parse_blocks_with_config,
};
use crate::config::{Config, Flavor};
use crate::syntax::SyntaxKind;

fn get_code_content(node: &crate::syntax::SyntaxNode) -> Option<String> {
    find_first(node, SyntaxKind::CODE_CONTENT).map(|n| n.text().to_string())
}

fn get_code_info_node(node: &crate::syntax::SyntaxNode) -> Option<crate::syntax::SyntaxNode> {
    node.descendants()
        .find(|element| element.kind() == SyntaxKind::CODE_INFO)
}

fn get_code_info(node: &crate::syntax::SyntaxNode) -> Option<String> {
    get_code_info_node(node).map(|n| n.text().to_string())
}

#[test]
fn parses_simple_backtick_code_block() {
    let input = "```\nprint(\"hello\")\n```\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CODE_BLOCK]);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "print(\"hello\")\n");
}

#[test]
fn parses_simple_tilde_code_block() {
    let input = "~~~\nprint(\"hello\")\n~~~\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CODE_BLOCK]);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "print(\"hello\")\n");
}

#[test]
fn parses_code_block_with_language() {
    let input = "```python\nprint(\"hello\")\n```\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CODE_BLOCK]);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "print(\"hello\")\n");

    let info = get_code_info(&node).unwrap();
    assert_eq!(info, "python");
}

#[test]
fn parses_code_block_with_attributes() {
    let input = "```{python}\nprint(\"hello\")\n```\n";
    let node = parse_blocks_quarto(input);

    assert_block_kinds_for_node(&node, &[SyntaxKind::CODE_BLOCK], input);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "print(\"hello\")\n");

    let info = get_code_info(&node).unwrap();
    assert_eq!(info, "{python}");
}

#[test]
fn parses_code_block_with_complex_attributes() {
    let input = "```{python #mycode .numberLines startFrom=\"100\"}\nprint(\"hello\")\n```\n";
    let node = parse_blocks_quarto(input);

    assert_block_kinds_for_node(&node, &[SyntaxKind::CODE_BLOCK], input);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "print(\"hello\")\n");

    let info = get_code_info(&node).unwrap();
    assert_eq!(info, "{python #mycode .numberLines startFrom=\"100\"}");
}

#[test]
fn parses_multiline_code_block() {
    let input = "```python\nfor i in range(10):\n    print(i)\n```\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CODE_BLOCK]);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "for i in range(10):\n    print(i)\n");
}

#[test]
fn code_block_can_interrupt_paragraph() {
    // Fenced code blocks with language identifiers can interrupt paragraphs
    // Bare fences (```) require a blank line to avoid ambiguity with inline code
    let input = "text\n```python\ncode\n```\n";
    let node = parse_blocks(input);

    // Should parse as paragraph followed by code block
    assert_block_kinds_for_node(
        &node,
        &[SyntaxKind::PARAGRAPH, SyntaxKind::CODE_BLOCK],
        input,
    );

    let code_content = get_code_content(&node).unwrap();
    assert_eq!(code_content, "code\n");
}

#[test]
fn bare_fence_without_closing_fence_does_not_interrupt_paragraph() {
    // Unclosed bare fences should not interrupt paragraphs.
    let input = "text\n```\ncode\n";
    // Use full parse to get inline parsing too
    let tree = crate::parse(input, None);

    // Should parse as single paragraph
    let paragraphs: Vec<_> = tree
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::PARAGRAPH)
        .collect();
    assert_eq!(paragraphs.len(), 1, "Should have one paragraph");

    let code_block = tree
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CODE_BLOCK);
    assert!(code_block.is_none(), "Should not contain fenced code block");
}

#[test]
fn code_block_with_language_can_interrupt_paragraph() {
    // Test with language identifier
    let input = "Some text:\n```r\na <- 1\n```\n";
    let node = parse_blocks(input);

    assert_block_kinds_for_node(
        &node,
        &[SyntaxKind::PARAGRAPH, SyntaxKind::CODE_BLOCK],
        input,
    );

    let code_content = get_code_content(&node).unwrap();
    assert_eq!(code_content, "a <- 1\n");

    let info = get_code_info(&node).unwrap();
    assert_eq!(info, "r");
}

#[test]
fn bare_fence_after_colon_with_command_transcript_can_interrupt_paragraph() {
    let input = "Some text:\n```\n% pandoc -t plain\n```\n";
    let node = parse_blocks(input);
    assert_block_kinds_for_node(
        &node,
        &[SyntaxKind::PARAGRAPH, SyntaxKind::CODE_BLOCK],
        input,
    );
}

#[test]
fn bare_fence_in_list_item_with_closing_fence_can_interrupt_paragraph() {
    let input = "- one\n  ```\n  code\n  ```\n- two\n";
    let node = parse_blocks(input);
    let has_code_block = node
        .descendants()
        .any(|n| n.kind() == SyntaxKind::CODE_BLOCK);
    assert!(
        has_code_block,
        "Expected fenced code block inside list item"
    );
}

#[test]
fn adjacent_bare_fences_with_command_transcripts_parse_as_two_code_blocks() {
    let input = "```\n% one\n```\n```\n% two\n```\n";
    let node = parse_blocks(input);
    let code_blocks = node
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::CODE_BLOCK)
        .count();
    assert_eq!(code_blocks, 2);
}

#[test]
fn parses_code_block_at_start_of_document() {
    let input = "```\ncode\n```\n";

    assert_block_kinds(input, &[SyntaxKind::CODE_BLOCK]);
}

#[test]
fn parses_code_block_after_blank_line() {
    let input = "text\n\n```\ncode\n```\n";
    let node = parse_blocks(input);

    let blocks: Vec<_> = node
        .descendants()
        .filter(|n| matches!(n.kind(), SyntaxKind::PARAGRAPH | SyntaxKind::CODE_BLOCK))
        .collect();

    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].kind(), SyntaxKind::PARAGRAPH);
    assert_eq!(blocks[1].kind(), SyntaxKind::CODE_BLOCK);
}

#[test]
fn requires_at_least_three_fence_chars() {
    let input = "``\ncode\n``\n";
    let node = parse_blocks(input);

    // Should not parse as code block
    assert!(find_first(&node, SyntaxKind::CODE_BLOCK).is_none());
}

#[test]
fn closing_fence_must_have_at_least_same_length() {
    let input = "````\ncode\n```\n";
    let node = parse_blocks(input);

    // Code block should be parsed, but without proper closing
    assert!(find_first(&node, SyntaxKind::CODE_BLOCK).is_some());

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "code\n```\n"); // The ``` becomes part of content
}

#[test]
fn closing_fence_can_be_longer() {
    let input = "```\ncode\n`````\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CODE_BLOCK]);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "code\n");
}

#[test]
fn mixed_fence_chars_dont_close() {
    let input = "```\ncode\n~~~\n";
    let node = parse_blocks(input);

    // Should parse code block but ~~~ becomes content
    assert!(find_first(&node, SyntaxKind::CODE_BLOCK).is_some());

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "code\n~~~\n");
}

#[test]
fn empty_code_block() {
    let input = "```\n```\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CODE_BLOCK]);

    // Should have no content node for empty blocks
    assert!(get_code_content(&node).is_none());
}

#[test]
fn code_block_with_leading_spaces() {
    let input = "  ```python\n  print(\"hello\")\n  ```\n";
    let node = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::CODE_BLOCK]);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "  print(\"hello\")\n");
}

#[test]
fn definition_list_inline_fence_parses_as_code_block() {
    let input = "Term\n: ```r\n  a <- 1\n  ```\n";
    let node = parse_blocks_quarto(input);

    let code_block = find_first(&node, SyntaxKind::CODE_BLOCK);
    assert!(
        code_block.is_some(),
        "Expected code block inside definition list"
    );

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "  a <- 1\n");

    let info = get_code_info(&node).unwrap();
    assert_eq!(info, "r");
}

#[test]
fn executable_chunk_parses_hashpipe_label_as_chunk_option() {
    let input = "```{r}\n#| label: foobar\na <- 1\n```\n";
    let node = parse_blocks_quarto(input);

    let code_block = find_first(&node, SyntaxKind::CODE_BLOCK).expect("expected code block");
    let info = get_code_info(&code_block).expect("expected code info");
    assert_eq!(info, "{r}");

    let has_label_option = code_block.descendants().any(|n| {
        if n.kind() != SyntaxKind::CHUNK_OPTION {
            return false;
        }
        let key = n
            .children_with_tokens()
            .find_map(|el| match el {
                rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::CHUNK_OPTION_KEY => {
                    Some(t.text().to_string())
                }
                _ => None,
            })
            .unwrap_or_default();
        let value = n
            .children_with_tokens()
            .find_map(|el| match el {
                rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::CHUNK_OPTION_VALUE => {
                    Some(t.text().to_string())
                }
                _ => None,
            })
            .unwrap_or_default();
        key == "label" && value == "foobar"
    });
    assert!(
        has_label_option,
        "expected hashpipe label to be parsed as CHUNK_OPTION"
    );
}

#[test]
fn executable_chunk_keeps_non_hashpipe_lines_in_code_content() {
    let input = "```{r}\n#| label: foobar\na <- 1\n# comment\n```\n";
    let node = parse_blocks_quarto(input);

    let content = get_code_content(&node).unwrap();
    assert_eq!(content, "#| label: foobar\na <- 1\n# comment\n");
}

#[test]
fn executable_chunk_multiline_hashpipe_continuation_is_not_top_level_text() {
    let input = "```{r}\n#| fig-cap: \"A multiline caption\n#|  that spans multiple lines and demonstrates\n#|  wrapping.\"\na <- 1\n```\n";
    let node = parse_blocks_quarto(input);
    let code_block = find_first(&node, SyntaxKind::CODE_BLOCK).expect("expected code block");
    let code_content = code_block
        .children()
        .find(|n| n.kind() == SyntaxKind::CODE_CONTENT)
        .expect("expected code content");

    let has_top_level_continuation_text = code_content.children_with_tokens().any(|el| match el {
        rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::TEXT => {
            t.text().trim_start().starts_with("#|  ")
        }
        _ => false,
    });
    assert!(
        !has_top_level_continuation_text,
        "multiline hashpipe continuation should not be emitted as top-level TEXT token"
    );
}

#[test]
fn executable_chunk_block_scalar_hashpipe_continuation_is_not_top_level_text() {
    let input = "```{r}\n#| fig-cap: |\n#|   A caption\n#|   spanning some lines\na <- 1\n```\n";
    let node = parse_blocks_quarto(input);
    let code_block = find_first(&node, SyntaxKind::CODE_BLOCK).expect("expected code block");
    let code_content = code_block
        .children()
        .find(|n| n.kind() == SyntaxKind::CODE_CONTENT)
        .expect("expected code content");

    let has_top_level_continuation_text = code_content.children_with_tokens().any(|el| match el {
        rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::TEXT => {
            t.text().trim_start().starts_with("#|   ")
        }
        _ => false,
    });
    assert!(
        !has_top_level_continuation_text,
        "block-scalar hashpipe continuation should not be emitted as top-level TEXT token"
    );
}

#[test]
fn executable_chunk_folded_block_scalar_hashpipe_continuation_is_not_top_level_text() {
    let input =
        "```{r}\n#| fig-cap: >-\n#|   A folded caption\n#|   spanning some lines\na <- 1\n```\n";
    let node = parse_blocks_quarto(input);
    let code_block = find_first(&node, SyntaxKind::CODE_BLOCK).expect("expected code block");
    let code_content = code_block
        .children()
        .find(|n| n.kind() == SyntaxKind::CODE_CONTENT)
        .expect("expected code content");

    let has_top_level_continuation_text = code_content.children_with_tokens().any(|el| match el {
        rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::TEXT => {
            t.text().trim_start().starts_with("#|   ")
        }
        _ => false,
    });
    assert!(
        !has_top_level_continuation_text,
        "folded block-scalar hashpipe continuation should not be emitted as top-level TEXT token"
    );
}

#[test]
fn executable_chunk_indented_hashpipe_value_continuation_is_not_top_level_text() {
    let input = "```{r}\n#| list:\n#|   - a\n#|   - b\na <- 1\n```\n";
    let node = parse_blocks_quarto(input);
    let code_block = find_first(&node, SyntaxKind::CODE_BLOCK).expect("expected code block");
    let code_content = code_block
        .children()
        .find(|n| n.kind() == SyntaxKind::CODE_CONTENT)
        .expect("expected code content");

    let has_top_level_continuation_text = code_content.children_with_tokens().any(|el| match el {
        rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::TEXT => {
            t.text().trim_start().starts_with("#|   - ")
        }
        _ => false,
    });
    assert!(
        !has_top_level_continuation_text,
        "indented hashpipe continuation should not be emitted as top-level TEXT token"
    );
}

#[test]
fn executable_chunk_emits_hashpipe_yaml_preamble_node() {
    let input = "```{r}\n#| echo: false\n#| fig-cap: |\n#|   A caption\nx <- 1\n```\n";
    let node = parse_blocks_quarto(input);
    let code_block = find_first(&node, SyntaxKind::CODE_BLOCK).expect("expected code block");
    let code_content = code_block
        .children()
        .find(|n| n.kind() == SyntaxKind::CODE_CONTENT)
        .expect("expected code content");
    let preamble = code_content
        .children()
        .find(|n| n.kind() == SyntaxKind::HASHPIPE_YAML_PREAMBLE)
        .expect("expected hashpipe preamble node");
    assert_eq!(
        preamble.text().to_string(),
        "#| echo: false\n#| fig-cap: |\n#|   A caption\n"
    );
}

#[test]
fn executable_chunk_emits_hashpipe_yaml_content_node() {
    let input = "```{r}\n#| echo: false\n#| fig-cap: |\n#|   A caption\nx <- 1\n```\n";
    let node = parse_blocks_quarto(input);
    let code_block = find_first(&node, SyntaxKind::CODE_BLOCK).expect("expected code block");
    let code_content = code_block
        .children()
        .find(|n| n.kind() == SyntaxKind::CODE_CONTENT)
        .expect("expected code content");
    let preamble = code_content
        .children()
        .find(|n| n.kind() == SyntaxKind::HASHPIPE_YAML_PREAMBLE)
        .expect("expected hashpipe preamble node");
    let preamble_content = preamble
        .children()
        .find(|n| n.kind() == SyntaxKind::HASHPIPE_YAML_CONTENT)
        .expect("expected hashpipe preamble content node");
    assert_eq!(
        preamble_content.text().to_string(),
        "#| echo: false\n#| fig-cap: |\n#|   A caption\n"
    );
}

#[test]
fn display_code_block_keeps_hashpipe_line_as_plain_text() {
    let input = "```r\n#| label: foobar\na <- 1\n```\n";
    let node = parse_blocks_quarto(input);

    let has_chunk_option = node
        .descendants()
        .any(|n| n.kind() == SyntaxKind::CHUNK_OPTION);
    assert!(
        !has_chunk_option,
        "display-only code blocks should not parse hashpipe as chunk options"
    );
}

#[test]
fn backtick_fenced_code_blocks_respect_extension_guard() {
    let input = "```r\na <- 1\n```\n";
    let mut config = Config::default();
    config.extensions.backtick_code_blocks = false;

    let disabled = parse_blocks_with_config(input, &config);
    assert!(
        find_first(&disabled, SyntaxKind::CODE_BLOCK).is_none(),
        "backtick_code_blocks disabled should prevent backtick fenced code parsing"
    );

    config.extensions.backtick_code_blocks = true;
    let enabled = parse_blocks_with_config(input, &config);
    assert!(
        find_first(&enabled, SyntaxKind::CODE_BLOCK).is_some(),
        "backtick_code_blocks enabled should allow backtick fenced code parsing"
    );
}

#[test]
fn tilde_fenced_code_blocks_respect_extension_guard() {
    let input = "~~~r\na <- 1\n~~~\n";
    let mut config = Config::default();
    config.extensions.fenced_code_blocks = false;

    let disabled = parse_blocks_with_config(input, &config);
    assert!(
        find_first(&disabled, SyntaxKind::CODE_BLOCK).is_none(),
        "fenced_code_blocks disabled should prevent tilde fenced code parsing"
    );

    config.extensions.fenced_code_blocks = true;
    let enabled = parse_blocks_with_config(input, &config);
    assert!(
        find_first(&enabled, SyntaxKind::CODE_BLOCK).is_some(),
        "fenced_code_blocks enabled should allow tilde fenced code parsing"
    );
}

#[test]
fn fenced_code_attributes_respect_extension_guard() {
    let input = "```{python}\na <- 1\n```\n";
    let mut config = Config {
        flavor: Flavor::Quarto,
        ..Default::default()
    };
    config.extensions.fenced_code_attributes = false;

    let disabled = parse_blocks_with_config(input, &config);
    assert!(
        find_first(&disabled, SyntaxKind::CODE_BLOCK).is_none(),
        "fenced_code_attributes disabled should prevent brace-info fenced code parsing"
    );

    config.extensions.fenced_code_attributes = true;
    let enabled = parse_blocks_with_config(input, &config);
    assert!(
        find_first(&enabled, SyntaxKind::CODE_BLOCK).is_some(),
        "fenced_code_attributes enabled should allow brace-info fenced code parsing"
    );
}

#[test]
fn raw_attribute_respects_extension_guard_for_fenced_code() {
    let input = "```{=html}\n<div>raw</div>\n```\n";
    let mut config = Config::default();
    config.extensions.raw_attribute = false;
    config.extensions.fenced_code_attributes = false;

    let disabled = parse_blocks_with_config(input, &config);
    assert!(
        find_first(&disabled, SyntaxKind::CODE_BLOCK).is_none(),
        "raw_attribute disabled should prevent raw-attribute fenced code parsing"
    );

    config.extensions.raw_attribute = true;
    let enabled = parse_blocks_with_config(input, &config);
    assert!(
        find_first(&enabled, SyntaxKind::CODE_BLOCK).is_some(),
        "raw_attribute enabled should allow raw-attribute fenced code parsing"
    );
}

#[test]
fn tex_math_gfm_parses_math_fence_as_display_math() {
    let input = "``` math\nx + y\n```\n";
    let mut config = Config::default();
    config.extensions.tex_math_gfm = true;

    let tree = parse_blocks_with_config(input, &config);
    assert!(
        find_first(&tree, SyntaxKind::DISPLAY_MATH).is_some(),
        "tex_math_gfm enabled should parse ``` math fences as display math"
    );
}

// Indented code block tests

#[test]
fn parses_indented_code_block() {
    let input = "
    code line 1
    code line 2";
    let tree = parse_blocks(input);

    assert_eq!(find_all(&tree, SyntaxKind::CODE_BLOCK).len(), 1);
    let code_blocks = find_all(&tree, SyntaxKind::CODE_BLOCK);
    let code = &code_blocks[0];
    let text = code.text().to_string();
    assert!(text.contains("code line 1"));
    assert!(text.contains("code line 2"));
}

#[test]
fn indented_code_block_with_blank_line() {
    let input = "
    code line 1

    code line 2";
    let tree = parse_blocks(input);

    assert_eq!(find_all(&tree, SyntaxKind::CODE_BLOCK).len(), 1);
}

#[test]
fn indented_code_requires_blank_line_before() {
    let input = "paragraph
    not code";
    let tree = parse_blocks(input);

    // Should be a single paragraph, not a code block
    assert_eq!(find_all(&tree, SyntaxKind::CODE_BLOCK).len(), 0);
    assert_eq!(find_all(&tree, SyntaxKind::PARAGRAPH).len(), 1);
}

#[test]
fn indented_code_with_tab() {
    let input = "
\tcode with tab";
    let tree = parse_blocks(input);

    assert_eq!(find_all(&tree, SyntaxKind::CODE_BLOCK).len(), 1);
}

#[test]
fn indented_code_with_list_marker() {
    let input = "
    * one
    * two";
    let tree = parse_blocks(input);

    assert_eq!(find_all(&tree, SyntaxKind::CODE_BLOCK).len(), 1);
}

#[test]
fn indented_code_in_blockquote() {
    let input = ">
>     code in blockquote";
    let tree = parse_blocks(input);

    assert_eq!(find_all(&tree, SyntaxKind::BLOCKQUOTE).len(), 1);
    assert_eq!(find_all(&tree, SyntaxKind::CODE_BLOCK).len(), 1);
}
