use crate::block_parser::tests::helpers::{
    assert_block_kinds, count_children, find_all, find_first, get_text, parse_blocks,
};
use crate::syntax::SyntaxKind;

#[test]
fn simple_bullet_list() {
    let input = "* one\n* two\n* three\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 3);
}

#[test]
fn bullet_list_requires_space_after_marker() {
    let input = "*one\n*two\n";
    let tree = parse_blocks(input);
    // Should not parse as list
    assert!(find_first(&tree, SyntaxKind::List).is_none());
}

#[test]
fn bullet_list_with_different_markers() {
    let input = "* item\n+ item\n- item\n";
    let tree = parse_blocks(input);
    // Should create three separate lists (different markers)
    let lists = find_all(&tree, SyntaxKind::List);
    assert_eq!(lists.len(), 3);
}

#[test]
fn bullet_list_indented_1_to_3_spaces() {
    let input = " * one space\n  * two spaces\n   * three spaces\n";
    let tree = parse_blocks(input);
    // All should be valid list items
    let list_items = find_all(&tree, SyntaxKind::ListItem);
    assert_eq!(list_items.len(), 3);
}

#[test]
fn bullet_list_indented_4_spaces_is_code() {
    let input = "    * not a list\n";
    let tree = parse_blocks(input);
    // Should be code block, not list
    assert!(find_first(&tree, SyntaxKind::List).is_none());
}

#[test]
fn bullet_list_with_continuation() {
    let input = "* here is my first\n  list item.\n* and my second.\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 2);
}

#[test]
fn bullet_list_lazy_continuation() {
    let input = "* here is my first\nlist item.\n* and my second.\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 2);
}

#[test]
fn nested_bullet_lists() {
    let input = "* fruits\n  + apples\n  + pears\n* vegetables\n";
    let tree = parse_blocks(input);
    let outer_list = find_first(&tree, SyntaxKind::List).expect("should find outer list");
    assert_eq!(count_children(&outer_list, SyntaxKind::ListItem), 2);

    // Should have nested list inside first item
    let nested_lists = find_all(&tree, SyntaxKind::List);
    assert!(
        nested_lists.len() >= 2,
        "should have at least 2 lists (outer + nested)"
    );
}

#[test]
fn loose_list_with_blank_lines() {
    let input = "* one\n\n* two\n\n* three\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 3);
}

#[test]
fn simple_ordered_list() {
    let input = "1. one\n2. two\n3. three\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 3);
}

#[test]
fn ordered_list_numbers_ignored() {
    let input = "5. one\n7. two\n1. three\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 3);
}

#[test]
fn ordered_list_with_hash_marker() {
    let input = "#. one\n#. two\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 2);
}

#[test]
fn ordered_list_requires_space_after_marker() {
    let input = "1.one\n2.two\n";
    let tree = parse_blocks(input);
    // Should not parse as list
    assert!(find_first(&tree, SyntaxKind::List).is_none());
}

#[test]
fn mixed_markers_create_separate_lists() {
    let input = "(2) Two\n(5) Three\n1. Four\n* Five\n";
    let tree = parse_blocks(input);
    // Should create separate lists for each marker type
    let lists = find_all(&tree, SyntaxKind::List);
    assert!(lists.len() >= 3, "should have at least 3 separate lists");
}

#[test]
fn task_list_unchecked() {
    let input = "- [ ] unchecked task\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 1);
}

#[test]
fn task_list_checked() {
    let input = "- [x] checked task\n- [X] also checked\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 2);
}

#[test]
fn list_with_multiple_paragraphs() {
    let input = "* First paragraph.\n\n  Continued.\n\n* Second item.\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 2);
}

#[test]
fn list_after_blank_line() {
    let input = "\n* item\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::List).expect("should find list after blank");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 1);
}

#[test]
fn list_after_paragraph() {
    let input = "Not a list.\n\n* Now a list\n";
    let tree = parse_blocks(input);
    assert_block_kinds(
        input,
        &[
            SyntaxKind::PARAGRAPH,
            SyntaxKind::BlankLine,
            SyntaxKind::List,
        ],
    );
}

// Fancy lists tests - require fancy_lists extension

#[test]
fn fancy_list_lower_alpha_period() {
    use crate::config::{Config, Extensions};
    let config = Config {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "a. first\nb. second\nc. third\n";
    let tree = crate::block_parser::BlockParser::new(input, &config)
        .parse()
        .0;
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 3);
}

#[test]
fn fancy_list_lower_alpha_right_paren() {
    use crate::config::{Config, Extensions};
    let config = Config {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "a) first\nb) second\nc) third\n";
    let tree = crate::block_parser::BlockParser::new(input, &config)
        .parse()
        .0;
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 3);
}

#[test]
fn fancy_list_lower_alpha_parens() {
    use crate::config::{Config, Extensions};
    let config = Config {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "(a) first\n(b) second\n(c) third\n";
    let tree = crate::block_parser::BlockParser::new(input, &config)
        .parse()
        .0;
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 3);
}

#[test]
fn fancy_list_upper_alpha_period() {
    use crate::config::{Config, Extensions};
    let config = Config {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "A.  first\nB.  second\nC.  third\n";
    let tree = crate::block_parser::BlockParser::new(input, &config)
        .parse()
        .0;
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 3);
}

#[test]
fn fancy_list_upper_alpha_period_requires_two_spaces() {
    use crate::config::{Config, Extensions};
    let config = Config {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    // One space should NOT parse as list (to avoid false positives like "B. Russell")
    let input = "A. first\nB. second\n";
    let tree = crate::block_parser::BlockParser::new(input, &config)
        .parse()
        .0;
    assert!(find_first(&tree, SyntaxKind::List).is_none());

    // Two spaces SHOULD parse as list
    let input_valid = "A.  first\nB.  second\n";
    let tree_valid = crate::block_parser::BlockParser::new(input_valid, &config)
        .parse()
        .0;
    let list = find_first(&tree_valid, SyntaxKind::List).expect("should find list with 2 spaces");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 2);
}

#[test]
fn fancy_list_lower_roman_period() {
    use crate::config::{Config, Extensions};
    let config = Config {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "i. first\nii. second\niii. third\n";
    let tree = crate::block_parser::BlockParser::new(input, &config)
        .parse()
        .0;
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 3);
}

#[test]
fn fancy_list_lower_roman_right_paren() {
    use crate::config::{Config, Extensions};
    let config = Config {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "i) first\nii) second\niii) third\n";
    let tree = crate::block_parser::BlockParser::new(input, &config)
        .parse()
        .0;
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 3);
}

#[test]
fn fancy_list_lower_roman_parens() {
    use crate::config::{Config, Extensions};
    let config = Config {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "(i) first\n(ii) second\n(iii) third\n";
    let tree = crate::block_parser::BlockParser::new(input, &config)
        .parse()
        .0;
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 3);
}

#[test]
fn fancy_list_upper_roman_period() {
    use crate::config::{Config, Extensions};
    let config = Config {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "I. first\nII. second\nIII. third\n";
    let tree = crate::block_parser::BlockParser::new(input, &config)
        .parse()
        .0;
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 3);
}

#[test]
fn fancy_list_upper_roman_right_paren() {
    use crate::config::{Config, Extensions};
    let config = Config {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "I) first\nII) second\nIII) third\n";
    let tree = crate::block_parser::BlockParser::new(input, &config)
        .parse()
        .0;
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 3);
}

#[test]
fn fancy_list_disabled_when_extension_off() {
    // With fancy_lists disabled, alphabetic markers should not parse as lists
    use crate::config::{Config, Extensions};
    let config = Config {
        extensions: Extensions {
            fancy_lists: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "a. first\nb. second\n";
    let (tree, _) = crate::block_parser::BlockParser::new(input, &config).parse();
    assert!(find_first(&tree, SyntaxKind::List).is_none());
}

#[test]
fn fancy_list_complex_roman() {
    use crate::config::{Config, Extensions};
    let config = Config {
        extensions: Extensions {
            fancy_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input =
        "iv. fourth\nv. fifth\nvi. sixth\nvii. seventh\nviii. eighth\nix. ninth\nx. tenth\n";
    let tree = crate::block_parser::BlockParser::new(input, &config)
        .parse()
        .0;
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 7);
}

// Example lists tests - require example_lists extension

#[test]
fn example_list_basic() {
    use crate::config::{Config, Extensions};
    let config = Config {
        extensions: Extensions {
            example_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "(@) First example\n(@) Second example\n(@) Third example\n";
    let tree = crate::block_parser::BlockParser::new(input, &config)
        .parse()
        .0;
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 3);
}

#[test]
fn example_list_with_labels() {
    use crate::config::{Config, Extensions};
    let config = Config {
        extensions: Extensions {
            example_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "(@good) This is a good example\n(@bad) This is a bad example\n";
    let tree = crate::block_parser::BlockParser::new(input, &config)
        .parse()
        .0;
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 2);
}

#[test]
fn example_list_mixed_labeled_unlabeled() {
    use crate::config::{Config, Extensions};
    let config = Config {
        extensions: Extensions {
            example_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "(@) First example\n(@foo) Labeled example\n(@) Another example\n";
    let tree = crate::block_parser::BlockParser::new(input, &config)
        .parse()
        .0;
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 3);
}

#[test]
fn example_list_separated_by_text() {
    use crate::config::{Config, Extensions};
    let config = Config {
        extensions: Extensions {
            example_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    // According to spec, example lists can be separated and continue numbering
    let input = "(@) First example\n\nSome text.\n\n(@) Second example\n";
    let tree = crate::block_parser::BlockParser::new(input, &config)
        .parse()
        .0;
    let lists = find_all(&tree, SyntaxKind::List);
    // Should have 2 separate lists
    assert_eq!(lists.len(), 2);
    // Each should have 1 item
    assert_eq!(count_children(&lists[0], SyntaxKind::ListItem), 1);
    assert_eq!(count_children(&lists[1], SyntaxKind::ListItem), 1);
}

#[test]
fn example_list_disabled_when_extension_off() {
    // With example_lists disabled, (@) should not parse as a list
    use crate::config::{Config, Extensions};
    let config = Config {
        extensions: Extensions {
            example_lists: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "(@) example\n";
    let (tree, _) = crate::block_parser::BlockParser::new(input, &config).parse();
    assert!(find_first(&tree, SyntaxKind::List).is_none());
}

#[test]
fn example_list_with_underscores_and_hyphens() {
    use crate::config::{Config, Extensions};
    let config = Config {
        extensions: Extensions {
            example_lists: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let input = "(@my_label) Example with underscore\n(@my-label) Example with hyphen\n";
    let tree = crate::block_parser::BlockParser::new(input, &config)
        .parse()
        .0;
    let list = find_first(&tree, SyntaxKind::List).expect("should find list");
    assert_eq!(count_children(&list, SyntaxKind::ListItem), 2);
}
