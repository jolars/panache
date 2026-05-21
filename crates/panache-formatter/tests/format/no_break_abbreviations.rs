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

    // With `lang: de`, the built-in German profile treats `bzw.` as a non-break.
    let de = assert_idempotent(input, &cfg(Some("de"), BTreeMap::new()));
    assert_eq!(
        de,
        "Erstens bzw. zweitens ist wichtig.\nZweiter Satz folgt.\n"
    );

    // Without a language, English rules apply and `bzw.` ends a sentence.
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

    // Without the config, `foo.` is a plain sentence end.
    let bare = assert_idempotent(input, &cfg(None, BTreeMap::new()));
    assert_eq!(bare, "Alpha foo.\nbeta gamma.\nDelta.\n");
}

#[test]
fn per_language_bucket_only_applies_to_matching_language() {
    let input = "Třeba např. tohle platí. Druhá věta.\n";
    let abbreviations = BTreeMap::from([("cs".to_string(), vec!["např.".to_string()])]);

    // `lang: cs` picks up the Czech bucket.
    let cs = assert_idempotent(input, &cfg(Some("cs"), abbreviations.clone()));
    assert_eq!(cs, "Třeba např. tohle platí.\nDruhá věta.\n");

    // `lang: de` does not see the `cs` bucket, so `např.` ends a sentence.
    let de = assert_idempotent(input, &cfg(Some("de"), abbreviations));
    assert_eq!(de, "Třeba např.\ntohle platí.\nDruhá věta.\n");
}

#[test]
fn region_subtag_selects_primary_language_bucket() {
    let input = "Erstens bzw. zweitens ist wichtig. Zweiter Satz folgt.\n";
    // `de-AT` should fold to the built-in German profile.
    let out = assert_idempotent(input, &cfg(Some("de-AT"), BTreeMap::new()));
    assert_eq!(
        out,
        "Erstens bzw. zweitens ist wichtig.\nZweiter Satz folgt.\n"
    );
}
