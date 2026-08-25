use panache_formatter::Config;
use panache_formatter::config::{Extensions, Flavor};
use panache_formatter::format;

#[test]
fn preserves_inline_code_whitespace() {
    let input = "This is `foo   bar` inline code.";
    let output = format(input, None, None);
    similar_asserts::assert_eq!(output, "This is `foo   bar` inline code.\n");
}

#[test]
fn preserves_inline_math_whitespace() {
    let input = "Math: $x   +   y$";
    let output = format(input, None, None);
    similar_asserts::assert_eq!(output, "Math: $x   +   y$\n");
}

#[test]
fn preserves_host_bookdown_label_when_math_formatting_is_disabled() {
    let input = "Math: $x   +   y (\\#eq:sum)$\n";
    let mut config = Config::default();
    config.parser_extensions.bookdown_equation_references = true;

    let output = format(input, Some(config), None);

    similar_asserts::assert_eq!(output, input);
}

#[test]
fn text_reference_paragraph_is_not_wrapped() {
    let input = "(ref:foo) A scatterplot of the data `cars` using **base** R graphics.\n";
    let mut config = Config::default();
    config.parser_extensions.bookdown_references = true;
    config.formatter_extensions.bookdown_references = true;
    config.line_width = 40;
    let output = format(input, Some(config), None);
    similar_asserts::assert_eq!(output, input);
}

#[test]
fn abbreviation_year_stays_on_same_line_when_wrapping() {
    let input = "M.A. 2007\n";
    let config = Config {
        line_width: 6,
        ..Default::default()
    };
    let output = format(input, Some(config), None);
    similar_asserts::assert_eq!(output, "M.A. 2007\n");
}

#[test]
fn standalone_presentation_pause_is_preserved() {
    let input = "Before\n\n. . .\n\nAfter\n";
    let output = format(input, None, None);
    similar_asserts::assert_eq!(output, input);
}

#[test]
fn standalone_presentation_pause_stays_idempotent_with_wrapping() {
    let input = "Before\n\n. . .\n\nAfter\n";
    let config = Config {
        line_width: 8,
        ..Default::default()
    };
    let first = format(input, Some(config.clone()), None);
    let second = format(&first, Some(config), None);
    similar_asserts::assert_eq!(first, second);
    assert!(first.contains("\n\n. . .\n\n"));
}

#[test]
fn plus_conjunction_at_wrapped_line_start_is_idempotent() {
    let input = "Övrig blandad inkomst (inkomst från fåmansföretag, aktiv + passiv ej pensionsgrundande inkomst) redovisas.\n";
    let first = format(input, None, None);
    let second = format(&first, None, None);
    similar_asserts::assert_eq!(first, second);
    assert!(!first.contains("\n\n"), "no list interruption: {first:?}");
}

#[test]
fn dash_before_year_in_emphasis_is_idempotent() {
    let input = "*Se \"Registerbaserade arbetsmarknadsstatistiken - 2004\".*\n";
    let first = format(input, None, None);
    let second = format(&first, None, None);
    similar_asserts::assert_eq!(first, input);
    similar_asserts::assert_eq!(second, first);
}

#[test]
fn genuine_bullet_list_stays_a_list() {
    let input = "- a\n- b\n";
    let first = format(input, None, None);
    similar_asserts::assert_eq!(first, "- a\n- b\n");
    similar_asserts::assert_eq!(format(&first, None, None), first);
}

fn gfm_config(line_width: usize) -> Config {
    let flavor = Flavor::Gfm;
    Config {
        flavor,
        parser_extensions: Extensions::for_flavor(flavor),
        line_width,
        ..Default::default()
    }
}

#[test]
fn plus_conjunction_reflow_is_idempotent_under_gfm() {
    let input = "Näringsinkomst netto egenavgift Övrig blandad inkomst (inkomst från fåmansföretag, aktiv + passiv ej pensionsgrundande inkomst)\n";
    let config = gfm_config(88);
    let first = format(input, Some(config.clone()), None);
    let second = format(&first, Some(config), None);
    similar_asserts::assert_eq!(first, second);
    assert!(
        !first.lines().any(|l| l.starts_with("+ ")),
        "marker reflowed to line start: {first:?}"
    );
    assert!(!first.contains("\n\n"), "no list interruption: {first:?}");
}

#[test]
fn genuine_bullet_list_stays_a_list_under_gfm() {
    let input = "- a\n- b\n";
    let config = gfm_config(80);
    let first = format(input, Some(config.clone()), None);
    similar_asserts::assert_eq!(first, "- a\n- b\n");
    similar_asserts::assert_eq!(format(&first, Some(config), None), first);
}
