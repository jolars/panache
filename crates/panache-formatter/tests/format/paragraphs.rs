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

// A mid-sentence `+` conjunction must survive reflow without being reparsed as
// a bullet-list marker when it lands at column 1 on a wrapped line. Formatting
// must be idempotent: re-formatting the wrapped output must not insert a blank
// line or convert the continuation line into a list.
#[test]
fn plus_conjunction_at_wrapped_line_start_is_idempotent() {
    let input = "Övrig blandad inkomst (inkomst från fåmansföretag, aktiv + passiv ej pensionsgrundande inkomst) redovisas.\n";
    let first = format(input, None, None);
    let second = format(&first, None, None);
    similar_asserts::assert_eq!(first, second);
    assert!(!first.contains("\n\n"), "no list interruption: {first:?}");
}

// A `-` before a year inside emphasis must not be reinterpreted as a list
// marker. The sentence fits on one line, so it stays unchanged and idempotent.
#[test]
fn dash_before_year_in_emphasis_is_idempotent() {
    let input = "*Se \"Registerbaserade arbetsmarknadsstatistiken - 2004\".*\n";
    let first = format(input, None, None);
    let second = format(&first, None, None);
    similar_asserts::assert_eq!(first, input);
    similar_asserts::assert_eq!(second, first);
}

// Guard: a genuine bullet list still formats (and stays) a list.
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

// Under the CommonMark dialect (gfm/commonmark) a list interrupts a paragraph
// with no blank line, so a `+` conjunction reflowed to column 1 becomes a list
// marker and the second pass inserts a blank line. Reflow must keep the marker
// off the line start; format twice must equal format once.
#[test]
fn plus_conjunction_reflow_is_idempotent_under_gfm() {
    let input = "Näringsinkomst netto egenavgift Övrig blandad inkomst (inkomst från fåmansföretag, aktiv + passiv ej pensionsgrundande inkomst)\n";
    let config = gfm_config(88);
    let first = format(input, Some(config.clone()), None);
    let second = format(&first, Some(config), None);
    similar_asserts::assert_eq!(first, second);
    // The `+` must not land at column 1 (which would parse as a list item).
    assert!(
        !first.lines().any(|l| l.starts_with("+ ")),
        "marker reflowed to line start: {first:?}"
    );
    assert!(!first.contains("\n\n"), "no list interruption: {first:?}");
}

// Guard: a genuine bullet list under gfm still formats as a list.
#[test]
fn genuine_bullet_list_stays_a_list_under_gfm() {
    let input = "- a\n- b\n";
    let config = gfm_config(80);
    let first = format(input, Some(config.clone()), None);
    similar_asserts::assert_eq!(first, "- a\n- b\n");
    similar_asserts::assert_eq!(format(&first, Some(config), None), first);
}
