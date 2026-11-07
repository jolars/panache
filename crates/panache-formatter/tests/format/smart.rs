use panache_formatter::config::{Flavor, WrapMode};
use panache_formatter::{Config, format};

#[test]
fn smart_enabled_normalizes_curly_quotes_and_dashes() {
    let mut cfg = Config::default();
    cfg.formatter_extensions.smart = true;
    cfg.wrap = Some(WrapMode::Preserve);
    let input = "Curly: ‘single’ “double” – en — em\n";
    let expected = "Curly: 'single' \"double\" -- en --- em\n";
    let out = format(input, Some(cfg), None);
    assert_eq!(out, expected);
}

#[test]
fn smart_disabled_preserves_curly_quotes_and_dashes() {
    let mut cfg = Config::default();
    cfg.formatter_extensions.smart = false;
    cfg.wrap = Some(WrapMode::Preserve);
    let input = "Curly: ‘single’ “double” – en — em\n";
    let out = format(input, Some(cfg), None);
    assert_eq!(out, input);
}

#[test]
fn smart_quotes_only_normalizes_quotes_not_dashes_or_ellipsis() {
    let mut cfg = Config::default();
    cfg.formatter_extensions.smart = false;
    cfg.formatter_extensions.smart_quotes = true;
    cfg.wrap = Some(WrapMode::Preserve);
    let input = "Curly: ‘single’ “double” – en — em …\n";
    let expected = "Curly: 'single' \"double\" – en — em …\n";
    let out = format(input, Some(cfg), None);
    assert_eq!(out, expected);
}

#[test]
fn smart_normalizes_unicode_ellipsis() {
    let mut cfg = Config::default();
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
    let mut cfg = Config::default();
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
        let mut cfg = Config::default();
        cfg.formatter_extensions.smart = true;
        let cfg_wrap = format!("{wrap:?}");
        cfg.wrap = Some(wrap);
        let out = format(input, Some(cfg), None);
        assert_eq!(out, input, "verbatim content changed with {cfg_wrap}");
    }
}

#[test]
fn smart_does_not_normalize_autolink_url() {
    let mut cfg = Config::default();
    cfg.formatter_extensions.smart = true;
    let input = "<https://example.com/a—b>\n";
    let out = format(input, Some(cfg.clone()), None);
    assert_eq!(out, input, "autolink URL must stay verbatim");
    assert_eq!(format(&out, Some(cfg), None), input, "must be idempotent");
}

#[test]
fn smart_normalizes_dashes_in_atx_heading() {
    let mut cfg = Config::default();
    cfg.formatter_extensions.smart = true;
    let input = "# em — dash and en – dash\n";
    let expected = "# em --- dash and en -- dash\n";
    assert_eq!(format(input, Some(cfg), None), expected);
}

#[test]
fn smart_dash_only_paragraph_does_not_become_thematic_break() {
    let mut cfg = Config::default();
    cfg.formatter_extensions.smart = true;
    let out = format("—\n", Some(cfg.clone()), None);
    assert_eq!(out, "—\n");
    assert_eq!(format(&out, Some(cfg), None), "—\n", "must be idempotent");
}

#[test]
fn smart_multi_dash_paragraph_does_not_become_thematic_break() {
    let mut cfg = Config::default();
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
    let mut cfg = Config::default();
    cfg.formatter_extensions.smart = true;
    let out = format("–\n", Some(cfg.clone()), None);
    assert_eq!(out, "--\n");
    assert_eq!(format(&out, Some(cfg), None), "--\n", "must be idempotent");
}

#[test]
fn smart_dash_within_text_still_normalizes() {
    let mut cfg = Config::default();
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
