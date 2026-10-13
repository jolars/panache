use panache_formatter::format;

#[test]
fn unterminated_toml_preserves_trailing_content() {
    use panache_formatter::config::{Config, Extensions, Flavor, FormatterExtensions};

    let config = Config {
        flavor: Flavor::Hugo,
        parser_extensions: Extensions::for_flavor(Flavor::Hugo),
        formatter_extensions: FormatterExtensions::for_flavor(Flavor::Hugo),
        ..Default::default()
    };
    for input in [
        "+++\ntitle = \"\"\"\nKeep these spaces   \n\n",
        "+++\ntitle = \"\"\"\nKeep these spaces   ",
        "+++\ninvalid TOML\n\n\t\n",
        "+++\n\n\n",
        "+++",
    ] {
        let output = format(input, Some(config.clone()), None);
        assert_eq!(output, input);
        assert_eq!(format(&output, Some(config.clone()), None), output);
    }
}

#[test]
fn front_matter_and_paragraph() {
    let input = "---\ntitle: hi\n---\n\nHello world\n";
    let output = format(input, None, None);

    assert!(output.contains("title: hi"));
    assert!(output.contains("Hello world"));
}

#[test]
fn leading_blank_before_front_matter_is_idempotent() {
    let input =
        "\n---\ntitle: \"OTexts.com views by country\"\ndate: \"2026-03-26\"\nformat: html\n---\n";
    let once = format(input, None, None);
    let twice = format(&once, None, None);

    assert_eq!(once, twice);
    assert_eq!(
        once,
        "---\ntitle: \"OTexts.com views by country\"\ndate: \"2026-03-26\"\nformat: html\n---\n"
    );
}
