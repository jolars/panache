use panache_formatter::config::WrapMode;
use panache_formatter::{Config, format};
use std::collections::BTreeMap;

fn cfg(lang: Option<&str>, abbreviations: BTreeMap<String, Vec<String>>) -> Config {
    Config {
        wrap: Some(WrapMode::Sentence),
        lang: lang.map(str::to_string),
        no_break_abbreviations: abbreviations,
        ..Default::default()
    }
}

fn assert_idempotent(input: &str, config: &Config) -> String {
    let out = format(input, Some(config.clone()), None);
    let out2 = format(&out, Some(config.clone()), None);
    assert_eq!(out, out2, "sentence wrapping must be idempotent");
    out
}

#[test]
fn builtin_german_profile_keeps_abbreviation_on_one_line() {
    let input = "Erstens bzw. zweitens ist wichtig. Zweiter Satz folgt.\n";

    let de = assert_idempotent(input, &cfg(Some("de"), BTreeMap::new()));
    assert_eq!(
        de,
        "Erstens bzw. zweitens ist wichtig.\nZweiter Satz folgt.\n"
    );

    let en = assert_idempotent(input, &cfg(None, BTreeMap::new()));
    assert_eq!(
        en,
        "Erstens bzw.\nzweitens ist wichtig.\nZweiter Satz folgt.\n"
    );
}

#[test]
fn flat_default_bucket_applies_regardless_of_language() {
    let input = "Alpha foo. beta gamma. Delta.\n";
    let abbreviations = BTreeMap::from([("default".to_string(), vec!["foo.".to_string()])]);

    let out = assert_idempotent(input, &cfg(None, abbreviations));
    assert_eq!(out, "Alpha foo. beta gamma.\nDelta.\n");

    let bare = assert_idempotent(input, &cfg(None, BTreeMap::new()));
    assert_eq!(bare, "Alpha foo.\nbeta gamma.\nDelta.\n");
}

#[test]
fn per_language_bucket_only_applies_to_matching_language() {
    let input = "Třeba např. tohle platí. Druhá věta.\n";
    let abbreviations = BTreeMap::from([("cs".to_string(), vec!["např.".to_string()])]);

    let cs = assert_idempotent(input, &cfg(Some("cs"), abbreviations.clone()));
    assert_eq!(cs, "Třeba např. tohle platí.\nDruhá věta.\n");

    let de = assert_idempotent(input, &cfg(Some("de"), abbreviations));
    assert_eq!(de, "Třeba např.\ntohle platí.\nDruhá věta.\n");
}

#[test]
fn region_subtag_selects_primary_language_bucket() {
    let input = "Erstens bzw. zweitens ist wichtig. Zweiter Satz folgt.\n";
    let out = assert_idempotent(input, &cfg(Some("de-AT"), BTreeMap::new()));
    assert_eq!(
        out,
        "Erstens bzw. zweitens ist wichtig.\nZweiter Satz folgt.\n"
    );
}

#[test]
fn document_language_overrides_config_across_paragraphs_and_lists() {
    let input = "---\nlang: de-AT\n---\n\nAlpha foo. beta bzw. gamma. Delta.\n\n- Alpha foo. beta bzw. gamma. Delta.\n";
    let abbreviations = BTreeMap::from([("de".to_string(), vec!["foo.".to_string()])]);

    for wrap in [WrapMode::Sentence, WrapMode::Semantic] {
        let mut config = cfg(Some("en"), abbreviations.clone());
        config.wrap = Some(wrap);
        let out = assert_idempotent(input, &config);
        assert!(out.contains("\nAlpha foo. beta bzw. gamma.\nDelta.\n"));
        assert!(out.contains("\n- Alpha foo. beta bzw. gamma.\n  Delta.\n"));
    }
}

#[test]
fn document_language_applies_when_table_caption_is_first() {
    let input = "---\nlang: de\n---\n\n| A | B |\n|---|---|\n| C | D |\n\n: Alpha foo. beta bzw. gamma. Delta.\n\nAlpha foo. beta bzw. gamma. Delta.\n";
    let abbreviations = BTreeMap::from([("de".to_string(), vec!["foo.".to_string()])]);
    let out = assert_idempotent(input, &cfg(Some("en"), abbreviations));
    assert!(out.contains("  : Alpha foo. beta bzw. gamma.\n    Delta.\n"));
    assert!(out.contains("\nAlpha foo. beta bzw. gamma.\nDelta.\n"));
}

#[test]
fn subtree_formatting_uses_ancestor_document_language() {
    use panache_formatter::formatter::{FormattedCodeMap, Formatter};
    use panache_formatter::syntax::{AstNode, Paragraph};

    let config = cfg(Some("en"), BTreeMap::new());
    let tree = panache_formatter::parser::parse(
        "---\nlang: de\n---\n\nErstens bzw. zweitens. Danach.\n",
        Some(config.parser_options()),
    );
    let paragraph = tree.children().find_map(Paragraph::cast).unwrap();
    let out = Formatter::new(config, FormattedCodeMap::new(), None).format(paragraph.syntax());
    assert_eq!(out, "Erstens bzw. zweitens.\nDanach.\n");
}
