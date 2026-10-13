use panache_parser::syntax::{AstNode, HugoShortcode, SyntaxKind, TomlMetadata};
use panache_parser::{Dialect, Extensions, Flavor, ParserOptions, parse};

mod common;

fn parse_hugo(input: &str) -> panache_parser::SyntaxNode {
    let tree = parse(input, Some(ParserOptions::for_flavor(Flavor::Hugo)));
    assert_eq!(tree.text().to_string(), input);
    tree
}

#[test]
fn quoted_shortcode_blocks_do_not_create_prefix_only_prose() {
    for input in [
        include_str!("fixtures/cases/hugo_quoted_heading_shortcode/input.md"),
        include_str!("fixtures/cases/hugo_adjacent_quoted_shortcodes/input.md"),
    ] {
        let tree = parse_hugo(input);
        assert!(
            !tree
                .descendants()
                .any(|node| matches!(node.kind(), SyntaxKind::PLAIN | SyntaxKind::PARAGRAPH))
        );
    }
}

#[test]
fn definition_text_precedes_shortcode_blocks() {
    for input in [
        "Term\n: Intro.\n  {{< foo >}}\n",
        "> Term\n> : Intro.\n>   {{< foo >}}\n",
        "- Term\n  : Intro.\n    {{< foo >}}\n",
    ] {
        parse_hugo(input);
    }
}

#[test]
fn definition_prose_holds_shortcode_continuations() {
    for call in [
        "{{< foo\n    arg=42\n  >}}",
        "{{% foo\n  arg=`a\nb`\n%}}",
        "{{< box >}}\n\n# Literal heading\n> Literal quote\n{{< /box >}}",
    ] {
        for body in [": Before ", ": Intro.\n  Before "] {
            for (prefix, continuation) in [("", ""), ("> ", "> "), ("- ", "  ")] {
                let body = format!("{body}{} after.", call.replace('\n', "\n  "));
                let input = format!(
                    "{prefix}Term\n{continuation}{}\n",
                    body.replace('\n', &format!("\n{continuation}"))
                );
                let tree = parse_hugo(&input);
                let definition = tree
                    .descendants()
                    .find(|node| node.kind() == SyntaxKind::DEFINITION)
                    .unwrap();
                let shortcode = definition
                    .descendants()
                    .find_map(HugoShortcode::cast)
                    .unwrap();
                assert_eq!(
                    panache_parser::syntax::text_without_line_prefixes(shortcode.syntax()),
                    call,
                    "{input:?}"
                );
            }
        }
    }
}

#[test]
fn paired_bodies_protect_setext_underlines() {
    for (open, close) in [
        ("{{< box >}}", "{{< /box >}}"),
        ("{{% box %}}", "{{% /box %}}"),
    ] {
        for underline in ["===", "---"] {
            for (prefix, continuation) in [("", ""), ("- ", "  "), ("> ", "> ")] {
                let input = format!(
                    "{prefix}{open}\n{continuation}{underline}\n{continuation}Keep   text.\n{continuation}{close}\n"
                );
                let tree = parse_hugo(&input);
                let shortcode = tree.descendants().find_map(HugoShortcode::cast).unwrap();
                assert_eq!(
                    panache_parser::syntax::text_without_line_prefixes(&shortcode.body().unwrap()),
                    format!("\n{underline}\nKeep   text.\n"),
                    "{input:?}"
                );
            }
        }
    }
}

#[test]
fn link_labels_skip_shortcode_arguments_and_bodies() {
    for call in [
        "{{< foo arg=\"]\" >}}",
        "{{% foo arg=`[` %}}",
        "{{< foo arg=\"[inner](other.md)\" >}}",
        "{{< box >}}] [inner](other.md){{< /box >}}",
    ] {
        for (prefix, kind) in [("", SyntaxKind::LINK), ("!", SyntaxKind::IMAGE_LINK)] {
            let input = format!("{prefix}[Read {call}](page.md)\n");
            let tree = parse_hugo(&input);
            let link = tree.descendants().find(|node| node.kind() == kind).unwrap();
            let shortcode = link.descendants().find_map(HugoShortcode::cast).unwrap();
            assert_eq!(shortcode.syntax().text().to_string(), call, "{input:?}");
        }
    }
}

#[test]
fn held_shortcode_continuations_can_end_setext_headings() {
    for input in [
        "Heading {{< foo\n  arg=42\n>}}\n===\n",
        "First line\nHeading {{% foo\n  arg=42\n%}} after\n---\n",
        "> Heading {{< foo\n>   arg=42\n> >}}\n> ===\n",
        "- Heading {{< foo\n    arg=42\n  >}}\n  ===\n",
        "> First line\n> Heading {{% foo\n>   arg=42\n> %}}\n> ===\n",
        "- First line\n  Heading {{% foo\n    arg=42\n  %}}\n  ---\n",
    ] {
        let tree = parse_hugo(input);
        let heading = tree
            .descendants()
            .find(|node| node.kind() == SyntaxKind::HEADING)
            .unwrap();
        assert!(
            heading
                .descendants()
                .any(|node| node.kind() == SyntaxKind::HUGO_SHORTCODE),
            "{input:?}"
        );
        assert!(
            heading
                .descendants()
                .any(|node| node.kind() == SyntaxKind::SETEXT_HEADING_UNDERLINE),
            "{input:?}"
        );
    }
}

#[test]
fn incomplete_heading_shortcodes_keep_trailing_decoration_bytes() {
    for input in [
        "# Heading {{< foo\n  arg=42 #\n",
        "# Heading {{% foo arg=42 #\n",
        "# Heading {{< foo\n  arg=42 {#payload}\n",
        "> # Heading {{< foo\n>   arg=42 #\n",
        "- # Heading {{< foo\n    arg=42 #\n",
    ] {
        let tree = parse_hugo(input);
        let shortcode = tree.descendants().find_map(HugoShortcode::cast).unwrap();
        let payload = panache_parser::syntax::text_without_line_prefixes(shortcode.syntax());
        assert!(
            payload.ends_with('#') || payload.ends_with("{#payload}"),
            "{input:?}: {payload:?}"
        );
    }
}

#[test]
fn many_unpaired_inline_shortcodes_remain_separate() {
    let input = format!("Before {}after.\n", "{{< ref \"page.md\" >}} ".repeat(4000));
    let tree = parse_hugo(&input);
    let calls: Vec<_> = tree.descendants().filter_map(HugoShortcode::cast).collect();
    assert_eq!(calls.len(), 4000);
    assert!(calls.iter().all(|call| call.body().is_none()));
    assert!(
        !tree
            .descendants()
            .any(|node| node.kind() == SyntaxKind::HUGO_SHORTCODE_BLOCK)
    );
}

#[test]
fn strikeout_delimiters_skip_shortcode_payloads() {
    for call in [
        "{{% foo arg=\"x~~y   z\" %}}",
        "{{< foo arg=`x~~y   z` >}}",
        "{{< box >}}x~~y   z{{< /box >}}",
        "{{%/* foo arg=\"x~~y   z\" */%}}",
    ] {
        let input = format!("Before ~~{call}~~ after.\n");
        let tree = parse_hugo(&input);
        let strikeout = tree
            .descendants()
            .find(|node| node.kind() == SyntaxKind::STRIKEOUT)
            .unwrap();
        assert_eq!(strikeout.text().to_string(), format!("~~{call}~~"));
        let shortcode = strikeout
            .descendants()
            .find_map(HugoShortcode::cast)
            .unwrap();
        assert_eq!(shortcode.syntax().text().to_string(), call);
    }
}

#[test]
fn bare_uris_stop_before_shortcode_openers() {
    for call in [
        "{{< foo arg=\"a   b\" >}}",
        "{{% foo arg=`a   b` %}}",
        "{{< box >}}a   b{{< /box >}}",
        "{{</* foo arg=\"a   b\" */>}}",
        "{{< unfinished arg=\"a   b\"",
    ] {
        let prefix = "https://example.com/";
        let input = format!("{prefix}{call}\n");
        let tree = parse_hugo(&input);
        let url = tree
            .descendants()
            .find(|node| node.kind() == SyntaxKind::AUTO_LINK)
            .unwrap();
        assert_eq!(url.text().to_string(), prefix);
        let shortcode = tree.descendants().find_map(HugoShortcode::cast).unwrap();
        assert_eq!(shortcode.syntax().text().to_string(), call);
    }
}

#[test]
fn bare_uri_shortcode_boundaries_require_the_extension() {
    let input = "https://example.com/{{< foo >}}\n";
    let mut options = ParserOptions::for_flavor(Flavor::Hugo);
    options.extensions.hugo_shortcodes = false;
    let tree = parse(input, Some(options));
    assert_eq!(tree.text().to_string(), input);
    let url = tree
        .descendants()
        .find(|node| node.kind() == SyntaxKind::AUTO_LINK)
        .unwrap();
    assert_eq!(url.text().to_string(), "https://example.com/{{");
}

#[test]
fn many_unpaired_table_shortcodes_remain_in_one_cell() {
    let input = format!(
        "| A | B |\n| --- | --- |\n| {}| Text |\n",
        "{{< ref \"page|name.md\" >}} ".repeat(2000)
    );
    let tree = parse_hugo(&input);
    let row = tree
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::TABLE_ROW)
        .last()
        .unwrap();
    assert_eq!(
        row.children()
            .filter(|node| node.kind() == SyntaxKind::TABLE_CELL)
            .count(),
        2
    );
    let calls: Vec<_> = row.descendants().filter_map(HugoShortcode::cast).collect();
    assert_eq!(calls.len(), 2000);
    assert!(calls.iter().all(|call| call.body().is_none()));
}

#[test]
fn hugo_defaults_follow_goldmark_without_gfm_extras() {
    let options = ParserOptions::for_flavor(Flavor::Hugo);
    assert_eq!(options.dialect, Dialect::CommonMark);
    let ext = options.extensions;
    assert!(ext.pipe_tables && ext.strikeout && ext.task_lists && ext.autolink_bare_uris);
    assert!(ext.definition_lists && ext.footnotes && ext.header_attributes);
    assert!(ext.auto_identifiers && ext.gfm_auto_identifiers && ext.yaml_metadata_block);
    assert!(ext.toml_metadata_block && ext.hugo_shortcodes);
    assert!(!ext.alerts && !ext.emoji && !ext.tex_math_dollars && !ext.tex_math_gfm);
    assert!(!ext.citations && !ext.inline_footnotes && !ext.quarto_shortcodes);
}

#[test]
fn toml_front_matter_is_opaque_and_document_scoped() {
    for input in [
        "+++\n+++\n",
        "\u{feff}\n\n+++\n\ninvalid TOML\n+++\n",
        "+++\ntitle = 'unclosed'",
        "+++\r\nvalue = 42\r\n+++\r\n",
    ] {
        let tree = parse_hugo(input);
        let metadata = tree.children().find_map(TomlMetadata::cast).unwrap();
        assert!(metadata.content().is_some());
        assert_eq!(metadata.is_closed(), input.trim_end().ends_with("+++"));
    }
    for input in [
        "Text.\n\n+++\nkey = 1\n+++\n",
        "> +++\n> key = 1\n> +++\n",
        "- +++\n  key = 1\n  +++\n",
        "```\n+++\nkey = 1\n+++\n```\n",
    ] {
        assert!(
            !parse_hugo(input)
                .descendants()
                .any(|node| TomlMetadata::can_cast(node.kind()))
        );
    }
    let options = ParserOptions::for_flavor(Flavor::Hugo);
    let tree =
        panache_parser::parser::Parser::new_fragment("+++\nkey = 1\n+++\n", &options).parse();
    assert!(
        !tree
            .descendants()
            .any(|node| TomlMetadata::can_cast(node.kind()))
    );
}

#[test]
fn bodies_are_opaque_and_calls_do_not_become_blockquotes() {
    let input = "Before {{% box %}}\n\n# literal heading\n> literal quote\n{{< box >}}nested{{< /box >}}\n{{% /box %}} after.\n";
    let tree = parse_hugo(input);
    let shortcode = tree.descendants().find_map(HugoShortcode::cast).unwrap();
    assert!(shortcode.opening_tag().unwrap().is_markdown());
    assert!(
        shortcode
            .body()
            .unwrap()
            .text()
            .to_string()
            .contains("# literal heading")
    );
    assert!(
        !tree
            .descendants()
            .any(|node| matches!(node.kind(), SyntaxKind::HEADING | SyntaxKind::BLOCK_QUOTE))
    );
}

#[test]
fn extensions_are_independent_and_off_in_other_flavors() {
    for flavor in [
        Flavor::CommonMark,
        Flavor::Pandoc,
        Flavor::Quarto,
        Flavor::Myst,
        Flavor::Gfm,
    ] {
        let ext = Extensions::for_flavor(flavor);
        assert!(!ext.toml_metadata_block && !ext.hugo_shortcodes);
    }
    let mut options = ParserOptions::for_flavor(Flavor::Hugo);
    options.extensions.hugo_shortcodes = false;
    options.extensions.toml_metadata_block = false;
    let tree = parse("+++\nx = 1\n+++\n\n{{< foo >}}\n", Some(options));
    assert!(!tree.descendants().any(|node| matches!(
        node.kind(),
        SyntaxKind::TOML_METADATA | SyntaxKind::HUGO_SHORTCODE
    )));
    let mut options = ParserOptions::for_flavor(Flavor::CommonMark);
    options.extensions.hugo_shortcodes = true;
    options.extensions.toml_metadata_block = true;
    let tree = parse("+++\nx = 1\n+++\n\n{{< foo\n>}}\n", Some(options));
    assert!(
        tree.descendants()
            .any(|node| TomlMetadata::can_cast(node.kind()))
    );
    assert!(
        tree.descendants()
            .any(|node| HugoShortcode::can_cast(node.kind()))
    );
}

#[test]
fn adjacent_multiline_calls_keep_both_closing_delimiters() {
    let tree = parse_hugo("Before {{< first\n  value=42\n>}}{{< second\n  value=42\n>}} after.\n");
    let closing = tree
        .descendants_with_tokens()
        .filter_map(|element| element.into_token())
        .filter(|token| token.kind() == SyntaxKind::HUGO_SHORTCODE_MARKER_CLOSE)
        .map(|token| token.text().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(closing, [">}}", ">}}"]);
}

#[test]
fn shortcode_destinations_are_links_and_reference_definitions() {
    for destination in [
        "{{< relref \"about.md\" >}}",
        "{{% ref `about (draft).md` %}}#section",
        "/prefix/{{< ref \"a)b.md\" >}}/suffix",
        "<{{< relref \"about.md\" >}}>",
    ] {
        for prefix in ["", "> ", "- "] {
            let input = format!("{prefix}[Read more]({destination} \"Title\")\n");
            let tree = parse_hugo(&input);
            assert!(
                tree.descendants()
                    .any(|node| node.kind() == SyntaxKind::LINK)
            );
        }
        let input = format!("[Read more][page]\n\n[page]: {destination} \"Title\"\n");
        let tree = parse_hugo(&input);
        assert!(
            tree.descendants()
                .any(|node| node.kind() == SyntaxKind::REFERENCE_DEFINITION)
        );
        assert!(
            tree.descendants()
                .filter(|node| node.kind() == SyntaxKind::LINK)
                .count()
                >= 2
        );
    }
}

#[test]
fn standalone_shortcode_in_footnote_preserves_content_indent() {
    for input in [
        "[^n]: Intro\n\n    {{< foo >}}\n\nAfter.\n",
        "[^n]: Intro\n\n    {{< foo\n      value=42\n    >}}\n\nAfter.\n",
        "> [^n]: Intro\n>\n>     {{< foo >}}\n>\n> After.\n",
        "[^n]: Intro\n\n    - Nested\n\n      {{< foo >}}\n",
    ] {
        let tree = parse_hugo(input);
        assert!(
            tree.descendants()
                .any(|node| node.kind() == SyntaxKind::HUGO_SHORTCODE_BLOCK)
        );
    }
}

#[test]
fn heading_shortcodes_keep_their_arguments_and_real_attributes() {
    for input in [
        "# Heading {{< foo value=\"a b\" >}}\n",
        "Heading {{% foo value=\"a b\" %}}\n===\n",
        "# Heading {{< foo value=\"a b\" >}} {#real .class}\n",
    ] {
        let tree = parse_hugo(input);
        let shortcode = tree.descendants().find_map(HugoShortcode::cast).unwrap();
        assert!(
            shortcode
                .syntax()
                .text()
                .to_string()
                .contains("value=\"a b\"")
        );
        let attributes = tree
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::ATTRIBUTE)
            .count();
        assert_eq!(attributes, usize::from(input.contains("{#real")));
    }
}

#[test]
fn heading_attributes_ignore_braces_in_opaque_shortcode_payloads() {
    for call in [
        "{{< foo value=`a } b` >}}",
        "{{< foo value=`a { b` >}}",
        "{{< box >}}a } b{{< /box >}}",
    ] {
        let input = format!("# Heading {call} {{#real}}\n");
        let tree = parse_hugo(&input);
        let attribute = tree
            .descendants()
            .find(|node| node.kind() == SyntaxKind::ATTRIBUTE)
            .unwrap();
        assert_eq!(attribute.text().to_string(), "{#real}");
        let shortcode = tree.descendants().find_map(HugoShortcode::cast).unwrap();
        assert_eq!(shortcode.syntax().text().to_string(), call);
    }
}

#[test]
fn shortcode_destinations_require_the_extension() {
    let mut options = ParserOptions::for_flavor(Flavor::Hugo);
    options.extensions.hugo_shortcodes = false;
    for input in [
        "[Read more]({{< relref \"about.md\" >}})\n",
        "[Read more][page]\n\n[page]: {{< relref \"about.md\" >}}\n",
    ] {
        let tree = parse(input, Some(options.clone()));
        assert_eq!(tree.text().to_string(), input);
        assert!(!tree.descendants().any(|node| matches!(
            node.kind(),
            SyntaxKind::LINK | SyntaxKind::REFERENCE_DEFINITION
        )));
    }
}

#[test]
fn headings_hold_multiline_shortcode_continuations() {
    let call = "{{< foo\n  value=\"a b\"\n>}}";
    for (prefix, continuation) in [
        ("# Heading ", ""),
        ("> # Heading ", "> "),
        ("- # Heading ", "  "),
    ] {
        let input = format!(
            "{prefix}{} after\n\nNext.\n",
            call.replace('\n', &format!("\n{continuation}"))
        );
        let tree = parse_hugo(&input);
        let heading = tree
            .descendants()
            .find(|node| node.kind() == SyntaxKind::HEADING)
            .unwrap();
        let shortcode = heading.descendants().find_map(HugoShortcode::cast).unwrap();
        assert_eq!(
            panache_parser::syntax::text_without_line_prefixes(shortcode.syntax()),
            call,
            "{input:?}"
        );
    }
}

fn check_hugo_edit(before: &str, needle: &str, insert: &str) {
    use panache_parser::parser::{CostGuards, fingerprint, parse_with_errors};
    let options = ParserOptions::for_flavor(Flavor::Hugo);
    let (old, errors) = parse_with_errors(before, Some(options.clone()));
    let start = before.find(needle).unwrap();
    let end = start + needle.len();
    let updated = format!("{}{insert}{}", &before[..start], &before[end..]);
    let (full, full_errors) = parse_with_errors(&updated, Some(options.clone()));
    for cutoff in [CostGuards::Enforced, CostGuards::Ignored] {
        let inc = common::reparse_or_full_with_cost_guards(
            &updated,
            Some(options.clone()),
            &old,
            &errors,
            (start, end),
            (start, start + insert.len()),
            cutoff,
        );
        assert_eq!(inc.tree.text().to_string(), updated);
        assert_eq!(
            fingerprint(&inc.tree),
            fingerprint(&full),
            "{}",
            inc.strategy
        );
        assert_eq!(inc.errors, full_errors);
    }
}

#[test]
fn incremental_edits_can_change_distant_shortcode_pairing() {
    let input = "{{< foo >}}\n\nOne.\n\nTwo.\n\nThree.\n\n{{< /bar >}}\n\nAfter.\n";
    check_hugo_edit(input, "/bar", "/foo");
    check_hugo_edit(input, "Three.", "Three.\n\n{{< /foo >}}");
    check_hugo_edit(input, "foo", "bar");
    check_hugo_edit(&input.replace("/bar", "/foo"), "{{< /foo >}}", "");
    check_hugo_edit(
        &input.replace("{{< /bar >}}", "{{<\n/bar\n>}}"),
        "/bar",
        "/foo",
    );
    check_hugo_edit(
        &input.replace("{{< /bar >}}", "{{% /bar %}}"),
        "{{% /bar %}}",
        "{{< /foo >}}",
    );
}

#[test]
fn incremental_windows_do_not_manufacture_toml_front_matter() {
    check_hugo_edit(
        "First.\n\nSecond.\n\n+++\nkey = 1\n+++\n",
        "key = 1",
        "key = 12",
    );
}

#[test]
fn incremental_edits_can_expose_table_shortcodes_to_distant_pairing() {
    let input = format!(
        "Intro.\n\n| A | B |\n| --- | --- |\n| {{{{< foo >}}}} | Text |\n\nOne.\n\nTwo.\n\nThree.\n\n{{{{< /foo >}}}}\n\n{}",
        "Unrelated paragraph.\n\n".repeat(200)
    );
    assert!(
        parse_hugo(&input)
            .descendants()
            .any(|n| n.kind() == SyntaxKind::PIPE_TABLE)
    );
    let updated = input.replace("| A | B |", "Prefix\n| A | B |");
    assert!(
        parse_hugo(&updated)
            .descendants()
            .filter_map(HugoShortcode::cast)
            .any(|call| call.body().is_some())
    );
    check_hugo_edit(&input, "| A | B |", "Prefix\n| A | B |");
    check_hugo_edit(&updated, "Prefix\n", "");
}

#[test]
fn incremental_edits_preserve_toml_after_leading_blank_lines() {
    for prefix in ["\n", "\u{feff}\n\n"] {
        let input = format!(
            "{prefix}+++\ntitle = 'Keep me'\n+++\n\n{}",
            "Unrelated paragraph.\n\n".repeat(200)
        );
        check_hugo_edit(&input, "+++\n", "+++\n\n\n");
        check_hugo_edit(&input, "title = 'Keep me'", "\ntitle = 'Keep me'");
    }
}

#[test]
fn continuation_lookahead_skips_backslash_escaped_openers() {
    let tree = parse_hugo("Text {{< foo >}} \\{{< box >}}\n\n# Heading\n\n{{< /box >}}\n");
    assert!(
        tree.descendants()
            .any(|node| node.kind() == SyntaxKind::HEADING)
    );
    assert_eq!(
        tree.descendants().filter_map(HugoShortcode::cast).count(),
        2
    );
}

#[test]
fn checkbox_gap_is_list_trivia_before_a_shortcode_block() {
    let tree = parse_hugo("- [ ] {{< foo >}}\n");
    let block = tree
        .descendants()
        .find(|node| node.kind() == SyntaxKind::HUGO_SHORTCODE_BLOCK)
        .unwrap();
    assert_eq!(block.text().to_string(), "{{< foo >}}\n");
}

#[test]
fn shortcode_arguments_do_not_split_pipe_table_cells() {
    for call in [
        "{{< foo arg=\"a|b\" >}}",
        "{{% foo arg=`a|b` %}}",
        "{{< box >}}a|b{{< /box >}}",
    ] {
        let input = format!("| A | B | C |\n| --- | --- | --- |\n| {call} | Text |\n");
        let tree = parse_hugo(&input);
        let shortcode = tree.descendants().find_map(HugoShortcode::cast).unwrap();
        assert_eq!(shortcode.syntax().text().to_string(), call);
        let row = tree
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::TABLE_ROW)
            .last()
            .unwrap();
        assert_eq!(
            row.children()
                .filter(|node| node.kind() == SyntaxKind::TABLE_CELL)
                .count(),
            2
        );
    }
}

#[test]
fn footnote_marker_lines_hold_shortcode_continuations() {
    let call = "{{< foo\n  arg=`a\nb`\n>}}";
    for (prefix, continuation, suffix) in [
        ("[^n]: ", "    ", ""),
        ("[^n]: Before ", "    ", " after"),
        ("> [^n]: ", ">     ", ""),
    ] {
        let input = format!(
            "{prefix}{}{suffix}\n\nAfter.\n",
            call.replace('\n', &format!("\n{continuation}"))
        );
        let tree = parse_hugo(&input);
        let shortcode = tree.descendants().find_map(HugoShortcode::cast).unwrap();
        assert_eq!(
            panache_parser::syntax::text_without_line_prefixes(shortcode.syntax()),
            call,
            "{input:?}"
        );
    }
}

#[test]
fn lookahead_reuses_pairing_across_calls_and_container_changes() {
    let input = "{{< foo >}}\n\n- {{< foo >}}\n\n  {{< foo >}}\n\n> {{< foo >}}\n>\n> {{< foo >}}\n\n{{< foo >}}\n\n{{< /foo >}}\n";
    let tree = parse_hugo(input);
    assert_eq!(
        tree.descendants().filter_map(HugoShortcode::cast).count(),
        6
    );
    assert!(
        tree.descendants()
            .filter_map(HugoShortcode::cast)
            .any(|call| call.body().is_some())
    );
    let input = "`{{< unfinished`\n\n{{< foo >}}\n\n\\{{< unfinished\n\n{{% bar %}}\n";
    let tree = parse_hugo(input);
    assert_eq!(
        tree.descendants().filter_map(HugoShortcode::cast).count(),
        2
    );
    assert!(
        tree.descendants()
            .filter_map(HugoShortcode::cast)
            .any(|call| call.opening_tag().unwrap().content().unwrap().trim() == "foo")
    );
}
