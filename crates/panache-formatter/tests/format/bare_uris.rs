use panache_formatter::{Config, format};

fn format_with_bare_uris(input: &str) -> String {
    let mut config = Config::default();
    config.parser_extensions.autolink_bare_uris = true;
    format(input, Some(config), None)
}

#[test]
fn autolink_bare_uri_basic() {
    let input = "http://google.com is a search engine.\n";
    let output = format_with_bare_uris(input);
    similar_asserts::assert_eq!(
        output,
        "[http://google.com](http://google.com) is a search engine.\n"
    );
}

#[test]
fn autolink_bare_uri_with_query() {
    let input = "Try this query: http://google.com?search=fish&time=hour.\n";
    let output = format_with_bare_uris(input);
    similar_asserts::assert_eq!(
        output,
        "Try this query:\n[http://google.com?search=fish&time=hour](http://google.com?search=fish&time=hour).\n"
    );
}

#[test]
fn autolink_bare_uri_in_parens() {
    let input = "(http://google.com).\n";
    let output = format_with_bare_uris(input);
    similar_asserts::assert_eq!(output, "([http://google.com](http://google.com)).\n");
}

#[test]
fn autolink_bare_uri_uppercase() {
    let input = "HTTPS://GOOGLE.COM,\n";
    let output = format_with_bare_uris(input);
    similar_asserts::assert_eq!(output, "[HTTPS://GOOGLE.COM](HTTPS://GOOGLE.COM),\n");
}

#[test]
fn autolink_bare_uri_less_common_schemes() {
    similar_asserts::assert_eq!(
        format_with_bare_uris("ssh://host\n"),
        "[ssh://host](ssh://host)\n"
    );
    similar_asserts::assert_eq!(
        format_with_bare_uris("mongodb://localhost/db\n"),
        "[mongodb://localhost/db](mongodb://localhost/db)\n"
    );
}

#[test]
fn strong_ending_in_colon_is_not_autolinked() {
    let input = "**Note:**\n";
    let output = format_with_bare_uris(input);
    similar_asserts::assert_eq!(output, "**Note:**\n");
    similar_asserts::assert_eq!(format_with_bare_uris(&output), output);
}
