use panache_formatter::config::{Flavor, WrapMode};
use panache_formatter::{Config, SmartPunctuation, format};

fn unicode_config() -> Config {
    let mut cfg = Config::default();
    cfg.formatter_extensions.smart = true;
    cfg.smart_punctuation = SmartPunctuation::Unicode;
    cfg
}

/// Format with `cfg`, asserting that formatting the result again is a no-op.
fn format_idempotent(input: &str, cfg: &Config) -> String {
    let out = format(input, Some(cfg.clone()), None);
    assert_eq!(
        format(&out, Some(cfg.clone()), None),
        out,
        "must be idempotent"
    );
    out
}

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

#[test]
fn unicode_writes_curly_quotes_dashes_and_ellipsis() {
    let input = "He said \"hi\" -- it's 'fine'... --- ok\n";
    let expected = "He said “hi” – it’s ‘fine’… — ok\n";
    assert_eq!(format_idempotent(input, &unicode_config()), expected);
}

#[test]
fn unicode_pairs_quotes_across_markup_lines_and_punctuation() {
    let input = "A \"*b*\" and (\"c\"), \"d\nacross\".\n";
    let expected = "A “*b*” and (“c”), “d across”.\n";
    assert_eq!(format_idempotent(input, &unicode_config()), expected);
}

#[test]
fn unicode_pairs_nested_quotes() {
    let input = "\"She said 'no' to me\"\n";
    assert_eq!(
        format_idempotent(input, &unicode_config()),
        "“She said ‘no’ to me”\n"
    );
}

#[test]
fn unicode_leaves_unpaired_quotes_as_written() {
    let input = "a \" b and “ c\n";
    assert_eq!(format_idempotent(input, &unicode_config()), input);
}

#[test]
fn unicode_leaves_code_urls_and_link_titles_verbatim() {
    let input = "[\"a\"](http://x--y.z \"t\") `\"b\" --` <http://u--v>\n";
    let expected = "[“a”](http://x--y.z \"t\") `\"b\" --` <http://u--v>\n";
    assert_eq!(format_idempotent(input, &unicode_config()), expected);
}

#[test]
fn unicode_localizes_quotes_from_document_lang() {
    let input = "---\nlang: de-DE\n---\n\nEr sagte \"hallo\" und 'tschüss'.\n";
    let out = format_idempotent(input, &unicode_config());
    assert!(out.ends_with("Er sagte „hallo“ und ‚tschüss‘.\n"), "{out}");
}

#[test]
fn unicode_relocalizes_curly_quotes_from_config_lang() {
    let mut cfg = unicode_config();
    cfg.lang = Some("fr".to_string());
    assert_eq!(
        format_idempotent("Il dit “oui”.\n", &cfg),
        "Il dit «oui».\n"
    );
}

#[test]
fn unicode_uses_explicit_quotes() {
    let mut cfg = unicode_config();
    cfg.quotes = Some(['»', '«', '›', '‹']);
    assert_eq!(format_idempotent("\"a\" 'b'\n", &cfg), "»a« ›b‹\n");
}

#[test]
fn unicode_with_smart_quotes_only_leaves_dashes() {
    let mut cfg = unicode_config();
    cfg.formatter_extensions.smart = false;
    cfg.formatter_extensions.smart_quotes = true;
    assert_eq!(format_idempotent("\"a\" -- b\n", &cfg), "“a” -- b\n");
}

#[test]
fn unicode_without_smart_changes_nothing() {
    let mut cfg = unicode_config();
    cfg.formatter_extensions.smart = false;
    let input = "\"a\" -- b “c” – d\n";
    assert_eq!(format_idempotent(input, &cfg), input);
}

#[test]
fn preserve_keeps_both_spellings() {
    let mut cfg = unicode_config();
    cfg.smart_punctuation = SmartPunctuation::Preserve;
    let input = "\"a\" -- b “c” – d\n";
    assert_eq!(format_idempotent(input, &cfg), input);
}

#[test]
fn smart_keeps_verbatim_content_under_wrap_preserve() {
    let mut cfg = Config::default();
    cfg.formatter_extensions.smart = true;
    cfg.wrap = Some(WrapMode::Preserve);
    let input = "`a “b” –` $c – d$ <http://e—f> [g]{title=\"“h”\"} <span title=\"“i”\">j</span>\n";
    let out = format(input, Some(cfg), None);
    assert_eq!(out, input, "only prose may change");
}

#[test]
fn smart_normalizes_strikeout_and_citation_affixes() {
    let mut cfg = Config::default();
    cfg.formatter_extensions.smart = true;
    let input = "~~a “b”~~ [see “c” @key, p. 3–4]\n";
    let expected = "~~a \"b\"~~ [see \"c\" @key, p. 3--4]\n";
    assert_eq!(format_idempotent(input, &cfg), expected);
}

#[test]
fn unicode_opens_quotes_before_any_inline() {
    let input = "\"*a* b\" and \"[c](u) d\"\n";
    let expected = "“*a* b” and “[c](u) d”\n";
    assert_eq!(format_idempotent(input, &unicode_config()), expected);
}

#[test]
fn unicode_writes_lone_single_quotes_as_apostrophes() {
    let cfg = unicode_config();
    assert_eq!(
        format_idempotent("'tis the season's\n", &cfg),
        "’tis the season’s\n"
    );
    assert_eq!(
        format_idempotent("the dogs' bowls\n", &cfg),
        "the dogs’ bowls\n"
    );
}

#[test]
fn unicode_nests_english_double_quotes_as_single_quotes() {
    let mut cfg = unicode_config();
    cfg.lang = Some("it".to_string());
    let out = format_idempotent("\"a 'b' c\"\n", &cfg);
    assert_eq!(out, "«a “b” c»\n");
}
