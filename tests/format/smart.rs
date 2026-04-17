use panache::config::{Flavor, WrapMode};
use panache::{Config, format};

#[test]
fn smart_enabled_normalizes_curly_quotes_and_dashes() {
    let mut cfg = Config::default();
    cfg.extensions.smart = true;
    cfg.wrap = Some(WrapMode::Preserve);
    let input = "Curly: ‘single’ “double” – en — em\n";
    let expected = "Curly: 'single' \"double\" -- en --- em\n";
    let out = format(input, Some(cfg), None);
    assert_eq!(out, expected);
}

#[test]
fn smart_disabled_preserves_curly_quotes_and_dashes() {
    let mut cfg = Config::default();
    cfg.extensions.smart = false;
    cfg.wrap = Some(WrapMode::Preserve);
    let input = "Curly: ‘single’ “double” – en — em\n";
    let out = format(input, Some(cfg), None);
    assert_eq!(out, input);
}

#[test]
fn smart_quotes_only_normalizes_quotes_not_dashes_or_ellipsis() {
    let mut cfg = Config::default();
    cfg.extensions.smart = false;
    cfg.extensions.smart_quotes = true;
    cfg.wrap = Some(WrapMode::Preserve);
    let input = "Curly: ‘single’ “double” – en — em …\n";
    let expected = "Curly: 'single' \"double\" – en — em …\n";
    let out = format(input, Some(cfg), None);
    assert_eq!(out, expected);
}

#[test]
fn smart_normalizes_unicode_ellipsis() {
    let mut cfg = Config::default();
    cfg.extensions.smart = true;
    cfg.extensions.smart_quotes = false;
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
            extensions: panache::config::Extensions::for_flavor(Flavor::Pandoc),
            ..Default::default()
        }
        .extensions
        .smart
    );
    assert!(
        Config {
            flavor: Flavor::Quarto,
            extensions: panache::config::Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        }
        .extensions
        .smart
    );
    assert!(
        Config {
            flavor: Flavor::RMarkdown,
            extensions: panache::config::Extensions::for_flavor(Flavor::RMarkdown),
            ..Default::default()
        }
        .extensions
        .smart
    );
    assert!(
        !Config {
            flavor: Flavor::Gfm,
            extensions: panache::config::Extensions::for_flavor(Flavor::Gfm),
            ..Default::default()
        }
        .extensions
        .smart
    );
    assert!(
        !Config {
            flavor: Flavor::CommonMark,
            extensions: panache::config::Extensions::for_flavor(Flavor::CommonMark),
            ..Default::default()
        }
        .extensions
        .smart
    );
    assert!(
        !Config {
            flavor: Flavor::MultiMarkdown,
            extensions: panache::config::Extensions::for_flavor(Flavor::MultiMarkdown),
            ..Default::default()
        }
        .extensions
        .smart
    );
}
