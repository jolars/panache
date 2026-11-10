use panache_formatter::config::{Extensions, Flavor, FormatterExtensions, WrapMode};
use panache_formatter::syntax::{AstNode, ImageLink, Link, ReferenceDefinition};
use panache_formatter::{Config, SmartPunctuation, format};

fn assert_mode(input: &str, expected: &str, mode: SmartPunctuation) {
    assert_config(
        input,
        expected,
        Config {
            smart_punctuation: mode,
            ..Config::default()
        },
    );
}

fn assert_config(input: &str, expected: &str, config: Config) {
    for wrap in [
        WrapMode::Preserve,
        WrapMode::Reflow,
        WrapMode::Sentence,
        WrapMode::Semantic,
    ] {
        let cfg = Config {
            wrap: Some(wrap.clone()),
            line_width: 1000,
            math: panache_formatter::MathMode::Verbatim,
            ..config.clone()
        };
        let out = format(input, Some(cfg.clone()), None);
        assert_eq!(out, expected, "{wrap:?}");
        assert_eq!(format(&out, Some(cfg), None), out, "{wrap:?}: idempotency");
    }
}

#[test]
fn default_preserves_punctuation_in_every_flavor() {
    let input = "„Hallo“ ”Hej” “hello” ‘hi’ it's it’s -- --- ... – — …\n";
    assert_eq!(format(input, None, None), input);
    for flavor in [
        Flavor::Pandoc,
        Flavor::Quarto,
        Flavor::RMarkdown,
        Flavor::CommonMark,
        Flavor::Gfm,
        Flavor::MultiMarkdown,
        Flavor::Hugo,
        Flavor::Mdsvex,
        Flavor::Myst,
    ] {
        assert_config(
            input,
            input,
            Config {
                flavor,
                parser_extensions: Extensions::for_flavor(flavor),
                formatter_extensions: FormatterExtensions::for_flavor(flavor),
                ..Config::default()
            },
        );
    }
    assert_eq!(SmartPunctuation::default(), SmartPunctuation::Preserve);
}

#[test]
fn default_preserves_punctuation_in_headings_references_and_literals() {
    let input = concat!(
        "# “hello” — world\n\n",
        "[“hello” — world] and [‘label’][].\n\n",
        "[‘label’]: /url\n\n",
        "`“code” —` and `\\textit{“a”}`{=latex} and $“math”$.\n",
    );
    assert_config(input, input, Config::default());
}

#[test]
fn unicode_converts_ascii_punctuation() {
    assert_mode(
        "He said \"hi\" -- it's 'fine'... --- ok\n",
        "He said “hi” – it’s ‘fine’… — ok\n",
        SmartPunctuation::Unicode,
    );
}

#[test]
fn unicode_pairs_quotes_across_markup() {
    assert_mode(
        "A \"*b*\" and (\"c\"), \"[d](u)\".\n",
        "A “*b*” and (“c”), “[d](u)”.\n",
        SmartPunctuation::Unicode,
    );
}

#[test]
fn unicode_pairs_quotes_inside_delimited_prose() {
    assert_mode(
        "~~\"hi\"~~ ~~'hi'~~ ^\"hi\"^ ^'hi'^ ~\"hi\"~ ~'hi'~\n",
        "~~“hi”~~ ~~‘hi’~~ ^“hi”^ ^‘hi’^ ~“hi”~ ~‘hi’~\n",
        SmartPunctuation::Unicode,
    );
}

#[test]
fn unicode_converts_native_html_span_content() {
    for flavor in [Flavor::Pandoc, Flavor::Quarto] {
        assert_config(
            "<span title=\"'literal' --\">\"hi\" -- *'there'*</span>\n",
            "<span title=\"'literal' --\">“hi” – *‘there’*</span>\n",
            Config {
                flavor,
                parser_extensions: panache_formatter::config::Extensions::for_flavor(flavor),
                formatter_extensions: FormatterExtensions::for_flavor(flavor),
                smart_punctuation: SmartPunctuation::Unicode,
                ..Config::default()
            },
        );
    }
}

#[test]
fn smart_preserves_opaque_citation_content() {
    let input = concat!(
        "[@a, [x](https://a--b)]\n\n",
        "[@a, `\"code\" -- …` $x -- y$ \\\"escaped\\\" \\-\\- “literal”]\n",
    );
    for mode in [SmartPunctuation::Unicode, SmartPunctuation::Ascii] {
        assert_mode(input, input, mode);
    }
    assert_mode(
        "\"[@a, literal --]\" --\n",
        "“[@a, literal --]” –\n",
        SmartPunctuation::Unicode,
    );
}

#[test]
fn unicode_slices_multiline_table_cells_before_expanding_quotes() {
    assert_config(
        "------------\n\"a\"  b\n---  ------\n\"x\"  y\n------------\n",
        "-----------\n“a”  b\n---- ------\n“x”  y\n-----------\n",
        Config {
            smart_punctuation: SmartPunctuation::Unicode,
            table_indent: 0,
            ..Config::default()
        },
    );
}

#[test]
fn unicode_keeps_multiline_table_content_in_its_source_columns() {
    assert_config(
        "-----------------------------\n\"a\"     wxyz  tail\n------- ----- ---------------\n\"b\"     data  `x--y`\n-----------------------------\n",
        "-----------------------------\n“a”     wxyz  tail\n------- ----- ---------------\n“b”     data  `x--y`\n-----------------------------\n",
        Config {
            smart_punctuation: SmartPunctuation::Unicode,
            table_indent: 0,
            ..Config::default()
        },
    );
}

#[test]
fn unicode_normalizes_multiline_table_continuations_before_reflow() {
    let input = "---------------------\nh1         h2\n---------- ----------\na          b\n           x--y\n---------------------\n";
    for wrap in [
        None,
        Some(WrapMode::Preserve),
        Some(WrapMode::Reflow),
        Some(WrapMode::Sentence),
        Some(WrapMode::Semantic),
    ] {
        let expected = if wrap == Some(WrapMode::Preserve) {
            "---------------------\nh1         h2\n---------- ----------\na          b\n           x–y\n---------------------\n"
        } else {
            "---------------------\nh1         h2\n---------- ----------\na          b x–y\n---------------------\n"
        };
        let config = Config {
            wrap,
            smart_punctuation: SmartPunctuation::Unicode,
            table_indent: 0,
            ..Config::default()
        };
        let out = format(input, Some(config.clone()), None);
        assert_eq!(out, expected, "{:?}", config.wrap);
        assert_eq!(format(&out, Some(config), None), out, "idempotency");
    }
}

#[test]
fn unicode_pairs_quotes_across_multiline_table_cell_lines() {
    let input = "--------------------------\nh1         h2\n---------- ---------------\na          \"hello\n           *world*\" `x--y`\n--------------------------\n";
    let expected = "--------------------------\nh1         h2\n---------- ---------------\na          “hello *world*”\n           `x--y`\n--------------------------\n";
    let config = Config {
        smart_punctuation: SmartPunctuation::Unicode,
        table_indent: 0,
        ..Config::default()
    };
    let out = format(input, Some(config.clone()), None);
    assert_eq!(out, expected);
    assert_eq!(format(&out, Some(config), None), out, "idempotency");
}

#[test]
fn unicode_preserves_multiline_table_reference_labels() {
    for flavor in [Flavor::CommonMark, Flavor::Gfm] {
        let mut extensions = Extensions::for_flavor(flavor);
        extensions.multiline_tables = true;
        for image in ["", "!"] {
            for suffix in ["", "[]"] {
                let table = format!(
                    "---------------------\nh1         h2\n---------- ----------\na          prose--\n           {image}[foo--]{suffix}\n---------------------\n"
                );
                for input in [
                    format!("{table}\n[foo--]: /url\n"),
                    format!("[foo--]: /url\n\n{table}"),
                ] {
                    assert_config(
                        &input,
                        &input.replace("prose--", "prose–"),
                        Config {
                            flavor,
                            parser_extensions: extensions.clone(),
                            formatter_extensions: FormatterExtensions::for_flavor(flavor),
                            smart_punctuation: SmartPunctuation::Unicode,
                            table_indent: 0,
                            ..Config::default()
                        },
                    );
                }
            }
        }
    }
}

#[test]
fn unicode_handles_nested_quotes_and_apostrophes() {
    assert_mode(
        "\"She said 'no'\" in the season's finale.\n",
        "“She said ‘no’” in the season’s finale.\n",
        SmartPunctuation::Unicode,
    );
    assert_mode(
        "'tis the season.\n",
        "’tis the season.\n",
        SmartPunctuation::Unicode,
    );
    assert_mode(
        "the dogs' bowls.\n",
        "the dogs’ bowls.\n",
        SmartPunctuation::Unicode,
    );
}

#[test]
fn unicode_preserves_existing_quote_styles_and_unpaired_double_quotes() {
    let input = "„Hallo“ ”Hej” «bonjour» and “hello” with an unpaired \".\n";
    assert_mode(input, input, SmartPunctuation::Unicode);
}

#[test]
fn unicode_preserves_escaped_and_verbatim_punctuation() {
    let input = concat!(
        "\\\"a\\\" \\-\\- \\.\\.\\.x `\"b\" --` $c -- d$ <https://e--f> ",
        "[g](https://h--i \"title --\") [j]{title=\"'k' --\"} <span title=\"'l' --\">m</span>\n",
    );
    assert_mode(input, input, SmartPunctuation::Unicode);
}

#[test]
fn unicode_pairs_quotes_beside_escaped_punctuation() {
    for flavor in [Flavor::CommonMark, Flavor::Gfm, Flavor::Pandoc] {
        for (input, expected) in [
            ("\"hello\"\\!\n", "“hello”\\!\n"),
            ("\\(\"world\"\\)\n", "\\(“world”\\)\n"),
            ("\\['again'\\]\n", "\\[‘again’\\]\n"),
        ] {
            assert_config(
                input,
                expected,
                Config {
                    flavor,
                    parser_extensions: Extensions::for_flavor(flavor),
                    formatter_extensions: FormatterExtensions::for_flavor(flavor),
                    smart_punctuation: SmartPunctuation::Unicode,
                    ..Config::default()
                },
            );
        }
    }
}

#[test]
fn unicode_preserves_heading_implicit_references() {
    for flavor in [Flavor::Pandoc, Flavor::Quarto, Flavor::RMarkdown] {
        assert_config(
            concat!(
                "# Hello -- world\n\n[Hello -- world]\n\n",
                "# *foo--bar* {#fixed}\n\n[*foo--bar*][] -- prose\n",
            ),
            concat!(
                "# Hello -- world\n\n[Hello -- world]\n\n",
                "# *foo--bar* {#fixed}\n\n[*foo--bar*][] – prose\n",
            ),
            Config {
                flavor,
                parser_extensions: Extensions::for_flavor(flavor),
                formatter_extensions: FormatterExtensions::for_flavor(flavor),
                smart_punctuation: SmartPunctuation::Unicode,
                ..Config::default()
            },
        );
    }
}

#[test]
fn unicode_preserves_explicit_heading_references_without_automatic_identifiers() {
    for flavor in [Flavor::Pandoc, Flavor::Quarto, Flavor::RMarkdown] {
        let mut extensions = Extensions::for_flavor(flavor);
        extensions.auto_identifiers = false;
        assert_config(
            concat!(
                "# Hello -- world {#fixed}\n\n[Hello -- world]\n\n",
                "# *foo--bar* {#nested}\n\n[*foo--bar*][] -- prose\n",
            ),
            concat!(
                "# Hello -- world {#fixed}\n\n[Hello -- world]\n\n",
                "# *foo--bar* {#nested}\n\n[*foo--bar*][] – prose\n",
            ),
            Config {
                flavor,
                parser_extensions: extensions,
                formatter_extensions: FormatterExtensions::for_flavor(flavor),
                smart_punctuation: SmartPunctuation::Unicode,
                ..Config::default()
            },
        );
    }
}

#[test]
fn unicode_preserves_generated_heading_ids() {
    for flavor in [Flavor::Gfm, Flavor::Hugo] {
        assert_config(
            "# *foo--bar*\n\n[go](#foo--bar) -- prose\n",
            "# *foo--bar*\n\n[go](#foo--bar) – prose\n",
            Config {
                flavor,
                parser_extensions: Extensions::for_flavor(flavor),
                formatter_extensions: FormatterExtensions::for_flavor(flavor),
                smart_punctuation: SmartPunctuation::Unicode,
                ..Config::default()
            },
        );
    }
}

#[test]
fn unicode_converts_headings_without_automatic_identifiers() {
    assert_config(
        "# \"hello\" -- *world...*\n",
        "# “hello” – *world…*\n",
        Config {
            flavor: Flavor::CommonMark,
            parser_extensions: Extensions::for_flavor(Flavor::CommonMark),
            formatter_extensions: FormatterExtensions::for_flavor(Flavor::CommonMark),
            smart_punctuation: SmartPunctuation::Unicode,
            ..Config::default()
        },
    );
}

#[test]
fn unicode_preserves_bare_addresses_without_autolinking() {
    for flavor in [Flavor::CommonMark, Flavor::Gfm, Flavor::Pandoc] {
        let mut extensions = Extensions::for_flavor(flavor);
        extensions.autolink_bare_uris = false;
        for address in [
            "https://example.com/a--b...",
            "HTTP://example.com/a---b",
            "ftp://example.com/a--b",
            "doi:10.1234/a--b",
            "www.example.com/it's--ok",
            "me--you@example.com...",
            "mailto:me--you@example.com",
            "https://example.com/foo*foo--bar*tail...",
            "person*foo--bar*@example.com...",
        ] {
            assert_config(
                &format!("See café: {address} -- 'prose'\n"),
                &format!("See café: {address} – ‘prose’\n"),
                Config {
                    flavor,
                    parser_extensions: extensions.clone(),
                    formatter_extensions: FormatterExtensions::for_flavor(flavor),
                    smart_punctuation: SmartPunctuation::Unicode,
                    ..Config::default()
                },
            );
        }
    }
}

#[test]
fn unicode_preserves_gfm_bare_urls_in_text_tokens() {
    assert_config(
        concat!(
            "See www.example.com/a--b... -- 'ok'\n\n",
            "*www.example.com/a---b* and www.example.com/it's--ok -- end\n\n",
            "Contact person--name@example.com... -- end\n",
            "\nhttps://example.com/a--b... -- end\n\n<https://www.example.com/a--b>... <person--name@example.com>...\n",
        ),
        concat!(
            "See www.example.com/a--b... – ‘ok’\n\n",
            "*www.example.com/a---b* and www.example.com/it's--ok – end\n\n",
            "Contact person--name@example.com... – end\n",
            "\nhttps://example.com/a--b... – end\n\n<https://www.example.com/a--b>… <person--name@example.com>…\n",
        ),
        Config {
            flavor: Flavor::Gfm,
            parser_extensions: Extensions::for_flavor(Flavor::Gfm),
            formatter_extensions: FormatterExtensions::for_flavor(Flavor::Gfm),
            smart_punctuation: SmartPunctuation::Unicode,
            ..Config::default()
        },
    );
}

#[test]
fn unicode_preserves_gfm_emails_after_non_prose_at_signs() {
    for prefix in ["`@`", "`@@`", "[link](mailto:other@example.com)"] {
        assert_config(
            &format!("{prefix}person--name@example.com -- 'prose'\n"),
            &format!("{prefix}person--name@example.com – ‘prose’\n"),
            Config {
                flavor: Flavor::Gfm,
                parser_extensions: Extensions::for_flavor(Flavor::Gfm),
                formatter_extensions: FormatterExtensions::for_flavor(Flavor::Gfm),
                smart_punctuation: SmartPunctuation::Unicode,
                ..Config::default()
            },
        );
    }
}

#[test]
fn unicode_preserves_gfm_bare_links_across_inline_nodes() {
    for link in [
        "www.example.com/foo*foo--bar*",
        "https://example.com/foo*foo--bar*tail...",
        "www.example.com/foo**bar--*baz---qux***tail...",
        "www.example.com/foo~~bar---baz~~tail--end",
        "www.example.com/foo*it's--'ok'...end*tail---end",
        "person*foo--bar*@example.com...",
    ] {
        assert_config(
            &format!("See café: {link} -- 'prose'\n"),
            &format!("See café: {link} – ‘prose’\n"),
            Config {
                flavor: Flavor::Gfm,
                parser_extensions: Extensions::for_flavor(Flavor::Gfm),
                formatter_extensions: FormatterExtensions::for_flavor(Flavor::Gfm),
                smart_punctuation: SmartPunctuation::Unicode,
                ..Config::default()
            },
        );
    }
}

#[test]
fn unicode_does_not_extend_bare_link_protection_past_whitespace() {
    assert_config(
        "www.example.com/foo*bar-- baz--* -- prose\n",
        "www.example.com/foo*bar-- baz–* – prose\n",
        Config {
            flavor: Flavor::Gfm,
            parser_extensions: Extensions::for_flavor(Flavor::Gfm),
            formatter_extensions: FormatterExtensions::for_flavor(Flavor::Gfm),
            smart_punctuation: SmartPunctuation::Unicode,
            ..Config::default()
        },
    );
}

#[test]
fn unicode_does_not_protect_link_text_because_of_bare_urls_in_syntax() {
    for flavor in [Flavor::CommonMark, Flavor::Gfm, Flavor::Pandoc] {
        assert_config(
            "[hello--](https://www.example.com) [world--](mailto:a@example.com) [again--](https://example.com) -- prose\n",
            "[hello–](https://www.example.com) [world–](mailto:a@example.com) [again–](https://example.com) – prose\n",
            Config {
                flavor,
                parser_extensions: Extensions::for_flavor(flavor),
                formatter_extensions: FormatterExtensions::for_flavor(flavor),
                smart_punctuation: SmartPunctuation::Unicode,
                ..Config::default()
            },
        );
    }
}

#[test]
fn unicode_pairs_quotes_across_line_block_continuations() {
    assert_mode(
        "| \"hello\n  world\"\n| 'another\n  *line*'\n",
        "| “hello world”\n| ‘another *line*’\n",
        SmartPunctuation::Unicode,
    );
    assert_mode(
        "| \"hello\n| world\"\n",
        "| \"hello\n| world\"\n",
        SmartPunctuation::Unicode,
    );
}

#[test]
fn smart_preserves_literals_across_line_block_continuations() {
    for (opening, continuation, joined) in [
        (
            "[text](url",
            "\"a — -- 'b'...\")",
            "[text](url \"a — -- 'b'...\")",
        ),
        (
            "![text](url \"a",
            "— -- 'b'...\")",
            "![text](url \"a — -- 'b'...\")",
        ),
        (
            "<span",
            "title=\"a — -- 'b'...\">text</span>",
            "<span title=\"a — -- 'b'...\">text</span>",
        ),
        (
            "<i title=\"a",
            "— -- 'b'...\">text</i>",
            "<i title=\"a — -- 'b'...\">text</i>",
        ),
        ("`a", "— -- 'b'...`", "`a — -- 'b'...`"),
        (
            "[text]{title=\"a",
            "— -- 'b'...\"}",
            "[text]{title=\"a — -- 'b'...\"}",
        ),
        ("$a", "— -- 'b'...$", "$a — -- 'b'...$"),
    ] {
        for (start, prefix) in [("| ", ""), ("> | ", "> "), ("- | ", "  ")] {
            for (mode, prose) in [
                (SmartPunctuation::Ascii, "--- \"prose\" --"),
                (SmartPunctuation::Unicode, "— “prose” –"),
                (SmartPunctuation::Preserve, "— \"prose\" --"),
            ] {
                assert_mode(
                    &format!("{start}{opening}\n{prefix}  {continuation} — \"prose\" --\n"),
                    &format!("{start}{joined} {prose}\n"),
                    mode,
                );
            }
        }
    }
}

#[test]
fn ascii_preserves_bare_addresses_across_inline_nodes() {
    for autolink in [false, true] {
        let mut extensions = Extensions::for_flavor(Flavor::Pandoc);
        extensions.autolink_bare_uris = autolink;
        for prefix in ["", "| "] {
            for address in [
                "https://example.com/foo*foo–bar*tail…",
                "www.example.com/foo**bar–*baz—qux***tail…",
                "www.example.com/foo*it’s–‘ok’…end*tail—end",
                "person*foo–bar*@example.com…",
            ] {
                assert_config(
                    &format!("{prefix}See {address} — ‘prose’\n"),
                    &format!("{prefix}See {address} --- 'prose'\n"),
                    Config {
                        parser_extensions: extensions.clone(),
                        smart_punctuation: SmartPunctuation::Ascii,
                        ..Config::default()
                    },
                );
            }
        }
    }
}

#[test]
fn ascii_does_not_extend_bare_link_protection_past_whitespace() {
    assert_mode(
        "| www.example.com/foo*bar– baz–* — prose\n",
        "| www.example.com/foo*bar– baz--* --- prose\n",
        SmartPunctuation::Ascii,
    );
}

#[test]
fn ascii_does_not_protect_link_text_because_of_bare_urls_in_syntax() {
    assert_mode(
        "| [hello–](https://example.com) [world–](mailto:a@example.com) — prose\n",
        "| [hello--](https://example.com) [world--](mailto:a@example.com) --- prose\n",
        SmartPunctuation::Ascii,
    );
}

#[test]
fn ascii_line_block_dashes_do_not_become_table_separators() {
    for dash in ["—", "–", ":—:", "— | —", "— + —"] {
        let input = format!("| A\n| {dash}\n");
        assert_mode(&input, &input, SmartPunctuation::Ascii);
    }
    assert_mode(
        "| A\n| text —\n",
        "| A\n| text ---\n",
        SmartPunctuation::Ascii,
    );
}

#[test]
fn ascii_preserves_line_block_dashes_after_joining_continuations() {
    assert_mode(
        "| A\n| —\n  | —\n",
        "| A\n| — | —\n",
        SmartPunctuation::Ascii,
    );
}

#[test]
fn smart_preserves_line_block_structure_in_containers() {
    for (start, prefix) in [("> | A", "> "), ("- | A", "  ")] {
        let ascii = format!("{start}\n{prefix}| —\n");
        assert_mode(&ascii, &ascii, SmartPunctuation::Ascii);

        let unicode = format!("{start}\n{prefix}| \"hello\n{prefix}  world\"\n");
        let expected = format!("{start}\n{prefix}| “hello world”\n");
        assert_mode(&unicode, &expected, SmartPunctuation::Unicode);
    }
}

#[test]
fn preserve_keeps_both_spellings() {
    let input = "\"a\" 'b' -- --- ... “c” ‘d’ – — …\n";
    assert_mode(input, input, SmartPunctuation::Preserve);
}

#[test]
fn unicode_works_without_smart_extensions() {
    let cfg = Config {
        flavor: Flavor::CommonMark,
        parser_extensions: panache_formatter::config::Extensions::for_flavor(Flavor::CommonMark),
        formatter_extensions: panache_formatter::config::FormatterExtensions::for_flavor(
            Flavor::CommonMark,
        ),
        smart_punctuation: SmartPunctuation::Unicode,
        ..Config::default()
    };
    assert_eq!(format("\"a\" -- b...\n", Some(cfg), None), "“a” – b…\n");
}

#[test]
fn unicode_pairs_quotes_inside_link_text_and_spans() {
    assert_mode(
        "[\"a\"](u) [\"b\"]{.x} \"<span>c</span>\"\n",
        "[“a”](u) [“b”]{.x} “<span>c</span>”\n",
        SmartPunctuation::Unicode,
    );
}

#[test]
fn unicode_does_not_resolve_literal_reference_labels() {
    for (label, converted) in [
        ("foo--", "foo–"),
        ("*foo--*", "*foo–*"),
        ("'foo'", "‘foo’"),
        ("[foo--]", "&#91;foo–"),
    ] {
        for image in ["", "!"] {
            for suffix in ["", "[]", "[foo--]"] {
                for heading in ["", "# "] {
                    for wrap in [
                        WrapMode::Preserve,
                        WrapMode::Reflow,
                        WrapMode::Sentence,
                        WrapMode::Semantic,
                    ] {
                        let input = format!(
                            "{heading}{image}[{label}]{suffix} -- prose\n\n[{converted}]: /url\n[foo–]: /other\n"
                        );
                        let config = Config {
                            flavor: Flavor::CommonMark,
                            parser_extensions: Extensions::for_flavor(Flavor::CommonMark),
                            formatter_extensions: FormatterExtensions::for_flavor(
                                Flavor::CommonMark,
                            ),
                            smart_punctuation: SmartPunctuation::Unicode,
                            wrap: Some(wrap),
                            ..Config::default()
                        };
                        let output = format(&input, Some(config.clone()), None);
                        assert!(output.contains("– prose"), "{output:?}");
                        let tree = panache_formatter::parser::parse(
                            &output,
                            Some(config.parser_options()),
                        );
                        assert!(
                            tree.descendants()
                                .filter(|node| Link::can_cast(node.kind())
                                    || ImageLink::can_cast(node.kind()))
                                .all(|node| node.ancestors().any(|ancestor| {
                                    ReferenceDefinition::can_cast(ancestor.kind())
                                })),
                            "created a reference: {output:?}"
                        );
                        assert_eq!(format(&output, Some(config), None), output, "idempotency");
                    }
                }
            }
        }
    }
}

#[test]
fn unicode_escapes_multiple_reference_conflicts_in_a_container() {
    assert_config(
        "[foo–]: /one\n[*bar…*]: /two\n\n> # [foo--] and [*bar...*] -- prose\n",
        "[foo–]: /one\n[*bar…*]: /two\n\n> # &#91;foo–] and &#91;*bar…*] – prose\n",
        Config {
            flavor: Flavor::CommonMark,
            parser_extensions: Extensions::for_flavor(Flavor::CommonMark),
            formatter_extensions: FormatterExtensions::for_flavor(Flavor::CommonMark),
            smart_punctuation: SmartPunctuation::Unicode,
            ..Config::default()
        },
    );
}

#[test]
fn unicode_escapes_reference_conflicts_without_creating_math() {
    let mut extensions = Extensions::for_flavor(Flavor::CommonMark);
    extensions.tex_math_single_backslash = true;
    assert_config(
        "# [foo--] -- prose\n\n[foo--] -- prose\n\n[foo–]: /url\n",
        "# &#91;foo–] – prose\n\n&#91;foo–] – prose\n\n[foo–]: /url\n",
        Config {
            flavor: Flavor::CommonMark,
            parser_extensions: extensions,
            formatter_extensions: FormatterExtensions::for_flavor(Flavor::CommonMark),
            smart_punctuation: SmartPunctuation::Unicode,
            ..Config::default()
        },
    );
}

#[test]
fn unicode_converts_literal_brackets_without_reference_conflicts() {
    assert_config(
        "# [*foo--*] -- prose\n\n[other]: /url\n",
        "# [*foo–*] – prose\n\n[other]: /url\n",
        Config {
            flavor: Flavor::CommonMark,
            parser_extensions: Extensions::for_flavor(Flavor::CommonMark),
            formatter_extensions: FormatterExtensions::for_flavor(Flavor::CommonMark),
            smart_punctuation: SmartPunctuation::Unicode,
            ..Config::default()
        },
    );
}

#[test]
fn unicode_preserves_implicit_reference_labels() {
    for flavor in [Flavor::CommonMark, Flavor::Pandoc] {
        for image in ["", "!"] {
            for suffix in ["", "[]"] {
                let input = format!(
                    "See {image}[foo--bar]{suffix} and {image}[*foo--bar*]{suffix} --.\n\n[foo--bar]: https://example.com\n[*foo--bar*]: https://example.com\n"
                );
                let expected = input.replace(" --.", " –.");
                assert_config(
                    &input,
                    &expected,
                    Config {
                        flavor,
                        parser_extensions: panache_formatter::config::Extensions::for_flavor(
                            flavor,
                        ),
                        formatter_extensions: FormatterExtensions::for_flavor(flavor),
                        smart_punctuation: SmartPunctuation::Unicode,
                        ..Config::default()
                    },
                );
            }
        }
    }
}

#[test]
fn ascii_preserves_implicit_reference_labels() {
    assert_mode(
        "[foo–bar][] [foo–bar] ![foo–bar][] ![foo–bar] —\n\n[foo–bar]: https://example.com\n",
        "[foo–bar][] [foo–bar] ![foo–bar][] ![foo–bar] ---\n\n[foo–bar]: https://example.com\n",
        SmartPunctuation::Ascii,
    );
}

#[test]
fn unicode_converts_explicit_reference_text_without_changing_labels() {
    assert_mode(
        "See [foo--bar][key--name] and ![foo--bar][key--name].\n\n[key--name]: https://example.com\n",
        "See [foo–bar][key--name] and ![foo–bar][key--name].\n\n[key--name]: https://example.com\n",
        SmartPunctuation::Unicode,
    );
}

#[test]
fn unicode_preserves_bookdown_identifiers() {
    let flavor = Flavor::RMarkdown;
    assert_config(
        concat!(
            "Table: (\\#tab:my--table) \"Caption\" --\n\n",
            "(ref:my--text) \"Text\" --\n\n",
            "Use \\@ref(tab:my--table) and (ref:my--text) --.\n",
        ),
        concat!(
            "Table: (\\#tab:my--table) “Caption” –\n\n",
            "(ref:my--text) “Text” –\n\n",
            "Use \\@ref(tab:my--table) and (ref:my--text) –.\n",
        ),
        Config {
            flavor,
            parser_extensions: panache_formatter::config::Extensions::for_flavor(flavor),
            formatter_extensions: FormatterExtensions::for_flavor(flavor),
            smart_punctuation: SmartPunctuation::Unicode,
            ..Config::default()
        },
    );
}

#[test]
fn unicode_converts_bookdown_like_prose_when_extension_is_disabled() {
    assert_mode(
        "Use (ref:my--text) --.\n",
        "Use (ref:my–text) –.\n",
        SmartPunctuation::Unicode,
    );
}

#[test]
fn unicode_pairs_quotes_at_raw_html_boundaries() {
    for flavor in [Flavor::CommonMark, Flavor::Pandoc] {
        assert_config(
            concat!(
                "<em>\"hi\"</em> and \"a\"<br>\"b\".\n\n",
                "\"<em>hi</em>\" and <em>'hi'</em>.\n\n",
                "<em title=\"'literal' --\">\"hi\" --</em> and \"`code--`\".\n",
            ),
            concat!(
                "<em>“hi”</em> and “a”<br>“b”.\n\n",
                "“<em>hi</em>” and <em>‘hi’</em>.\n\n",
                "<em title=\"'literal' --\">“hi” –</em> and “`code--`”.\n",
            ),
            Config {
                flavor,
                parser_extensions: panache_formatter::config::Extensions::for_flavor(flavor),
                formatter_extensions: FormatterExtensions::for_flavor(flavor),
                smart_punctuation: SmartPunctuation::Unicode,
                ..Config::default()
            },
        );
    }
}

#[test]
fn unicode_converts_standalone_figure_captions() {
    for flavor in [Flavor::Pandoc, Flavor::Quarto, Flavor::RMarkdown] {
        assert_config(
            "![\"hi\" -- *'there'* `code--`](image--name.png \"title --\")\n",
            "![“hi” – *‘there’* `code--`](image--name.png \"title --\")\n",
            Config {
                flavor,
                parser_extensions: panache_formatter::config::Extensions::for_flavor(flavor),
                formatter_extensions: FormatterExtensions::for_flavor(flavor),
                smart_punctuation: SmartPunctuation::Unicode,
                ..Config::default()
            },
        );
    }
}

#[test]
fn unicode_converts_inline_children_in_definition_terms() {
    assert_mode(
        "[\"term\" --](url) [\"span\" --]{.x} *[\"nested\" --](url)*\n:   Definition\n",
        "[“term” –](url) [“span” –]{.x} *[“nested” –](url)*\n:   Definition\n",
        SmartPunctuation::Unicode,
    );
}

#[test]
fn unicode_applies_to_terms_line_blocks_and_table_cells() {
    let cfg = Config {
        smart_punctuation: SmartPunctuation::Unicode,
        ..Config::default()
    };
    let input = "\"Term\" --\n:   Definition\n\n| \"Line\" --\n\n| \"Cell\" |\n| -------- |\n| value -- |\n";
    let out = format(input, Some(cfg.clone()), None);
    for text in ["“Term” –", "| “Line” –", "“Cell”", "value –"] {
        assert!(out.contains(text), "missing {text:?}: {out}");
    }
    assert_eq!(format(&out, Some(cfg), None), out);
}

#[test]
fn smart_enabled_normalizes_curly_quotes_and_dashes() {
    let mut cfg = Config {
        smart_punctuation: SmartPunctuation::Ascii,
        ..Config::default()
    };
    cfg.formatter_extensions.smart = true;
    cfg.wrap = Some(WrapMode::Preserve);
    let input = "Curly: ‘single’ “double” – en — em\n";
    let expected = "Curly: 'single' \"double\" -- en --- em\n";
    let out = format(input, Some(cfg), None);
    assert_eq!(out, expected);
}

#[test]
fn preserve_keeps_curly_quotes_and_dashes() {
    let cfg = Config {
        smart_punctuation: SmartPunctuation::Preserve,
        wrap: Some(WrapMode::Preserve),
        ..Config::default()
    };
    let input = "Curly: ‘single’ “double” – en — em\n";
    let out = format(input, Some(cfg), None);
    assert_eq!(out, input);
}

#[test]
fn ascii_without_smart_extensions_preserves_punctuation() {
    let cfg = Config {
        smart_punctuation: SmartPunctuation::Ascii,
        formatter_extensions: FormatterExtensions {
            smart: false,
            smart_quotes: false,
            ..FormatterExtensions::default()
        },
        ..Config::default()
    };
    let input = "Curly: ‘single’ “double” – en — em …\n";
    assert_config(input, input, cfg);
}

#[test]
fn ascii_with_smart_quotes_only_preserves_dashes_and_ellipsis() {
    let cfg = Config {
        smart_punctuation: SmartPunctuation::Ascii,
        formatter_extensions: FormatterExtensions {
            smart: false,
            smart_quotes: true,
            ..FormatterExtensions::default()
        },
        ..Config::default()
    };
    assert_config(
        "Curly: ‘single’ “double” – en — em …\n",
        "Curly: 'single' \"double\" – en — em …\n",
        cfg,
    );
}

#[test]
fn ascii_with_smart_normalizes_quotes_dashes_and_ellipsis() {
    assert_mode(
        "Curly: ‘single’ “double” – en — em …\n",
        "Curly: 'single' \"double\" -- en --- em ...\n",
        SmartPunctuation::Ascii,
    );
}

#[test]
fn smart_normalizes_unicode_ellipsis() {
    let mut cfg = Config {
        smart_punctuation: SmartPunctuation::Ascii,
        ..Config::default()
    };
    cfg.formatter_extensions.smart = true;
    cfg.formatter_extensions.smart_quotes = false;
    cfg.wrap = Some(WrapMode::Preserve);
    let input = "Wait… and then more\n";
    let expected = "Wait... and then more\n";
    let out = format(input, Some(cfg), None);
    assert_eq!(out, expected);
}

#[test]
fn smart_does_not_normalize_inline_code_span_content() {
    let mut cfg = Config {
        smart_punctuation: SmartPunctuation::Ascii,
        ..Config::default()
    };
    cfg.formatter_extensions.smart = true;
    let input = "`a — b … “q” –`\n";
    let out = format(input, Some(cfg.clone()), None);
    assert_eq!(out, input, "code span content must stay verbatim");
    assert_eq!(format(&out, Some(cfg), None), input, "must be idempotent");
}

#[test]
fn smart_does_not_normalize_verbatim_inlines() {
    let input = concat!(
        "`<abbr title='it’s fine'>IIF</abbr>`{=html}\n",
        "\n",
        "`\\textit{“a”}`{=latex} `“a” – b` $“a”$ <i title=\"x’y\">z</i>\n",
        "\n",
        "[a]{title=\"x’y\"} <span title=\"x’y\">z</span>\n",
    );
    for wrap in [
        WrapMode::Preserve,
        WrapMode::Reflow,
        WrapMode::Sentence,
        WrapMode::Semantic,
    ] {
        let mut cfg = Config {
            smart_punctuation: SmartPunctuation::Ascii,
            ..Config::default()
        };
        cfg.formatter_extensions.smart = true;
        let cfg_wrap = format!("{wrap:?}");
        cfg.wrap = Some(wrap);
        let out = format(input, Some(cfg), None);
        assert_eq!(out, input, "verbatim content changed with {cfg_wrap}");
    }
}

#[test]
fn smart_does_not_normalize_autolink_url() {
    let mut cfg = Config {
        smart_punctuation: SmartPunctuation::Ascii,
        ..Config::default()
    };
    cfg.formatter_extensions.smart = true;
    let input = "<https://example.com/a—b>\n";
    let out = format(input, Some(cfg.clone()), None);
    assert_eq!(out, input, "autolink URL must stay verbatim");
    assert_eq!(format(&out, Some(cfg), None), input, "must be idempotent");
}

#[test]
fn smart_normalizes_dashes_in_atx_heading() {
    let mut cfg = Config {
        smart_punctuation: SmartPunctuation::Ascii,
        ..Config::default()
    };
    cfg.formatter_extensions.smart = true;
    let input = "# em — dash and en – dash\n";
    let expected = "# em --- dash and en -- dash\n";
    assert_eq!(format(input, Some(cfg), None), expected);
}

#[test]
fn smart_dash_only_paragraph_does_not_become_thematic_break() {
    let mut cfg = Config {
        smart_punctuation: SmartPunctuation::Ascii,
        ..Config::default()
    };
    cfg.formatter_extensions.smart = true;
    let out = format("—\n", Some(cfg.clone()), None);
    assert_eq!(out, "—\n");
    assert_eq!(format(&out, Some(cfg), None), "—\n", "must be idempotent");
}

#[test]
fn ascii_definition_terms_do_not_become_block_markers() {
    for term in ["—", "– –", "— –"] {
        let input = format!("{term}\n:   definition\n");
        assert_mode(&input, &input, SmartPunctuation::Ascii);
    }
    assert_mode(
        "–\n:   definition\n",
        "--\n:   definition\n",
        SmartPunctuation::Ascii,
    );
}

#[test]
fn ascii_preserves_dash_terms_in_containers() {
    for input in [
        "> —\n> :   definition\n",
        "- —\n  :   definition\n",
        "term\n\n:   intro\n\n    —\n    :   nested\n",
    ] {
        assert_mode(input, input, SmartPunctuation::Ascii);
    }
}

#[test]
fn smart_multi_dash_paragraph_does_not_become_thematic_break() {
    let mut cfg = Config {
        smart_punctuation: SmartPunctuation::Ascii,
        ..Config::default()
    };
    cfg.formatter_extensions.smart = true;
    let out = format("– –\n", Some(cfg.clone()), None);
    assert!(
        !out.lines().any(|l| {
            let t = l.trim();
            !t.is_empty() && t.chars().all(|c| c == '-' || c == ' ')
        }),
        "must not manufacture an all-dash line, got {out:?}"
    );
    assert_eq!(format(&out, Some(cfg), None), out, "must be idempotent");
}

#[test]
fn smart_single_en_dash_paragraph_normalizes_safely() {
    let mut cfg = Config {
        smart_punctuation: SmartPunctuation::Ascii,
        ..Config::default()
    };
    cfg.formatter_extensions.smart = true;
    let out = format("–\n", Some(cfg.clone()), None);
    assert_eq!(out, "--\n");
    assert_eq!(format(&out, Some(cfg), None), "--\n", "must be idempotent");
}

#[test]
fn smart_dash_within_text_still_normalizes() {
    let mut cfg = Config {
        smart_punctuation: SmartPunctuation::Ascii,
        ..Config::default()
    };
    cfg.formatter_extensions.smart = true;
    assert_eq!(format("a — b\n", Some(cfg), None), "a --- b\n");
}

#[test]
fn smart_defaults_follow_flavor() {
    assert!(
        Config {
            flavor: Flavor::Pandoc,
            formatter_extensions: panache_formatter::config::FormatterExtensions::for_flavor(
                Flavor::Pandoc
            ),
            parser_extensions: panache_formatter::config::Extensions::for_flavor(Flavor::Pandoc),
            ..Default::default()
        }
        .formatter_extensions
        .smart
    );
    assert!(
        Config {
            flavor: Flavor::Quarto,
            formatter_extensions: panache_formatter::config::FormatterExtensions::for_flavor(
                Flavor::Quarto
            ),
            parser_extensions: panache_formatter::config::Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        }
        .formatter_extensions
        .smart
    );
    assert!(
        Config {
            flavor: Flavor::RMarkdown,
            formatter_extensions: panache_formatter::config::FormatterExtensions::for_flavor(
                Flavor::RMarkdown
            ),
            parser_extensions: panache_formatter::config::Extensions::for_flavor(Flavor::RMarkdown),
            ..Default::default()
        }
        .formatter_extensions
        .smart
    );
    assert!(
        !Config {
            flavor: Flavor::Gfm,
            formatter_extensions: panache_formatter::config::FormatterExtensions::for_flavor(
                Flavor::Gfm
            ),
            parser_extensions: panache_formatter::config::Extensions::for_flavor(Flavor::Gfm),
            ..Default::default()
        }
        .formatter_extensions
        .smart
    );
    assert!(
        !Config {
            flavor: Flavor::CommonMark,
            formatter_extensions: panache_formatter::config::FormatterExtensions::for_flavor(
                Flavor::CommonMark
            ),
            parser_extensions: panache_formatter::config::Extensions::for_flavor(
                Flavor::CommonMark
            ),
            ..Default::default()
        }
        .formatter_extensions
        .smart
    );
    assert!(
        !Config {
            flavor: Flavor::MultiMarkdown,
            formatter_extensions: panache_formatter::config::FormatterExtensions::for_flavor(
                Flavor::MultiMarkdown
            ),
            parser_extensions: panache_formatter::config::Extensions::for_flavor(
                Flavor::MultiMarkdown
            ),
            ..Default::default()
        }
        .formatter_extensions
        .smart
    );
}
