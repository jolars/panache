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
    let gap = experimental_block::try_parse_with_errors(input, &quarto).unwrap_err();
    assert_eq!(gap.construct, "simple table");
    assert_eq!(gap.offset, input.find("---").unwrap());
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
fn quarto_body_reports_deferred_child_offset() {
    let options = ParserOptions::for_flavor(Flavor::Quarto);
    let input = include_str!("fixtures/cases/definition_body_table_nested_containers/input.md");
    let gap = experimental_block::try_parse_with_errors(input, &options).unwrap_err();
    assert_eq!(gap.construct, "simple table");
    assert_eq!(gap.offset, input.find("A    B").unwrap());
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
    for (input, opener, expected) in [
        (
            ": Caption\n\nA    B\n--- ---\nx    y\n",
            ": Caption",
            "extended Pandoc block",
        ),
        (
            "-------\nA    B\n--- ---\nx    y\n-------\n\n: Caption\n",
            ": Caption",
            "extended Pandoc block",
        ),
        (
            "-------\nA    B\n--- ---\nx    y\n-------\n: Caption\n",
            ": Caption",
            "extended Pandoc block",
        ),
        ("> | line\n> :   definition\n", "| line", "line block"),
        (
            "> <div>\n> :   content\n> </div>\n",
            "<div>",
            "Pandoc HTML block",
        ),
        (
            "[^n]: note\n\n    ```math\n    x\n    ```\n",
            "```math",
            "fenced math",
        ),
    ] {
        let gap = experimental_block::try_parse_with_errors(input, &options).expect_err(input);
        assert_eq!(
            (gap.offset, gap.construct),
            (input.find(opener).unwrap(), expected),
            "{input}"
        );
    }
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
