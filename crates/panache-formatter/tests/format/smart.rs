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
