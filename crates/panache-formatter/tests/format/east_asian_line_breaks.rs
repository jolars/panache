use panache_formatter::config::FormatterExtensions;
use panache_formatter::{Config, format};

fn cfg_with_ext(enabled: bool) -> Config {
    Config {
        formatter_extensions: FormatterExtensions {
            east_asian_line_breaks: enabled,
            ..FormatterExtensions::default()
        },
        ..Config::default()
    }
}

#[test]
fn suppresses_soft_break_space_between_wide_chars() {
    let input = "路\n名\n";
    let out = format(input, Some(cfg_with_ext(true)), None);
    assert_eq!(out, "路名\n");
    let out2 = format(&out, Some(cfg_with_ext(true)), None);
    assert_eq!(out, out2);
}

#[test]
fn keeps_space_between_wide_and_ascii() {
    let input = "了\n5600\n";
    let out = format(input, Some(cfg_with_ext(true)), None);
    assert_eq!(out, "了 5600\n");
    let out2 = format(&out, Some(cfg_with_ext(true)), None);
    assert_eq!(out, out2);
}

#[test]
fn keeps_space_between_ascii_and_wide() {
    let input = "5600\n南\n";
    let out = format(input, Some(cfg_with_ext(true)), None);
    assert_eq!(out, "5600 南\n");
    let out2 = format(&out, Some(cfg_with_ext(true)), None);
    assert_eq!(out, out2);
}

#[test]
fn keeps_space_between_two_ascii_words() {
    let input = "foo\nbar\n";
    let out = format(input, Some(cfg_with_ext(true)), None);
    assert_eq!(out, "foo bar\n");
}

#[test]
fn extension_off_keeps_space_between_wide_chars() {
    let input = "路\n名\n";
    let out = format(input, Some(cfg_with_ext(false)), None);
    assert_eq!(out, "路 名\n");
}

#[test]
fn suppresses_inside_link_text() {
    // Pandoc's filter suppresses the space inside a link text when both
    // adjacent chars are wide. The link's own surface markup is unaffected.
    let input = "[象限\n角](https://example.com)\n";
    let out = format(input, Some(cfg_with_ext(true)), None);
    assert_eq!(out, "[象限角](https://example.com)\n");
    let out2 = format(&out, Some(cfg_with_ext(true)), None);
    assert_eq!(out, out2);
}
