#![cfg(feature = "experimental-block-parser")]

use panache_parser::parser::{Edit, ReparseStrategy, experimental_block, parse_with_errors};
use panache_parser::{Flavor, ParserOptions, SyntaxNode};

fn check(input: &str, options: &ParserOptions) -> SyntaxNode {
    let (expected, errors) = parse_with_errors(input, Some(options.clone()));
    let (actual, actual_errors) =
        experimental_block::try_parse_with_errors(input, options).unwrap();
    assert_eq!(actual.to_string(), input);
    assert_eq!(actual.green(), expected.green(), "{input}");
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
