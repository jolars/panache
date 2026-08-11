use super::helpers::{
    assert_block_kinds, assert_block_kinds_for_node, find_all, find_first, parse_blocks,
};
use crate::syntax::SyntaxKind;

#[test]
fn definition_list_allows_nested_list_after_blank_line() {
    let input = "Term\n\n:  Definition\n\n    - Bullet\n";
    let tree = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::DEFINITION_LIST]);
    assert!(
        find_first(&tree, SyntaxKind::LIST).is_some(),
        "Expected list to be nested inside definition"
    );
}

#[test]
fn definition_list_plain_starts_list_at_content_column_without_blank_line() {
    // A list marker indented to the definition's content column starts a nested
    // list inside the definition, even without a separating blank line. Matches
    // pandoc-native (`pandoc -f markdown -t native`).
    let input = "A definition list with nested items\n:   Here comes a list (or wait, is it?)\n    - A\n    - B\n";
    let tree = parse_blocks(input);

    assert_block_kinds(input, &[SyntaxKind::DEFINITION_LIST]);

    let definition = find_first(&tree, SyntaxKind::DEFINITION).expect("definition");
    assert!(
        find_first(&definition, SyntaxKind::PLAIN).is_some(),
        "definition should contain PLAIN for the leading text"
    );
    let nested_list = find_first(&definition, SyntaxKind::LIST).expect("nested list");
    assert_eq!(
        nested_list
            .children()
            .filter(|child| child.kind() == SyntaxKind::LIST_ITEM)
            .count(),
        2,
        "nested list should contain both items"
    );
}

#[test]
fn definition_list_content_starting_with_list_marker_parses_as_list() {
    let input = "Term\n:   - One\n    - Two\n";
    let tree = parse_blocks(input);

    let definition = find_first(&tree, SyntaxKind::DEFINITION).expect("should find definition");

    assert!(
        find_first(&definition, SyntaxKind::LIST).is_some(),
        "definition should contain LIST when content starts with list marker"
    );

    let has_direct_plain_child = definition
        .children()
        .any(|child| child.kind() == SyntaxKind::PLAIN);
    assert!(
        !has_direct_plain_child,
        "list-only definition should not have a direct PLAIN child"
    );
}

#[test]
fn definition_marker_without_content_preserves_newline_losslessly() {
    let input = "Input\n:   \n\n````markdown\n";
    let tree = parse_blocks(input);

    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn definition_content_can_start_with_atx_heading() {
    let input = "Term\n: # Header\n";
    let tree = parse_blocks(input);

    let definition = find_first(&tree, SyntaxKind::DEFINITION).expect("should find definition");

    assert!(
        find_first(&definition, SyntaxKind::HEADING).is_some(),
        "definition should contain HEADING"
    );
    assert!(
        find_first(&definition, SyntaxKind::PLAIN).is_none(),
        "heading-only definition should not be parsed as PLAIN"
    );
}

#[test]
fn definition_list_continues_across_blank_lines_with_additional_definitions() {
    let input = "Term\n: Def\n\n: Def\n";
    let tree = parse_blocks(input);

    let definition_lists = find_all(&tree, SyntaxKind::DEFINITION_LIST);
    assert_eq!(
        definition_lists.len(),
        1,
        "should remain one definition list"
    );

    let definition_items = find_all(&tree, SyntaxKind::DEFINITION_ITEM);
    assert_eq!(
        definition_items.len(),
        1,
        "should remain one definition item"
    );

    let definitions = find_all(&tree, SyntaxKind::DEFINITION);
    assert_eq!(
        definitions.len(),
        2,
        "should have two definitions for one term"
    );
}

#[test]
fn definition_marker_after_blank_line_does_not_create_orphan_item() {
    let input = "Term\n: Def\n\n: Def\n";
    let tree = parse_blocks(input);

    let definition_item = find_first(&tree, SyntaxKind::DEFINITION_ITEM).expect("definition item");
    let term_count = definition_item
        .children()
        .filter(|child| child.kind() == SyntaxKind::TERM)
        .count();
    assert_eq!(
        term_count, 1,
        "definition item should keep exactly one term"
    );
}

#[test]
fn definition_marker_after_list_definition_closes_nested_list() {
    let input = "Orange\n:   - a\n    - b\n:   Also a color\n";
    let tree = parse_blocks(input);

    let definition_item = find_first(&tree, SyntaxKind::DEFINITION_ITEM).expect("definition item");
    let definitions = definition_item
        .children()
        .filter(|child| child.kind() == SyntaxKind::DEFINITION)
        .count();
    assert_eq!(
        definitions, 2,
        "marker after list definition should create a sibling definition"
    );

    let nested_definition_item = definition_item
        .descendants()
        .any(|node| node.kind() == SyntaxKind::DEFINITION_ITEM && node != definition_item);
    assert!(
        !nested_definition_item,
        "list content should not capture a nested DEFINITION_ITEM"
    );
}

#[test]
fn dedented_list_after_blank_line_does_not_continue_definition_list() {
    let input = "Term\n\n:   - List\n    - a\n\n- b\n";
    let tree = parse_blocks(input);

    assert_block_kinds(
        input,
        &[
            SyntaxKind::DEFINITION_LIST,
            SyntaxKind::BLANK_LINE,
            SyntaxKind::LIST,
        ],
    );

    let definition = find_first(&tree, SyntaxKind::DEFINITION).expect("definition");
    let nested_list = find_first(&definition, SyntaxKind::LIST).expect("nested list");
    assert_eq!(
        nested_list
            .children()
            .filter(|child| child.kind() == SyntaxKind::LIST_ITEM)
            .count(),
        2,
        "definition list should only contain the indented items"
    );

    assert_eq!(
        find_all(&tree, SyntaxKind::LIST).len(),
        2,
        "expected one nested list and one top-level list"
    );
}

#[test]
fn orphan_colon_marker_with_content_is_paragraph() {
    // A `:` marker with no preceding term is not a definition list; pandoc
    // treats the whole line as a paragraph (`Para [Str ":", Space, Str "foo"]`).
    let input = ":   foo\n";
    let tree = parse_blocks(input);

    assert!(
        find_first(&tree, SyntaxKind::DEFINITION_LIST).is_none(),
        "orphan `:` marker should not open a definition list"
    );
    assert_block_kinds(input, &[SyntaxKind::PARAGRAPH]);
}

#[test]
fn orphan_tilde_marker_with_content_is_paragraph() {
    let input = "~   foo\n";
    let tree = parse_blocks(input);

    assert!(
        find_first(&tree, SyntaxKind::DEFINITION_LIST).is_none(),
        "orphan `~` marker should not open a definition list"
    );
    assert_block_kinds(input, &[SyntaxKind::PARAGRAPH]);
}

#[test]
fn orphan_bare_marker_with_body_next_line_is_paragraph() {
    // Bare marker with the body on the next line, no term above: pandoc yields
    // `Para [Str ":", SoftBreak, Str "foo"]`.
    let input = ":\n    foo\n";
    let tree = parse_blocks(input);

    assert!(
        find_first(&tree, SyntaxKind::DEFINITION_LIST).is_none(),
        "bare orphan `:` marker should not open a definition list"
    );
}

#[test]
fn colon_marker_line_becomes_term_when_next_line_is_marker() {
    // `: foo` / `: bar` has no explicit term, but pandoc makes the first line
    // the term (literal `: foo`) and the second the definition (`bar`).
    let input = ":   foo\n:   bar\n";
    let tree = parse_blocks(input);

    let definition_list =
        find_first(&tree, SyntaxKind::DEFINITION_LIST).expect("should be a definition list");
    let term = find_first(&definition_list, SyntaxKind::TERM).expect("term");
    assert_eq!(
        term.text().to_string().trim_end(),
        ":   foo",
        "the marker-shaped first line should be the literal term"
    );
    let definition = find_first(&definition_list, SyntaxKind::DEFINITION).expect("definition");
    assert!(
        definition.text().to_string().contains("bar"),
        "second marker line supplies the definition body"
    );
}

#[test]
fn colon_table_caption_before_table_is_not_definition_list() {
    let input = "Here's a table with a reference:\n\n: (\\#tab:mytable) A table with a reference.\n\n| A   | B   | C   |\n| --- | --- | --- |\n| 1   | 2   | 3   |\n";
    let tree = parse_blocks(input);

    assert!(
        find_first(&tree, SyntaxKind::DEFINITION_LIST).is_none(),
        "colon table caption before a table should not be parsed as DEFINITION_LIST"
    );
    assert!(
        find_first(&tree, SyntaxKind::PIPE_TABLE).is_some(),
        "expected PIPE_TABLE to be parsed for colon caption + table"
    );
    assert!(
        find_first(&tree, SyntaxKind::TABLE_CAPTION).is_some(),
        "expected TABLE_CAPTION node for colon caption"
    );
}

/// Assert the whole document is a single definition list nested `depth`
/// blockquotes deep, with `term` / `definition` as its one item.
fn assert_quoted_definition_list(input: &str, depth: usize, term: &str, definition: &str) {
    let tree = parse_blocks(input);
    assert_eq!(tree.text().to_string(), input, "parse must be lossless");

    assert_block_kinds(input, &[SyntaxKind::BLOCK_QUOTE]);
    assert_eq!(
        find_all(&tree, SyntaxKind::BLOCK_QUOTE).len(),
        depth,
        "definition list should stay {depth} blockquotes deep"
    );

    let list = find_first(&tree, SyntaxKind::DEFINITION_LIST).expect("definition list");
    assert_eq!(
        find_first(&list, SyntaxKind::TERM)
            .expect("term")
            .text()
            .to_string()
            .trim_end(),
        term
    );
    assert!(
        find_first(&list, SyntaxKind::DEFINITION)
            .expect("definition body")
            .text()
            .to_string()
            .contains(definition),
        "definition body should carry {definition:?}"
    );
}

#[test]
fn definition_list_in_blockquote_keeps_its_body() {
    // The term look-ahead runs on container-stripped lines, so it sees `: b`
    // through the `> ` prefix. Pandoc: `BlockQuote [DefinitionList [(a, [[Plain
    // b]])]]`.
    assert_quoted_definition_list("> a\n> : b\n", 1, "a", "b");
}

#[test]
fn definition_list_in_nested_blockquote_keeps_its_body() {
    assert_quoted_definition_list("> > a\n> > : b\n", 2, "a", "b");
}

#[test]
fn lazy_definition_marker_stays_inside_blockquote() {
    // Pandoc folds lazy lines into the blockquote's raw content before parsing
    // blocks, so the unquoted `: b` is the term's definition rather than a
    // top-level paragraph.
    assert_quoted_definition_list("> a\n: b\n", 1, "a", "b");
}

#[test]
fn lazy_definition_marker_stays_inside_nested_blockquote() {
    assert_quoted_definition_list("> > a\n: b\n", 2, "a", "b");
}

#[test]
fn lazy_definition_marker_reduced_depth_stays_inside_blockquote() {
    // One `>` under a depth-2 quote is still lazy: the marker belongs to the
    // inner definition list, not to the outer quote.
    assert_quoted_definition_list("> > a\n> : b\n", 2, "a", "b");
}

#[test]
fn lazy_definition_markers_add_further_definitions() {
    let input = "> > a\n: b\n: c\n";
    let tree = parse_blocks(input);
    assert_eq!(tree.text().to_string(), input, "parse must be lossless");

    let list = find_first(&tree, SyntaxKind::DEFINITION_LIST).expect("definition list");
    let definitions = find_all(&list, SyntaxKind::DEFINITION);
    assert_eq!(definitions.len(), 2, "both lazy markers open a definition");
    assert!(definitions[0].text().to_string().contains('b'));
    assert!(definitions[1].text().to_string().contains('c'));
}

#[test]
fn trailing_marker_line_outside_the_item_is_not_a_definition_marker() {
    // `ContainerPrefix::strip` advances the item's content column with
    // `advance_columns`, which counts any character as a column. Inside a
    // two-column item that turns `"c :"` into `":"`, so the term lookahead
    // used to see a bare marker and promote the line above it. Pandoc reads
    // `BulletList [[Plain [a, SoftBreak, b]]]` + `Para [c, Space, ":"]`.
    for input in [
        "- a\nb\n\nc :\n",
        "- a\n  b\n\nc :\n",
        "- a\nb\n\nc ~\n",
        "- a\n\n  b\n\nc :\n",
    ] {
        let tree = parse_blocks(input);
        assert!(
            find_first(&tree, SyntaxKind::DEFINITION_LIST).is_none(),
            "a marker line outside the list item must not open a definition list: {input:?}"
        );
        assert_block_kinds(
            input,
            &[
                SyntaxKind::LIST,
                SyntaxKind::BLANK_LINE,
                SyntaxKind::PARAGRAPH,
            ],
        );
    }
}

#[test]
fn trailing_marker_line_outside_the_item_is_inert_in_a_blockquote() {
    let input = "> - a\n> b\n>\n> c :\n";
    let tree = parse_blocks(input);
    assert!(
        find_first(&tree, SyntaxKind::DEFINITION_LIST).is_none(),
        "pandoc reads this as BlockQuote [BulletList, Para]"
    );
}

#[test]
fn ordered_and_tab_variants_already_agree_with_pandoc() {
    // Controls for the case above: an ordered marker gives content_col 3 (the
    // faked slice is just the newline) and a tab overshoots column 2, so
    // neither ever reached the bad path. They must stay put.
    for input in ["1. a\nb\n\nc :\n", "- a\nb\n\nc\t:\n"] {
        assert!(
            find_first(&parse_blocks(input), SyntaxKind::DEFINITION_LIST).is_none(),
            "{input:?}"
        );
    }
}

#[test]
fn definition_marker_at_the_item_content_column_still_opens_a_definition() {
    // The gate must only reject lines that fall short of the content column.
    let input = "- a\n\n  b\n\n  : def\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::DEFINITION_LIST).expect("definition list");
    let term = find_first(&list, SyntaxKind::TERM).expect("term");
    assert_eq!(term.text().to_string().trim(), "b");
}

#[test]
fn multiline_paragraph_last_line_is_not_a_term() {
    // Pandoc's term is read where a block may start, so it is always a
    // one-line block of its own: `a\nb\n\n: def` is `Para [a, SoftBreak, b]`
    // + `Para [":", Space, "def"]`, not a definition list on `b`.
    let input = "a\nb\n\n: def\n";
    let tree = parse_blocks(input);
    assert!(
        find_first(&tree, SyntaxKind::DEFINITION_LIST).is_none(),
        "the last line of a multi-line paragraph is not a term"
    );
    assert_block_kinds(
        input,
        &[
            SyntaxKind::PARAGRAPH,
            SyntaxKind::BLANK_LINE,
            SyntaxKind::PARAGRAPH,
        ],
    );
}

#[test]
fn blank_line_resets_the_one_line_block_rule() {
    // The rule is per block: a blank line closes the paragraph, so `b` is a
    // one-line block again and does become a term.
    let input = "a\n\nb\n\n: def\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::DEFINITION_LIST).expect("definition list");
    assert_eq!(
        find_first(&list, SyntaxKind::TERM)
            .expect("term")
            .text()
            .to_string()
            .trim(),
        "b"
    );
}

#[test]
fn multiline_item_content_last_line_is_not_a_term() {
    // Same rule through a list item's buffered content. Pandoc:
    // `BulletList [[Para [a, SoftBreak, b], Para [":", Space, "def"]]]`.
    let input = "- a\n  b\n\n  : def\n";
    let tree = parse_blocks(input);
    assert!(
        find_first(&tree, SyntaxKind::DEFINITION_LIST).is_none(),
        "buffered item content is the analogue of an open paragraph"
    );
    let item = find_first(&tree, SyntaxKind::LIST_ITEM).expect("list item");
    assert!(
        item.text().to_string().contains(": def"),
        "the marker line must stay inside the item, not escape it"
    );
}

#[test]
fn multiline_paragraph_last_line_is_not_a_term_in_a_blockquote() {
    let input = "> a\n> b\n>\n> : def\n";
    assert!(
        find_first(&parse_blocks(input), SyntaxKind::DEFINITION_LIST).is_none(),
        "pandoc reads this as BlockQuote [Para, Para]"
    );
}

#[test]
fn refused_term_still_lets_a_marker_pair_form_a_term() {
    // `: def` is itself a one-line block whose next line is a marker, so it
    // becomes the literal term. Pandoc: `Para [a, SoftBreak, b]` +
    // `DefinitionList [([": def"], [[Plain "def2"]])]`.
    let input = "a\nb\n\n: def\n: def2\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::DEFINITION_LIST).expect("definition list");
    assert_eq!(
        find_first(&list, SyntaxKind::TERM)
            .expect("term")
            .text()
            .to_string()
            .trim_end(),
        ": def"
    );
}

#[test]
fn footnote_body_first_line_term_still_opens_a_definition_list() {
    // `footnote_first_line_term_lookahead` must stay live: its term is the
    // body's first line, so it is a one-line block by construction.
    let input = "x[^1]\n\n[^1]: Term\n\n    :   Definition\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::DEFINITION_LIST).expect("definition list");
    assert_eq!(
        find_first(&list, SyntaxKind::TERM)
            .expect("term")
            .text()
            .to_string()
            .trim(),
        "Term"
    );
}

#[test]
fn definition_marker_is_detected_at_a_deep_item_content_column() {
    // Pandoc reparses item contents from the item's content column, so the
    // 0-3 space allowance is measured from there, not from column 0. With a
    // content column of 4 the marker used to be read as indented code, which
    // left the term above it with no definition at all.
    let input = "- - foo\n\n    bar\n\n    : baz\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::DEFINITION_LIST).expect("definition list");
    assert_eq!(
        find_first(&list, SyntaxKind::TERM)
            .expect("term")
            .text()
            .to_string()
            .trim(),
        "bar"
    );
    let definition = find_first(&list, SyntaxKind::DEFINITION).expect("definition");
    assert!(
        definition.text().to_string().contains("baz"),
        "the term must get its definition body"
    );
}

#[test]
fn definition_marker_beyond_the_content_column_stays_indented_code() {
    // Four further columns past the item's content column is an indented code
    // block, exactly as at top level.
    let input = "- a\n\n  bar\n\n      : baz\n";
    assert!(
        find_first(&parse_blocks(input), SyntaxKind::DEFINITION_LIST).is_none(),
        "the 0-3 allowance still applies, measured from the content column"
    );
}

#[test]
fn term_on_the_list_marker_line_nests_the_definition_list_in_the_item() {
    // Pandoc reparses item contents as a fresh block sequence, so `- Term`
    // followed by a marker line is `BulletList [[DefinitionList [(Term,
    // [[Plain def]])]]]` — the definition list lives inside the item.
    for input in [
        "- Term\n  : def\n",
        "- Term\n  ~ def\n",
        "- Term\n\n  : def\n",
        "1. Term\n   : def\n",
    ] {
        let tree = parse_blocks(input);
        let item = find_first(&tree, SyntaxKind::LIST_ITEM).expect("list item");
        let list = find_first(&item, SyntaxKind::DEFINITION_LIST)
            .unwrap_or_else(|| panic!("definition list inside the item: {input:?}"));
        assert_eq!(
            find_first(&list, SyntaxKind::TERM)
                .expect("term")
                .text()
                .to_string()
                .trim(),
            "Term",
            "{input:?}"
        );
        assert!(
            find_first(&list, SyntaxKind::DEFINITION).is_some(),
            "term must get its definition body: {input:?}"
        );
    }
}

#[test]
fn blocks_that_outrank_a_term_still_win_on_the_marker_line() {
    // Everything pandoc's reader reaches before `definitionList` keeps the
    // marker line; only then does the term lookahead get a turn.
    for input in [
        "- # H\n  : def\n",
        "- ***\n  : def\n",
        "- <!-- c -->\n  : def\n",
        "- a\n  b\n  : def\n",
    ] {
        let tree = parse_blocks(input);
        let item = find_first(&tree, SyntaxKind::LIST_ITEM).expect("list item");
        assert!(
            find_first(&item, SyntaxKind::TERM).is_none(),
            "no term should be detected for {input:?}"
        );
    }
}

#[test]
fn a_dedented_marker_does_not_make_the_marker_line_a_term() {
    // The marker must reach the item's content column.
    for input in ["- Term\n: def\n", "- Term\n : def\n"] {
        let tree = parse_blocks(input);
        assert!(find_first(&tree, SyntaxKind::TERM).is_none(), "{input:?}");
    }
}

#[test]
fn a_dedented_marker_closes_the_list_instead_of_continuing_it() {
    // A definition marker is a block start, so it is not lazy continuation
    // text. Dedented below the item's content column it lands outside the
    // item entirely: pandoc gives `BulletList [[Plain "Term"]]` followed by
    // `Para [":", Space, "def"]`.
    for input in ["- Term\n: def\n", "- Term\n : def\n"] {
        let tree = parse_blocks(input);
        assert_block_kinds_for_node(&tree, &[SyntaxKind::LIST, SyntaxKind::PARAGRAPH], input);
        let item = find_first(&tree, SyntaxKind::LIST_ITEM).expect("list item");
        assert_eq!(item.text().to_string(), "- Term\n", "{input:?}");
        assert_eq!(tree.text().to_string(), input, "{input:?}");
    }
}

#[test]
fn a_marker_at_the_content_column_breaks_the_items_paragraph() {
    // Same rule one column further in: the marker still cannot continue the
    // paragraph, but it does stay inside the item, so pandoc splits the item
    // into `Plain [a, SoftBreak, b]` and `Plain [":", Space, "def"]`.
    let input = "- a\n  b\n  : def\n";
    let tree = parse_blocks(input);
    let item = find_first(&tree, SyntaxKind::LIST_ITEM).expect("list item");
    let blocks: Vec<_> = item
        .children()
        .map(|c| c.kind())
        .filter(|k| matches!(k, SyntaxKind::PLAIN | SyntaxKind::PARAGRAPH))
        .collect();
    assert_eq!(blocks, [SyntaxKind::PLAIN, SyntaxKind::PLAIN]);
    assert!(find_first(&tree, SyntaxKind::DEFINITION_LIST).is_none());
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn a_marker_at_the_content_column_breaks_the_definition_body_block() {
    // Pandoc re-reads a definition body from its content column, so a marker
    // that reaches it is a block start inside the *same* definition, not a
    // second one: `T\n:   a\n    b\n    : def` is
    // `DefinitionList [(T, [[Plain "a b", Plain ": def"]])]`.
    for input in [
        "T\n:   a\n    b\n    : def\n",
        "T\n: a\n  b\n  : def\n",
        "T\n:   a\n    b\n      : def\n",
        "T\n:   a\n    b\n    ~ def\n",
    ] {
        let tree = parse_blocks(input);
        let definitions = find_all(&tree, SyntaxKind::DEFINITION);
        assert_eq!(definitions.len(), 1, "{input:?}");
        let blocks: Vec<_> = definitions[0]
            .children()
            .map(|child| child.kind())
            .filter(|kind| matches!(kind, SyntaxKind::PLAIN | SyntaxKind::PARAGRAPH))
            .collect();
        assert_eq!(blocks, [SyntaxKind::PLAIN, SyntaxKind::PLAIN], "{input:?}");
        assert_eq!(tree.text().to_string(), input, "{input:?}");
    }
}

#[test]
fn a_marker_under_a_single_body_line_promotes_it_to_a_nested_term() {
    // A term is a one-line block, and a definition body is re-read as its own
    // block sequence, so a single buffered body line *is* a term: the marker
    // below it opens a definition list nested in that body rather than a
    // second definition of the outer term. `T\n:   a\n    : def` is
    // `DefinitionList [(T, [[DefinitionList [(a, [[Plain "def"]])]]])]`.
    for input in [
        "T\n:   a\n    : def\n",
        "T\n: a\n  : def\n",
        "T\n:   a\n      : def\n",
        "T\n:   a\n    ~ def\n",
        "T\n:   a\n    :   def\n",
    ] {
        let tree = parse_blocks(input);
        let lists = find_all(&tree, SyntaxKind::DEFINITION_LIST);
        assert_eq!(lists.len(), 2, "{input:?}");

        let outer_definition = find_first(&lists[0], SyntaxKind::DEFINITION).expect("definition");
        assert!(
            outer_definition
                .children()
                .all(|child| child.kind() != SyntaxKind::PLAIN),
            "the buffered line became a term, so no PLAIN is left in the body: {input:?}"
        );
        assert_eq!(
            outer_definition
                .children()
                .filter(|child| child.kind() == SyntaxKind::DEFINITION_LIST)
                .count(),
            1,
            "{input:?}"
        );

        let terms = find_all(&tree, SyntaxKind::TERM);
        assert_eq!(terms.len(), 2, "{input:?}");
        assert_eq!(terms[1].text().to_string().trim(), "a", "{input:?}");
        assert_eq!(tree.text().to_string(), input, "{input:?}");
    }
}

#[test]
fn a_marker_under_a_marker_that_broke_the_body_block_promotes_it_to_a_term() {
    // The line a broken block leaves behind is itself a one-line block, so a
    // second marker under it promotes it in turn:
    // `T\n:   a\n    b\n    : def\n    : def2` is
    // `DefinitionList [(T, [[Plain "a b", DefinitionList [(": def", ...)]]])]`.
    let input = "T\n:   a\n    b\n    : def\n    : def2\n";
    let tree = parse_blocks(input);

    let lists = find_all(&tree, SyntaxKind::DEFINITION_LIST);
    assert_eq!(lists.len(), 2);

    let outer_definition = find_first(&lists[0], SyntaxKind::DEFINITION).expect("definition");
    let blocks: Vec<_> = outer_definition
        .children()
        .map(|child| child.kind())
        .filter(|kind| {
            matches!(
                kind,
                SyntaxKind::PLAIN | SyntaxKind::PARAGRAPH | SyntaxKind::DEFINITION_LIST
            )
        })
        .collect();
    assert_eq!(blocks, [SyntaxKind::PLAIN, SyntaxKind::DEFINITION_LIST]);

    let terms = find_all(&tree, SyntaxKind::TERM);
    assert_eq!(terms.len(), 2);
    assert_eq!(terms[1].text().to_string().trim(), ": def");
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn a_marker_across_a_blank_line_promotes_the_single_body_line_to_a_term() {
    // A blank line does not detach the marker from the block above it: the
    // body's last block is still a one-line block, so it is still a term.
    // `T\n:   a\n\n    :   b` is
    // `DefinitionList [(T, [[DefinitionList [(a, [[Para "b"]])]]])]`.
    for input in [
        "T\n:   a\n\n    :   b\n",
        "T\n\n:   a\n\n    :   b\n",
        "T\n:   a\n\n    : b\n",
        "T\n:   a\n\n    ~ b\n",
    ] {
        let tree = parse_blocks(input);
        let lists = find_all(&tree, SyntaxKind::DEFINITION_LIST);
        assert_eq!(lists.len(), 2, "{input:?}");

        let outer_definition = find_first(&lists[0], SyntaxKind::DEFINITION).expect("definition");
        assert!(
            outer_definition
                .children()
                .all(|child| child.kind() != SyntaxKind::PLAIN),
            "the buffered line became a term, so no PLAIN is left in the body: {input:?}"
        );
        assert_eq!(
            find_all(&tree, SyntaxKind::DEFINITION).len(),
            2,
            "the marker defines `a`, not `T` a second time: {input:?}"
        );

        let terms = find_all(&tree, SyntaxKind::TERM);
        assert_eq!(terms.len(), 2, "{input:?}");
        assert_eq!(terms[1].text().to_string().trim(), "a", "{input:?}");
        assert_eq!(tree.text().to_string(), input, "{input:?}");
    }
}

#[test]
fn a_marker_across_a_blank_line_promotes_a_later_body_block_too() {
    // The promotion reads the body's *last* block, not its first: the earlier
    // blocks stay `PLAIN` siblings inside the same body.
    // `T\n:   a\n\n    b\n\n    : c` is
    // `DefinitionList [(T, [[Plain "a", DefinitionList [(b, [[Para "c"]])]]])]`.
    let input = "T\n:   a\n\n    b\n\n    : c\n";
    let tree = parse_blocks(input);

    let lists = find_all(&tree, SyntaxKind::DEFINITION_LIST);
    assert_eq!(lists.len(), 2);

    let outer_definition = find_first(&lists[0], SyntaxKind::DEFINITION).expect("definition");
    let blocks: Vec<_> = outer_definition
        .children()
        .map(|child| child.kind())
        .filter(|kind| {
            matches!(
                kind,
                SyntaxKind::PLAIN | SyntaxKind::PARAGRAPH | SyntaxKind::DEFINITION_LIST
            )
        })
        .collect();
    assert_eq!(blocks, [SyntaxKind::PLAIN, SyntaxKind::DEFINITION_LIST]);

    let terms = find_all(&tree, SyntaxKind::TERM);
    assert_eq!(terms.len(), 2);
    assert_eq!(terms[1].text().to_string().trim(), "b");
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn a_multi_line_body_block_across_a_blank_line_is_not_promoted() {
    // Only a *one-line* block is a term, blank line or not. Pandoc keeps
    // `a\nb` a `Plain` and reads the marker line as the body's next block, so
    // the marker must not promote it and must not become a second definition
    // of `T` either.
    for input in [
        "T\n:   a\n    b\n\n    : c\n",
        "T\n:   x\n\n    a\n    b\n\n    : c\n",
    ] {
        let tree = parse_blocks(input);
        assert_eq!(
            find_all(&tree, SyntaxKind::DEFINITION_LIST).len(),
            1,
            "{input:?}"
        );
        assert_eq!(find_all(&tree, SyntaxKind::TERM).len(), 1, "{input:?}");
        assert_eq!(
            find_all(&tree, SyntaxKind::DEFINITION).len(),
            1,
            "the marker line is a further block of the same body: {input:?}"
        );
        assert_eq!(tree.text().to_string(), input, "{input:?}");
    }
}

#[test]
fn a_marker_across_a_blank_line_promotes_inside_a_list_item_and_a_blockquote() {
    // The lookahead reads the marker line in the body's own frame, so the
    // promotion survives any container above it. Inside a list item the
    // definition's content column is absolute (item indent included), which is
    // what the dedent test has to measure against.
    for input in [
        "- T\n\n  : a\n\n    : b\n",
        "> T\n>\n> :   a\n>\n>     :   b\n",
        "> - T\n>\n>   : a\n>\n>     : b\n",
    ] {
        let tree = parse_blocks(input);
        assert_eq!(
            find_all(&tree, SyntaxKind::DEFINITION_LIST).len(),
            2,
            "{input:?}"
        );
        let terms = find_all(&tree, SyntaxKind::TERM);
        assert_eq!(terms.len(), 2, "{input:?}");
        assert_eq!(terms[1].text().to_string().trim(), "a", "{input:?}");
        assert_eq!(tree.text().to_string(), input, "{input:?}");
    }
}

#[test]
fn two_blank_lines_detach_the_marker_from_the_body_block_above_it() {
    // A term keeps at most one blank line between itself and its definition,
    // so two blanks leave `a` a body block of its own, and the marker line
    // becomes a further block of the same body:
    // `DefinitionList [(T, [[Plain "a", Plain ": b"]])]`.
    for input in [
        "T\n:   a\n\n\n    :   b\n",
        "T\n:   a\n\n\n    :   b\n\n\n    :   c\n",
    ] {
        let tree = parse_blocks(input);

        assert_eq!(
            find_all(&tree, SyntaxKind::DEFINITION_LIST).len(),
            1,
            "{input:?}"
        );
        assert_eq!(find_all(&tree, SyntaxKind::TERM).len(), 1, "{input:?}");
        assert_eq!(
            find_all(&tree, SyntaxKind::DEFINITION).len(),
            1,
            "the detached marker is body content, not a second definition: {input:?}"
        );
        assert_eq!(tree.text().to_string(), input, "{input:?}");
    }
}

#[test]
fn two_blank_lines_detach_a_term_from_its_definition_marker() {
    // Pandoc's term parser allows one optional blank line before the first
    // definition marker, so two blanks leave no term at all and the marker
    // line is just a paragraph: `T\n\n\n: b` is `Para "T"` + `Para ": b"`.
    for input in [
        "T\n\n\n: b\n",
        "T\n\n\n\n: b\n",
        "> T\n>\n>\n> : b\n",
        "- T\n\n\n  : b\n",
    ] {
        let tree = parse_blocks(input);
        assert!(
            find_first(&tree, SyntaxKind::DEFINITION_LIST).is_none(),
            "{input:?}"
        );
        assert_eq!(tree.text().to_string(), input, "{input:?}");
    }
}

#[test]
fn a_marker_in_an_empty_definition_body_is_body_content() {
    // A body that has not started yet is still a body: pandoc reads a marker
    // at its content column as the body's first block, not as a second
    // definition of the term. `T\n:\n    : b` is
    // `DefinitionList [(T, [[Plain ": b"]])]`.
    for input in ["T\n:\n    : b\n", "T\n:\n\n    : b\n"] {
        let tree = parse_blocks(input);
        assert_eq!(
            find_all(&tree, SyntaxKind::DEFINITION).len(),
            1,
            "{input:?}"
        );
        assert_eq!(find_all(&tree, SyntaxKind::TERM).len(), 1, "{input:?}");
        assert_eq!(tree.text().to_string(), input, "{input:?}");
    }
}

#[test]
fn a_dedented_marker_across_two_blank_lines_still_opens_a_sibling_definition() {
    // Blank lines do not detach a *definition* from its list, only a term from
    // its first marker: below the body's content column the marker is still a
    // second definition of the same term, however many blanks precede it.
    for input in [
        "T\n:   a\n\n\n: b\n",
        "T\n:   a\n\n\n  : b\n",
        "T\n:   a\n\n\n    :   b\n\n  : d\n",
    ] {
        let tree = parse_blocks(input);
        assert_eq!(
            find_all(&tree, SyntaxKind::DEFINITION_LIST).len(),
            1,
            "{input:?}"
        );
        assert_eq!(
            find_all(&tree, SyntaxKind::DEFINITION).len(),
            2,
            "{input:?}"
        );
        assert_eq!(find_all(&tree, SyntaxKind::TERM).len(), 1, "{input:?}");
        assert_eq!(tree.text().to_string(), input, "{input:?}");
    }
}

#[test]
fn a_dedented_marker_across_a_blank_line_still_opens_a_sibling_definition() {
    // Below the body's content column the marker is a second definition of the
    // same term, so the blank-line lookahead must not promote across it:
    // `DefinitionList [(T, [[Plain "a"], [Plain "b"]])]`.
    for input in ["T\n:   a\n\n  : b\n", "T\n:   a\n\n: b\n"] {
        let tree = parse_blocks(input);
        assert_eq!(
            find_all(&tree, SyntaxKind::DEFINITION_LIST).len(),
            1,
            "{input:?}"
        );
        assert_eq!(
            find_all(&tree, SyntaxKind::DEFINITION).len(),
            2,
            "{input:?}"
        );
        assert_eq!(find_all(&tree, SyntaxKind::TERM).len(), 1, "{input:?}");
        assert_eq!(tree.text().to_string(), input, "{input:?}");
    }
}

#[test]
fn a_promoted_term_keeps_its_own_body_continuations() {
    // The nested definition is a definition like any other: its body keeps
    // reading continuation lines from its own content column.
    let input = "T\n:   a\n    : def\n    more\n";
    let tree = parse_blocks(input);

    let lists = find_all(&tree, SyntaxKind::DEFINITION_LIST);
    assert_eq!(lists.len(), 2);
    let nested_definition = find_first(&lists[1], SyntaxKind::DEFINITION).expect("definition");
    let plains = find_all(&nested_definition, SyntaxKind::PLAIN);
    assert_eq!(plains.len(), 1);
    assert_eq!(plains[0].text().to_string().trim(), "def\n    more");
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn a_dedented_marker_leaves_the_nested_definition_list() {
    // A marker is a block start, read in the frame of the body it lands in.
    // Below the nested list's frame — the content column of the body holding
    // it — it is a second definition of the *outer* term:
    // `DefinitionList [(T, [[DefinitionList [(a, ...)]], [Plain "sibling"]])]`.
    for input in [
        "T\n:   a\n    : def\n  : sibling\n",
        "T\n:   a\n    : def\n: sibling\n",
    ] {
        let tree = parse_blocks(input);
        let lists = find_all(&tree, SyntaxKind::DEFINITION_LIST);
        assert_eq!(lists.len(), 2, "{input:?}");

        let outer_item = find_first(&lists[0], SyntaxKind::DEFINITION_ITEM).expect("item");
        assert_eq!(
            outer_item
                .children()
                .filter(|child| child.kind() == SyntaxKind::DEFINITION)
                .count(),
            2,
            "the sibling belongs to the outer term: {input:?}"
        );
        assert_eq!(find_all(&tree, SyntaxKind::TERM).len(), 2, "{input:?}");
        assert_eq!(tree.text().to_string(), input, "{input:?}");
    }
}

#[test]
fn a_dedented_plain_line_stays_a_lazy_continuation_of_the_nested_body() {
    // Only a marker is a block start; plain text below the nested frame is
    // still a lazy continuation of the innermost body, so nothing unwinds.
    let input = "T\n:   a\n    : def\n  tail\n";
    let tree = parse_blocks(input);

    assert_eq!(find_all(&tree, SyntaxKind::DEFINITION_LIST).len(), 2);
    assert_eq!(find_all(&tree, SyntaxKind::PLAIN).len(), 1);
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn a_term_below_the_body_frame_closes_the_nested_definition_list() {
    // Across a blank line a nested list only stays open for a term that
    // reaches the body holding it, or `T2` would become a term of the list
    // nested in `T`'s body instead of a sibling of `T`.
    let input = "T\n:   a\n    : def\n\nT2\n:   x\n";
    let tree = parse_blocks(input);

    let lists = find_all(&tree, SyntaxKind::DEFINITION_LIST);
    assert_eq!(lists.len(), 2);
    assert_eq!(
        lists[0]
            .children()
            .filter(|child| child.kind() == SyntaxKind::DEFINITION_ITEM)
            .count(),
        2,
        "T and T2 are siblings"
    );
    assert_eq!(tree.text().to_string(), input);
}

#[test]
fn a_dedented_marker_still_opens_a_sibling_definition() {
    // Below the body's content column the marker is a definition of the same
    // term again, whatever the block above it is doing:
    // `DefinitionList [(T, [[Plain "a b"], [Plain "def"]])]`.
    for input in ["T\n:   a\n    b\n  : def\n", "T\n:   a\n    b\n: def\n"] {
        let tree = parse_blocks(input);
        assert_eq!(
            find_all(&tree, SyntaxKind::DEFINITION).len(),
            2,
            "{input:?}"
        );
        assert_eq!(tree.text().to_string(), input, "{input:?}");
    }
}

#[test]
fn definition_list_in_list_item_survives_siblings_and_trailing_blocks() {
    let input = "- Term\n  : def\n- Term2\n  : def2\n";
    let tree = parse_blocks(input);
    assert_eq!(
        find_all(&tree, SyntaxKind::LIST_ITEM).len(),
        2,
        "both bullet items must survive"
    );
    assert_eq!(find_all(&tree, SyntaxKind::TERM).len(), 2);
}

#[test]
fn blank_line_between_items_stays_at_the_list_level() {
    // A definition list opened on a list-marker line must not swallow the
    // blank line that separates two sibling items: the `LIST` needs to see it
    // to stay loose. The next line, `- Term`, is a sibling marker at indent 0
    // and never reaches the item's content column, so it cannot continue the
    // definition list as a term.
    let input = "- Term\n  : def\n\n- Term\n  : def\n";
    let tree = parse_blocks(input);
    let list = find_first(&tree, SyntaxKind::LIST).expect("list");
    assert!(
        list.children().any(|c| c.kind() == SyntaxKind::BLANK_LINE),
        "the separator must be a direct child of LIST, not absorbed by the \
         definition list inside the first item"
    );
}
