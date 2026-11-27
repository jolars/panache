#![cfg(feature = "experimental-block-parser")]

use panache_parser::parser::{Edit, ReparseStrategy, experimental_block, parse_with_errors};
use panache_parser::{Flavor, ParserOptions, SyntaxNode};

fn check(input: &str, options: &ParserOptions) -> SyntaxNode {
    let (expected, errors) = parse_with_errors(input, Some(options.clone()));
    let (actual, actual_errors) =
        experimental_block::try_parse_with_errors(input, options).unwrap();
    assert_eq!(actual.to_string(), input, "expected {expected:#?}");
    assert_eq!(
        actual.green(),
        expected.green(),
        "{input}\nactual {actual:#?}\nexpected {expected:#?}"
    );
    assert_eq!(actual_errors, errors);
    actual
}

#[test]
fn commonmark_core_slice_preserves_cst() {
    let options = ParserOptions::for_flavor(Flavor::CommonMark);
    for input in [
        "# Title\r\n\r\nfirst *line*\r\nsecond",
        "Setext title\n============\n\n***\n",
        "Before.\n\n- first\n  continuation\n  - nested\n- last\n\nAfter.\n",
        "- first\n\n- last\n",
        "> first\n> continued\nlazy\n\nNext.\n",
        "<div>\nraw content\n</div>\n\nNext.\n",
        "[reference][label]\n\n[label]: https://example.com\n",
        "```rust\nfn main() {}\n",
    ] {
        check(input, &options);
    }
}

#[test]
fn quarto_slice_preserves_cst_and_yaml_diagnostics() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for input in [
        "---\ntitle: Title\n---\n\n::: {.note}\n:::: {.inner}\nHello.\n::::\n:::\n",
        "```{r}\n#| label: [\nprint(1)\n```\n",
        "a | b\n--|--\nx | y\n",
        "a | b\n--|--\nx | y\n\n: Table caption\n",
    ] {
        check(input, &options);
    }
}

#[test]
fn structural_reparse_reuses_both_sides_across_edits() {
    for flavor in [Flavor::CommonMark, Flavor::Quarto] {
        let options = ParserOptions::for_flavor(flavor);
        let mut text = format!(
            "# Intro\n\nUnaffected before.\n\nOriginal text.\n\n{}",
            "Unaffected after.\n\n".repeat(60),
        );
        let mut tree = check(&text, &options);
        for (before, after) in [
            ("Original text.", "Revised *text*."),
            ("Revised *text*.", "Final **text**."),
        ] {
            let offset = text.find(before).unwrap();
            let edit = Edit {
                range: offset..offset + before.len(),
                insert: after.into(),
            };
            text = edit.apply(&text);
            let result =
                experimental_block::reparse(&tree.green().to_owned(), &[], &edit, &text, &options)
                    .expect("expected a structural splice");
            assert_eq!(result.strategy, ReparseStrategy::Region);
            let updated = SyntaxNode::new_root(result.green);
            assert_eq!(updated.green(), check(&text, &options).green());
            for (previous, current) in [
                (tree.first_child(), updated.first_child()),
                (tree.last_child(), updated.last_child()),
            ] {
                let previous = previous.unwrap();
                let current = current.unwrap();
                assert!(std::ptr::eq(previous.green(), current.green()));
            }
            tree = updated;
        }
    }
}

#[test]
fn reference_definition_change_declines_splice_and_updates_links() {
    let options = ParserOptions::for_flavor(Flavor::CommonMark);
    let text = "[label]\n\n[label]: https://example.com\n";
    let old = check(text, &options);
    let offset = text.rfind("label").unwrap();
    let edit = Edit {
        range: offset..offset + "label".len(),
        insert: "renamed".into(),
    };
    let updated = edit.apply(text);
    assert!(
        experimental_block::reparse(&old.green().to_owned(), &[], &edit, &updated, &options)
            .is_none()
    );
    let new = check(&updated, &options);
    assert_ne!(
        old.first_child().unwrap().green(),
        new.first_child().unwrap().green()
    );
}

#[test]
fn window_strategies_parse_with_the_experimental_engine() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for (text, needle, expected) in [
        (
            "para one\n\npara two\n\npara three\n\npara four\n\npara five\n",
            "five",
            ReparseStrategy::SuffixWindow,
        ),
        (
            "# Intro\n\nalpha\n\n# Middle\n\nbeta section\n\n# End\n\nomega\n",
            "section",
            ReparseStrategy::SectionWindow,
        ),
    ] {
        let base = check(text, &options);
        let offset = text.find(needle).unwrap();
        let edit = Edit {
            range: offset..offset + needle.len(),
            insert: needle.to_ascii_uppercase(),
        };
        let updated = edit.apply(text);
        let (parsed, scanned) = experimental_block::measure_fragment_work(
            &base.green().to_owned(),
            &[],
            &edit,
            &updated,
            &options,
            true,
        );
        let parsed = parsed.unwrap();
        assert_eq!(parsed.strategy, expected);
        assert!(scanned > 0);
        assert_eq!(&*parsed.green, check(&updated, &options).green());
    }
}

#[test]
fn hashpipe_edit_shifts_and_preserves_its_diagnostic() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    let input = format!(
        "Intro paragraph.\n\n```{{r}}\n#| label: [\nprint(1)\n```\n\n{}",
        "Unchanged prose.\n\n".repeat(60),
    );
    let (tree, errors) = experimental_block::try_parse_with_errors(&input, &options).unwrap();
    assert_eq!(errors.len(), 1);
    let start = input.find("label").unwrap();
    let edit = Edit {
        range: start..start + "label".len(),
        insert: "description".into(),
    };
    let updated = edit.apply(&input);
    let reparsed =
        experimental_block::reparse(&tree.green().to_owned(), &errors, &edit, &updated, &options)
            .expect("editing code cell metadata should splice");
    let (full, expected_errors) =
        experimental_block::try_parse_with_errors(&updated, &options).unwrap();
    assert_eq!(reparsed.errors, expected_errors);
    assert_eq!(&*reparsed.green, full.green());
}

#[test]
fn mixed_lists_and_quotes_preserve_cst() {
    for flavor in [Flavor::CommonMark, Flavor::Quarto] {
        let options = ParserOptions::for_flavor(flavor);
        for input in [
            "- > - foo\n",
            "> - first\n> - second\n",
            "> - first\n  lazy\n",
            "> - # heading\n>   text\n",
        ] {
            check(input, &options);
        }
    }
    check(
        "- > first\r\n  > continued\r\n- next",
        &ParserOptions::for_flavor(Flavor::Quarto),
    );
}

#[test]
fn mixed_container_region_edits_match_full_parse() {
    let options = ParserOptions::for_flavor(Flavor::CommonMark);
    let mut text = format!(
        "# Intro\n\n> - original\n> - second\n\n{}",
        "Unaffected prose.\n\n".repeat(60)
    );
    let mut tree = check(&text, &options);
    for (old, new) in [("original", "updated"), ("updated", "revised *text*")] {
        let start = text.find(old).unwrap();
        let edit = Edit {
            range: start..start + old.len(),
            insert: new.into(),
        };
        text = edit.apply(&text);
        let result =
            experimental_block::reparse(&tree.green().to_owned(), &[], &edit, &text, &options)
                .expect("expected a mixed-container splice");
        assert_eq!(result.strategy, ReparseStrategy::Region);
        let updated = SyntaxNode::new_root(result.green);
        assert_eq!(updated.green(), check(&text, &options).green());
        for (before, after) in [
            (tree.first_child().unwrap(), updated.first_child().unwrap()),
            (tree.last_child().unwrap(), updated.last_child().unwrap()),
        ] {
            assert!(std::ptr::eq(before.green(), after.green()));
        }
        tree = updated;
    }
    let start = text.find("second").unwrap();
    let edit = Edit {
        range: start..start + "second".len(),
        insert: "<div>".into(),
    };
    let updated = edit.apply(&text);
    let result =
        experimental_block::reparse(&tree.green().to_owned(), &[], &edit, &updated, &options)
            .expect("expected a mixed-container splice for the HTML edit");
    assert_eq!(
        SyntaxNode::new_root(result.green).green(),
        check(&updated, &options).green()
    );
}

#[test]
fn quoted_ordered_sublist_respects_pandoc_compatibility() {
    let mut options = ParserOptions::for_flavor(Flavor::Quarto);
    for compat in [
        panache_parser::PandocCompat::V3_9,
        panache_parser::PandocCompat::V3_10,
    ] {
        options.pandoc_compat = compat;
        check("> - outer\n>   2. child\n", &options);
    }
}

#[test]
fn commonmark_html_in_quoted_lists_preserves_cst() {
    let options = ParserOptions::for_flavor(Flavor::CommonMark);
    check("Intro\n\n> - <div>\n", &options);
}

#[test]
fn mixed_containers_respect_blank_before_blockquote() {
    let mut options = ParserOptions::for_flavor(Flavor::Quarto);
    let input = "Paragraph\n> - quoted item\n";
    check(input, &options);

    options.extensions.blank_before_blockquote = false;
    check(input, &options);
}

#[test]
fn fenced_code_in_a_quoted_list_preserves_its_prefixes() {
    let options = ParserOptions::for_flavor(Flavor::CommonMark);
    check("> - item\n>   ```\n>   code\n>   ```\n", &options);
}

#[test]
fn fenced_div_can_contain_a_mixed_list_and_quote() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    check("::: {.note}\n> - item\n:::\n", &options);
}

#[test]
fn quoted_list_preserves_empty_items_and_blank_lines() {
    let options = ParserOptions::for_flavor(Flavor::CommonMark);
    check("> -\n>\n> - next\n", &options);
    check("> - a\n>\tcontinued\n", &options);
}

#[test]
fn quarto_block_openers_respect_open_paragraphs() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for input in [
        "Prose\n::: note\nText\n:::\n",
        "> First\n> # Next\n",
        "> A\n> - B\n",
        "    first\n    \u{a0}\n",
    ] {
        check(input, &options);
    }
}

#[test]
fn commonmark_non_initial_ordered_markers_continue_paragraphs() {
    let options = ParserOptions::for_flavor(Flavor::CommonMark);
    for input in ["> A\n> 2. B\n", "- parent\n  2. child\n"] {
        check(input, &options);
    }
}

#[test]
fn commonmark_container_prose_can_start_with_punctuation() {
    let options = ParserOptions::for_flavor(Flavor::CommonMark);
    check("> #hashtag\n*emphasis*\n", &options);
    check("- +literal\n  -dash\n", &options);
}

#[test]
fn commonmark_lazy_quote_accepts_inline_html() {
    check(
        "> paragraph\n<span>\n",
        &ParserOptions::for_flavor(Flavor::CommonMark),
    );
}

#[test]
fn commonmark_nested_quote_owns_all_opener_indentation() {
    check(
        "- item\n    > quote\n\n      paragraph\n",
        &ParserOptions::for_flavor(Flavor::CommonMark),
    );
}

#[test]
fn commonmark_core_blocks_inside_list_and_quote_paragraphs() {
    let options = ParserOptions::for_flavor(Flavor::CommonMark);
    for input in [
        "- paragraph\n  ***\n",
        "> paragraph\n> ***\n",
        "> - paragraph\n>   <div>\n",
        "> paragraph\n> <div>\n",
        "- paragraph\n  <div>\n",
        "> paragraph\n> [ref]: /url\n",
        "> - paragraph\n>   ---\n",
        "- > \n  > text\n",
    ] {
        check(input, &options);
    }
}

#[test]
fn commonmark_html_in_quote_inside_list() {
    check(
        "- > <div>\n  > body\n",
        &ParserOptions::for_flavor(Flavor::CommonMark),
    );
}

#[test]
fn commonmark_fence_interrupting_a_quote_inside_a_list_keeps_its_prefix_once() {
    let options = ParserOptions::for_flavor(Flavor::CommonMark);
    let input = "- > a\n  > ```\n  > code\n  > ```\n";
    let (tree, errors) = experimental_block::try_parse_with_errors(input, &options).unwrap();
    assert_eq!(tree.to_string(), input);
    assert!(errors.is_empty());
    assert!(
        tree.descendants()
            .any(|node| node.kind() == panache_parser::syntax::SyntaxKind::CODE_BLOCK)
    );
}

#[test]
fn commonmark_setext_heading_on_a_list_continuation_line() {
    check(
        "- a\n  ===\n",
        &ParserOptions::for_flavor(Flavor::CommonMark),
    );
}

#[test]
fn commonmark_empty_item_opener_keeps_quoted_paragraph_together() {
    let options = ParserOptions::for_flavor(Flavor::CommonMark);
    check("> - \n>   next\n", &options);
    check("- > - \n  >   next\n", &options);
}

#[test]
fn commonmark_heading_interrupting_a_list_preserves_its_indent() {
    let options = ParserOptions::for_flavor(Flavor::CommonMark);
    check("- a\n  ## b\n", &options);
    check("> - a\n>   ## b\n", &options);
}

#[test]
fn commonmark_multiline_reference_stays_inside_its_list_item() {
    let options = ParserOptions::for_flavor(Flavor::CommonMark);
    check("- a\n\n  [ref]:\n  /url\n- next\n", &options);
    let input = "- a\n\n  [ref]:\n/url\n";
    let (tree, _) = experimental_block::try_parse_with_errors(input, &options).unwrap();
    assert_eq!(tree.to_string(), input);
    assert!(
        !tree
            .descendants()
            .any(|node| node.kind() == panache_parser::syntax::SyntaxKind::REFERENCE_DEFINITION)
    );
}

#[test]
fn commonmark_invalid_reference_definition_is_container_prose() {
    let options = ParserOptions::for_flavor(Flavor::CommonMark);
    check("> [missing-destination]:\n", &options);
    check("- [missing-destination]:\n", &options);
}

#[test]
fn commonmark_nested_quote_marker_respects_paragraph_precedence() {
    use panache_parser::syntax::SyntaxKind as K;

    let options = ParserOptions::for_flavor(Flavor::CommonMark);
    for (underline, kind) in [
        ("-", K::HEADING),
        ("---", K::HEADING),
        ("===", K::HEADING),
        ("2. b", K::PARAGRAPH),
        ("*", K::PARAGRAPH),
    ] {
        let input = format!("- > a\n  > {underline}\n");
        let (tree, errors) = experimental_block::try_parse_with_errors(&input, &options).unwrap();
        assert_eq!(tree.to_string(), input);
        assert!(errors.is_empty());
        let quote = tree
            .descendants()
            .find(|node| node.kind() == K::BLOCK_QUOTE)
            .unwrap();
        assert_eq!(
            quote.children().map(|node| node.kind()).collect::<Vec<_>>(),
            [kind],
            "{tree:#?}"
        );
    }
}

#[test]
fn commonmark_task_checkbox_does_not_change_list_continuation() {
    let options = ParserOptions::for_flavor(Flavor::CommonMark);
    check("- [x] done\n  continued\n- next\n", &options);
    check("> - [ ] **open**\n", &options);
    check("> a\n> - [x] done\n", &options);
    check("- > a\n  > - [x] done\n", &options);
}

#[test]
fn commonmark_indented_code_in_alternating_list_and_quote_frames() {
    let options = ParserOptions::for_flavor(Flavor::CommonMark);
    for (input, expected_code) in [
        (
            "- > -     code\n  >         more\n  > - sibling\n",
            "    code\n  >         more\n",
        ),
        (
            "> - >       code\n>   >\n>   >       more\n> - sibling\n",
            "      code\n>   >\n>   >       more\n",
        ),
    ] {
        let (tree, errors) = experimental_block::try_parse_with_errors(input, &options).unwrap();
        assert_eq!(tree.to_string(), input);
        assert!(errors.is_empty());
        let code: Vec<_> = tree
            .descendants()
            .filter(|node| node.kind() == panache_parser::syntax::SyntaxKind::CODE_BLOCK)
            .collect();
        assert_eq!(code.len(), 1, "{tree:#?}");
        assert_eq!(code[0].to_string(), expected_code);
    }
}

#[test]
fn commonmark_html_in_alternating_list_and_quote_frames() {
    let options = ParserOptions::for_flavor(Flavor::CommonMark);
    check("- > - <div>\n  >   body\n", &options);
    check("> - > <div>\n>   > body\n", &options);
}

#[test]
fn commonmark_fence_in_alternating_list_and_quote_frames() {
    let options = ParserOptions::for_flavor(Flavor::CommonMark);
    check("- > - ```\n  >   body\n  >   ```\n", &options);

    // The incumbent loses a fence byte in this order of containers.
    let input = "> - > ```\n>   > body\n>   > ```\n";
    let (tree, errors) = experimental_block::try_parse_with_errors(input, &options).unwrap();
    assert_eq!(tree.to_string(), input);
    assert!(errors.is_empty());
    let code = tree
        .descendants()
        .find(|node| node.kind() == panache_parser::syntax::SyntaxKind::CODE_BLOCK)
        .unwrap();
    assert_eq!(code.to_string(), "```\n>   > body\n>   > ```\n");
}

#[test]
fn commonmark_fence_in_quote_inside_list() {
    check(
        "- > ```\n  > code\n  > ```\n",
        &ParserOptions::for_flavor(Flavor::CommonMark),
    );
}

#[test]
fn commonmark_fence_on_quoted_list_marker_line() {
    check(
        "> - ```\n>   code\n>   ```\n",
        &ParserOptions::for_flavor(Flavor::CommonMark),
    );
}

#[test]
fn quarto_task_items_and_tabbed_continuations_preserve_cst() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    check("- [x] done\n", &options);
    check("- > foo\n  \tcode\n", &options);
}

#[test]
fn quarto_fancy_and_example_lists_preserve_cst() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for input in [
        include_str!("fixtures/cases/lists_fancy/input.md"),
        include_str!("fixtures/cases/lists_example_reset/input.md"),
        include_str!("fixtures/cases/list_restricted_marker_outdent/input.md"),
        "a. First\n\ni. Ninth\nj. Tenth\n",
    ] {
        check(input, &options);
    }
}

#[test]
fn four_space_rule_uses_configured_list_content_column() {
    let mut options = ParserOptions::for_flavor(Flavor::Quarto);
    options.extensions.four_space_rule = true;
    check("- first\n    - child\n", &options);
    check("- first\n  - second\n", &options);
}

#[test]
fn commonmark_html_keeps_unicode_whitespace_and_following_text() {
    check(
        "<div>\n\u{a0}\nvisible\n",
        &ParserOptions::for_flavor(Flavor::CommonMark),
    );
}

#[test]
fn quarto_pipe_table_accepts_lowercase_caption() {
    check(
        "table: Measurements\n\nA | B\n--|--\n1 | 2\n",
        &ParserOptions::for_flavor(Flavor::Quarto),
    );
}

#[test]
fn quarto_container_openers_respect_blank_before_rules() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    check("> first\n> > nested\n", &options);
    check("> text\n> ~~~\n> code\n> ~~~\n", &options);
    check("> text\n>\n> ~~~\n> code\n> ~~~\n", &options);
}

#[test]
fn quarto_promotes_standalone_container_images() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for input in [
        "> ![Caption](image.png)\n",
        "- ![Caption](image.png)\n",
        "> - ![Caption](image.png)\n",
        "> ![Caption](image.png)\n> continued\n",
    ] {
        check(input, &options);
    }
}

#[test]
fn lazy_quote_continuations_respect_dialect() {
    check(
        "> quoted\n: ordinary text\n",
        &ParserOptions::for_flavor(Flavor::CommonMark),
    );
    check(
        "> quoted\n- list\n",
        &ParserOptions::for_flavor(Flavor::Quarto),
    );
}

#[test]
fn reference_definitions_with_optional_indent_inside_quotes() {
    let options = ParserOptions::for_flavor(Flavor::CommonMark);
    for input in ["> [id]: /url\n", ">  [id]: /url\n", ">    [id]: /url\n"] {
        check(input, &options);
    }
}

#[test]
fn list_code_span_keeps_tab_straddling_content_column() {
    for flavor in [Flavor::CommonMark, Flavor::Quarto] {
        check("- ``foo\n\tbar``\n", &ParserOptions::for_flavor(flavor));
    }
}

#[test]
fn commonmark_quoted_heading_preserves_paragraph_prefix() {
    check(
        "> note\n> # title\n",
        &ParserOptions::for_flavor(Flavor::CommonMark),
    );
}

#[test]
fn quarto_list_and_quote_options_preserve_cst() {
    use panache_parser::PandocCompat;

    let mut options = ParserOptions::for_flavor(Flavor::Quarto);
    for compat in [
        PandocCompat::V3_9,
        PandocCompat::V3_10,
        PandocCompat::V3_11,
        PandocCompat::Latest,
    ] {
        options.pandoc_compat = compat;
        for input in [
            "- Outer\n  > 2. Child\n",
            "> 2. First\n> 3. Second\n",
            "- Outer\n\n  2. Child\n",
            "(@first) First\n(2@second) Second\n(@third) Third\n",
        ] {
            check(input, &options);
        }
    }
    for enabled in [false, true] {
        options.extensions.fancy_lists = enabled;
        options.extensions.example_lists = enabled;
        options.extensions.lists_without_preceding_blankline = enabled;
        for input in [
            "a. First\nb. Second\n",
            "(@first) First\n",
            "Intro\n- Item\n",
            "> Intro\n> - Item\n",
            "> Intro\n- Item\n",
        ] {
            check(input, &options);
        }
    }
}

#[test]
fn quarto_chained_container_edits_preserve_cst() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    let mut text = format!(
        "# Intro\n\n> - item\n>   a. child\n>   b. sibling\n\n{}",
        "Unaffected prose.\n\n".repeat(60)
    );
    let mut tree = check(&text, &options);
    let mut splices = 0;
    for (old, new) in [
        ("a. child", "1. child"),
        (">   b. sibling", ">\n>   b. sibling"),
    ] {
        let start = text.find(old).unwrap();
        let edit = Edit {
            range: start..start + old.len(),
            insert: new.into(),
        };
        text = edit.apply(&text);
        let expected = check(&text, &options);
        if let Some(result) =
            experimental_block::reparse(&tree.green().to_owned(), &[], &edit, &text, &options)
        {
            let updated = SyntaxNode::new_root(result.green);
            assert_eq!(updated.green(), expected.green());
            assert!(result.errors.is_empty());
            assert!(std::ptr::eq(
                tree.last_child().unwrap().green(),
                updated.last_child().unwrap().green()
            ));
            tree = updated;
            splices += 1;
        } else {
            tree = expected;
        }
    }
    assert!(splices > 0);
}

#[test]
fn quarto_list_and_quote_boundaries_preserve_cst() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for input in [
        "- item\n\n  second paragraph\n- sibling\n",
        "- item\n\n- second\n  continued\n",
        "-\n- content\n",
        "-\n  ordinary\n",
        include_str!("fixtures/cases/list_item_table_under_bare_marker/input.md"),
        "> - first\n>\n> - second\n",
        "> - first\n>   continuation\n",
        "- ` `\n  lazy",
        "- ```\n  code\n  ```\n",
        "> Title\n> =====\n",
        "> [id]: /x\n> [use][id]\n",
        "- a | b\n  --|--\n  x | y\n",
        include_str!("fixtures/cases/list_item_pipe_table_caption_after_pandoc/input.md"),
        include_str!("fixtures/cases/list_item_pipe_table_caption_before/input.md"),
        include_str!("fixtures/cases/list_item_pipe_table_continuation_blank_pandoc/input.md"),
        "> ::: note\n> text\n> :::\n",
        include_str!("fixtures/cases/list_item_fenced_div_marker_line_pandoc/input.md"),
        include_str!("fixtures/cases/blockquote_list_no_marker_continues_pandoc/input.md"),
        include_str!("fixtures/cases/lists_wrapping_nested/input.md"),
        include_str!("fixtures/cases/lists_nested/input.md"),
        include_str!("fixtures/cases/quoted_item_nested_quote_after_blank/input.md"),
        include_str!("fixtures/cases/blockquote_list_blockquote/input.md"),
        include_str!("fixtures/cases/blockquote_indented_code_continuation/input.md"),
        include_str!(
            "fixtures/cases/list_item_indented_code_marker_line_partial_overflow/input.md"
        ),
        include_str!("fixtures/cases/blockquote_lazy_fence_body_deindents_pandoc/input.md"),
        include_str!("fixtures/cases/list_item_overindented_fence_is_lazy_pandoc/input.md"),
        include_str!(
            "fixtures/cases/blockquote_lazy_tilde_fence_ends_quoted_list_item_pandoc/input.md"
        ),
        include_str!("fixtures/cases/blockquote_lazy_fence_ends_quoted_list_item_pandoc/input.md"),
        "> text\n> ***\n",
    ] {
        check(input, &options);
    }
}

#[test]
fn quarto_grid_table_at_quoted_list_item_opener() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    let input = "> - +---+\n>   | a |\n>   +---+\n";
    check(input, &options);
}

#[test]
fn shared_container_review_regressions() {
    let commonmark = ParserOptions::for_flavor(Flavor::CommonMark);
    let quarto = ParserOptions::for_flavor(Flavor::Quarto);
    for options in [&commonmark, &quarto] {
        check("- # Zoo\n\talpha\n", options);
    }
    check("> - a\n>\n>\n", &commonmark);
    for input in [
        "- a\n***\n- b\n",
        "- a\n# Heading\n- b\n",
        "- a\n> quote\n- b\n",
        "- [x]: /url\n",
        "- a\n  | prose\n",
        "> a\n> | prose\n",
        "- a\n    ====\n",
        "- > 2. a\n",
        "- - a\n\n    b\n",
        "- a\n\n  # h\n",
        "- ::: note\n  - a\n  :::\n",
        "> > a | b\n> ---|---\n",
        "> a | b\n> ---|---\n> - x | y\n",
        "> - a | b\n>   ---|---\n>   x | y\n",
    ] {
        check(input, &quarto);
    }
    check("- Term\n  : def\n", &quarto);
    let input = "- a\n\n  ---\n  x\n  ---\n";
    check(input, &quarto);
}

#[test]
fn quarto_nested_body_fixtures_preserve_cst() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for input in [
        include_str!("fixtures/cases/definition_list/input.md"),
        include_str!("fixtures/cases/definition_list_blockquote_continuation/input.md"),
        include_str!("fixtures/cases/definition_list_in_list_blockquote/input.md"),
        include_str!("fixtures/cases/definition_list_pandoc_bare_leading_list/input.md"),
        include_str!("fixtures/cases/footnote_bare_marker_body_heading/input.md"),
        include_str!("fixtures/cases/footnote_bare_marker_body_table/input.md"),
        include_str!("fixtures/cases/footnote_fenced_code_indent/input.md"),
        include_str!("fixtures/cases/definition_list_bare_marker_pandoc/input.md"),
        include_str!("fixtures/cases/definition_list_pandoc_loose_compact/input.md"),
        include_str!("fixtures/cases/definition_list_inner_list_no_blank/input.md"),
        include_str!("fixtures/cases/definition_list_in_list_item_nested/input.md"),
        include_str!("fixtures/cases/definition_list_lazy_marker_in_blockquote_pandoc/input.md"),
        include_str!("fixtures/cases/definition_list_nested_second_term_indent/input.md"),
        include_str!("fixtures/cases/footnote_def_paragraph/input.md"),
        include_str!("fixtures/cases/footnote_ref_in_footnote_def_is_text/input.md"),
        include_str!("fixtures/cases/footnote_definition_in_list_item/input.md"),
        include_str!("fixtures/cases/footnote_with_blockquote/input.md"),
        include_str!("fixtures/cases/footnote_marker_line_blocks/input.md"),
        include_str!("fixtures/cases/pipe_table_in_footnote_stops_at_note_marker/input.md"),
        include_str!("fixtures/cases/footnote_continuation_tab_indent_pandoc/input.md"),
        include_str!("fixtures/cases/footnote_paragraph_no_list_interrupt/input.md"),
        include_str!("fixtures/cases/definition_list_in_nested_blockquote_pandoc/input.md"),
        include_str!("fixtures/cases/definition_list_nesting/input.md"),
        include_str!("fixtures/cases/footnote_definition_list/input.md"),
        include_str!("fixtures/cases/definition_list_orphan_marker/input.md"),
        include_str!("fixtures/cases/definition_term_trailing_whitespace/input.md"),
    ] {
        check(input, &options);
    }
}

#[test]
fn quarto_body_boundaries_and_extension_overrides() {
    let mut options = ParserOptions::for_flavor(Flavor::Quarto);
    for input in [
        "Term  \r\n:\tdefinition\r\n\r\nNext\r\n~   sibling",
        "Term\n:   first\n    lazy\n\n    > quoted\n\nOutside\n",
        "Term\n\n\n: orphan\n",
        "Term\n:   body\n\n: caption\n\na | b\n--|--\nx | y\n",
        "[^id]: opener\nunderindented prose\n[^next]: sibling\n\nOutside\n",
        "[^id]:\n    # Heading\n\n    text\n\nOutside\n",
        "[^id]: body\n    [^next]: indented\n\nOutside\n",
        "::: note\nTerm\n:   body\n:::\nOutside\n",
        "::: note\n[^id]: body\n:::\nOutside\n",
        "- [^id]: marker-line note\n",
        "> [^id]: quoted note\n>     continuation\n> [^next]: sibling\n",
        "Term\n:   definition\n\n    [^id]: note\n\nOutside\n",
        "Term\n:   > quoted\n    > body\n:   next definition\n",
        "[^id]: term\n    :   definition\n\n[^next]: sibling\n",
        "[^id]: body\n\n    - list\n    - sibling\n\nOutside\n",
        "[^id]: body\n\n    ```\n    [^other]: code\n    ```\n[^other]: sibling\n",
    ] {
        check(input, &options);
    }
    for compat in [
        panache_parser::PandocCompat::V3_7,
        panache_parser::PandocCompat::Latest,
    ] {
        options.pandoc_compat = compat;
        check("Term\n: a\n\n  short continuation\n\nOutside\n", &options);
    }
    options.extensions.definition_lists = false;
    options.extensions.footnotes = false;
    check("Term\n~   text\n\n[^n]: body\n", &options);
}

#[test]
fn quarto_chained_body_edits_preserve_cst() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    let mut text = format!(
        "# Intro\n\nTerm\n:   definition text\n\n{}[^note]: note body\n\n{}",
        "A separator paragraph.\n\n".repeat(30),
        "Unaffected paragraph.\n\n".repeat(40)
    );
    let mut tree = check(&text, &options);
    for (old, new) in [
        ("definition text", "modified text"),
        ("[^note]", "[^renamed]"),
    ] {
        let start = text.find(old).unwrap();
        let edit = Edit {
            range: start..start + old.len(),
            insert: new.into(),
        };
        text = edit.apply(&text);
        let expected = check(&text, &options);
        let attempt =
            experimental_block::reparse(&tree.green().to_owned(), &[], &edit, &text, &options);
        if old == "[^note]" {
            assert!(attempt.is_none());
        } else {
            let result = attempt.expect("definition body text can be spliced");
            let updated = SyntaxNode::new_root(result.green);
            assert_eq!(updated.green(), expected.green());
            assert!(result.errors.is_empty());
            assert!(std::ptr::eq(
                tree.last_child().unwrap().green(),
                updated.last_child().unwrap().green()
            ));
        }
        tree = expected;
    }
}

#[test]
fn quarto_note_preserves_nested_block_diagnostics() {
    let input = "[^note]:\n    ```{r}\n    #| label: [\n    print(1)\n    ```\n";
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    assert!(!parse_with_errors(input, Some(options.clone())).1.is_empty());
    check(input, &options);
}

#[test]
fn quarto_body_tables_preserve_nested_prefixes() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    let input = include_str!("fixtures/cases/definition_body_table_nested_containers/input.md");
    check(input, &options);
}

#[test]
fn quarto_definition_marker_ends_definition_body_block() {
    check(
        include_str!("fixtures/cases/definition_marker_ends_definition_body_block/input.md"),
        &ParserOptions::for_flavor(Flavor::Quarto),
    );
}

#[test]
fn quarto_definition_marker_ends_list_item_block() {
    check(
        include_str!("fixtures/cases/definition_marker_ends_list_item_block/input.md"),
        &ParserOptions::for_flavor(Flavor::Quarto),
    );
}

#[test]
fn quarto_definition_marker_promotes_definition_body_term() {
    check(
        include_str!("fixtures/cases/definition_marker_promotes_definition_body_term/input.md"),
        &ParserOptions::for_flavor(Flavor::Quarto),
    );
}

#[test]
fn quarto_definition_marker_tab_straddle() {
    check(
        include_str!("fixtures/cases/definition_marker_tab_straddle/input.md"),
        &ParserOptions::for_flavor(Flavor::Quarto),
    );
}

#[test]
fn quarto_definition_continuation_tab_indent_pandoc() {
    check(
        include_str!("fixtures/cases/definition_continuation_tab_indent_pandoc/input.md"),
        &ParserOptions::for_flavor(Flavor::Quarto),
    );
}

#[test]
fn quarto_body_continuations_preserve_cst() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for input in [
        "> [^n]: note\n>\nOutside\n",
        "> [^n]: note\n>\n>\nOutside\n",
        "[^n]:\nUnindented text\n",
        "Term\n:   ```\n    code\n    ```\noutside\n",
        "[^n]: note\n# Heading\n",
        "[^a]: first\n\n    > quote\n    [^b]: next\n",
        "[^n]: note\n\n    ```\n    sample\n```\n",
        "- Term\n  :   `first\n  second`\n",
        "> Intro\n> [^n]: note\n",
        "> Term\n> :   first\n> :   second\n",
    ] {
        check(input, &options);
    }
}

#[test]
fn quarto_body_opening_precedence_preserves_cst() {
    let mut options = ParserOptions::for_flavor(Flavor::Quarto);
    for input in [
        "[^n]: [link]: https://example.com\n",
        "[^n]: | one\n    | two\n",
        "Term\n:   ***\n",
        "Before.\n\n---\n: orphan\n",
        "Term\n:   A | B\n    --|--\n    X | Y\n",
        "Term\n:   text\n\n    # Heading\n",
        "[^n]: Heading\n-------\n",
    ] {
        check(input, &options);
    }
    options.extensions.fenced_code_blocks = false;
    check("~~~text\n:   meaning\n", &options);
}

#[test]
fn quarto_wrapped_body_table_captions() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for input in [
        "[^n]: note\n\n    Table: A caption\n    Continued caption\n\n    A | B\n    --|--\n    X | Y\n",
        "[^n]: note\n\n    A | B\n    --|--\n    X | Y\n\n    Table: A caption\n    Continued caption\n",
    ] {
        check(input, &options);
    }
}

#[test]
fn quarto_deferred_body_blocks_preserve_openers() {
    let mut options = ParserOptions::for_flavor(Flavor::Quarto);
    options.extensions.tex_math_gfm = true;
    let input = "[^n]: note\n\n    ```math\n    x\n    ```\n";
    let gap = experimental_block::try_parse_with_errors(input, &options).expect_err(input);
    assert_eq!(
        (gap.offset, gap.construct),
        (input.find("```math").unwrap(), "fenced math")
    );
}

#[test]
fn quarto_footnote_prose_containing_table_like_lines() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for body in [
        "    sentence with a literal | pipe\n\n",
        "    a sentence\n    - another sentence\n\n",
    ] {
        let input = format!("[^n]: note\n\n{}Outside\n", body.repeat(128));
        check(&input, &options);
    }
}

#[test]
fn grid_tables_preserve_cst_across_pandoc_compatibility_targets() {
    use panache_parser::{PandocCompat, syntax::SyntaxKind};

    let grid = "+-----+---------+\n| one | - first |\n|     | - next  |\n+=====+=========+\n| two | *three* |\n+-----+---------+\n";
    for compat in [PandocCompat::V3_11, PandocCompat::V3_12] {
        let mut options = ParserOptions::for_flavor(Flavor::Quarto);
        options.pandoc_compat = compat;
        for input in [
            grid.to_owned(),
            format!("Table: Leading caption\n\n{grid}"),
            format!("{grid}\n: Trailing caption\n"),
            grid.lines().map(|line| format!("> {line}\n")).collect(),
            format!(
                "- item\n\n{}\n- next\n",
                grid.lines()
                    .map(|line| format!("  {line}\n"))
                    .collect::<String>()
            ),
            format!(
                "[^note]: text\n\n{}\nOutside.\n",
                grid.lines()
                    .map(|line| format!("    {line}\n"))
                    .collect::<String>()
            ),
        ] {
            let tree = check(&input, &options);
            assert!(
                tree.descendants()
                    .any(|node| node.kind() == SyntaxKind::GRID_TABLE),
                "{input}"
            );
        }
        let mixed = include_str!("fixtures/cases/grid_table_mixed_blocks_pandoc/input.md");
        let tree = check(mixed, &options);
        let native = panache_parser::to_pandoc_ast_with_options(&tree, &options);
        let paragraph = if compat == PandocCompat::V3_11 {
            "Plain"
        } else {
            "Para"
        };
        assert!(
            native.contains(&format!("{paragraph} [ Str \"text\" ]")),
            "{native}"
        );
        assert!(native.contains("Header 1"), "{native}");

        check(&format!("::: note\n{grid}\n: Caption\n:::\n"), &options);
        check(&format!("A paragraph\n{grid}"), &options);
        options.extensions.grid_tables = false;
        check(
            "+-----+---------+\n| one | value   |\n+-----+---------+\n",
            &options,
        );
    }
}

#[test]
fn grid_table_container_fixtures_preserve_cst() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for input in [
        include_str!("fixtures/cases/blockquote_list_item_grid_table_rowspan/input.md"),
        include_str!("fixtures/cases/fenced_div_close_grid_table/input.qmd"),
        include_str!("fixtures/cases/grid_table_in_list_blockquote/input.md"),
        include_str!("fixtures/cases/grid_table_indented_is_paragraph/input.md"),
        include_str!("fixtures/cases/list_item_grid_table_caption_after_pandoc/input.md"),
        include_str!("fixtures/cases/list_item_grid_table_marker_line_pandoc/input.md"),
    ] {
        check(input, &options);
    }
}

#[test]
fn grid_table_boundaries() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for input in [
        "Table: Caption\n\n+---+\n| a |\n+---+\n",
        "# Heading\n+---+\n| a |\n+---+\n",
        "> +---+\n| a |\n> +---+\n",
        "- item\n\n  +---+\n| a |\n  +---+\n",
        "[^note]: +---+\n    | a |\n    +---+\n",
        "term\n\n:   +---+\n    | a |\n    +---+\n",
        "> ::: note\n> +---+\n> | a |\n> +---+\n>\n> : Caption\n> :::\n",
    ] {
        check(input, &options);
    }
}

#[test]
fn grid_table_cell_edit_preserves_cst() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    let input = format!(
        "# Intro\n\n+-------+-------+\n| alpha | beta  |\n+-------+-------+\n\n{}",
        "Unchanged paragraph.\n\n".repeat(60)
    );
    let tree = check(&input, &options);
    let offset = input.find("alpha").unwrap();
    let edit = Edit {
        range: offset..offset + "alpha".len(),
        insert: "gamma".into(),
    };
    let updated = edit.apply(&input);
    let expected = check(&updated, &options);
    let result =
        experimental_block::reparse(&tree.green().to_owned(), &[], &edit, &updated, &options)
            .expect("grid table cell edit can be spliced");
    assert_eq!(result.green, expected.green().to_owned());
    assert!(result.errors.is_empty());
}

#[test]
fn jupyter_table_lookahead_crosses_container_boundaries_safely() {
    let mut options = ParserOptions::for_flavor(Flavor::Quarto);
    options.extensions.tex_math_jupyter = true;
    check(
        "[^n]:\n    a | b\n    --|--\n    $x\n\nc | $y\nz$ | d\n--|--\n",
        &options,
    );
}

#[test]
fn grid_table_after_indented_quote_opener_is_lossless() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    check("10. a\n\n    > +---+\n    > | a |\n    > +---+\n", &options);
}

#[test]
fn grid_caption_ends_at_sibling_list_marker() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    check(
        "- +---+\n  | a |\n  +---+\n\n  Table: Caption\n- sibling\n",
        &options,
    );
}

#[test]
fn unfinished_grid_on_list_marker_line_stays_prose() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    check("- +---+\n  | a |\n  done\n", &options);
}

#[test]
fn pipe_caption_in_note_ends_at_div_closer() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    check(
        "[^n]: note\n\n    ::: note\n    a | b\n    --|--\n    x | y\n\n    : cap\n    :::\n",
        &options,
    );
}

#[test]
fn footnote_rule_indented_with_a_tab_is_not_setext() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    check("See [^n].\n\n[^n]:\n\t---\n", &options);
}

#[test]
fn lazy_definition_continuation_preserves_nested_list_indent() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    check("Term\n:   definition\n    - `one\n  two`\n", &options);
}

#[test]
fn lazy_quote_continuation_after_body_blank_classifies_heading() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for input in [
        "> Term\n> :   definition\n>\n# Heading\n",
        "> Term\n> :   definition\n>\n# Heading\nText inside the quote.\n",
    ] {
        check(input, &options);
    }
}

#[test]
fn dash_rules_do_not_claim_colon_prose_as_table_captions() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for input in [": prose\n\n- - -\n", "--- ---\n: prose\n"] {
        check(input, &options);
    }
}

#[test]
fn definition_opener_can_start_a_pipe_prefixed_term() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    check("Term\n:   | literal\n    : def\n", &options);
}

#[test]
fn pandoc_tilde_math_fence_does_not_interrupt_quoted_prose() {
    let mut options = ParserOptions::for_flavor(Flavor::Quarto);
    options.extensions.tex_math_gfm = true;
    check("> prose\n> ~~~math\n> x\n> ~~~\n", &options);
}

#[test]
fn dash_table_fixtures_preserve_cst() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for input in [
        include_str!("fixtures/cases/adjacent_simple_then_pipe_table_captions/input.md"),
        include_str!("fixtures/cases/definition_body_table_caption/input.md"),
        include_str!("fixtures/cases/definition_body_table_nested_containers/input.md"),
        include_str!("fixtures/cases/definition_body_table_no_caption/input.md"),
        include_str!("fixtures/cases/issue_224_simple_table_short_header_losslessness/input.md"),
        include_str!("fixtures/cases/issue_398_headerless_multiline_dash_closer/input.md"),
        include_str!("fixtures/cases/list_item_simple_table_self_indent_pandoc/input.md"),
        include_str!("fixtures/cases/multiline_table_basic/input.md"),
        include_str!("fixtures/cases/multiline_table_blank_after_opener/input.md"),
        include_str!("fixtures/cases/multiline_table_caption/input.md"),
        include_str!("fixtures/cases/multiline_table_caption_after/input.md"),
        include_str!("fixtures/cases/multiline_table_caption_in_blockquote/input.md"),
        include_str!("fixtures/cases/multiline_table_headerless/input.md"),
        include_str!("fixtures/cases/multiline_table_headerless_single_column/input.md"),
        include_str!("fixtures/cases/multiline_table_headerless_single_column_doc_start/input.md"),
        include_str!("fixtures/cases/multiline_table_headerless_single_column_no_closer/input.md"),
        include_str!("fixtures/cases/multiline_table_in_div_stops_at_closer/input.md"),
        include_str!("fixtures/cases/multiline_table_in_list_blockquote/input.md"),
        include_str!("fixtures/cases/multiline_table_inline_formatting/input.md"),
        include_str!("fixtures/cases/multiline_table_no_blank_rows/input.md"),
        include_str!("fixtures/cases/multiline_table_single_col_code_fence/input.md"),
        include_str!("fixtures/cases/multiline_table_single_row/input.md"),
        include_str!("fixtures/cases/multiline_table_spaced_top_border/input.md"),
        include_str!("fixtures/cases/multiline_table_stops_at_blockquote_blank/input.md"),
        include_str!("fixtures/cases/multiline_table_two_dash_full_width/input.md"),
        include_str!("fixtures/cases/multiline_table_two_dash_headerless_single_column/input.md"),
        include_str!("fixtures/cases/simple_table/input.md"),
        include_str!("fixtures/cases/simple_table_closer_before_div_closer/input.md"),
        include_str!("fixtures/cases/simple_table_header_closer_requires_rows/input.md"),
        include_str!("fixtures/cases/simple_table_header_with_closer/input.md"),
        include_str!("fixtures/cases/simple_table_headerless_multi_column_no_closer/input.md"),
        include_str!("fixtures/cases/simple_table_headerless_single_column/input.md"),
        include_str!("fixtures/cases/simple_table_headerless_single_column_no_closer/input.md"),
        include_str!("fixtures/cases/simple_table_in_div_stops_at_closer/input.md"),
        include_str!("fixtures/cases/simple_table_in_footnote_stops_at_note_marker/input.md"),
        include_str!("fixtures/cases/simple_table_in_list_blockquote/input.md"),
        include_str!("fixtures/cases/simple_table_in_list_item_stops_at_sibling_marker/input.md"),
        include_str!("fixtures/cases/simple_table_in_nested_item_stops_at_band_marker/input.md"),
        include_str!("fixtures/cases/simple_table_stops_at_div_closer_in_quote/input.md"),
        include_str!("fixtures/cases/simple_table_wide_cell/input.md"),
        ": Caption\n\nA    B\n--- ---\nx    y\n",
        "-------\nA    B\n--- ---\nx    y\n-------\n\n: Caption\n",
        "-------\nA    B\n--- ---\nx    y\n-------\n: Caption\n",
        "- item\n--- ---\nx    y\n",
        "- A    B\n  --- ---\n  x    y\n",
        "> ------\n> title\n----- ------\n> data\n> --------\n",
    ] {
        check(input, &options);
    }
}

#[test]
fn dash_tables_match_pinned_pandoc_native() {
    use panache_parser::{pandoc_ast::normalize_native, to_pandoc_ast_with_options};

    let options = ParserOptions::for_flavor(Flavor::Quarto);
    macro_rules! check_native {
        ($fixture:literal) => {{
            let input = include_str!(concat!(
                "fixtures/pandoc-conformance/corpus/",
                $fixture,
                "/input.md"
            ));
            let expected = include_str!(concat!(
                "fixtures/pandoc-conformance/corpus/",
                $fixture,
                "/expected.native"
            ));
            let tree = check(input, &options);
            assert_eq!(
                normalize_native(&to_pandoc_ast_with_options(&tree, &options)),
                normalize_native(expected),
                "{}",
                $fixture,
            );
        }};
    }
    check_native!("0123-imported-multiline_table_caption");
    check_native!("0124-imported-multiline_table_caption_after");
    check_native!("0490-block-simple-table-headerless-single-column");
    check_native!("0541-block-blockquote-simple-table-aligns");
}

#[test]
fn dash_tables_respect_extension_overrides() {
    use panache_parser::syntax::SyntaxKind;

    let simple = "A   B\n--- ---\nx   y\n";
    let multiline = "-------\nA   B\n--- ---\nx   y\n\nz   w\n-------\n";
    for (input, kind) in [
        (simple, SyntaxKind::SIMPLE_TABLE),
        (multiline, SyntaxKind::MULTILINE_TABLE),
    ] {
        let mut options = ParserOptions::for_flavor(Flavor::Quarto);
        let contains_table = |options: &ParserOptions| {
            check(input, options)
                .descendants()
                .any(|n| n.kind() == kind)
        };
        assert!(contains_table(&options));
        match kind {
            SyntaxKind::SIMPLE_TABLE => options.extensions.simple_tables = false,
            _ => options.extensions.multiline_tables = false,
        }
        assert!(!contains_table(&options));
    }

    let mut options = ParserOptions::for_flavor(Flavor::CommonMark);
    options.extensions.multiline_tables = true;
    check(
        include_str!("../../../tests/fixtures/cases/multiline_table_reference_commonmark/input.md"),
        &options,
    );
}

#[test]
fn dash_table_edits_preserve_cst() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for table in [
        "Name   Year\n------ ----\nalpha  2026\n",
        "-----------\nName   Year\n------ ----\nalpha  2026\n\nbeta   2027\n-----------\n",
    ] {
        let mut input = format!("# Intro\n\n{table}\n{}", "After.\n\n".repeat(60));
        let mut tree = check(&input, &options);
        for (old, new) in [("alpha", "gamma"), ("2026", "2028")] {
            let offset = input.find(old).unwrap();
            let edit = Edit {
                range: offset..offset + old.len(),
                insert: new.into(),
            };
            input = edit.apply(&input);
            let result =
                experimental_block::reparse(&tree.green().to_owned(), &[], &edit, &input, &options)
                    .expect("table cell edit can be spliced");
            let expected = check(&input, &options);
            assert_eq!(result.green, expected.green().to_owned());
            assert!(result.errors.is_empty());
            let updated = SyntaxNode::new_root(result.green);
            assert!(std::ptr::eq(
                tree.last_child().unwrap().green(),
                updated.last_child().unwrap().green()
            ));
            tree = updated;
        }
    }
}

#[test]
fn dash_table_respects_containing_blocks() {
    use panache_parser::syntax::SyntaxKind;

    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for (input, kind) in [
        ("> # Heading\n> --- ---\n> x    y\n", SyntaxKind::HEADING),
        (
            "> ```text\n> --- ---\n> x    y\n> ```\n",
            SyntaxKind::CODE_BLOCK,
        ),
        (
            "[^n]: Table: Caption\n\n    A    B\n    --- ---\n    x    y\n\nOutside.\n",
            SyntaxKind::TABLE_CAPTION,
        ),
        (
            "[^n]: ---------\n    A    B\n    --- ---\n    x    y\n    ---------\n\nOutside.\n",
            SyntaxKind::MULTILINE_TABLE,
        ),
    ] {
        assert!(
            check(input, &options)
                .descendants()
                .any(|node| node.kind() == kind)
        );
    }

    for input in [
        "- Table: Simple caption\n\n  A    B\n  --- ---\n  x    y\n\n- Next\n",
        "> - : Multiline caption\n>\n>   --------\n>   A    B\n>   --- ---\n>   x    y\n>   --------\n>\n> - Next\n",
        "- A    B\n  --- ---\n  x    y\n\n  : Trailing caption\n\n- Next\n",
        "> + --------\n>   A    B\n>   --- ---\n>   x    y\n>   --------\n>\n>   : Trailing caption\n>\n> + Next\n",
        "- Intro\n\n  A    B\n  --- ---\n  x    y\n\n  : Trailing caption\n\n- Next\n",
    ] {
        let tree = check(input, &options);
        let caption = tree
            .descendants()
            .find(|node| node.kind() == SyntaxKind::TABLE_CAPTION)
            .unwrap();
        assert!(
            caption
                .ancestors()
                .any(|node| node.kind() == SyntaxKind::LIST_ITEM),
            "{input}\n{tree:#?}"
        );
    }

    let input = "::: note\n\n- item\n\n  A    B\n  --- ---\n  x    y\n:::\n\nOutside.\n";
    assert!(
        !check(input, &options)
            .descendants()
            .any(|node| node.kind() == SyntaxKind::SIMPLE_TABLE)
    );
    for input in [
        "> ::: note\n> A    B\n> --- ---\n> x    y\n> :::\n\nOutside.\n",
        "> ::: note\n> --------\n> A    B\n> --- ---\n> x    y\n> --------\n>\n> : Caption\n> :::\n\nOutside.\n",
    ] {
        let tree = check(input, &options);
        assert!(
            tree.descendants()
                .any(|node| node.kind() == SyntaxKind::DIV_FENCE_CLOSE)
        );
        assert!(tree.descendants().any(|node| matches!(
            node.kind(),
            SyntaxKind::SIMPLE_TABLE | SyntaxKind::MULTILINE_TABLE
        )));
    }
}

#[test]
fn dash_table_review_container_regressions() {
    use panache_parser::syntax::SyntaxKind as K;
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for (input, kind, count) in [
        (
            "- > - A    B\n  >   --- ---\n  >   x    y\n  >\n  >   : Caption\n",
            K::TABLE_CAPTION,
            1,
        ),
        (
            "10. Intro\n\n    > A    B\n    > --- ---\n    > x    y\n",
            K::SIMPLE_TABLE,
            1,
        ),
        (
            "- > A    B\n  > --- ---\n  > x    y\n- sibling\n",
            K::LIST_ITEM,
            2,
        ),
        (
            "[^n]: Text\n\n    A    B\n    --- ---\n    x    y\nOther\n\nSee [^n].\n",
            K::TABLE_ROW,
            2,
        ),
        (
            "> ::: note\n> --------\n> A    B\n> --- ---\n> x    y\n>   :::\n> --------\n> :::\n",
            K::MULTILINE_TABLE,
            1,
        ),
        (
            "10. Intro\n\n    --------\n    A    B\n    --- ---\n    x    y\n    --------\n",
            K::MULTILINE_TABLE,
            1,
        ),
        (
            "10. Intro\n\n    Table: Caption\n\n    A    B\n    --- ---\n    x    y\n",
            K::TABLE_CAPTION,
            1,
        ),
        (
            "> A    B\n> --- ---\n> x    y\n\n: Caption\n",
            K::TABLE_CAPTION,
            0,
        ),
        (
            "A    B\n--- ---\nx    y\n--- ---\n: Caption\n",
            K::TABLE_ROW,
            1,
        ),
        (
            "- <https://x.test>    B\n  ----------------- ---\n  x                 y\n\n  : Caption\n",
            K::TABLE_CAPTION,
            1,
        ),
        (
            "- [x] A    B\n  --- ---\n  x    y\n\n  : Caption\n",
            K::TABLE_CAPTION,
            1,
        ),
    ] {
        let tree = check(input, &options);
        assert_eq!(
            tree.descendants()
                .filter(|node| node.kind() == kind)
                .count(),
            count,
            "{input}\n{tree:#?}"
        );
    }

    let task_header = check(
        "- [x] A    B\n  --- ---\n  x    y\n\n  : Caption\n",
        &options,
    );
    assert!(
        task_header
            .descendants()
            .any(|node| node.kind() == K::TABLE_HEADER && node.to_string().starts_with("[x]"))
    );
    assert!(
        !task_header
            .descendants_with_tokens()
            .any(|element| element.kind() == K::TASK_CHECKBOX)
    );

    let unicode = "* --- ---\n  A\n  abcé\n  --- ---\n";
    assert_eq!(
        parse_with_errors(unicode, Some(options.clone()))
            .0
            .to_string(),
        unicode
    );
    assert_eq!(
        experimental_block::try_parse_with_errors(unicode, &options)
            .unwrap()
            .0
            .to_string(),
        unicode
    );

    let mut disabled = options;
    disabled.extensions.table_captions = false;
    let tree = check("A    B\n--- ---\nx    y\n\nTable: Caption\n", &disabled);
    assert!(
        !tree
            .descendants()
            .any(|node| node.kind() == K::TABLE_CAPTION)
    );
}

#[test]
fn dash_table_review_incremental_regressions() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    use panache_parser::syntax::SyntaxKind as K;
    for (body, anchor, insert, kind) in [
        (
            "--- ---\na   b\n\nc   d\n-------\n\n",
            "-------",
            "\n",
            K::MULTILINE_TABLE,
        ),
        (
            "A    B\n--- ---\nx    y\n\nOutside.\n\n",
            "Outside.",
            "Table: Caption\n",
            K::TABLE_CAPTION,
        ),
    ] {
        let input = format!("{body}{}", "Unrelated paragraph.\n\n".repeat(60));
        let tree = check(&input, &options);
        let start = input.find(anchor).unwrap();
        let edit = Edit {
            range: start..start,
            insert: insert.into(),
        };
        let updated = edit.apply(&input);
        let full = check(&updated, &options);
        assert!(full.descendants().any(|node| node.kind() == kind));
        let reparsed =
            experimental_block::reparse(&tree.green().to_owned(), &[], &edit, &updated, &options);
        if let Some(reparsed) = reparsed {
            assert_eq!(reparsed.green, full.green().to_owned());
            assert!(reparsed.errors.is_empty());
        }
    }
}

#[test]
fn quarto_html_extension_overrides_preserve_cst() {
    use panache_parser::syntax::SyntaxKind as K;

    let input = "<div id=\"note\">\n\n**Body**\n\n</div>\n\n<pre>\n**Raw**\n</pre>\n";
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    let tree = check(input, &options);
    assert!(
        tree.descendants()
            .any(|node| node.kind() == K::HTML_BLOCK_DIV)
    );
    assert!(
        tree.descendants()
            .any(|node| node.kind() == K::HTML_BLOCK_RAW)
    );
    assert_eq!(
        tree.descendants()
            .filter(|node| node.kind() == K::STRONG)
            .count(),
        1
    );

    let mut overridden = options.clone();
    overridden.extensions.native_divs = false;
    for markdown in [false, true] {
        overridden.extensions.markdown_in_html_blocks = markdown;
        assert!(
            !check(input, &overridden)
                .descendants()
                .any(|node| node.kind() == K::HTML_BLOCK_DIV)
        );
    }

    overridden.extensions.raw_html = false;
    for input in [input, "> <div>**Body**</div>\n", "- <div>**Body**</div>\n"] {
        assert!(!check(input, &overridden).descendants().any(|node| {
            matches!(
                node.kind(),
                K::HTML_BLOCK | K::HTML_BLOCK_DIV | K::HTML_BLOCK_RAW
            )
        }));
    }
}

#[test]
fn quarto_html_container_boundaries_are_lossless() {
    use panache_parser::syntax::SyntaxKind as K;

    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for input in [
        include_str!("fixtures/cases/html_block_comment_in_list_blockquote/input.md"),
        "- > <!-- hi\n  > --> trailing\n  > text\n- next\n",
        "- > <div>\n  > inside\n- next\n  </div>\n",
        "- > prose\n  > <div>\n  > text\n  > </div>\n- next\n",
        "> - prose\n>   <div>\n>   text\n>   </div>\n> - next\n",
        "> - text\n>   \n>   <div>\n>   text\n>   </div>\n> - next\n",
        "- <pre>\n  **raw**\n  </pre>\n- next\n",
        "> <div>\n> **body**\n> </div>\n\nnext\n",
    ] {
        let tree = check(input, &options);
        let block = tree
            .descendants()
            .find(|node| matches!(node.kind(), K::HTML_BLOCK_DIV | K::HTML_BLOCK_RAW))
            .expect("a container owns the HTML block");
        assert!(!block.to_string().contains("next"), "{input}");
    }
}

#[test]
fn quarto_html_edits_preserve_cst() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    let mut input = format!(
        "# HTML\n\n> - <div id=\"a\">\n>   Body text here.\n>   </div>\n> - next\n\n{}",
        "Distant paragraph.\n\n".repeat(60)
    );
    let mut tree = check(&input, &options);
    for (before, after) in [
        ("a\">", "b\">"),
        ("Body text here.", "Body **text** here."),
        (">   </div>\n", ""),
        ("> - next\n", ">   </div>\n> - next\n"),
    ] {
        let start = input.find(before).unwrap();
        let edit = Edit {
            range: start..start + before.len(),
            insert: after.into(),
        };
        input = edit.apply(&input);
        let expected = check(&input, &options);
        let result =
            experimental_block::reparse(&tree.green().to_owned(), &[], &edit, &input, &options);
        if before == "Body text here." {
            assert!(
                result.is_some(),
                "a local HTML-body edit should reuse the rest of the tree"
            );
        }
        if let Some(result) = result {
            assert_eq!(result.green, expected.green().to_owned());
            assert!(result.errors.is_empty());
        }
        tree = expected;
    }
}

#[test]
fn quarto_html_edits_can_pair_across_a_reparse_region() {
    use panache_parser::syntax::SyntaxKind as K;

    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for (prefix, insert, kind) in [
        ("<div bad.name=x>\n\n", "</div>\n\n", K::HTML_BLOCK_RAW),
        ("<hr\n\n", ">\n\n", K::HTML_BLOCK),
    ] {
        let input = format!("{prefix}{}", "More text.\n\n".repeat(60));
        let tree = check(&input, &options);
        assert!(!tree.descendants().any(|node| node.kind() == kind));
        let start = input.rfind("More text.").unwrap();
        let edit = Edit {
            range: start..start,
            insert: insert.into(),
        };
        let updated = edit.apply(&input);
        let expected = check(&updated, &options);
        assert!(expected.descendants().any(|node| node.kind() == kind));
        assert!(
            experimental_block::reparse(&tree.green().to_owned(), &[], &edit, &updated, &options)
                .is_none()
        );
    }
}

#[test]
fn quarto_html_review_container_regressions() {
    use panache_parser::syntax::SyntaxKind as K;

    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for (input, expected_kind) in [
        (
            "- > Quote.\n  >\n  > ::: box\n  > <!-- a --> tail\n  > :::\n  > Outside\n",
            K::DIV_FENCE_CLOSE,
        ),
        (
            "- <div id=\"box\">hi</div>\n  more\n- after\n",
            K::HTML_BLOCK_DIV,
        ),
        ("> <hr\n> :   Definition\n", K::DEFINITION_LIST),
    ] {
        let tree = check(input, &options);
        assert!(
            tree.descendants().any(|node| node.kind() == expected_kind),
            "{tree:#?}"
        );
    }

    let input = "- >\n  > <!--a--><!--b--> tail\n  > next\n";
    let trees = [
        parse_with_errors(input, Some(options.clone())).0,
        experimental_block::try_parse_with_errors(input, &options)
            .unwrap()
            .0,
    ];
    for tree in trees {
        assert_eq!(tree.to_string(), input);
        assert_eq!(
            tree.descendants()
                .filter(|node| node.kind() == K::HTML_BLOCK_RAW)
                .count(),
            2
        );
        assert!(tree.descendants().any(|node| {
            node.kind() == K::PARAGRAPH && node.to_string().contains(" tail\n  > next")
        }));
    }

    let input = "Term\n\n:   > a\n    > <div\n    > id=x>\n    > </div>\n";
    let (tree, errors) = experimental_block::try_parse_with_errors(input, &options).unwrap();
    assert!(errors.is_empty());
    assert_eq!(tree.to_string(), input);
    assert!(
        tree.descendants_with_tokens()
            .filter_map(|element| element.into_token())
            .any(|token| token.kind() == K::ATTR_ID && token.text() == "x"),
        "{tree:#?}"
    );
}

#[test]
fn quarto_html_comment_fusion_stays_local() {
    use panache_parser::syntax::SyntaxKind as K;

    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for (tail, kind) in [
        ("  > <!--a--> tail\n  > next\n  >\n", K::PARAGRAPH),
        ("  > <!--a--> # Head\n", K::HEADING),
    ] {
        let input = format!("- > Quote.\n  >\n{}", tail.repeat(128));
        let tree = check(&input, &options);
        assert_eq!(
            tree.descendants()
                .filter(|node| node.kind() == K::HTML_BLOCK_RAW)
                .count(),
            128
        );
        assert_eq!(
            tree.descendants()
                .filter(|node| node.kind() == kind
                    && node
                        .prev_sibling()
                        .is_some_and(|node| node.kind() == K::HTML_BLOCK_RAW))
                .count(),
            128
        );
    }
}

#[test]
fn quarto_html_reparse_reuses_only_independent_prose() {
    use panache_parser::syntax::SyntaxKind as K;

    let options = ParserOptions::for_flavor(Flavor::Quarto);
    let input = "<div\n\nsome bad.name attribute\n>\n\nBody\n";
    let tree = check(input, &options);
    let start = input.find("bad.name").unwrap();
    let edit = Edit {
        range: start..start + "bad.name".len(),
        insert: "goodname".into(),
    };
    let updated = edit.apply(input);
    assert!(
        check(&updated, &options)
            .descendants()
            .any(|node| node.kind() == K::HTML_BLOCK_DIV)
    );
    assert!(
        experimental_block::reparse(&tree.green().to_owned(), &[], &edit, &updated, &options)
            .is_none()
    );

    let input = format!(
        "<div>closed</div>\n\n{}A person cant finish yet.\n",
        "Earlier text.\n\n".repeat(60)
    );
    let tree = check(&input, &options);
    let start = input.find("cant").unwrap() + 3;
    let edit = Edit {
        range: start..start,
        insert: "'".into(),
    };
    let updated = edit.apply(&input);
    let result =
        experimental_block::reparse(&tree.green().to_owned(), &[], &edit, &updated, &options)
            .expect("a closed tag cannot block token reuse in distant prose");
    assert_eq!(result.strategy, ReparseStrategy::Token);
    assert_eq!(result.green, check(&updated, &options).green().to_owned());
}

#[test]
fn quarto_html_preserves_list_and_quote_boundaries() {
    use panache_parser::{
        pandoc_ast::normalize_native, syntax::SyntaxKind as K, to_pandoc_ast_with_options,
    };
    let options = ParserOptions::for_flavor(Flavor::Quarto);

    let input = include_str!(
        "fixtures/pandoc-conformance/corpus/0616-block-html-before-sibling-fenced-div/input.md"
    );
    let expected = include_str!(
        "fixtures/pandoc-conformance/corpus/0616-block-html-before-sibling-fenced-div/expected.native"
    );
    for tree in [
        parse_with_errors(input, Some(options.clone())).0,
        experimental_block::try_parse_with_errors(input, &options)
            .unwrap()
            .0,
    ] {
        assert_eq!(tree.to_string(), input);
        assert_eq!(
            normalize_native(&to_pandoc_ast_with_options(&tree, &options)),
            normalize_native(expected)
        );
        assert_eq!(
            tree.descendants()
                .filter(|node| node.kind() == K::FENCED_DIV)
                .count(),
            1
        );
    }

    let input = "> - item\n  <div>body</div>\n";
    let tree = experimental_block::try_parse_with_errors(input, &options)
        .unwrap()
        .0;
    assert_eq!(tree.to_string(), input);
    assert!(
        tree.descendants()
            .any(|node| node.kind() == K::HTML_BLOCK_DIV)
    );

    for (input, owner) in [
        ("- <div>x</div>\nnext\n", K::LIST_ITEM),
        ("> <div>x</div>\nnext\n", K::BLOCK_QUOTE),
    ] {
        let tree = experimental_block::try_parse_with_errors(input, &options)
            .unwrap()
            .0;
        assert_eq!(tree.to_string(), input);
        let container = tree
            .descendants()
            .find(|node| node.kind() == owner)
            .unwrap();
        assert!(container.children().any(|node| {
            matches!(node.kind(), K::PARAGRAPH | K::PLAIN) && node.to_string() == "next\n"
        }));
    }
}

#[test]
fn quarto_html_task_item_keeps_matched_tags_together() {
    use panache_parser::{
        pandoc_ast::normalize_native, syntax::SyntaxKind as K, to_pandoc_ast_with_options,
    };
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    let input = "- [ ] <div>\n  **bold**\n  </div>\n";
    let tree = experimental_block::try_parse_with_errors(input, &options)
        .unwrap()
        .0;
    assert_eq!(tree.to_string(), input);
    assert_eq!(
        normalize_native(&to_pandoc_ast_with_options(&tree, &options)),
        normalize_native(
            r#"[ BulletList [[ Plain [Str "\9744"], Div ("", [], []) [Para [Strong [Str "bold"]]] ]] ]"#
        )
    );
    assert!(
        tree.descendants()
            .any(|node| node.kind() == K::HTML_BLOCK_DIV
                && node.to_string().starts_with("<div>")
                && node.descendants().any(|node| node.kind() == K::STRONG))
    );
}

#[test]
fn quarto_html_comment_preserves_a_fence_with_blank_content() {
    use panache_parser::syntax::SyntaxKind as K;
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    let input = "<!--a-->```javascript\n\nlet x=1;\n```\n\nAfter.\n";
    let tree = check(input, &options);
    let code = tree
        .descendants()
        .find(|node| node.kind() == K::CODE_BLOCK)
        .unwrap();
    assert_eq!(code.to_string(), "```javascript\n\nlet x=1;\n```\n");
}

#[test]
fn quarto_html_comment_before_complete_tags_stays_local() {
    use panache_parser::syntax::SyntaxKind as K;
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    let input = format!(
        "- > Intro.\n  >\n{}",
        "  > <!--a--><div>x</div>\n".repeat(500)
    );
    let tree = check(&input, &options);
    assert_eq!(
        tree.descendants()
            .filter(|node| node.kind() == K::HTML_BLOCK_DIV)
            .count(),
        500
    );
}

#[test]
fn quarto_html_looking_code_preserves_distant_token_reuse() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    let input = format!(
        "Inline <em>example</em>.\n\n```text\n{}>\n```\n\nA person cant finish yet.\n",
        "<div\n".repeat(8000)
    );
    let tree = check(&input, &options);
    let start = input.find("cant").unwrap() + 3;
    let edit = Edit {
        range: start..start,
        insert: "'".into(),
    };
    let updated = edit.apply(&input);
    let result =
        experimental_block::reparse(&tree.green().to_owned(), &[], &edit, &updated, &options)
            .expect("HTML-looking code leaves the distant prose edit eligible for reuse");
    assert_eq!(result.strategy, ReparseStrategy::Token);
    assert_eq!(result.green, check(&updated, &options).green().to_owned());
}

#[test]
fn quarto_html_comment_keeps_structural_tails_across_blank_lines() {
    use panache_parser::syntax::SyntaxKind as K;
    let options = ParserOptions::for_flavor(Flavor::Quarto);

    for (tail, kind) in [
        ("<pre>\n\n*raw*\n</pre>\n", K::HTML_BLOCK_RAW),
        ("<div>\n\n*body*\n\n</div>\n", K::HTML_BLOCK_DIV),
        ("::: note\n\n*body*\n\n:::\n", K::FENCED_DIV),
        ("::: note\n\n```text\n:::\n```\n\n:::\n", K::FENCED_DIV),
    ] {
        let input = format!("<!-- note -->{tail}\nAfter.\n");
        let tree = check(&input, &options);
        assert!(
            tree.children()
                .any(|child| child.kind() == kind && child.text() == tail)
        );
    }

    let input = "> <!-- note -->```text\n> line\n>";
    for tree in [
        parse_with_errors(input, Some(options.clone())).0,
        experimental_block::try_parse_with_errors(input, &options)
            .unwrap()
            .0,
    ] {
        assert_eq!(tree.text(), input);
    }
}

#[test]
fn quarto_html_body_rejects_a_fragment_that_changes_source_bytes() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    let input = "- <div> > Quote\n   : Definition\n  </div>\n";
    let (tree, _) = experimental_block::try_parse_with_errors(input, &options).unwrap();
    assert_eq!(tree.text(), input);
}

#[test]
fn quarto_html_keeps_lazy_headings_in_lists_and_quotes() {
    use panache_parser::{
        pandoc_ast::normalize_native, syntax::SyntaxKind as K, to_pandoc_ast_with_options,
    };
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for (input, owner, expected) in [
        (
            "> <hr>\n# Heading\n",
            K::BLOCK_QUOTE,
            r#"[ BlockQuote [RawBlock (Format "html") "<hr>", Header 1 ("heading", [], []) [Str "Heading"]] ]"#,
        ),
        (
            "- <div>x</div>\n# Heading\n- next\n",
            K::LIST_ITEM,
            r#"[ BulletList [[Div ("", [], []) [Plain [Str "x"]], Header 1 ("heading", [], []) [Str "Heading"]], [Plain [Str "next"]]] ]"#,
        ),
    ] {
        let tree = experimental_block::try_parse_with_errors(input, &options)
            .unwrap()
            .0;
        assert_eq!(tree.text(), input);
        let heading = tree
            .descendants()
            .find(|node| node.kind() == K::HEADING)
            .unwrap();
        assert_eq!(heading.parent().unwrap().kind(), owner);
        assert_eq!(
            normalize_native(&to_pandoc_ast_with_options(&tree, &options)),
            normalize_native(expected)
        );
    }
}

#[test]
fn pandoc_special_blocks_respect_extension_overrides() {
    use panache_parser::syntax::SyntaxKind as K;

    for (input, kind) in [
        (
            "% Title\n  continuation\n% Author\n% Date\n% Body\n",
            K::PANDOC_TITLE_BLOCK,
        ),
        (
            "| first\n  continued\n| second\n\nparagraph\n",
            K::LINE_BLOCK,
        ),
        (
            "\\newcommand{\\example}{\n  replacement\n}\n\\medskip\n\nparagraph\n",
            K::TEX_BLOCK,
        ),
        (
            "paragraph\n\\begin{tabular}{c}\nx\n\\end{tabular}\n\nfollowing\n",
            K::TEX_BLOCK,
        ),
    ] {
        for enabled in [true, false] {
            let mut options = ParserOptions::for_flavor(Flavor::Quarto);
            match kind {
                K::PANDOC_TITLE_BLOCK => options.extensions.pandoc_title_block = enabled,
                K::LINE_BLOCK => options.extensions.line_blocks = enabled,
                K::TEX_BLOCK => options.extensions.raw_tex = enabled,
                _ => unreachable!(),
            }
            let tree = check(input, &options);
            assert_eq!(tree.children().any(|node| node.kind() == kind), enabled);
        }
    }
}

#[test]
fn pandoc_special_block_fixtures_preserve_cst() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for input in [
        include_str!("fixtures/cases/pandoc_title_block/input.md"),
        include_str!("fixtures/cases/line_blocks/input.md"),
        include_str!("fixtures/cases/line_block_indented_marker_continues/input.md"),
        include_str!("fixtures/cases/line_block_in_list_blockquote/input.md"),
        include_str!("fixtures/cases/line_block_on_list_marker_line/input.md"),
        include_str!("fixtures/cases/line_block_in_list_item_lazy_pipe_line/input.md"),
        include_str!("fixtures/cases/raw_tex_commands/input.md"),
        include_str!("fixtures/cases/latex_environment/input.md"),
        include_str!("fixtures/cases/raw_tex_math_environment_pandoc/input.md"),
    ] {
        check(input, &options);
    }
}

#[test]
fn pandoc_special_block_boundaries_preserve_cst() {
    use panache_parser::syntax::SyntaxKind as K;

    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for input in [
        "# Heading\n\n% Ordinary prose\n",
        "Paragraph\n| still prose\n",
        " |\nnext\n",
        "Paragraph\n\\medskip\n",
        "\\begin{equation}\nx\n\\end{equation}\n",
    ] {
        let tree = check(input, &options);
        assert!(!tree.descendants().any(|node| matches!(
            node.kind(),
            K::PANDOC_TITLE_BLOCK | K::LINE_BLOCK | K::TEX_BLOCK
        )));
    }

    for input in [
        "|\r\n| a",
        " \\medskip\r\n\\hfill",
        "\\begin{tabular}{c}\r\nx\r\n\\end{tabular}",
        "> - item\n>\n>   \\medskip\n",
        "> alpha\n> \\begin{tabular}{c}\n> x\n> \\end{tabular}\n",
        "> \\newcommand{\\example}{\n>   replacement\n> }\n",
        "- item\n\n  \\begin{tabular}{c}\n  x\n  \\end{tabular}\n- sibling\n",
        "Note[^a].\n\n[^a]: body\n\n    | first\n    | second\n",
        "> | line\n> :   definition\n",
        "> - item\n>\n>   | first\n>   | second\n",
        "> - > | first\n>   > | second\n",
        "- > - | first\n  >   | second\n",
        "> - > \\medskip\n",
    ] {
        check(input, &options);
    }
}

#[test]
fn pandoc_special_blocks_match_pinned_pandoc_native() {
    use panache_parser::{pandoc_ast::normalize_native, to_pandoc_ast_with_options};

    let options = ParserOptions::for_flavor(Flavor::Quarto);
    macro_rules! check_native {
        ($fixture:literal) => {{
            let input = include_str!(concat!(
                "fixtures/pandoc-conformance/corpus/",
                $fixture,
                "/input.md"
            ));
            let expected = include_str!(concat!(
                "fixtures/pandoc-conformance/corpus/",
                $fixture,
                "/expected.native"
            ));
            let tree = check(input, &options);
            assert_eq!(
                normalize_native(&to_pandoc_ast_with_options(&tree, &options)),
                normalize_native(expected),
                "{}",
                $fixture,
            );
        }};
    }
    check_native!("0098-imported-line_blocks");
    check_native!("0130-imported-pandoc_title_block");
    check_native!("0141-imported-raw_tex_commands");
    check_native!("0095-imported-latex_environment");

    let quoted = "> \\begin{tabular}{c}\n> x\n> \\end{tabular}\n";
    let tree = check(quoted, &options);
    assert_eq!(
        normalize_native(&to_pandoc_ast_with_options(&tree, &options)),
        normalize_native(
            r#"[ BlockQuote [ RawBlock (Format "tex") "\\begin{tabular}{c}\nx\n\\end{tabular}" ] ]"#
        ),
    );
}

#[test]
fn pandoc_special_blocks_preserve_reviewed_container_boundaries() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for input in [
        "> - |\n>   # H\n",
        "> - item\n>\n>   |\n>   * * *\n",
        "Text[^n].\n\n[^n]: > \\medskip\n",
        "Term\n:   > \\medskip\n",
        "> \\medskip\n# Heading\n",
        "- | a\n# Heading\n",
        "- <!-- a -->\n  |\n  \\medskip\n",
        "::: note\n> \\begin{tabular}{c}x\\end{tabular}\n:::\n",
        "- <!-- a -->\n  |\n<div>\ntext\n</div>\n",
        "- ::: note\n  |\n  # H\n  :::\n",
        "- > text\n  >\n  > \\medskip\n",
        "- | a\nb\n",
        "> \\medskip\nb\n",
        "- <!-- a -->\n  |\n",
        "- <!-- a -->\n  |\n  b\n",
        "- <!-- a -->\n  |\n  # H\n",
        "- ::: note\n  |\n  b\n  :::\n",
    ] {
        check(input, &options);
    }
}

#[test]
fn bare_line_block_stops_at_the_sibling_list_item() {
    use panache_parser::syntax::SyntaxKind as K;

    let options = ParserOptions::for_flavor(Flavor::Quarto);
    let tree = check("- |\n - next\n", &options);
    let list = tree.first_child().unwrap();
    assert_eq!(
        list.children()
            .filter(|child| child.kind() == K::LIST_ITEM)
            .count(),
        2
    );
    assert_eq!(
        list.first_child().unwrap().first_child().unwrap().kind(),
        K::LINE_BLOCK
    );
}

#[test]
fn pandoc_native_normalizes_raw_tex_line_endings() {
    use panache_parser::to_pandoc_ast_with_options;

    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for input in ["\\medskip\n", "\\begin{tabular}{c}\nx\n\\end{tabular}\n"] {
        let crlf = input.replace('\n', "\r\n");
        assert_eq!(
            to_pandoc_ast_with_options(&check(&crlf, &options), &options),
            to_pandoc_ast_with_options(&check(input, &options), &options),
        );
    }
}

#[test]
fn reviewed_tex_and_verse_cases_are_lossless_and_match_the_incumbent() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for input in [
        "\\begin{center}% \\end{center}\nbody\n\\end{center}\n",
        "\\begin{center}\\verb|\\end{center}|\nbody\n\\end{center}\n",
        "> \\begin{center}\n> body \\end{center}\n\n# outside\n",
        "> > \\newcommand{\\x}{\n> hello\n> > }\n",
        "Text[^n].\n\n[^n]: > \\newcommand{\\foo}{\n\n# Outside\n\nText\n",
        "Term\n:   > \\newcommand{\\foo}{\n\n# Outside\n\nText\n",
        "Term\n:   > \\medskip\n    > ::: note\n    > x\n    > :::\n",
        "Text[^n].\n\n[^n]: > \\medskip\n    > ::: note\n    > x\n    > :::\n",
        "> - | verse\n<div>\nbody\n</div>\n",
        "> \\medskip\n| next\n",
        "> | verse\n* * *\n",
        "Term\n:   > | verse\n",
        "Text[^n].\n\n[^n]: > | verse\n",
        "- ::: note\n  \\medskip\n  :::\n",
        "> - item\n>\n>   \\begin{center}\n>   body\n>   \\end{center}\n>   Heading\n>   =======\n",
    ] {
        check(input, &options);
    }
}

#[test]
fn bare_verse_does_not_absorb_indented_content() {
    use panache_parser::syntax::SyntaxKind as K;

    let options = ParserOptions::for_flavor(Flavor::Quarto);
    for (input, following) in [
        ("|\n    code\n", K::CODE_BLOCK),
        ("|\n  | next\n", K::PARAGRAPH),
    ] {
        let tree = check(input, &options);
        assert_eq!(
            tree.children().map(|node| node.kind()).collect::<Vec<_>>(),
            [K::LINE_BLOCK, following]
        );
    }
}

#[test]
fn planner_defers_raw_environment_inside_open_inline_math() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    let input = "\\begin{equation}\n\\begin{bmatrix}1\\end{bmatrix}\n\\end{equation}\n";
    let gap = experimental_block::try_parse_with_errors(input, &options).unwrap_err();
    assert_eq!(gap.offset, "\\begin{equation}\n".len());
    assert_eq!(gap.construct, "nested inline math environment");
}
