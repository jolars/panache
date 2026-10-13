use panache_formatter::config::{
    Config, Extensions, Flavor, FormatterExtensions, LineEnding, WrapMode,
};
use panache_formatter::format;

fn config(wrap: WrapMode) -> Config {
    Config {
        flavor: Flavor::Hugo,
        parser_extensions: Extensions::for_flavor(Flavor::Hugo),
        formatter_extensions: FormatterExtensions::for_flavor(Flavor::Hugo),
        wrap: Some(wrap),
        line_width: 30,
        ..Config::default()
    }
}

fn shortcode_payloads(text: &str) -> Vec<String> {
    let tree = panache_parser::parse(
        text,
        Some(panache_parser::ParserOptions::for_flavor(Flavor::Hugo)),
    );
    tree.descendants()
        .filter(|node| node.kind() == panache_parser::syntax::SyntaxKind::HUGO_SHORTCODE)
        .map(|node| panache_parser::syntax::text_without_line_prefixes(&node))
        .collect()
}

#[test]
fn reviewed_container_shortcodes_preserve_payloads_and_are_idempotent() {
    for input in [
        include_str!("fixtures/cases/hugo_quoted_heading_shortcode/input.md"),
        include_str!("fixtures/cases/hugo_adjacent_quoted_shortcodes/input.md"),
        include_str!("fixtures/cases/hugo_task_heading_shortcode/input.md"),
        include_str!("fixtures/cases/hugo_task_prose_preserve/input.md"),
        include_str!("fixtures/cases/hugo_leading_definition_shortcode/input.md"),
    ] {
        for wrap in [
            WrapMode::Reflow,
            WrapMode::Sentence,
            WrapMode::Semantic,
            WrapMode::Preserve,
        ] {
            let cfg = config(wrap);
            let output = format(input, Some(cfg.clone()), None);
            assert_eq!(
                shortcode_payloads(&output),
                shortcode_payloads(input),
                "{input:?}"
            );
            assert_eq!(format(&output, Some(cfg), None), output, "{input:?}");
        }
    }
}

#[test]
fn atomic_link_prose_breaks_collapse_across_tokens() {
    for flavor in [
        Flavor::CommonMark,
        Flavor::Pandoc,
        Flavor::Gfm,
        Flavor::Hugo,
    ] {
        for wrap in [WrapMode::Sentence, WrapMode::Semantic] {
            let mut cfg = config(wrap);
            cfg.flavor = flavor;
            cfg.parser_extensions = Extensions::for_flavor(flavor);
            cfg.formatter_extensions = FormatterExtensions::for_flavor(flavor);
            for (input, expected) in [
                ("[a\n b](page.md)\n", "[a b](page.md)\n"),
                ("[a \n b](page.md)\n", "[a b](page.md)\n"),
                ("[a\n\tb](page.md)\n", "[a b](page.md)\n"),
                ("See ![a\n b](image.png).\n", "See ![a b](image.png).\n"),
                ("[a\n `b  c`](page.md)\n", "[a `b  c`](page.md)\n"),
            ] {
                let output = format(input, Some(cfg.clone()), None);
                assert_eq!(output, expected, "{flavor:?}");
                assert_eq!(
                    format(&output, Some(cfg.clone()), None),
                    output,
                    "{flavor:?}"
                );
            }
        }
    }
}

#[test]
fn preserve_wrap_trims_task_prose_padding_around_literal_continuations() {
    let call = "{{< foo\n  arg=`a\n  b`\n>}}";
    for (prefix, continuation) in [("- [ ] ", "  "), ("> - [ ] ", ">   ")] {
        let padded_call = call.replace('\n', &format!("\n{continuation}"));
        let input = format!("{prefix}Before {padded_call} after.\n{continuation}  More prose.\n");
        let expected = format!("{prefix}Before {padded_call} after.\n{continuation}More prose.\n");
        let cfg = config(WrapMode::Preserve);
        let output = format(&input, Some(cfg.clone()), None);
        assert_eq!(output, expected);
        assert_eq!(shortcode_payloads(&output), vec![call.to_string()]);
        assert_eq!(format(&output, Some(cfg), None), output);
    }
}

#[test]
fn reviewed_shortcode_payloads_survive_every_wrapping_mode() {
    for input in [
        include_str!("fixtures/cases/hugo_definition_shortcode/input.md"),
        include_str!("fixtures/cases/hugo_definition_marker_multiline/input.md"),
        include_str!("fixtures/cases/hugo_definition_continuation_multiline/input.md"),
        include_str!("fixtures/cases/hugo_paired_setext_body/input.md"),
        include_str!("fixtures/cases/hugo_link_labels/input.md"),
        include_str!("fixtures/cases/hugo_setext_multiline/input.md"),
        include_str!("fixtures/cases/hugo_incomplete_heading/input.md"),
        include_str!("fixtures/cases/hugo_incomplete_heading_nested/input.md"),
        include_str!("fixtures/cases/hugo_strikeout_multiline/input.md"),
        include_str!("fixtures/cases/hugo_strikeout_shortcode_delimiters/input.md"),
        include_str!("fixtures/cases/hugo_bare_uri_shortcodes/input.md"),
    ] {
        for wrap in [
            WrapMode::Reflow,
            WrapMode::Sentence,
            WrapMode::Semantic,
            WrapMode::Preserve,
        ] {
            let cfg = config(wrap);
            let output = format(input, Some(cfg.clone()), None);
            assert_eq!(
                shortcode_payloads(&output),
                shortcode_payloads(input),
                "{input:?} -> {output:?}"
            );
            assert_eq!(format(&output, Some(cfg), None), output, "{input:?}");
        }
    }
}

#[test]
fn definition_prose_shortcode_payloads_survive_formatting() {
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
                for wrap in [
                    WrapMode::Reflow,
                    WrapMode::Sentence,
                    WrapMode::Semantic,
                    WrapMode::Preserve,
                ] {
                    let cfg = config(wrap);
                    let output = format(&input, Some(cfg.clone()), None);
                    assert_eq!(
                        shortcode_payloads(&output),
                        vec![call.to_string()],
                        "{output:?}"
                    );
                    assert_eq!(format(&output, Some(cfg), None), output);
                }
            }
        }
    }
}

#[test]
fn wrapping_keeps_shortcodes_inside_inline_containers() {
    use panache_parser::syntax::SyntaxKind;
    let call = "{{< relref \"a-very-long-page-name-that-exceeds-the-line-width.md\" >}}";
    for (prefix, suffix, kind) in [
        ("*Before ", " after.*", SyntaxKind::EMPHASIS),
        ("**Before ", " after.**", SyntaxKind::STRONG),
        ("[Before ", " after.](page.md)", SyntaxKind::LINK),
        ("![Before ", " after.](image.png)", SyntaxKind::IMAGE_LINK),
    ] {
        let input = format!("{prefix}{call}{suffix}\n");
        for wrap in [
            WrapMode::Reflow,
            WrapMode::Sentence,
            WrapMode::Semantic,
            WrapMode::Preserve,
        ] {
            let cfg = config(wrap);
            let output = format(&input, Some(cfg.clone()), None);
            let tree = panache_parser::parse(
                &output,
                Some(panache_parser::ParserOptions::for_flavor(Flavor::Hugo)),
            );
            let container = tree
                .descendants()
                .find(|node| node.kind() == kind)
                .unwrap_or_else(|| panic!("lost {kind:?}: {input:?} -> {output:?}"));
            assert!(
                container
                    .descendants()
                    .any(|node| node.kind() == SyntaxKind::HUGO_SHORTCODE),
                "{input:?} -> {output:?}"
            );
            assert_eq!(shortcode_payloads(&output), vec![call.to_string()]);
            assert_eq!(format(&output, Some(cfg), None), output);
        }
    }
}

#[test]
fn atomic_links_preserve_shortcode_argument_newlines() {
    let call = "{{< foo arg=`a\nb` >}}";
    for wrap in [WrapMode::Sentence, WrapMode::Semantic] {
        for input in [
            format!("[Before\n{call} after](page.md)\n"),
            format!("![Before\n{call} after](image.png)\n"),
        ] {
            let cfg = config(wrap.clone());
            let output = format(&input, Some(cfg.clone()), None);
            assert_eq!(
                shortcode_payloads(&output),
                vec![call.to_string()],
                "{input:?} -> {output:?}"
            );
            assert!(
                !output.contains("Before\n"),
                "prose newlines should normalize: {output:?}"
            );
            assert_eq!(format(&output, Some(cfg), None), output);
        }
    }
}

#[test]
fn checkbox_shortcode_spacing_is_idempotent() {
    for wrap in [
        WrapMode::Reflow,
        WrapMode::Sentence,
        WrapMode::Semantic,
        WrapMode::Preserve,
    ] {
        for input in [
            "- [ ] {{< foo >}}\n",
            "- [x]   {{< foo\n    arg=42\n  >}}\n",
        ] {
            let cfg = config(wrap.clone());
            let output = format(input, Some(cfg.clone()), None);
            assert!(
                output.starts_with("- [ ] {{<") || output.starts_with("- [x] {{<"),
                "{output:?}"
            );
            assert_eq!(shortcode_payloads(&output), shortcode_payloads(input));
            assert_eq!(format(&output, Some(cfg), None), output);
        }
    }
}

#[test]
fn preserve_wrap_does_not_duplicate_shortcode_container_indentation() {
    for input in [
        "- Intro.\n\n  Before {{< foo\n    arg=`a\n  b`\n  >}} after.\n",
        "> - Intro.\n>\n>   Before {{< foo\n>     arg=`a\n>   b`\n>   >}} after.\n",
    ] {
        let cfg = config(WrapMode::Preserve);
        let output = format(input, Some(cfg.clone()), None);
        assert_eq!(output, input);
        assert_eq!(format(&output, Some(cfg), None), output);
    }
}

#[test]
fn footnote_marker_shortcode_payloads_survive_formatting() {
    let input = "[^n]: Before {{< foo\n      arg=`a\n    b`\n    >}} after.\n\nAfter.\n";
    for wrap in [
        WrapMode::Reflow,
        WrapMode::Sentence,
        WrapMode::Semantic,
        WrapMode::Preserve,
    ] {
        let cfg = config(wrap);
        let output = format(input, Some(cfg.clone()), None);
        assert_eq!(
            shortcode_payloads(&output),
            vec!["{{< foo\n  arg=`a\nb`\n>}}".to_string()],
            "{output:?}"
        );
        assert_eq!(format(&output, Some(cfg), None), output);
    }
}

#[test]
fn initial_inline_shortcodes_stay_attached_to_following_prose() {
    for call in ["{{< foo >}}", "{{% foo %}}", "{{< a >}} {{% b %}}"] {
        for prefix in ["", "- ", "> ", "[^n]: ", "> [^n]: ", "Term\n: "] {
            let input = format!("{prefix}{call} followingtext. More prose follows.\n");
            for wrap in [
                WrapMode::Reflow,
                WrapMode::Sentence,
                WrapMode::Semantic,
                WrapMode::Preserve,
            ] {
                let mut cfg = config(wrap);
                cfg.line_width = 25;
                let output = format(&input, Some(cfg.clone()), None);
                assert!(
                    output.contains(&format!("{call} followingtext.")),
                    "{input:?} -> {output:?}"
                );
                assert_eq!(shortcode_payloads(&output), shortcode_payloads(&input));
                assert_eq!(format(&output, Some(cfg), None), output, "{input:?}");
            }
        }
    }
}

#[test]
fn hugo_heading_source_spaces_survive_formatting() {
    let input = include_str!("fixtures/cases/hugo_heading_spacing/input.md");
    for wrap in [
        WrapMode::Reflow,
        WrapMode::Sentence,
        WrapMode::Semantic,
        WrapMode::Preserve,
    ] {
        let cfg = config(wrap);
        let output = format(input, Some(cfg.clone()), None);
        assert!(output.contains("# A  B\n"), "{output:?}");
        assert!(output.contains("[Jump](#a--b)"), "{output:?}");
        assert!(output.contains("## *C  D* `E  F`"), "{output:?}");
        assert!(output.contains("> # G  H"), "{output:?}");
        assert!(output.contains("- # I  J"), "{output:?}");
        assert_eq!(format(&output, Some(cfg), None), output);
    }
}

#[test]
fn initial_shortcodes_preserve_abbreviation_sentence_boundaries() {
    for wrap in [WrapMode::Sentence, WrapMode::Semantic] {
        for abbreviation in ["e.g.", "Dr.", "U.S."] {
            let input = format!("{{{{< foo >}}}} {abbreviation} Example follows. Next sentence.\n");
            let cfg = config(wrap.clone());
            let output = format(&input, Some(cfg.clone()), None);
            assert_eq!(
                output,
                format!("{{{{< foo >}}}} {abbreviation} Example follows.\nNext sentence.\n")
            );
            assert_eq!(format(&output, Some(cfg), None), output);
        }
    }
}

#[test]
fn shortcode_table_arguments_survive_formatting() {
    let input = "| A | B | C |\n| --- | --- | --- |\n| {{< foo arg=\"a|b\" >}} | Text |\n";
    let cfg = config(WrapMode::Reflow);
    let output = format(input, Some(cfg.clone()), None);
    assert_eq!(
        shortcode_payloads(&output),
        vec!["{{< foo arg=\"a|b\" >}}".to_string()],
        "{output:?}"
    );
    assert_eq!(format(&output, Some(cfg), None), output);
}

#[test]
fn shortcode_payloads_survive_every_wrapping_mode() {
    let calls = [
        "{{< foo\n  attr=\"value\"\n>}}",
        "{{% foo\n  attr=\"value\"\n%}}",
        "{{< foo value=\">}} and é\" >}}",
        "{{% foo value=`raw\n%}} and more` %}}",
        "{{</* foo\n  attr=\"value\"\n*/>}}",
        "{{< box >}}\n\nKeep   this body.  \n# Heading\n{{< box >}}nested{{< /box >}}\n{{< /box >}}",
        "{{% box %}}\n\nKeep   this body.  \n> Quote\n{{% /box %}}",
        "{{< date.inline >}}\n{{- now | time.Format \"2006\" -}}\n{{< /date.inline >}}",
        "{{< unfinished\n  attr=\"value\"",
        "{{< first\n  value=42\n>}}{{% second\n  value=42\n%}}",
    ];
    for wrap in [
        WrapMode::Reflow,
        WrapMode::Sentence,
        WrapMode::Semantic,
        WrapMode::Preserve,
    ] {
        for call in calls {
            for (prefix, continuation, suffix) in [
                ("", "", "\n"),
                ("Text ", "", " after the call.\n"),
                ("- ", "  ", "\n"),
                ("- Text ", "  ", " after.\n"),
                ("> ", "> ", "\n"),
                ("> Text ", "> ", " after.\n"),
                ("> - ", ">   ", "\n"),
                ("- > ", "  > ", "\n"),
                ("# Heading ", "", "\n"),
                ("> # Heading ", "> ", "\n"),
                ("- # Heading ", "  ", "\n"),
            ] {
                let call = call.replace('\n', &format!("\n{continuation}"));
                let input = format!("{prefix}{call}{suffix}");
                let cfg = config(wrap.clone());
                let output = format(&input, Some(cfg.clone()), None);
                let payloads = |text: &str| {
                    let tree = panache_parser::parse(
                        text,
                        Some(panache_parser::ParserOptions::for_flavor(Flavor::Hugo)),
                    );
                    tree.descendants()
                        .filter(|node| {
                            node.kind() == panache_parser::syntax::SyntaxKind::HUGO_SHORTCODE
                        })
                        .map(|node| panache_parser::syntax::text_without_line_prefixes(&node))
                        .collect::<Vec<_>>()
                };
                assert_eq!(
                    payloads(&output),
                    payloads(&input),
                    "payload changed for {wrap:?}: {input:?} -> {output:?}"
                );
                assert_eq!(
                    format(&output, Some(cfg), None),
                    output,
                    "not idempotent: {input:?}"
                );
            }
        }
    }
}

#[test]
fn toml_contents_and_crlf_survive() {
    let input =
        "+++\r\ntitle   = 'Some   text'\r\n[params]\r\nvalue=42\r\n+++\r\n\r\nNormal   prose.\r\n";
    let mut cfg = config(WrapMode::Reflow);
    cfg.line_ending = Some(LineEnding::Crlf);
    let output = format(input, Some(cfg.clone()), None);
    assert!(
        output.starts_with("+++\r\ntitle   = 'Some   text'\r\n[params]\r\nvalue=42\r\n+++\r\n")
    );
    assert!(output.contains("Normal prose.\r\n"));
    assert_eq!(format(&output, Some(cfg), None), output);
}

#[test]
fn configuration_overrides_merge_with_hugo_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("panache.toml");
    std::fs::write(
        &path,
        "flavor = 'hugo'\n[extensions]\nhugo-shortcodes = false\n",
    )
    .unwrap();
    let (cfg, _) = panache::config::load(Some(&path), dir.path(), None, None).unwrap();
    assert_eq!(cfg.flavor, Flavor::Hugo);
    assert!(!cfg.extensions.hugo_shortcodes);
    assert!(cfg.extensions.toml_metadata_block && cfg.extensions.pipe_tables);
    assert!(!cfg.extensions.tex_math_dollars);

    std::fs::write(
        &path,
        "[flavors]\nhugo = ['content/**/*.md']\n[extensions.hugo]\nfootnotes = false\n",
    )
    .unwrap();
    let file = dir.path().join("content/post/page.md");
    let (cfg, _) = panache::config::load(Some(&path), dir.path(), Some(&file), None).unwrap();
    assert_eq!(cfg.flavor, Flavor::Hugo);
    assert!(cfg.extensions.hugo_shortcodes && cfg.extensions.toml_metadata_block);
    assert!(!cfg.extensions.footnotes);
}

#[test]
fn cli_accepts_hugo_and_preserves_metadata_and_calls() {
    let input =
        "+++\ntitle  = 'Keep   this'\n+++\n\n{{< foo\n  attr=\"value\"\n>}}\n\nNormal   prose.\n";
    assert_cmd::cargo::cargo_bin_cmd!("panache")
        .args(["--flavor", "hugo", "format", "--no-cache"])
        .write_stdin(input)
        .assert()
        .success()
        .stdout(input.replace("Normal   prose.", "Normal prose."));
}

#[test]
fn shortcode_link_destinations_survive_formatting() {
    for destination in [
        "{{< relref \"about.md\" >}}",
        "{{% ref `about (draft).md` %}}#section",
        "/prefix/{{< ref \"a)b.md\" >}}/suffix",
        "<{{< relref \"about.md\" >}}>",
    ] {
        for input in [
            format!("[Read more]({destination})\n"),
            format!("![Image]({destination} \"Title\")\n"),
            format!("[Read more][page]\n\n[page]: {destination} \"Title\"\n"),
        ] {
            let cfg = config(WrapMode::Reflow);
            let output = format(&input, Some(cfg.clone()), None);
            assert!(output.contains(destination), "{input:?} -> {output:?}");
            assert!(!output.contains("\\["), "{input:?} -> {output:?}");
            assert_eq!(format(&output, Some(cfg), None), output);
        }
    }
}

#[test]
fn multiline_shortcode_destinations_preserve_payloads_in_containers() {
    use panache_parser::syntax::{LinkDestUrl, text_without_line_prefixes};
    use rowan::ast::AstNode;

    for destination in [
        "{{< relref\n  \"about.md\"\n>}}",
        "{{%  ref arg=`a\n  \nb` %}}#section",
        "<{{<\n  relref \"about.md\"\n>}}>",
    ] {
        for label in ["[more]", "![image]"] {
            for (prefix, continuation) in [("", ""), ("- ", "  "), ("> ", "> "), ("> - ", ">   ")] {
                let source_dest = destination.replace('\n', &format!("\n{continuation}"));
                let input = format!("{prefix}{label}({source_dest} \"Title\")\n");
                for wrap in [
                    WrapMode::Reflow,
                    WrapMode::Sentence,
                    WrapMode::Semantic,
                    WrapMode::Preserve,
                ] {
                    let cfg = config(wrap.clone());
                    let output = format(&input, Some(cfg.clone()), None);
                    let tree = panache_parser::parse(
                        &output,
                        Some(panache_parser::ParserOptions::for_flavor(Flavor::Hugo)),
                    );
                    let url = tree
                        .descendants()
                        .find_map(LinkDestUrl::cast)
                        .unwrap_or_else(|| panic!("lost destination: {input:?} -> {output:?}"));
                    assert_eq!(
                        text_without_line_prefixes(url.syntax()),
                        destination,
                        "{wrap:?}: {input:?} -> {output:?}"
                    );
                    assert_eq!(format(&output, Some(cfg), None), output, "{input:?}");
                }
            }
        }
    }
}

#[test]
fn quoted_list_shortcodes_preserve_whitespace_only_payload_lines() {
    for call in [
        "{{< foo arg=`a\n  \nb` >}}",
        "{{% foo arg=\"a\n \t \nb\" %}}",
        "{{< foo arg=`a\n\nb` >}}",
    ] {
        for (prefix, continuation) in [
            ("> - ", ">   "),
            ("> > - ", "> >   "),
            ("- > - ", "  >   "),
            ("> - > ", ">   > "),
        ] {
            let input = format!(
                "{prefix}{}\n",
                call.replace('\n', &format!("\n{continuation}"))
            );
            for wrap in [
                WrapMode::Reflow,
                WrapMode::Sentence,
                WrapMode::Semantic,
                WrapMode::Preserve,
            ] {
                let cfg = config(wrap.clone());
                let output = format(&input, Some(cfg.clone()), None);
                assert_eq!(
                    shortcode_payloads(&output),
                    vec![call.to_string()],
                    "{wrap:?}: {input:?} -> {output:?}"
                );
                assert_eq!(format(&output, Some(cfg), None), output, "{input:?}");
            }
        }
    }
}

#[test]
fn shortcode_heading_arguments_are_not_attributes() {
    for input in [
        "# Heading {{< foo value=\"a b\" >}}\n",
        "Heading {{% foo value=\"a b\" %}}\n===\n",
        "# Heading {{< foo value=\"a b\" >}} {#real .class}\n",
    ] {
        let cfg = config(WrapMode::Reflow);
        let output = format(input, Some(cfg.clone()), None);
        assert!(
            output.contains("foo value=\"a b\""),
            "{input:?} -> {output:?}"
        );
        assert!(output.contains("{{"), "{input:?} -> {output:?}");
        assert_eq!(
            output.contains("{#real .class}"),
            input.contains("{#real .class}")
        );
        assert_eq!(format(&output, Some(cfg), None), output);
    }
}

#[test]
fn cached_parses_resolve_shortcode_reference_destinations() {
    use panache::salsa::{FileConfig, SalsaDb, parsed_tree_root, refdef_set};
    let input = "[Read more][page]\n\n[page]: {{< relref \"about.md\" >}}\n";
    let mut db = SalsaDb::default();
    let file = db.update_file_text(
        std::path::PathBuf::from("/virtual/page.md"),
        input.to_owned(),
    );
    let cfg = panache::Config {
        flavor: Flavor::Hugo,
        extensions: Extensions::for_flavor(Flavor::Hugo),
        ..panache::Config::default()
    };
    let config = FileConfig::new(&db, cfg.clone());
    assert!(refdef_set(&db, file, config).contains("page"));
    assert_eq!(
        parsed_tree_root(&db, file, config).green(),
        panache::parser::parse(input, Some(cfg)).green()
    );
}
