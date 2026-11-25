#![cfg(feature = "experimental-block-parser")]

use panache_parser::parser::{Edit, ReparseStrategy, experimental_block, parse_with_errors};
use panache_parser::{Flavor, ParserOptions, SyntaxNode};

fn check(input: &str, options: &ParserOptions) -> SyntaxNode {
    let (expected, errors) = parse_with_errors(input, Some(options.clone()));
    let (actual, actual_errors) =
        experimental_block::try_parse_with_errors(input, options).unwrap();
    assert_eq!(actual.to_string(), input);
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
            "- > first\r\n  > continued\r\n- next",
            "> - # heading\n>   text\n",
        ] {
            check(input, &options);
        }
    }
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
    assert!(
        experimental_block::reparse(
            &tree.green().to_owned(),
            &[],
            &edit,
            &edit.apply(&text),
            &options,
        )
        .is_none()
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
fn unsupported_nested_block_points_to_its_content() {
    let options = ParserOptions::for_flavor(Flavor::CommonMark);
    let input = "Intro\n\n> - <div>\n";
    let gap = experimental_block::try_parse_with_errors(input, &options).unwrap_err();
    assert_eq!(gap.offset, input.find("<div>").unwrap());
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
fn experimental_parser_declines_unimplemented_container_forms() {
    for (flavor, input) in [
        (Flavor::CommonMark, "> A\n> ---\n"),
        (Flavor::CommonMark, "-     code\n"),
        (Flavor::Quarto, "- [x] done\n"),
        (Flavor::Quarto, "- > foo\n  \tcode\n"),
        (Flavor::CommonMark, "- > foo\n- > - bar\n \n  \t- nested\n"),
    ] {
        let options = ParserOptions::for_flavor(flavor);
        assert!(
            experimental_block::try_parse_with_errors(input, &options).is_err(),
            "{input:?}"
        );
    }
}

#[test]
fn four_space_rule_uses_configured_list_content_column() {
    let mut options = ParserOptions::for_flavor(Flavor::Quarto);
    options.extensions.four_space_rule = true;
    check("- first\n    - child\n", &options);
    assert!(experimental_block::try_parse_with_errors("- first\n  - second\n", &options).is_err());
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
fn declines_reference_definitions_with_optional_indent_inside_quotes() {
    let options = ParserOptions::for_flavor(Flavor::CommonMark);
    for input in ["> [id]: /url\n", ">  [id]: /url\n", ">    [id]: /url\n"] {
        assert!(experimental_block::try_parse_with_errors(input, &options).is_err());
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
