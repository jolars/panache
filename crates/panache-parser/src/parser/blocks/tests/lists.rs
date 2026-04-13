use super::helpers::{
    assert_block_kinds, count_children, find_all, find_first, parse_blocks,
    parse_blocks_with_config,
};
use crate::options::{Extensions, Flavor, ParserOptions};
use crate::syntax::SyntaxKind;

#[test]
fn simple_bullet_list() {
    let input = "* one\n* two\n* three\n";
    let config = ParserOptions {
        flavor: Flavor::Quarto,
        extensions: Extensions::for_flavor(Flavor::Quarto),
        ..Default::default()
    };
    assert!(
        config.extensions.fenced_divs,
        "fenced_divs should be enabled for this test"
    );
    let tree = parse_blocks_with_config(input, &config);
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn bullet_list_requires_space_after_marker() {
    let input = "*one\n*two\n";
    let tree = parse_blocks(input);
    // Should not parse as list
    assert!(find_first(&tree, SyntaxKind::LIST).is_none());
}

#[test]
fn bullet_list_with_different_markers() {
    let input = "* item\n+ item\n- item\n";
    let tree = parse_blocks(input);
    // Should create ONE list (bullet markers are all equivalent per Pandoc)
    let lists = find_all(&tree, SyntaxKind::LIST);
    assert_eq!(lists.len(), 1);
}

#[test]
fn bullet_list_indented_1_to_3_spaces() {
    let input = " * one space\n  * two spaces\n   * three spaces\n";
    let tree = parse_blocks(input);
    // All should be valid list items
    let list_items = find_all(&tree, SyntaxKind::LIST_ITEM);
    assert_eq!(list_items.len(), 3);
}

#[test]
fn bullet_list_indented_4_spaces_is_code() {
    let input = "    * not a list\n";
    let tree = parse_blocks(input);
    // Should be code block, not list
    assert!(find_first(&tree, SyntaxKind::LIST).is_none());
}

#[test]
fn bullet_list_with_continuation() {
    let input = "* here is my first\n  list item.\n* and my second.\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 2);
}

#[test]
fn bullet_list_lazy_continuation() {
    let input = "* here is my first\nlist item.\n* and my second.\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 2);
}

#[test]
fn list_item_can_start_with_atx_heading() {
    let input = "- # Heading\n";
    let tree = parse_blocks(input);

    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    let list_item = list
        .children()
        .find(|n| n.kind() == SyntaxKind::LIST_ITEM)
        .expect("should find list item");

    assert!(
        find_first(&list_item, SyntaxKind::HEADING).is_some(),
        "list item should contain HEADING"
    );
    assert!(
        find_first(&list_item, SyntaxKind::PLAIN).is_none(),
        "heading-only list item should not be parsed as PLAIN"
    );
}

#[test]
fn nested_bullet_lists() {
    let input = "* fruits\n  + apples\n  + pears\n* vegetables\n";
    let tree = parse_blocks(input);
    let outer_list = find_first(&tree, SyntaxKind::LIST).expect("should find outer list");
    assert_eq!(count_children(&outer_list, SyntaxKind::LIST_ITEM), 2);

    // Should have nested list inside first item
    let nested_lists = find_all(&tree, SyntaxKind::LIST);
    assert!(
        nested_lists.len() >= 2,
        "should have at least 2 lists (outer + nested)"
    );
}

#[test]
fn outdented_item_after_nested_list_returns_to_outer_level() {
    let input = "* Item 1\n  + Nested item\n      *  Deeply nested\n +  Item 2\n";
    let tree = parse_blocks(input);
    let lists = find_all(&tree, SyntaxKind::LIST);

    let outer_list = lists.first().expect("should have an outer list");
    assert_eq!(count_children(outer_list, SyntaxKind::LIST_ITEM), 2);

    let top_level_items: Vec<_> = outer_list
        .children()
        .filter(|n| n.kind() == SyntaxKind::LIST_ITEM)
        .collect();
    let first_item = top_level_items
        .first()
        .expect("should have first list item");
    let second_item = top_level_items
        .get(1)
        .expect("should have second list item");

    assert!(
        find_first(first_item, SyntaxKind::LIST).is_some(),
        "first item should keep nested list"
    );
    assert!(
        find_first(second_item, SyntaxKind::LIST).is_none(),
        "second item should be at outer level, not nested"
    );
}

#[test]
fn fancy_list_continuation_with_nested_list_is_not_indented_code() {
    use crate::options::{Extensions, ParserOptions};

    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "(2) begins with 2\n(3) and now 3\n\n    with a continuation\n\n    iv. sublist with roman numerals,\n        starting with 4\n    v.  more items\n        (A)  a subsublist\n        (B)  a subsublist\n";

    let tree = crate::parser::Parser::new(input, &config).parse();

    assert!(
        find_first(&tree, SyntaxKind::CODE_BLOCK).is_none(),
        "continuation content should not parse as indented code"
    );

    let lists = find_all(&tree, SyntaxKind::LIST);
    assert!(
        lists.len() >= 3,
        "should contain outer, nested roman, and nested alpha lists"
    );
}

#[test]
fn loose_list_with_blank_lines() {
    let input = "* one\n\n* two\n\n* three\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn simple_ordered_list() {
    let input = "1. one\n2. two\n3. three\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn ordered_list_numbers_ignored() {
    let input = "5. one\n7. two\n1. three\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn ordered_list_with_hash_marker() {
    let input = "#. one\n#. two\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 2);
}

#[test]
fn ordered_list_requires_space_after_marker() {
    let input = "1.one\n2.two\n";
    let tree = parse_blocks(input);
    // Should not parse as list
    assert!(find_first(&tree, SyntaxKind::LIST).is_none());
}

#[test]
fn mixed_markers_create_separate_lists() {
    let input = "(2) Two\n(5) Three\n1. Four\n* Five\n";
    let tree = parse_blocks(input);
    // Should create separate lists for each marker type
    let lists = find_all(&tree, SyntaxKind::LIST);
    assert!(lists.len() >= 3, "should have at least 3 separate lists");
}

#[test]
fn task_list_unchecked() {
    let input = "- [ ] unchecked task\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 1);
}

#[test]
fn task_list_checked() {
    let input = "- [x] checked task\n- [X] also checked\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 2);
}

#[test]
fn list_with_multiple_paragraphs() {
    let input = "* First paragraph.\n\n  Continued.\n\n* Second item.\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 2);
}

#[test]
fn list_after_blank_line() {
    let input = "\n* item\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list after blank");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 1);
}

#[test]
fn list_after_paragraph() {
    let input = "Not a list.\n\n* Now a list\n";
    assert_block_kinds(
        input,
        &[
            SyntaxKind::PARAGRAPH,
            SyntaxKind::BLANK_LINE,
            SyntaxKind::LIST,
        ],
    );
}

#[test]
fn list_item_with_valid_fenced_divs_parses_as_fenced_div_nodes() {
    let input = "2.  Once your repository is created, clone it to your local computer.\n\n    ::: {.content-visible unless-meta=\"tool.is_rstudio\"}\n    You can do this any way you are comfortable, for instance in the Terminal, it might look like:\n\n    ``` {.bash filename=\"Terminal\"}\n    git clone git@github.com:<username>/<repo-name>.git\n    ```\n\n    Where you use your own user name and repo name.\n    :::\n\n    ::: {.content-visible when-meta=\"tool.is_rstudio\"}\n    You can do this any way you are comfortable, but one approach is to use **File** > **New Project**. In the **New Project** dialog, select **From Version Control**, then **Git**, and copy and paste the repo URL from GitHub.\n    :::\n";
    let tree = parse_blocks(input);
    let list_item = find_first(&tree, SyntaxKind::LIST_ITEM).expect("list item");
    let fenced_divs = find_all(&list_item, SyntaxKind::FENCED_DIV);
    assert_eq!(
        fenced_divs.len(),
        2,
        "expected two fenced divs inside list item"
    );
}

// Fancy lists tests - require fancy_lists extension

#[test]
fn fancy_list_lower_alpha_period() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "a. first\nb. second\nc. third\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn fancy_list_lower_alpha_right_paren() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "a) first\nb) second\nc) third\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn fancy_list_lower_alpha_parens() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "(a) first\n(b) second\n(c) third\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn fancy_list_upper_alpha_period() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "A.  first\nB.  second\nC.  third\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn fancy_list_upper_alpha_period_requires_two_spaces() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    // One space should NOT parse as list (to avoid false positives like "B. Russell")
    let input = "A. first\nB. second\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    assert!(find_first(&tree, SyntaxKind::LIST).is_none());

    // Two spaces SHOULD parse as list
    let input_valid = "A.  first\nB.  second\n";
    let tree_valid = crate::parser::Parser::new(input_valid, &config).parse();
    let list = find_first(&tree_valid, SyntaxKind::LIST).expect("should find list with 2 spaces");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 2);
}

#[test]
fn fancy_list_lower_roman_period() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "i. first\nii. second\niii. third\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn fancy_list_lower_roman_right_paren() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "i) first\nii) second\niii) third\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn fancy_list_lower_roman_parens() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "(i) first\n(ii) second\n(iii) third\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn fancy_list_upper_roman_period() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "I. first\nII. second\nIII. third\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn fancy_list_upper_roman_right_paren() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "I) first\nII) second\nIII) third\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn fancy_list_disabled_when_extension_off() {
    // With fancy_lists disabled, alphabetic markers should not parse as lists
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "a. first\nb. second\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    assert!(find_first(&tree, SyntaxKind::LIST).is_none());
}

#[test]
fn fancy_list_hash_marker_disabled_when_extension_off() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "#. first\n#. second\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    assert!(find_first(&tree, SyntaxKind::LIST).is_none());
}

#[test]
fn fancy_list_right_paren_decimal_disabled_when_extension_off() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "1) first\n2) second\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    assert!(find_first(&tree, SyntaxKind::LIST).is_none());
}

#[test]
fn fancy_list_parens_decimal_disabled_when_extension_off() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "(1) first\n(2) second\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    assert!(find_first(&tree, SyntaxKind::LIST).is_none());
}

#[test]
fn fancy_list_complex_roman() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input =
        "iv. fourth\nv. fifth\nvi. sixth\nvii. seventh\nviii. eighth\nix. ninth\nx. tenth\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 7);
}

// Example lists tests - require example_lists extension

#[test]
fn example_list_basic() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            example_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "(@) First example\n(@) Second example\n(@) Third example\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn example_list_with_labels() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            example_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "(@good) This is a good example\n(@bad) This is a bad example\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 2);
}

#[test]
fn example_list_mixed_labeled_unlabeled() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            example_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "(@) First example\n(@foo) Labeled example\n(@) Another example\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 3);
}

#[test]
fn example_list_separated_by_text() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            example_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    // According to spec, example lists can be separated and continue numbering
    let input = "(@) First example\n\nSome text.\n\n(@) Second example\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let lists = find_all(&tree, SyntaxKind::LIST);
    // Should have 2 separate lists
    assert_eq!(lists.len(), 2);
    // Each should have 1 item
    assert_eq!(count_children(&lists[0], SyntaxKind::LIST_ITEM), 1);
    assert_eq!(count_children(&lists[1], SyntaxKind::LIST_ITEM), 1);
}

#[test]
fn example_list_disabled_when_extension_off() {
    // With example_lists disabled, (@) should not parse as a list
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            example_lists: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "(@) example\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    assert!(find_first(&tree, SyntaxKind::LIST).is_none());
}

#[test]
fn example_list_with_underscores_and_hyphens() {
    use crate::options::{Extensions, ParserOptions};
    let config = ParserOptions {
        extensions: Extensions {
            example_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "(@my_label) Example with underscore\n(@my-label) Example with hyphen\n";
    let tree = crate::parser::Parser::new(input, &config).parse();
    let list = find_first(&tree, SyntaxKind::LIST).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::LIST_ITEM), 2);
}
